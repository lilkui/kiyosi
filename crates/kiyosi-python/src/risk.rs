use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

pub(crate) fn greek(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Greek> {
    let name = value.extract::<&str>()?;
    kiyosi::Greek::ALL
        .into_iter()
        .find(|g| g.name() == name)
        .ok_or_else(|| PyValueError::new_err("unknown Greek name"))
}

pub(crate) fn greeks_request(
    greeks: Option<&Bound<'_, PyAny>>,
    all: bool,
) -> PyResult<kiyosi::GreeksRequest> {
    let greeks = greeks.filter(|g| !g.is_none());
    if all {
        if greeks.is_some() {
            return Err(PyTypeError::new_err(
                "greeks must be omitted with all_greeks",
            ));
        }
        return Ok(kiyosi::GreeksRequest::all());
    }
    let greeks = greeks.ok_or_else(|| PyTypeError::new_err("provide greeks or all_greeks=True"))?;
    let measures = if greeks.is_instance_of::<pyo3::types::PyString>() {
        vec![greek(greeks)?]
    } else {
        greeks
            .try_iter()?
            .map(|item| greek(&item?))
            .collect::<PyResult<Vec<_>>>()?
    };
    Ok(kiyosi::GreeksRequest::new(measures))
}

/// Finite price and optional finite price sensitivities.
///
/// Notes
/// -----
/// Unavailable or unrequested Greeks are None. Spot derivatives are per spot
/// unit, volatility/rate derivatives per percentage point, and time
/// derivatives per calendar day of forward valuation time.
#[pyclass(frozen, module = "kiyosi._native")]
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
    fn require(&self, py: Python<'_>, measure: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(py, self.0.require(greek(measure)?))
    }
}

mod coupon;
mod implied;
pub(crate) use coupon::implied_coupon;
pub(crate) use implied::implied_volatility;
