use pyo3::exceptions::PyException;
use pyo3::prelude::*;

pyo3::create_exception!(
    kiyosi._native,
    KiyosiError,
    PyException,
    "Base exception for structured domain failures."
);
pyo3::create_exception!(
    kiyosi._native,
    InvalidInputError,
    KiyosiError,
    "Invalid parameters, dates, schedules or contract history."
);
pyo3::create_exception!(
    kiyosi._native,
    UnsupportedOperationError,
    KiyosiError,
    "Unsupported pricing combination or unavailable result measure."
);
pyo3::create_exception!(
    kiyosi._native,
    ComputationFailureError,
    KiyosiError,
    "Non-finite numerical result or unavailable computation state."
);
pyo3::create_exception!(
    kiyosi._native,
    RootNotBracketedError,
    KiyosiError,
    "Supplied interval does not bracket a root in the engine's domain."
);
pyo3::create_exception!(
    kiyosi._native,
    NonConvergenceError,
    KiyosiError,
    "Iteration or integration limit reached without convergence."
);

pub(crate) fn domain<T>(_py: Python<'_>, result: kiyosi::Result<T>) -> PyResult<T> {
    result.map_err(|error| match error {
        kiyosi::Error::InvalidInput(message) => InvalidInputError::new_err(message.into_owned()),
        kiyosi::Error::UnsupportedOperation(message) => {
            UnsupportedOperationError::new_err(message.into_owned())
        }
        kiyosi::Error::ComputationFailure(message) => {
            ComputationFailureError::new_err(message.into_owned())
        }
        kiyosi::Error::RootNotBracketed(message) => {
            RootNotBracketedError::new_err(message.into_owned())
        }
        kiyosi::Error::NonConvergence(message) => {
            NonConvergenceError::new_err(message.into_owned())
        }
        kiyosi::Error::Io(error) => error.into(),
    })
}
