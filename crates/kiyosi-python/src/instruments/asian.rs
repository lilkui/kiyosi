use crate::conversions::{direction, optional_real, python_timestamp, real, timestamp};
use crate::error::domain;
use pyo3::prelude::*;

macro_rules! average_option {
    ($name:ident) => {
        /// Continuously monitored average-price Asian option.
        ///
        /// Parameters
        /// ----------
        /// option_type : {'call', 'put'}
        ///     Call or put payoff direction.
        /// strike : float
        ///     Positive finite strike price.
        /// effective_time : datetime.datetime
        ///     Inclusive contract start.
        /// averaging_start_time : datetime.datetime
        ///     First averaging timestamp, within the contract life.
        /// expiry_time : datetime.datetime
        ///     Inclusive expiry time.
        /// realized_average : float or None, optional
        ///     Positive finite prior average; defaults to None (unknown). Must be None
        ///     at or before averaging starts, and supplied strictly after it; time
        ///     consistency is checked at pricing. History covers averaging strictly
        ///     before valuation; callers supply the applicable average.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If strike, realized_average, life times or the averaging window are
        ///     invalid.
        ///
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub kiyosi::$name);

        #[pymethods]
        impl $name {
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Call or put payoff direction.
    /// strike : float, optional
    ///     Positive finite strike price.
    /// effective_time, averaging_start_time : datetime.datetime, optional
    ///     Contract start and first averaging timestamp, respectively.
    /// expiry_time : datetime.datetime, optional
    ///     Inclusive expiry timestamp.
    /// realized_average : float or None, optional
    ///     Omission or Ellipsis retains the average; None clears it to unknown.
    ///
    /// Returns
    /// -------
    #[doc = stringify!($name)]
    ///     Independent contract of the same class, preserving omitted history and
    ///     terms.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation.
    /// TypeError
    ///     If a representation or field name is invalid.
    ///
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, averaging_start_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, realized_average=crate::conversions::OMITTED))]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] averaging_start_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] realized_average: Option<&Bound<'_, PyAny>>,

    ) -> PyResult<Self> {

        let changes = kiyosi::AveragePriceOptionChanges {
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
            averaging_start_time: averaging_start_time.map(crate::conversions::timestamp).transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            realized_average: realized_average.map(optional_real).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
            #[new]
            #[pyo3(signature = (*, option_type, strike, effective_time, averaging_start_time, expiry_time, realized_average=kiyosi::DEFAULT_REALIZED_AVERAGE))]
            fn new(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                effective_time: &Bound<'_, PyAny>, averaging_start_time: &Bound<'_, PyAny>,
                expiry_time: &Bound<'_, PyAny>, #[pyo3(from_py_with = optional_real)] realized_average: Option<f64>,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::$name::new(direction(option_type)?, real(strike)?,
                    timestamp(effective_time)?, timestamp(averaging_start_time)?, timestamp(expiry_time)?, realized_average))?))
            }
            /// Return call or put payoff direction.
            #[getter]
            fn option_type(&self) -> &'static str {
                if self.0.option_type() == kiyosi::OptionType::Call { "call" } else { "put" }
            }
            /// Return the strike price in underlying price units.
            #[getter]
            fn strike(&self) -> f64 { self.0.strike() }
            /// Return the supplied historical average, or None if unknown.
            #[getter]
            fn realized_average(&self) -> Option<f64> { self.0.realized_average() }
            /// Return inclusive first contract timestamp.
            #[getter]
            fn effective_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> { python_timestamp(py, self.0.effective_time()) }
            /// Return first averaging timestamp, within the contract life.
            #[getter]
            fn averaging_start_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> { python_timestamp(py, self.0.averaging_start_time()) }
            /// Return inclusive expiry time.
            #[getter]
            fn expiry_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> { python_timestamp(py, self.0.expiry_time()) }
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
