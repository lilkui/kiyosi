use crate::conversions::{direction, python_timestamp, real, supplied, timestamp};
use crate::error::domain;
use pyo3::prelude::*;

/// European vanilla option.
///
/// Parameters
/// ----------
/// option_type : {'call', 'put'}
///     Call or put payoff direction.
/// strike : float
///     Positive finite strike price.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
///
/// Raises
/// ------
/// InvalidInputError
///     If strike is nonpositive or non-finite, or expiry precedes
///     effective_time.
///
/// Notes
/// -----
/// Pricing permits valuation before effective_time for this fixed-strike
/// terminal payoff. Model data must cover valuation through expiry; valuation
/// after expiry remains invalid. Other contract families retain their own
/// valuation bounds.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct EuropeanOption(pub(crate) kiyosi::EuropeanOption);

#[pymethods]
impl EuropeanOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time))]
    fn new(
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let direction = direction(option_type)?;
        Ok(Self(domain(kiyosi::EuropeanOption::new(
            direction,
            real(strike)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
        ))?))
    }

    /// Return a validated replacement.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Call or put payoff direction.
    /// strike : float, optional
    ///     Positive finite strike price.
    /// effective_time : datetime.datetime, optional
    ///     Inclusive contract start.
    /// expiry_time : datetime.datetime, optional
    ///     Inclusive expiry time.
    ///
    /// Returns
    /// -------
    /// object
    ///     Validated copy; omitted fields or Ellipsis retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation.
    ///
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED))]
    fn replace(
        &self,
        #[pyo3(from_py_with = supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] effective_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::EuropeanOptionChanges {
            option_type: option_type.map(direction).transpose()?,
            strike: strike.map(real).transpose()?,
            effective_time: effective_time.map(timestamp).transpose()?,
            expiry_time: expiry_time.map(timestamp).transpose()?,
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
    /// Return the strike price in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    /// Return inclusive first contract timestamp.
    #[getter]
    fn effective_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.effective_time())
    }
    /// Return inclusive expiry time.
    #[getter]
    fn expiry_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.expiry_time())
    }
}
