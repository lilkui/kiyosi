use kiyosi_core as core;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

pyo3::create_exception!(kiyosi._native, KiyosiError, PyException);
pyo3::create_exception!(kiyosi._native, InvalidInputError, KiyosiError);
pyo3::create_exception!(kiyosi._native, UnsupportedOperationError, KiyosiError);
pyo3::create_exception!(kiyosi._native, ComputationFailureError, KiyosiError);
pyo3::create_exception!(kiyosi._native, RootNotBracketedError, KiyosiError);
pyo3::create_exception!(kiyosi._native, NonConvergenceError, KiyosiError);

pub(crate) fn domain<T>(_py: Python<'_>, result: core::Result<T>) -> PyResult<T> {
    result.map_err(|error| match error {
        core::Error::InvalidInput(message) => InvalidInputError::new_err(message.into_owned()),
        core::Error::UnsupportedOperation(message) => {
            UnsupportedOperationError::new_err(message.into_owned())
        }
        core::Error::ComputationFailure(message) => {
            ComputationFailureError::new_err(message.into_owned())
        }
        core::Error::RootNotBracketed(message) => {
            RootNotBracketedError::new_err(message.into_owned())
        }
        core::Error::NonConvergence(message) => NonConvergenceError::new_err(message.into_owned()),
        core::Error::Io(error) => error.into(),
    })
}
