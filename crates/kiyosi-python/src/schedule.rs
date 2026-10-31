use crate::calendar::TradingCalendar;
use crate::conversions::{check_integer, date, integer, python_date};
use crate::error::domain;
use pyo3::exceptions::PyIndexError;
use pyo3::prelude::*;
use pyo3::types::PyDate;

/// Immutable ordered observation dates with sequence access.
///
/// Notes
/// -----
/// Use fixed_interval_schedule or monthly_schedule. The dates property
/// returns a copy; iteration retains the schedule owner. Integer indexing
/// accepts negative indices and raises IndexError outside the sequence.
/// Slices are not supported. len(schedule) counts observations.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct ObservationSchedule(pub(crate) kiyosi::ObservationSchedule);

#[pymethods]
impl ObservationSchedule {
    /// Return a copy of the strictly ordered observation dates.
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

/// Creates adjusted observations at positive fixed calendar-day intervals.
///
/// Parameters
/// ----------
/// start : datetime.date
///     First date of the interval.
/// end : datetime.date
///     Last date of the interval.
/// interval_days : int
///     Positive calendar-day spacing of unadjusted observations, up to 2**63 - 1.
/// calendar : TradingCalendar, optional
///     Trading calendar. Defaults to weekdays with a 252-day basis.
///
/// Returns
/// -------
/// ObservationSchedule
///     Owned result; no input is mutated.
///
/// Raises
/// ------
/// InvalidInputError
///     If end precedes start or interval_days is nonpositive.
/// TypeError
///     If interval_days is not an integer or is bool.
/// OverflowError
///     If interval_days is outside the signed 64-bit range.
///
/// Notes
/// -----
/// Exclude start and include candidates through end. Following adjustment
/// merges collisions and stops before crossing end; end need not appear.
#[pyfunction]
#[pyo3(signature = (*, start, end, interval_days, calendar=TradingCalendar::default()))]
pub(crate) fn fixed_interval_schedule(
    start: &Bound<'_, PyAny>,
    end: &Bound<'_, PyAny>,
    interval_days: &Bound<'_, PyAny>,
    calendar: TradingCalendar,
) -> PyResult<ObservationSchedule> {
    let start = date(start)?;
    let end = date(end)?;
    check_integer(interval_days)?;
    let interval_days = interval_days.extract::<i64>()?;
    Ok(ObservationSchedule(domain(
        kiyosi::ObservationSchedule::fixed_interval(start, end, interval_days, &calendar.0),
    )?))
}

/// Create monthly observations anchored to the start day.
///
/// Parameters
/// ----------
/// start : datetime.date
///     First date of the interval.
/// end : datetime.date
///     Last date of the interval.
/// first_observation_month : int
///     Positive month offset from start to the first observation.
/// calendar : TradingCalendar, optional
///     Trading calendar. Defaults to weekdays with a 252-day basis.
///
/// Returns
/// -------
/// ObservationSchedule
///     Owned result; no input is mutated.
///
/// Raises
/// ------
/// InvalidInputError
///     If end precedes start or first_observation_month is nonpositive.
///
/// Notes
/// -----
/// Clamp each candidate to its month end while retaining the original
/// start-day anchor for later months.
/// Exclude start and include candidates through end. Following adjustment
/// merges collisions and stops before crossing end; end need not appear.
#[pyfunction]
#[pyo3(signature = (*, start, end, first_observation_month, calendar=TradingCalendar::default()))]
pub(crate) fn monthly_schedule(
    start: &Bound<'_, PyAny>,
    end: &Bound<'_, PyAny>,
    first_observation_month: &Bound<'_, PyAny>,
    calendar: TradingCalendar,
) -> PyResult<ObservationSchedule> {
    Ok(ObservationSchedule(domain(
        kiyosi::ObservationSchedule::monthly(
            date(start)?,
            date(end)?,
            integer(first_observation_month)?,
            &calendar.0,
        ),
    )?))
}
