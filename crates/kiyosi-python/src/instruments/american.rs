use super::vanilla::vanilla_option_methods;
use crate::conversions::{direction, python_timestamp, real, supplied, timestamp};
use crate::error::domain;
use pyo3::prelude::*;

/// Vanilla option exercisable throughout its inclusive contract life.
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
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AmericanOption(pub(crate) kiyosi::AmericanOption);

vanilla_option_methods!(AmericanOption, AmericanOptionChanges);
