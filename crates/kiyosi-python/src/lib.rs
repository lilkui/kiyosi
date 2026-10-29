//! Python representation adapters; all domain validation lives in kiyosi-core.

mod calendar;
mod conversions;
mod engines;
mod error;
mod instruments;
mod market;
mod registration;
mod risk;
mod schedule;

use pyo3::prelude::*;

#[pymodule]
fn _native(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    registration::register(py, module)
}
