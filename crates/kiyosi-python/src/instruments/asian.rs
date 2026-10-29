use crate::conversions::{check_changes, date, direction, python_date, real};
use crate::error::domain;
use kiyosi_core as core;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

macro_rules! average_option {
    ($name:ident) => {
        /// Continuous average-price option with explicit history.
        ///
        /// Parameters
        /// ----------
        /// option_type : {'call', 'put'}
        ///     Call or put payoff direction.
        /// strike : float
        ///     Positive finite exercise or purchase price.
        /// effective_date : datetime.date
        ///     Inclusive first contract date.
        /// averaging_start_date : datetime.date
        ///     First averaging date, within the contract life.
        /// expiry_date : datetime.date
        ///     Final contract date; expiry is at midnight UTC.
        /// realized_average : float, optional
        ///     Prior average; zero denotes no history. Pricing checks time
        ///     consistency.
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub core::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, option_type, strike, effective_date, averaging_start_date, expiry_date, realized_average=core::DEFAULT_REALIZED_AVERAGE))]
            fn new(py: Python<'_>, option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                effective_date: &Bound<'_, PyAny>, averaging_start_date: &Bound<'_, PyAny>,
                expiry_date: &Bound<'_, PyAny>, #[pyo3(from_py_with = real)] realized_average: f64) -> PyResult<Self> {
                Ok(Self(domain(py, core::$name::new(direction(option_type)?, real(strike)?,
                    date(effective_date)?, date(averaging_start_date)?, date(expiry_date)?, realized_average))?))
            }
            /// Return a validated replacement without changing this value.
            ///
            /// Parameters
            /// ----------
            /// realized_average : float, optional
            ///     Prior average; zero denotes no history. Pricing checks time
            ///     consistency.
            ///
            /// Returns
            /// -------
            /// object
            ///     New instance of the same class; omitted fields retain their values.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If replacement terms fail domain validation. The original is unchanged.
            #[pyo3(signature = (**fields))]
            fn replace(&self, py: Python<'_>, fields: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                check_changes(fields, &["realized_average"])?;
                let realized = fields.map(|f| f.get_item("realized_average")).transpose()?.flatten()
                    .as_ref().map(real).transpose()?.unwrap_or(self.0.realized_average());
                Ok(Self(domain(py, self.0.replace(realized))?))
            }
            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                if self.0.option_type() == core::OptionType::Call { "call" } else { "put" }
            }
            /// Return the exercise or purchase price in underlying price units.
            #[getter]
            fn strike(&self) -> f64 { self.0.strike() }
            /// Return the supplied historical average; zero denotes no history.
            #[getter]
            fn realized_average(&self) -> f64 { self.0.realized_average() }
            /// Return inclusive first contract date.
            #[getter]
            fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> { python_date(py, self.0.effective_date()) }
            /// Return first averaging date, within the contract life.
            #[getter]
            fn averaging_start_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> { python_date(py, self.0.averaging_start_date()) }
            /// Return final contract date; expiry is at midnight UTC.
            #[getter]
            fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> { python_date(py, self.0.expiry_date()) }
        }
    };
}
average_option!(GeometricAveragePriceOption);
average_option!(ArithmeticAveragePriceOption);

#[derive(FromPyObject)]
pub(crate) enum AsianInstrument<'py> {
    Geometric(PyRef<'py, GeometricAveragePriceOption>),
    Arithmetic(PyRef<'py, ArithmeticAveragePriceOption>),
}
