use crate::conversions::{date, direction, python_date, real, supplied};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::PyDate;

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
///
/// Raises
/// ------
/// InvalidInputError
///     If strike is nonpositive or non-finite, or expiry precedes effective_date.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AmericanOption(pub(crate) kiyosi::AmericanOption);

#[pymethods]
impl AmericanOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date))]
    fn new(
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let direction = direction(option_type)?;
        Ok(Self(domain(kiyosi::AmericanOption::new(
            direction,
            real(strike)?,
            date(effective_date)?,
            date(expiry_date)?,
        ))?))
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
    #[pyo3(signature = (*, option_type=None, strike=None, effective_date=None, expiry_date=None))]
    fn replace(
        &self,
        #[pyo3(from_py_with = supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] effective_date: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] expiry_date: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::AmericanOptionChanges {
            option_type: option_type.map(direction).transpose()?,
            strike: strike.map(real).transpose()?,
            effective_date: effective_date.map(date).transpose()?,
            expiry_date: expiry_date.map(date).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }

    /// Return call or put payoff direction.
    #[getter]
    fn option_type(&self) -> &'static str {
        match self.0.option_type() {
            kiyosi::OptionType::Call => "call",
            kiyosi::OptionType::Put => "put",
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
