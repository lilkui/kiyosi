//! Python representation adapters; all domain validation lives in kiyosi.

#![expect(
    clippy::doc_markdown,
    reason = "Python docstrings use NumPy conventions rather than Rust Markdown"
)]
#![allow(
    clippy::used_underscore_items,
    reason = "PyO3-generated code references intentionally private Python names"
)]

// Expand replacement methods before PyO3 parses the complete implementation.
macro_rules! value_methods {
    ($name:ident, $changes:ident, [$first_parameter:ident $(, $parameter:ident)*],
     {$($field:ident: $value:expr),+ $(,)?}, {$($method:item)*}
     $(, $py:ident => $replace:path)?) => {
        value_methods!(@documented $name, $changes, [$first_parameter $(, $parameter)*],
            {$($field: $value),+}, [
            /// Return a validated copy with replacement fields.
            ///
            /// Parameters
            /// ----------
            #[doc = concat!(stringify!($first_parameter) $(, ", ", stringify!($parameter))*, " : object, optional")]
            ///     Constructor fields to replace; see the class documentation for field
            ///     descriptions. Omitted fields and fields set to Ellipsis default to
            ///     the instance's values. None clears nullable fields.
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
        ], {$($method)*} $(, $py => $replace)?);
    };
    (@documented $name:ident, $changes:ident, [$($parameter:ident),+],
     {$($field:ident: $value:expr),+ $(,)?}, [$(#[$doc:meta])*], {$($method:item)*}
     $(, $py:ident => $replace:path)?) => {
        value_methods!(@documented $name, $changes, [$($parameter),+], |self|
            {$($field: $value),+}, [$(#[$doc])*], {$($method)*} $(, $py => $replace)?);
    };
    (@documented $name:ident, $changes:ident, [$($parameter:ident),+], |$slf:ident|
     {$($field:ident: $value:expr),+ $(,)?}, [$(#[$doc:meta])*], {$($method:item)*}
     $(, $py:ident => $replace:path)?) => {
        #[pyo3::pymethods]
        impl $name {
            #[classattr]
            const _STATE_FIELDS: &'static [&'static str] = &[$(stringify!($parameter)),+];

            $(#[$doc])*
            #[pyo3(signature = (*, $($parameter=crate::conversions::OMITTED),+))]
            #[allow(clippy::too_many_arguments, reason = "replacement field counts vary by expansion")]
            fn replace(
                &$slf,
                $($py: pyo3::Python<'_>,)?
                $(#[pyo3(from_py_with = crate::conversions::supplied)]
                    $parameter: Option<&pyo3::Bound<'_, pyo3::PyAny>>),+
            ) -> pyo3::PyResult<Self> {
                let changes = kiyosi::$changes { $($field: $value),+ };
                value_methods!(@apply $slf, changes $(, $py => $replace)?)
            }

            $($method)*
        }
    };
    (@apply $slf:ident, $changes:ident) => {
        Ok(Self($crate::error::domain($slf.0.replace($changes))?))
    };
    (@apply $slf:ident, $changes:ident, $py:ident => $replace:path) => {
        $replace($slf, $py, $changes)
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
