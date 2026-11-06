use crate::calendar::TradingCalendar;
use crate::conversions::{integer, integer_index, python_timestamp, timestamp};
use crate::error::domain;
use pyo3::exceptions::{PyIndexError, PyOverflowError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyList, PySlice};

/// Immutable ordered observation times with sequence access.
///
/// Parameters
/// ----------
/// times : iterable of datetime.datetime
///     Strictly increasing observation timestamps; empty input is valid.
/// start_time, end_time : datetime.datetime
///     Inclusive validation bounds, with start_time at or before end_time.
/// calendar : TradingCalendar, optional
///     Calendar used to validate each observation date; defaults to weekdays.
///
/// Raises
/// ------
/// InvalidInputError
///     If life bounds are reversed, times are unordered or outside life,
///     or an observation date is closed in the calendar.
///
/// Notes
/// -----
/// Construct from explicit times, or use fixed_interval_schedule or monthly_schedule.
/// Bounds and calendar validate the input; the schedule owns only its times.
/// Empty schedules are valid. The times property
/// returns a copy; iteration retains the schedule owner. Integer indexing
/// accepts negative indices, rejects bool, and raises IndexError outside the sequence.
/// Slices return owned lists of timestamps, including reversed slices.
/// Slice bounds and steps accept integers excluding bool; a zero step raises ValueError.
/// len(schedule) counts observations.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct ObservationSchedule(pub(crate) kiyosi::ObservationSchedule);

#[pymethods]
impl ObservationSchedule {
    #[new]
    #[pyo3(signature = (*, times, start_time, end_time, calendar=TradingCalendar::default()))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 extracts an owned calendar and supplies its default"
    )]
    fn new(
        times: &Bound<'_, PyAny>,
        start_time: &Bound<'_, PyAny>,
        end_time: &Bound<'_, PyAny>,
        calendar: TradingCalendar,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::ObservationSchedule::new(
            crate::conversions::timestamps(times)?,
            timestamp(start_time)?,
            timestamp(end_time)?,
            &calendar.0,
        ))?))
    }

    /// Return a copy of the strictly ordered observation times.
    #[getter]
    fn times<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        self.0
            .times()
            .iter()
            .map(|&timestamp| python_timestamp(py, timestamp))
            .collect()
    }
    fn __len__(&self) -> usize {
        self.0.len()
    }
    fn __getitem__<'py>(
        &self,
        py: Python<'py>,
        index: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if let Ok(slice) = index.cast::<PySlice>() {
            for name in ["start", "stop", "step"] {
                if slice.getattr(name)?.is_instance_of::<PyBool>() {
                    return Err(PyTypeError::new_err("expected an integer, excluding bool"));
                }
            }
            let length = isize::try_from(self.0.len())
                .map_err(|_| PyOverflowError::new_err("schedule is too long to slice"))?;
            let indices = slice.indices(length)?;
            let result = PyList::empty(py);
            let mut position = indices.start;
            for _ in 0..indices.slicelength {
                result.append(python_timestamp(
                    py,
                    self.0.times()[position.unsigned_abs()],
                )?)?;
                position = position.saturating_add(indices.step);
            }
            return Ok(result.into_any());
        }
        let index = integer_index(index)?;
        let index = index.extract::<isize>().map_err(|error| {
            if error.is_instance_of::<PyOverflowError>(py) {
                PyIndexError::new_err("schedule index out of range")
            } else {
                error
            }
        })?;
        let index = if index < 0 {
            self.0.len().checked_add_signed(index)
        } else {
            Some(index.unsigned_abs())
        };
        let timestamp = index
            .and_then(|index| self.0.times().get(index))
            .ok_or_else(|| PyIndexError::new_err("schedule index out of range"))?;
        python_timestamp(py, *timestamp)
    }
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 extracts an owned PyRef guard"
    )]
    fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Bound<'_, PyAny>> {
        // SAFETY: slf is live under the GIL and has a sequence slot from __getitem__.
        // PySeqIter_New returns a new reference (or null) and retains the schedule.
        unsafe { Bound::from_owned_ptr_or_err(slf.py(), pyo3::ffi::PySeqIter_New(slf.as_ptr())) }
    }
}

/// Creates adjusted observations at positive fixed calendar-day intervals.
///
/// Parameters
/// ----------
/// start_time : datetime.datetime
///     First timestamp of the interval.
/// end_time : datetime.datetime
///     Last timestamp of the interval.
/// interval_days : int
///     Positive calendar-day spacing of unadjusted observations, up to 2**63 - 1.
/// observation_clock_time : datetime.time
///     Required observation time of day, without timezone information.
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
///     If end_time precedes start_time or interval_days is nonpositive.
/// TypeError
///     If interval_days is not an integer or is bool.
/// OverflowError
///     If interval_days is outside the signed 64-bit range.
///
/// Notes
/// -----
/// Exclude start_time and include candidates through end_time. Following adjustment
/// merges collisions and stops before crossing `end_time`; `end_time` need not appear.
#[pyfunction]
#[pyo3(signature = (*, start_time, end_time, interval_days, observation_clock_time, calendar=TradingCalendar::default()))]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts an owned calendar and supplies its default"
)]
pub(crate) fn fixed_interval_schedule(
    start_time: &Bound<'_, PyAny>,
    end_time: &Bound<'_, PyAny>,
    interval_days: &Bound<'_, PyAny>,
    observation_clock_time: &Bound<'_, PyAny>,
    calendar: TradingCalendar,
) -> PyResult<ObservationSchedule> {
    let start_time = timestamp(start_time)?;
    let end_time = timestamp(end_time)?;
    let interval_days = integer_index(interval_days)?.extract::<i64>()?;
    Ok(ObservationSchedule(domain(
        kiyosi::ObservationSchedule::fixed_interval(
            start_time,
            end_time,
            interval_days,
            &calendar.0,
            crate::conversions::clock_time(observation_clock_time)?,
        ),
    )?))
}

/// Create monthly observations anchored to the start_time day.
///
/// Parameters
/// ----------
/// start_time : datetime.datetime
///     First timestamp of the interval.
/// end_time : datetime.datetime
///     Last timestamp of the interval.
/// first_observation_month : int
///     Positive month offset from start_time to the first observation.
/// observation_clock_time : datetime.time
///     Required observation time of day, without timezone information.
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
///     If end_time precedes start_time or first_observation_month is nonpositive.
///
/// Notes
/// -----
/// Clamp each candidate to its month end while retaining the original
/// starting-day anchor for later months.
/// Exclude start_time and include candidates through end_time. Following adjustment
/// merges collisions and stops before crossing `end_time`; `end_time` need not appear.
#[pyfunction]
#[pyo3(signature = (*, start_time, end_time, first_observation_month, observation_clock_time, calendar=TradingCalendar::default()))]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts an owned calendar and supplies its default"
)]
pub(crate) fn monthly_schedule(
    start_time: &Bound<'_, PyAny>,
    end_time: &Bound<'_, PyAny>,
    first_observation_month: &Bound<'_, PyAny>,
    observation_clock_time: &Bound<'_, PyAny>,
    calendar: TradingCalendar,
) -> PyResult<ObservationSchedule> {
    Ok(ObservationSchedule(domain(
        kiyosi::ObservationSchedule::monthly(
            timestamp(start_time)?,
            timestamp(end_time)?,
            integer(first_observation_month)?,
            &calendar.0,
            crate::conversions::clock_time(observation_clock_time)?,
        ),
    )?))
}
