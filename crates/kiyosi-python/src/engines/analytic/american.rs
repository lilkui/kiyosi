use crate::error::domain;
use crate::instruments::american::AmericanOption;
use crate::market::PricingContext;
use crate::risk;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::PyDict;
/// Bjerksund-Stensland 2002 American approximation with flat rates.
///
/// See Also
/// --------
/// AmericanOption : Supported contract.
#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct BjerksundStenslandVanillaEngine;

#[pymethods]
impl BjerksundStenslandVanillaEngine {
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
            AmericanOption,
        >,
        context: &PricingContext,
    ) -> PyResult<f64> {
        domain(
            py,
            core::BjerksundStenslandVanillaEngine.price(&instrument.0, &context.0),
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
    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        #[pyo3(from_py_with = crate::conversions::instrument)] instrument: PyRef<
            '_,
            AmericanOption,
        >,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<risk::PricingResult> {
        let request = risk::request(greeks, settings)?;
        let shifts = risk::shifts(settings)?;
        Ok(risk::PricingResult(domain(
            py,
            core::BjerksundStenslandVanillaEngine.price_with_greeks(
                &instrument.0,
                &context.0,
                request,
                shifts,
            ),
        )?))
    }
}
