use crate::conversions::{date, direction, python_date, real, supplied};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::PyDate;

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
        ///     Nonnegative finite prior average; defaults to zero. Must be zero
        ///     at or before averaging starts, and positive strictly after it;
        ///     time consistency is checked at pricing.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If strike, realized_average, life dates or the averaging window are invalid.
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub kiyosi::$name);

        #[pymethods]
        impl $name {
            #[new]
            #[pyo3(signature = (*, option_type, strike, effective_date, averaging_start_date, expiry_date, realized_average=kiyosi::DEFAULT_REALIZED_AVERAGE))]
            fn new(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                effective_date: &Bound<'_, PyAny>, averaging_start_date: &Bound<'_, PyAny>,
                expiry_date: &Bound<'_, PyAny>, #[pyo3(from_py_with = real)] realized_average: f64) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::$name::new(direction(option_type)?, real(strike)?,
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
            ///     New instance of the same class; omitted fields or Ellipsis retain their values.
            ///
            /// Raises
            /// ------
            /// KiyosiError
            ///     If replacement terms fail domain validation. The original is unchanged.
            #[pyo3(signature = (*, realized_average=crate::conversions::OMITTED))]
            fn replace(&self,
                #[pyo3(from_py_with = supplied)] realized_average: Option<&Bound<'_, PyAny>>,
            ) -> PyResult<Self> {
                let realized = realized_average.map(real).transpose()?.unwrap_or(self.0.realized_average());
                Ok(Self(domain(self.0.replace(realized))?))
            }
            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                if self.0.option_type() == kiyosi::OptionType::Call { "call" } else { "put" }
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

macro_rules! with_asian {
    ($instrument:expr, $option:ident, $body:expr) => {{
        match $instrument {
            $crate::instruments::asian::AsianInstrument::Geometric(option) => {
                let $option = &option.0;
                $body
            }
            $crate::instruments::asian::AsianInstrument::Arithmetic(option) => {
                let $option = &option.0;
                $body
            }
        }
    }};
}
pub(crate) use with_asian;
