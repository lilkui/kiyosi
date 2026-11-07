use crate::conversions::{direction, python_timestamp, real, supplied, timestamp};
use crate::error::domain;
use pyo3::prelude::*;

fn history(value: &Bound<'_, PyAny>) -> PyResult<Option<f64>> {
    if value.is_none() {
        Ok(None)
    } else {
        real(value).map(Some)
    }
}

macro_rules! average_option {
    ($name:ident) => {
        /// Continuously monitored average-price Asian option with explicit history.
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
        ///     at or before averaging starts, and supplied strictly after it;
        ///     time consistency is checked at pricing.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If strike, realized_average, life times or the averaging window are invalid.
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, PartialEq)]
        pub(crate) struct $name(pub kiyosi::$name);

        #[pymethods]
        impl $name {
            /// Return the timestamp through which supplied history is known, or None.
            #[getter]
            fn history_time<'py>(&self, py: pyo3::Python<'py>) -> pyo3::PyResult<Option<pyo3::Bound<'py, pyo3::PyAny>>> {
                kiyosi::OptionContract::history_time(&self.0).map(|time| crate::conversions::python_timestamp(py, time)).transpose()
            }

    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// averaging_start_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// realized_average : float or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    ///
    /// Returns
    /// -------
    /// object
    ///     Independent contract of the same class, preserving omitted history and terms.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation; the original is unchanged.
    /// TypeError
    ///     If a representation or field name is invalid.
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, averaging_start_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, realized_average=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "explicit Python replacement fields")]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] averaging_start_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] realized_average: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?
            .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

        let changes = kiyosi::AveragePriceOptionChanges {
            history_time: Some(history_time),
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
            averaging_start_time: averaging_start_time.map(crate::conversions::timestamp).transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            realized_average: realized_average.map(history).transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
            #[new]
            #[pyo3(signature = (*, option_type, strike, effective_time, averaging_start_time, expiry_time, realized_average=kiyosi::DEFAULT_REALIZED_AVERAGE, history_time=None))]
            fn new(option_type: &Bound<'_, PyAny>, strike: &Bound<'_, PyAny>,
                effective_time: &Bound<'_, PyAny>, averaging_start_time: &Bound<'_, PyAny>,
                expiry_time: &Bound<'_, PyAny>, #[pyo3(from_py_with = history)] realized_average: Option<f64>,
                #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<kiyosi::Timestamp>,
            ) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::OptionContract::with_history_time(domain(kiyosi::$name::new(direction(option_type)?, real(strike)?,
                    timestamp(effective_time)?, timestamp(averaging_start_time)?, timestamp(expiry_time)?, realized_average))?, history_time))?))
            }
            /// Return a validated replacement without changing this value.
            ///
            /// Parameters
            /// ----------
            /// realized_average : float or None, optional
            ///     Prior average; None restores unknown history. Pricing checks time
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
            #[pyo3(signature = (*, realized_average=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
            fn replace_history(&self,
                #[pyo3(from_py_with = supplied)] realized_average: Option<&Bound<'_, PyAny>>,

                #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
            ) -> PyResult<Self> {
                let history_time = history_time
                    .map(crate::conversions::optional_timestamp)
                    .transpose()?
                    .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

                let realized = realized_average.map(history).transpose()?.unwrap_or(self.0.realized_average());
                Ok(Self(domain(kiyosi::OptionContract::with_history_time(domain(self.0.replace_history(realized))?, history_time))?))
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
