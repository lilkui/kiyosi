use crate::conversions::{direction, python_timestamp, real, timestamp};
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
/// terminal payoff. Model data must cover valuation through expiry. Valuation
/// after expiry is invalid.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct EuropeanOption(pub(crate) kiyosi::EuropeanOption);

macro_rules! vanilla_option_methods {
    ($name:ident, $changes:ident) => {
        value_methods!($name, $changes,
            [option_type, strike, effective_time, expiry_time], {
                option_type: option_type.map(crate::conversions::direction).transpose()?,
                strike: strike.map(crate::conversions::real).transpose()?,
                effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
                expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            }, {
            #[new]
            #[pyo3(signature = (*, option_type, strike, effective_time, expiry_time))]
            fn new(
                option_type: &Bound<'_, PyAny>,
                strike: &Bound<'_, PyAny>,
                effective_time: &Bound<'_, PyAny>,
                expiry_time: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                let direction = direction(option_type)?;
                Ok(Self(domain(kiyosi::$name::new(
                    direction,
                    real(strike)?,
                    timestamp(effective_time)?,
                    timestamp(expiry_time)?,
                ))?))
            }

            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                crate::conversions::direction_name(self.0.option_type())
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
        });
    };
}
pub(super) use vanilla_option_methods;

vanilla_option_methods!(EuropeanOption, EuropeanOptionChanges);
