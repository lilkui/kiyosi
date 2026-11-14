//! Python representation adapters; all domain validation lives in kiyosi.

#![expect(
    clippy::doc_markdown,
    reason = "Python docstrings use NumPy conventions rather than Rust Markdown"
)]

// Expand replacement methods before PyO3 parses the complete implementation.
macro_rules! value_methods {
    ($name:ident, $changes:ident, [$($parameter:ident),+],
     {$($field:ident: $value:expr),+ $(,)?}, {$($method:item)*}) => {
        value_methods!(@documented $name, $changes, [$($parameter),+],
            {$($field: $value),+}, [
            /// Return a validated copy with replacement fields.
            ///
            /// The original value is unchanged on success and failure.
            ///
            /// Parameters
            /// ----------
            /// **changes : object, optional
            ///     Constructor fields to replace; see the class documentation for field
            ///     descriptions. Omission or Ellipsis retains a field; None clears
            ///     nullable fields.
            ///
            /// Returns
            /// -------
            /// object
            ///     Validated instance of the same class.
            ///
            /// Raises
            /// ------
            /// InvalidInputError
            ///     If fields fail constructor validation.
            /// TypeError
            ///     If a field name or representation is invalid.
        ], {$($method)*});
    };
    (@documented $name:ident, $changes:ident, [$($parameter:ident),+],
     {$($field:ident: $value:expr),+ $(,)?}, [$(#[$doc:meta])*], {$($method:item)*}) => {
        value_methods!(@documented $name, $changes, [$($parameter),+], |self|
            {$($field: $value),+}, [$(#[$doc])*], {$($method)*});
    };
    (@documented $name:ident, $changes:ident, [$($parameter:ident),+], |$slf:ident|
     {$($field:ident: $value:expr),+ $(,)?}, [$(#[$doc:meta])*], {$($method:item)*}) => {
        #[pyo3::pymethods]
        impl $name {
            #[classattr]
            const _STATE_FIELDS: &'static [&'static str] = &[$(stringify!($parameter)),+];

            $(#[$doc])*
            #[pyo3(signature = (*, $($parameter=crate::conversions::OMITTED),+))]
            #[allow(clippy::too_many_arguments, reason = "replacement field counts vary by expansion")]
            fn replace(
                &$slf,
                $(#[pyo3(from_py_with = crate::conversions::supplied)]
                    $parameter: Option<&pyo3::Bound<'_, pyo3::PyAny>>),+
            ) -> pyo3::PyResult<Self> {
                let changes = kiyosi::$changes { $($field: $value),+ };
                Ok(Self($crate::error::domain($slf.0.replace(changes))?))
            }

            $($method)*
        }
    };
}

mod calendar;
mod cancellation;
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
