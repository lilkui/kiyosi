macro_rules! monitoring_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        #[pyo3::pymethods]
        impl $name {
            $($extra)*
            /// Return the timestamp through which supplied history is known, or None.
            #[getter]
            fn history_time<'py>(&self, py: pyo3::Python<'py>) -> pyo3::PyResult<Option<pyo3::Bound<'py, pyo3::PyAny>>> {
                kiyosi::OptionContract::history_time(&self.0).map(|time| crate::conversions::python_timestamp(py, time)).transpose()
            }

            /// Returns an owned copy of the contract's observation calendar.
            #[getter]
            fn calendar(&self) -> $crate::calendar::TradingCalendar {
                $crate::calendar::TradingCalendar(self.0.calendar().clone())
            }
            /// Returns a validated replacement without changing this value.
            ///
            /// Clearing the final history payload clears an omitted history_time.
            /// An explicitly supplied history_time is validated and retained.
            ///
            /// Parameters
            /// ----------
            /// touch_state : {'untouched', 'touched'} or None, optional
            ///     Strictly prior history. None means unknown; untouched means known no
            ///     hit.
            ///
            /// history_time : datetime.datetime or None, optional
            ///     Timestamp of supplied history, covering events strictly before it.
            ///     Required at pricing whenever history is supplied. Refresh the history
            ///     payload and timestamp together.
            ///
            /// Returns
            /// -------
            /// object
            ///     New instance of the same class; omitted fields or Ellipsis retain their values.
            ///
            /// Raises
            /// ------
            /// InvalidInputError
            ///     If history_time falls outside the contract life.
            /// TypeError
            ///     If a field is unknown or touch_state is not a string, None or Ellipsis.
            /// ValueError
            ///     If touch_state is an unknown choice.
            #[pyo3(signature = (*, touch_state=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
            fn replace_history(
                &self,
                #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<
                    &pyo3::Bound<'_, pyo3::PyAny>,
                >,

                #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<&pyo3::Bound<'_, pyo3::PyAny>>,
            ) -> pyo3::PyResult<Self> {
                let history_time = history_time
                    .map(crate::conversions::optional_timestamp)
                    .transpose()?;

                let updated = match touch_state {
                    Some(value) => {
                        let state = $crate::instruments::barrier::history(value)?;
                        let at = state.and(kiyosi::OptionContract::history_time(&self.0));
                        $crate::error::domain(self.0.replace_history(state, at))?
                    },
                    None => self.0.clone(),
                };
                let history_time = history_time.unwrap_or(kiyosi::OptionContract::history_time(&updated));
                Ok(Self(crate::error::domain(kiyosi::OptionContract::with_history_time(updated, history_time))?))
            }
            /// Returns the inclusive contract start.
            #[getter]
            fn effective_time<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyAny>> {
                $crate::conversions::python_timestamp(py, self.0.effective_time())
            }
            /// Returns the inclusive expiry time.
            #[getter]
            fn expiry_time<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyAny>> {
                $crate::conversions::python_timestamp(py, self.0.expiry_time())
            }
            /// Returns continuous monitoring or observations on the supplied times.
            #[getter]
            fn observation_mode(&self) -> &'static str {
                match self.0.observation_mode() {
                    kiyosi::ObservationMode::Continuous => "continuous",
                    kiyosi::ObservationMode::Scheduled => "scheduled",
                }
            }
            /// Returns an owned copy of the monitoring schedule.
            #[getter]
            fn observation_schedule(&self) -> $crate::schedule::ObservationSchedule {
                $crate::schedule::ObservationSchedule(self.0.observation_schedule().clone())
            }
            /// Returns a copy of the contractual observation times.
            #[getter]
            fn observation_times<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<Vec<pyo3::Bound<'py, pyo3::types::PyAny>>> {
                self.0
                    .observation_times()
                    .iter()
                    .map(|&d| $crate::conversions::python_timestamp(py, d))
                    .collect()
            }
            /// Returns strictly prior touch history, or None if unknown.
            #[getter]
            fn touch_state(&self) -> Option<&'static str> {
                self.0.touch_state().map(|s| match s {
                    kiyosi::BarrierTouchState::Untouched => "untouched",
                    kiyosi::BarrierTouchState::Touched => "touched",
                })
            }
        }
    };
}

pub(crate) mod accumulator;
pub(crate) mod american;
pub(crate) mod asian;
pub(crate) mod autocallable;
pub(crate) mod barrier;
pub(crate) mod binary_barrier;
pub(crate) mod digital;
pub(crate) mod double_barrier;
pub(crate) mod double_touch;
pub(crate) mod range_accrual;
pub(crate) mod vanilla;
