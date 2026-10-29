use crate::conversions::{check_changes, date, direction, python_date, real};
use crate::error::domain;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

/// Vanilla option exercisable throughout its inclusive contract life.
///
/// Parameters
/// ----------
/// option_type : {'call', 'put'}
///     Call or put payoff direction.
/// strike : float
///     Positive finite exercise or purchase price.
/// effective_date : datetime.date
///     Inclusive first contract date.
/// expiry_date : datetime.date
///     Final contract date; expiry is at midnight UTC.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AmericanOption(pub(crate) core::AmericanOption);

#[pymethods]
impl AmericanOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date))]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let direction = direction(option_type)?;
        Ok(Self(domain(
            py,
            core::AmericanOption::new(
                direction,
                real(strike)?,
                date(effective_date)?,
                date(expiry_date)?,
            ),
        )?))
    }

    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Call or put payoff direction.
    /// strike : float, optional
    ///     Positive finite exercise or purchase price.
    /// effective_date : datetime.date, optional
    ///     Inclusive first contract date.
    /// expiry_date : datetime.date, optional
    ///     Final contract date; expiry is at midnight UTC.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (**fields))]
    fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            fields,
            &["option_type", "strike", "effective_date", "expiry_date"],
        )?;
        let item = |name| {
            fields
                .map(|f| f.get_item(name))
                .transpose()
                .map(Option::flatten)
        };
        let changes = core::AmericanOptionChanges {
            option_type: item("option_type")?.as_ref().map(direction).transpose()?,
            strike: item("strike")?.as_ref().map(real).transpose()?,
            effective_date: item("effective_date")?.as_ref().map(date).transpose()?,
            expiry_date: item("expiry_date")?.as_ref().map(date).transpose()?,
        };
        Ok(Self(domain(py, self.0.replace(changes))?))
    }

    /// Return call or put payoff direction.
    #[getter]
    fn option_type(&self) -> &'static str {
        match self.0.option_type() {
            core::OptionType::Call => "call",
            core::OptionType::Put => "put",
        }
    }
    /// Return the exercise or purchase price in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    /// Return inclusive first contract date.
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    /// Return final contract date; expiry is at midnight UTC.
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
}
