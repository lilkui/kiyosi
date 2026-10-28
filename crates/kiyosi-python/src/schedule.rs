use crate::calendar::TradingCalendar;
use crate::conversions::{date, integer, python_date};
use crate::{core, error::domain};
use pyo3::exceptions::PyIndexError;
use pyo3::prelude::*;
use pyo3::types::PyDate;

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct ObservationSchedule(pub(crate) core::ObservationSchedule);

#[pymethods]
impl ObservationSchedule {
    #[getter]
    fn dates<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDate>>> {
        self.0
            .dates()
            .iter()
            .map(|&date| python_date(py, date))
            .collect()
    }
    fn __len__(&self) -> usize {
        self.0.len()
    }
    fn __getitem__<'py>(&self, py: Python<'py>, index: isize) -> PyResult<Bound<'py, PyDate>> {
        let index = if index < 0 {
            index + self.0.len() as isize
        } else {
            index
        };
        let date = self
            .0
            .dates()
            .get(index as usize)
            .ok_or_else(|| PyIndexError::new_err("schedule index out of range"))?;
        python_date(py, *date)
    }
    fn __iter__(slf: PyRef<'_, Self>) -> ScheduleIterator {
        ScheduleIterator {
            owner: slf.into(),
            index: 0,
        }
    }
}

#[pyclass]
struct ScheduleIterator {
    owner: Py<ObservationSchedule>,
    index: usize,
}

#[pymethods]
impl ScheduleIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDate>>> {
        let date = self.owner.borrow(py).0.dates().get(self.index).copied();
        if date.is_some() {
            self.index += 1;
        }
        date.map(|date| python_date(py, date)).transpose()
    }
}

#[pyfunction]
#[pyo3(signature = (*, start, end, interval_days, calendar=TradingCalendar::default()))]
pub(crate) fn fixed_interval_schedule(
    py: Python<'_>,
    start: &Bound<'_, PyAny>,
    end: &Bound<'_, PyAny>,
    interval_days: &Bound<'_, PyAny>,
    calendar: TradingCalendar,
) -> PyResult<ObservationSchedule> {
    Ok(ObservationSchedule(domain(
        py,
        core::ObservationSchedule::fixed_interval(
            date(start)?,
            date(end)?,
            i64::from(integer(interval_days)?),
            &calendar.0,
        ),
    )?))
}

#[pyfunction]
#[pyo3(signature = (*, start, end, first_observation_month, calendar=TradingCalendar::default()))]
pub(crate) fn monthly_schedule(
    py: Python<'_>,
    start: &Bound<'_, PyAny>,
    end: &Bound<'_, PyAny>,
    first_observation_month: &Bound<'_, PyAny>,
    calendar: TradingCalendar,
) -> PyResult<ObservationSchedule> {
    Ok(ObservationSchedule(domain(
        py,
        core::ObservationSchedule::monthly(
            date(start)?,
            date(end)?,
            integer(first_observation_month)?,
            &calendar.0,
        ),
    )?))
}
