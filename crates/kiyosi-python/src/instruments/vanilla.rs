use crate::conversions::{date, direction, python_date, real};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::PyDate;

/// European vanilla option with a positive strike.
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
pub(crate) struct EuropeanOption(pub(crate) kiyosi::EuropeanOption);

#[pymethods]
impl EuropeanOption {
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
            kiyosi::EuropeanOption::new(
                direction,
                real(strike)?,
                date(effective_date)?,
                date(expiry_date)?,
            ),
        )?))
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
