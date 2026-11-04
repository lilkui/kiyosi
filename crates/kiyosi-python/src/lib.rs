//! Python representation adapters; all domain validation lives in kiyosi.

#![expect(
    clippy::doc_markdown,
    reason = "Python docstrings use NumPy conventions rather than Rust Markdown"
)]

mod calendar;
mod conversions;
mod engines;
mod error;
mod heston;
mod instruments;
mod local_volatility;
mod market;
mod registration;
mod representation;
mod risk;
mod schedule;
mod signatures;

use pyo3::prelude::*;

#[pymodule]
fn _native(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    registration::register(py, module)
}
