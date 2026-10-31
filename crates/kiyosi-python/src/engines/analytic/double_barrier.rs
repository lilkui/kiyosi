use crate::error::domain;
use crate::instruments::double_barrier::DoubleBarrierOption;
use crate::market::PricingContext;
use crate::risk;
use pyo3::prelude::*;
/// Flat-rate continuous double-barrier analytics.
///
/// See Also
/// --------
/// DoubleBarrierOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct AnalyticDoubleBarrierEngine;

#[pymethods]
impl AnalyticDoubleBarrierEngine {
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
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            DoubleBarrierOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        let option = &instrument.0;
        let context = &context.0;
        domain(
            py,
            py.detach(|| kiyosi::AnalyticDoubleBarrierEngine.price(option, context)),
        )
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
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            DoubleBarrierOption,
        >,
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
        let option = &instrument.0;
        let context = &context.0;
        Ok(risk::PricingResult(domain(
            py,
            py.detach(|| {
                kiyosi::AnalyticDoubleBarrierEngine
                    .price_with_greeks(option, context, request, shifts)
            }),
        )?))
    }
}
