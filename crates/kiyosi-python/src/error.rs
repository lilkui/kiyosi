use crate::core;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

pyo3::create_exception!(_native, KiyosiError, PyException);

pub(crate) fn domain<T>(py: Python<'_>, result: core::Result<T>) -> PyResult<T> {
    result.map_err(|error| {
        if let Some(native) = error.operational_error() {
            return match native.raw_os_error() {
                Some(code) => std::io::Error::from_raw_os_error(code).into(),
                None => std::io::Error::new(native.kind(), native.to_string()).into(),
            };
        }
        let exception = KiyosiError::new_err(error.message);
        let category = py
            .import("kiyosi._native")
            .and_then(|module| module.getattr("ErrorCategory"))
            .and_then(|enum_type| enum_type.getattr(error.category.name()));
        match category.and_then(|category| exception.value(py).setattr("category", category)) {
            Ok(()) => exception,
            Err(mapping_error) => mapping_error,
        }
    })
}
