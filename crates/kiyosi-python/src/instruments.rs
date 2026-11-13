macro_rules! monitoring_properties {
    // Signature order differs from conversion order to preserve boundary failures.
    ($name:ident,
     @replace $changes:ident [$($parameter:ident),+] |$slf:ident|
     {$($field:ident: $value:expr),+ $(,)?}
     [$(#[$replacement_doc:meta])*],
     $($extra:item),* $(,)?) => {
        monitoring_properties!(
            $name,
            $(#[$replacement_doc])*
            #[pyo3(signature = (*, $($parameter=crate::conversions::OMITTED),+))]
            #[expect(clippy::too_many_arguments, reason = "explicit Python replacement fields")]
            fn replace(
                &$slf,
                $(#[pyo3(from_py_with = crate::conversions::supplied)]
                    $parameter: Option<&pyo3::Bound<'_, pyo3::PyAny>>),+
            ) -> pyo3::PyResult<Self> {
                let changes = kiyosi::$changes {
                    $($field: $value),+
                };
                Ok(Self($crate::error::domain($slf.0.replace(changes))?))
            },
            $($extra),*
        );
    };
    ($name:ident, $($extra:item),* $(,)?) => {
        #[pyo3::pymethods]
        impl $name {
            $($extra)*

            fn __repr__(slf: &pyo3::Bound<'_, Self>) -> pyo3::PyResult<String> {
                let value = slf.borrow();
                let py = slf.py();
                crate::representation::format(slf.as_any(), [
                    ("calendar", crate::representation::calendar_preview(py, value.0.calendar())?),
                    ("observation_times", value.0.observation_times().map(|times| crate::representation::times(py, times)).transpose()?),
                ])
            }

            /// Return an owned copy of the contract's observation calendar.
            #[getter]
            fn calendar(&self) -> $crate::calendar::TradingCalendar {
                $crate::calendar::TradingCalendar(self.0.calendar().clone())
            }
            /// Return the inclusive contract start.
            #[getter]
            fn effective_time<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyAny>> {
                $crate::conversions::python_timestamp(py, self.0.effective_time())
            }
            /// Return the inclusive expiry time.
            #[getter]
            fn expiry_time<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyAny>> {
                $crate::conversions::python_timestamp(py, self.0.expiry_time())
            }
            /// Return continuous monitoring or observations on the supplied times.
            #[getter]
            fn observation_mode(&self) -> &'static str {
                match self.0.observation_mode() {
                    kiyosi::ObservationMode::Continuous => "continuous",
                    kiyosi::ObservationMode::Scheduled => "scheduled",
                }
            }
            /// Returns an owned schedule, or None for continuous monitoring.
            #[getter]
            fn observation_schedule(&self) -> Option<$crate::schedule::ObservationSchedule> {
                self.0.observation_schedule().cloned().map($crate::schedule::ObservationSchedule)
            }
            /// Return a copy of the contractual observation times.
            #[getter]
            fn observation_times<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<Option<pyo3::Bound<'py, pyo3::types::PyTuple>>> {
                self.0.observation_times().map(|times| {
                    $crate::conversions::python_timestamps(py, times)
                }).transpose()
            }
            /// Return strictly prior touch history, or None if unknown.
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
