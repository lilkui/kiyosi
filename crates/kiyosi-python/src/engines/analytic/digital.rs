use crate::engines::ScalarRequest;
use crate::error::domain;
use crate::instruments::digital::DigitalInstrument;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;
/// Analytic European digital pricing with deterministic rate curves.
///
/// See Also
/// --------
/// CashOrNothingOption : Supported contract.
/// AssetOrNothingOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticDigitalEngine;

#[pymethods]
impl AnalyticDigitalEngine {
    #[new]
    fn new() -> Self {
        Self
    }

    /// Return the price without changing the instrument or context.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    ///
    /// Returns
    /// -------
    /// float
    ///     Present value of remaining contractual payments.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If domain validation, engine applicability or numerical pricing fails.
    fn price(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: DigitalInstrument<'_>,
        context: &PricingContext,
    ) -> PyResult<f64> {
        self.evaluate(py, instrument, context, ScalarRequest::Price)
    }

    /// Return the price and selected sensitivities.
    ///
    /// Parameters
    /// ----------
    /// instrument : object
    ///     Supported contract; see the engine class for accepted types.
    /// context : PricingContext
    ///     Market snapshot and valuation instant.
    /// greeks : str or iterable of str, optional
    ///     Greek names. Required unless `all_greeks=True`; duplicates are ignored.
    /// all_greeks : bool, optional
    ///     Request every measure; omit `greeks` when True. Defaults to False.
    /// spot_shift : float, optional
    ///     Positive finite absolute spot bump; defaults to 0.01 spot units.
    /// volatility_shift : float, optional
    ///     Positive finite volatility bump in decimal units; defaults to 0.0001.
    /// rate_shift : float, optional
    ///     Positive finite parallel risk-free-rate bump in decimal units;
    ///     defaults to 0.0001.
    /// time_shift_days : int, optional
    ///     Positive forward/backward time bump in calendar days; defaults to 1.
    ///     All bumps are validated even when analytic Greeks are available.
    ///
    /// Returns
    /// -------
    /// PricingResult
    ///     Price and requested Greeks. Unsupported or undefined Greeks are None,
    ///     including every Greek at expiry. Units follow PricingResult.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If requests, shifts, pricing or a feasible bumped valuation fails.
    #[pyo3(signature = (instrument, context, greeks=None, *, all_greeks=false, spot_shift=kiyosi::NumericalShiftSettings::default().spot_shift, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python pricing controls"
    )]
    fn price_with_greeks(
        &self,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: DigitalInstrument<'_>,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::boolean)] all_greeks: bool,
        #[pyo3(from_py_with = crate::conversions::real)] spot_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
        #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
        #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::greeks_request(greeks, all_greeks)?;
        let shifts = kiyosi::NumericalShiftSettings {
            spot_shift,
            volatility_shift,
            rate_shift,
            time_shift_days,
        };
        Ok(risk::PricingResult(domain(
            kiyosi::AnalyticDigitalEngine.price_with_greeks(
                instrument.contract(),
                &context.0,
                request,
                shifts,
            ),
        )?))
    }
}

impl AnalyticDigitalEngine {
    pub(crate) fn evaluate(
        &self,
        py: Python<'_>,
        instrument: DigitalInstrument<'_>,
        context: &PricingContext,
        request: ScalarRequest,
    ) -> PyResult<f64> {
        let option = instrument.contract();
        let context = &context.0;
        let calculate = || request.evaluate(&kiyosi::AnalyticDigitalEngine, option, context);
        // Keep the GIL for short scalar formulas, but release it for iterative solves.
        domain(if matches!(request, ScalarRequest::Price) {
            calculate()
        } else {
            py.detach(calculate)
        })
    }
}
