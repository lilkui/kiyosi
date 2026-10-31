macro_rules! monitoring_properties {
    ($name:ident, $($extra:item),* $(,)?) => {
        #[pyo3::pymethods]
        impl $name {
            $($extra)*
            /// Returns a validated replacement without changing this value.
            ///
            /// Parameters
            /// ----------
            /// touch_state : {'untouched', 'touched'} or None, optional
            ///     Strictly prior history. None means unknown; untouched means known no
            ///     hit.
            ///
            /// Returns
            /// -------
            /// object
            ///     New instance of the same class; omitted fields or Ellipsis retain their values.
            ///
            /// Raises
            /// ------
            /// TypeError
            ///     If a field is unknown or touch_state is not a string, None or Ellipsis.
            /// ValueError
            ///     If touch_state is an unknown choice.
            #[pyo3(signature = (*, touch_state=crate::conversions::OMITTED))]
            fn replace(
                &self,
                #[pyo3(from_py_with = crate::conversions::supplied)] touch_state: Option<
                    &pyo3::Bound<'_, pyo3::PyAny>,
                >,
            ) -> pyo3::PyResult<Self> {
                let state = match touch_state {
                    Some(value) => $crate::instruments::barrier::history(value)?,
                    None => self.0.touch_state(),
                };
                Ok(Self(self.0.replace(state)))
            }
            /// Returns inclusive first contract date.
            #[getter]
            fn effective_date<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyDate>> {
                $crate::conversions::python_date(py, self.0.effective_date())
            }
            /// Returns final contract date; expiry is at midnight UTC.
            #[getter]
            fn expiry_date<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyDate>> {
                $crate::conversions::python_date(py, self.0.expiry_date())
            }
            /// Returns continuous monitoring or observations on the supplied dates.
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
            /// Returns a copy of the contractual observation dates.
            #[getter]
            fn observation_dates<'py>(
                &self,
                py: pyo3::Python<'py>,
            ) -> pyo3::PyResult<Vec<pyo3::Bound<'py, pyo3::types::PyDate>>> {
                self.0
                    .observation_dates()
                    .iter()
                    .map(|&d| $crate::conversions::python_date(py, d))
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
