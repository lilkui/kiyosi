use crate::conversions::{boolean, check_changes, integer, real};
use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

pub(crate) fn greek(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Greek> {
    let name = value.extract::<&str>()?;
    kiyosi::Greek::ALL
        .into_iter()
        .find(|g| g.name() == name)
        .ok_or_else(|| PyValueError::new_err("unknown Greek name"))
}

pub(crate) fn request(
    greeks: Option<&Bound<'_, PyAny>>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<kiyosi::GreeksRequest> {
    let all = settings
        .map(|s| s.get_item("all_greeks"))
        .transpose()?
        .flatten();
    greeks_request(
        greeks,
        all.as_ref().map(boolean).transpose()?.unwrap_or(false),
    )
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

pub(crate) fn real_setting(
    settings: Option<&Bound<'_, PyDict>>,
    name: &str,
    default: f64,
) -> PyResult<f64> {
    settings
        .map(|s| s.get_item(name))
        .transpose()?
        .flatten()
        .as_ref()
        .map(real)
        .transpose()
        .map(|v| v.unwrap_or(default))
}
pub(crate) fn integer_setting(
    settings: Option<&Bound<'_, PyDict>>,
    name: &str,
    default: i32,
) -> PyResult<i32> {
    settings
        .map(|s| s.get_item(name))
        .transpose()?
        .flatten()
        .as_ref()
        .map(integer)
        .transpose()
        .map(|v| v.unwrap_or(default))
}
pub(crate) fn shifts(
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<kiyosi::NumericalShiftSettings> {
    shifts_with_defaults(settings, kiyosi::NumericalShiftSettings::default())
}

pub(crate) fn shifts_with_defaults(
    settings: Option<&Bound<'_, PyDict>>,
    d: kiyosi::NumericalShiftSettings,
) -> PyResult<kiyosi::NumericalShiftSettings> {
    check_changes(
        settings,
        &[
            "all_greeks",
            "spot_shift",
            "volatility_shift",
            "rate_shift",
            "time_shift_days",
        ],
    )?;
    Ok(kiyosi::NumericalShiftSettings {
        spot_shift: real_setting(settings, "spot_shift", d.spot_shift)?,
        volatility_shift: real_setting(settings, "volatility_shift", d.volatility_shift)?,
        rate_shift: real_setting(settings, "rate_shift", d.rate_shift)?,
        time_shift_days: integer_setting(settings, "time_shift_days", d.time_shift_days)?,
    })
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
