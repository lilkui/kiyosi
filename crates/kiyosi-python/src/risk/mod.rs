use crate::conversions::{check_changes, integer, real};
use crate::error::domain;
use kiyosi_core as core;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict};

pub(crate) fn greek(value: &Bound<'_, PyAny>) -> PyResult<core::Greek> {
    let name = value.extract::<&str>()?;
    core::Greek::ALL
        .into_iter()
        .find(|g| g.name() == name)
        .ok_or_else(|| PyValueError::new_err("unknown Greek name"))
}

pub(crate) fn request(
    greeks: Option<&Bound<'_, PyAny>>,
    settings: Option<&Bound<'_, PyDict>>,
) -> PyResult<core::GreeksRequest> {
    let all = settings
        .map(|s| s.get_item("all_greeks"))
        .transpose()?
        .flatten();
    let all = if let Some(all) = all {
        if !all.is_instance_of::<PyBool>() {
            return Err(PyTypeError::new_err("all_greeks must be bool"));
        }
        all.extract::<bool>()?
    } else {
        false
    };
    let greeks = greeks.filter(|g| !g.is_none());
    if all {
        if greeks.is_some() {
            return Err(PyTypeError::new_err(
                "greeks must be omitted with all_greeks",
            ));
        }
        return Ok(core::GreeksRequest::all());
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
    Ok(core::GreeksRequest::new(measures))
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
) -> PyResult<core::NumericalShiftSettings> {
    shifts_with_defaults(settings, core::NumericalShiftSettings::default())
}

pub(crate) fn shifts_with_defaults(
    settings: Option<&Bound<'_, PyDict>>,
    d: core::NumericalShiftSettings,
) -> PyResult<core::NumericalShiftSettings> {
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
    Ok(core::NumericalShiftSettings {
        spot_shift: real_setting(settings, "spot_shift", d.spot_shift)?,
        volatility_shift: real_setting(settings, "volatility_shift", d.volatility_shift)?,
        rate_shift: real_setting(settings, "rate_shift", d.rate_shift)?,
        time_shift_days: integer_setting(settings, "time_shift_days", d.time_shift_days)?,
    })
}

#[pyclass(frozen, module = "kiyosi._native")]
pub(crate) struct PricingResult(pub core::PricingResult);

#[pymethods]
impl PricingResult {
    #[getter]
    fn price(&self) -> f64 {
        self.0.price()
    }
    #[getter]
    fn delta(&self) -> Option<f64> {
        self.0.get(core::Greek::Delta)
    }
    #[getter]
    fn gamma(&self) -> Option<f64> {
        self.0.get(core::Greek::Gamma)
    }
    #[getter]
    fn speed(&self) -> Option<f64> {
        self.0.get(core::Greek::Speed)
    }
    #[getter]
    fn theta(&self) -> Option<f64> {
        self.0.get(core::Greek::Theta)
    }
    #[getter]
    fn charm(&self) -> Option<f64> {
        self.0.get(core::Greek::Charm)
    }
    #[getter]
    fn color(&self) -> Option<f64> {
        self.0.get(core::Greek::Color)
    }
    #[getter]
    fn vega(&self) -> Option<f64> {
        self.0.get(core::Greek::Vega)
    }
    #[getter]
    fn vanna(&self) -> Option<f64> {
        self.0.get(core::Greek::Vanna)
    }
    #[getter]
    fn zomma(&self) -> Option<f64> {
        self.0.get(core::Greek::Zomma)
    }
    #[getter]
    fn rho(&self) -> Option<f64> {
        self.0.get(core::Greek::Rho)
    }
    fn require(&self, py: Python<'_>, measure: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(py, self.0.require(greek(measure)?))
    }
}

mod coupon;
mod implied;
pub(crate) use coupon::implied_coupon;
pub(crate) use implied::implied_volatility;
