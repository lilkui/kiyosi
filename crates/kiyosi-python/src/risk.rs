use crate::error::domain;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

pub(crate) fn greek(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Greek> {
    let name = value.extract::<&str>()?;
    kiyosi::Greek::ALL
        .into_iter()
        .find(|g| g.name() == name)
        .ok_or_else(|| PyValueError::new_err("unknown Greek name"))
}

pub(crate) fn greeks_request(greeks: &Bound<'_, PyAny>) -> PyResult<kiyosi::GreeksRequest> {
    if greeks.is_instance_of::<pyo3::types::PyString>() {
        if greeks.extract::<&str>()? == "all" {
            return Ok(kiyosi::GreeksRequest::all());
        }
        return Ok(kiyosi::GreeksRequest::new([greek(greeks)?]));
    }
    let mut measures = [None; kiyosi::Greek::ALL.len()];
    for item in greeks.try_iter()? {
        let measure = greek(&item?)?;
        measures[measure as usize] = Some(measure);
    }
    Ok(kiyosi::GreeksRequest::new(measures.into_iter().flatten()))
}

/// Finite price and optional finite price sensitivities.
///
/// Notes
/// -----
/// Unavailable or unrequested Greeks are None. Spot derivatives are per spot
/// unit, volatility/rate derivatives per percentage point (squared for volga), and time
/// derivatives per calendar day of forward valuation time.
/// Rho shifts the risk-free curve in parallel; dividend rho shifts the dividend-yield
/// curve in parallel, holding other market inputs fixed. Vega is the first volatility
/// derivative divided by 100; volga is the second divided by 10000.
/// Vanna is the spot-volatility cross derivative divided by 100; zomma is the
/// volatility derivative of gamma divided by 100. Theta, charm and color are
/// the valuation-time derivatives of price, delta and gamma, respectively,
/// with the market curves held fixed. Positive time increments advance valuation.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(PartialEq)]
pub(crate) struct PricingResult(pub kiyosi::PricingResult);

#[pymethods]
impl PricingResult {
    /// Return the finite price in contract payment units.
    #[getter]
    fn price(&self) -> f64 {
        self.0.price()
    }
    /// Return delta, or None if unrequested or unavailable.
    #[getter]
    fn delta(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Delta)
    }
    /// Return gamma, or None if unrequested or unavailable.
    #[getter]
    fn gamma(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Gamma)
    }
    /// Return speed, or None if unrequested or unavailable.
    #[getter]
    fn speed(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Speed)
    }
    /// Return theta, or None if unrequested or unavailable.
    #[getter]
    fn theta(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Theta)
    }
    /// Return charm, or None if unrequested or unavailable.
    #[getter]
    fn charm(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Charm)
    }
    /// Return color, or None if unrequested or unavailable.
    #[getter]
    fn color(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Color)
    }
    /// Return vega, or None if unrequested or unavailable.
    #[getter]
    fn vega(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Vega)
    }
    /// Return volga per squared volatility percentage point, or None if unavailable.
    #[getter]
    fn volga(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Volga)
    }
    /// Return vanna, or None if unrequested or unavailable.
    #[getter]
    fn vanna(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Vanna)
    }
    /// Return zomma, or None if unrequested or unavailable.
    #[getter]
    fn zomma(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Zomma)
    }
    /// Return rho, or None if unrequested or unavailable.
    #[getter]
    fn rho(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::Rho)
    }
    /// Return dividend rho per yield percentage point, or None if unavailable.
    #[getter]
    fn dividend_rho(&self) -> Option<f64> {
        self.0.get(kiyosi::Greek::DividendRho)
    }
    /// Return available, not_requested, unsupported or undefined for a Greek.
    ///
    /// Unsupported means the engine/model does not implement the measure; undefined
    /// means a supported measure cannot be evaluated at this valuation or stencil.
    fn status(&self, measure: &Bound<'_, PyAny>) -> PyResult<&'static str> {
        Ok(match self.0.status(greek(measure)?) {
            kiyosi::GreekStatus::Available => "available",
            kiyosi::GreekStatus::NotRequested => "not_requested",
            kiyosi::GreekStatus::Unsupported => "unsupported",
            kiyosi::GreekStatus::Undefined => "undefined",
        })
    }
    /// Return requested Greek names in the library's stable measure order.
    #[getter]
    fn requested(&self) -> Vec<&'static str> {
        kiyosi::Greek::ALL
            .into_iter()
            .filter(|&g| self.0.requested().has(g))
            .map(kiyosi::Greek::name)
            .collect()
    }
    /// Return engine/model-supported names, independent of valuation and stencil availability.
    #[getter]
    fn supported(&self) -> Vec<&'static str> {
        kiyosi::Greek::ALL
            .into_iter()
            .filter(|&g| self.0.supported().has(g))
            .map(kiyosi::Greek::name)
            .collect()
    }
    /// Return a requested Greek or raise when it is unavailable.
    ///
    /// Parameters
    /// ----------
    /// measure : str
    ///     Greek name, such as delta or vega.
    ///
    /// Returns
    /// -------
    /// float
    ///     Finite sensitivity in PricingResult reporting units.
    ///
    /// Raises
    /// ------
    /// UnsupportedOperationError
    ///     If the measure was not requested or is undefined at this valuation.
    fn require(&self, measure: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(self.0.require(greek(measure)?))
    }
}

mod coupon;
mod implied;
pub(crate) use coupon::{implied_phoenix_coupon, implied_snowball_coupon};
pub(crate) use implied::implied_volatility;

#[pyfunction]
#[pyo3(signature = (price, greeks, *, requested=None, supported=None))]
pub(crate) fn _pricing_result(
    price: &Bound<'_, PyAny>,
    greeks: &Bound<'_, pyo3::types::PyDict>,
    requested: Option<&Bound<'_, PyAny>>,
    supported: Option<&Bound<'_, PyAny>>,
) -> PyResult<PricingResult> {
    let mut result = domain(kiyosi::PricingResult::new(crate::conversions::real(price)?))?;
    for (name, value) in greeks.iter() {
        result = domain(result.with_greek(greek(&name)?, crate::conversions::real(&value)?))?;
    }
    let requested = requested
        .map(greeks_request)
        .transpose()?
        .unwrap_or(result.requested());
    let supported = supported
        .map(greeks_request)
        .transpose()?
        .unwrap_or(result.supported());
    Ok(PricingResult(result.with_request(requested, supported)))
}

/// Compute numerical BSM Greeks through any supported built-in scalar engine.
///
/// Parameters
/// ----------
/// engine, instrument : object
///     Built-in engine and one of its supported contracts.
/// context : PricingContext
///     BSM market snapshot; local-volatility and Heston models are rejected.
/// greeks : str or iterable of str
///     Required measure names or "all". Unsupported instrument/engine pairs are
///     checked before Greek names.
/// spot_shift : float or None, optional
///     Positive absolute spot bump, or None for the engine/product default.
/// volatility_shift, rate_shift : float, optional
///     Positive decimal-unit bumps. Defaults come from the Rust core.
/// time_shift_days : int, optional
///     Positive calendar-day bump, default 1.
///
/// Returns
/// -------
/// PricingResult
///     Numerical sensitivities with request and availability metadata. An unavailable
///     stencil has status "undefined". Units match native price_with_greeks.
///
/// Raises
/// ------
/// KiyosiError
///     If the request, settings, model, contract or a feasible bumped price fails.
/// OSError
///     If system entropy or worker creation fails. Monte Carlo draws are pinned
///     within the request, matching the Rust numerical_greeks operation.
#[pyfunction]
#[pyo3(signature = (engine, instrument, context, greeks, *, spot_shift=None, volatility_shift=kiyosi::NumericalShiftSettings::default().volatility_shift, rate_shift=kiyosi::NumericalShiftSettings::default().rate_shift, time_shift_days=kiyosi::NumericalShiftSettings::default().time_shift_days))]
#[expect(
    clippy::too_many_arguments,
    reason = "explicit Python numerical controls"
)]
pub(crate) fn numerical_greeks(
    py: Python<'_>,
    engine: &Bound<'_, PyAny>,
    instrument: &Bound<'_, PyAny>,
    context: &crate::market::PricingContext,
    greeks: &Bound<'_, PyAny>,
    spot_shift: Option<&Bound<'_, PyAny>>,
    #[pyo3(from_py_with = crate::conversions::real)] volatility_shift: f64,
    #[pyo3(from_py_with = crate::conversions::real)] rate_shift: f64,
    #[pyo3(from_py_with = crate::conversions::integer)] time_shift_days: i32,
) -> PyResult<PricingResult> {
    let result = crate::registration::evaluate(py, engine, instrument, context, || {
        Ok(crate::engines::NumericalGreeksRequest {
            request: greeks_request(greeks)?,
            settings: kiyosi::NumericalShiftSettings {
                spot_shift: spot_shift.map(crate::conversions::real).transpose()?,
                volatility_shift,
                rate_shift,
                time_shift_days,
            },
        })
    })?;
    Ok(PricingResult(result))
}
