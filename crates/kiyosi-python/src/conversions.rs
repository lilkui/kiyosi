use crate::error::domain;
use pyo3::IntoPyObjectExt;
use pyo3::exceptions::{PyOverflowError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyDate, PyDateAccess, PyDateTime, PyDelta, PyDeltaAccess, PyFloat, PyTime,
    PyTimeAccess, PyTuple,
};

pub(crate) fn duration(value: &Bound<'_, PyAny>) -> PyResult<std::time::Duration> {
    let value = value.cast::<PyDelta>()?;
    if value.hasattr("nanoseconds")? && value.getattr("nanoseconds")?.extract::<u32>()? != 0 {
        return Err(PyTypeError::new_err(
            "duration precision must not exceed microseconds",
        ));
    }
    let seconds = i64::from(value.get_days()) * 86_400 + i64::from(value.get_seconds());
    let seconds = u64::try_from(seconds)
        .map_err(|_| crate::error::InvalidInputError::new_err("time_shift must be positive"))?;
    Ok(std::time::Duration::new(
        seconds,
        value.get_microseconds().unsigned_abs() * 1000,
    ))
}

pub(crate) fn optional_duration(value: &Bound<'_, PyAny>) -> PyResult<Option<std::time::Duration>> {
    if value.is_none() {
        Ok(None)
    } else {
        duration(value).map(Some)
    }
}

pub(crate) fn instrument<'a, 'py, T>(value: &'a Bound<'py, PyAny>) -> PyResult<T>
where
    T: FromPyObject<'a, 'py>,
{
    pricing_argument(value, crate::registration::is_instrument)
}

pub(crate) fn engine<'a, 'py, T>(value: &'a Bound<'py, PyAny>) -> PyResult<T>
where
    T: FromPyObject<'a, 'py>,
{
    pricing_argument(value, crate::registration::is_engine)
}

fn pricing_argument<'a, 'py, T>(
    value: &'a Bound<'py, PyAny>,
    known: impl FnOnce(&Bound<'py, PyAny>) -> bool,
) -> PyResult<T>
where
    T: FromPyObject<'a, 'py>,
{
    value.extract::<T>().map_err(|error| {
        let error: PyErr = error.into();
        if error.is_instance_of::<PyTypeError>(value.py()) && known(value) {
            crate::error::UnsupportedOperationError::new_err(
                "unsupported instrument or engine for this pricing operation",
            )
        } else {
            error
        }
    })
}

// PyO3 displays nonliteral defaults as Ellipsis, which supplied treats as omission.
pub(crate) const OMITTED: Option<&Bound<'_, PyAny>> = None;

// Ellipsis marks omission. None is an explicitly supplied value.
#[expect(
    clippy::unnecessary_wraps,
    reason = "PyO3 from_py_with callbacks return PyResult"
)]
pub(crate) fn supplied<'a, 'py>(
    value: &'a Bound<'py, PyAny>,
) -> PyResult<Option<&'a Bound<'py, PyAny>>> {
    Ok((!value.is_instance_of::<pyo3::types::PyEllipsis>()).then_some(value))
}

pub(crate) fn real(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    if let Ok(number) = value.cast_exact::<PyFloat>() {
        return Ok(number.value());
    }
    let numbers = value.py().import("numbers")?;
    let decimal =
        if !value.is_instance_of::<PyBool>() && value.is_instance(&numbers.getattr("Real")?)? {
            None
        } else {
            let decimal = value.py().import("decimal")?.getattr("Decimal")?;
            if !value.is_instance(&decimal)? {
                return Err(PyTypeError::new_err(
                    "expected a real number or Decimal, excluding bool",
                ));
            }
            Some(decimal)
        };
    let converted: f64 = value.extract()?;
    if converted.is_infinite() && !value.is_instance_of::<PyFloat>() {
        let overflowed = if decimal.is_some() {
            value.call_method0("is_finite")?.extract::<bool>()?
        } else {
            !value.eq(converted)?
        };
        if overflowed {
            return Err(PyOverflowError::new_err(
                "number is outside the float range",
            ));
        }
    }
    // Integer scalar equality may round both operands (for example, NumPy integers).
    let exact = if value.is_instance(&numbers.getattr("Integral")?)? {
        integer_index(value)?
    } else {
        value.clone()
    };
    if converted.is_finite() && !value.is_instance_of::<PyFloat>() {
        let equal = if let Some(decimal) = decimal {
            // Decimal-to-Decimal equality avoids signaling FloatOperation in the caller's context.
            exact.eq(decimal.call_method1("from_float", (converted,))?)?
        } else {
            exact.eq(converted)?
        };
        if !equal {
            return Err(PyValueError::new_err(
                "number cannot be represented exactly as a float; convert to float explicitly to round",
            ));
        }
    }
    Ok(converted)
}

pub(crate) fn boolean(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    value.cast::<PyBool>()?.extract()
}

pub(crate) fn optional_real(value: &Bound<'_, PyAny>) -> PyResult<Option<f64>> {
    if value.is_none() {
        Ok(None)
    } else {
        real(value).map(Some)
    }
}

pub(crate) fn reals(value: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    collect_values(value, real)
}

pub(crate) fn collect_values<T>(
    value: &Bound<'_, PyAny>,
    convert: impl Fn(&Bound<'_, PyAny>) -> PyResult<T>,
) -> PyResult<Vec<T>> {
    collect_values_checked(value, convert, |_| Ok(()))
}

pub(crate) fn collect_values_checked<'py, T>(
    value: &Bound<'py, PyAny>,
    convert: impl Fn(&Bound<'py, PyAny>) -> PyResult<T>,
    check_count: impl Fn(usize) -> PyResult<()>,
) -> PyResult<Vec<T>> {
    value
        .try_iter()?
        .enumerate()
        .map(|(index, item)| {
            let item = item?;
            check_count(index + 1)?;
            if index % 1024 == 0 {
                value.py().check_signals()?;
            }
            convert(&item)
        })
        .collect()
}

pub(crate) fn collect_matching_values<'py, T>(
    value: &Bound<'py, PyAny>,
    convert: impl Fn(&Bound<'py, PyAny>) -> PyResult<T>,
    expected: usize,
    validate: impl Fn(usize, usize) -> kiyosi::Result<()>,
) -> PyResult<Vec<T>> {
    let values = collect_values_checked(value, convert, |count| {
        if count > expected {
            domain(validate(count, expected))?;
        }
        Ok(())
    })?;
    domain(validate(values.len(), expected))?;
    Ok(values)
}

pub(crate) fn direction_name(value: kiyosi::OptionType) -> &'static str {
    match value {
        kiyosi::OptionType::Call => "call",
        kiyosi::OptionType::Put => "put",
    }
}

pub(crate) fn barrier_type_name(value: kiyosi::BarrierType) -> &'static str {
    match value {
        kiyosi::BarrierType::UpAndIn => "up_and_in",
        kiyosi::BarrierType::UpAndOut => "up_and_out",
        kiyosi::BarrierType::DownAndIn => "down_and_in",
        kiyosi::BarrierType::DownAndOut => "down_and_out",
    }
}

pub(crate) fn double_barrier_type_name(value: kiyosi::DoubleBarrierType) -> &'static str {
    match value {
        kiyosi::DoubleBarrierType::KnockIn => "knock_in",
        kiyosi::DoubleBarrierType::KnockOut => "knock_out",
    }
}

pub(crate) fn rebate_timing_name(value: kiyosi::RebateTiming) -> &'static str {
    match value {
        kiyosi::RebateTiming::AtHit => "at_hit",
        kiyosi::RebateTiming::AtExpiry => "at_expiry",
    }
}

pub(crate) fn settlement_timing_name(value: kiyosi::SettlementTiming) -> &'static str {
    match value {
        kiyosi::SettlementTiming::AtHit => "at_hit",
        kiyosi::SettlementTiming::AtExpiry => "at_expiry",
    }
}

pub(crate) fn direction(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::OptionType> {
    match value.extract::<&str>()? {
        "call" => Ok(kiyosi::OptionType::Call),
        "put" => Ok(kiyosi::OptionType::Put),
        _ => Err(PyValueError::new_err("option_type must be call or put")),
    }
}

pub(crate) fn integer(value: &Bound<'_, PyAny>) -> PyResult<i32> {
    integer_index(value)?.extract()
}

pub(crate) fn integer_usize(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    integer_index(value)?.extract()
}

pub(crate) fn optional_seed(value: &Bound<'_, PyAny>) -> PyResult<Option<u64>> {
    if value.is_none() {
        Ok(None)
    } else {
        integer_index(value)?.extract().map(Some)
    }
}

pub(crate) fn integer_index<'py>(value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    if value.is_instance_of::<PyBool>() {
        return Err(PyTypeError::new_err("expected an integer, excluding bool"));
    }
    value
        .py()
        .import("operator")?
        .call_method1("index", (value,))
}

pub(crate) fn dates(value: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::Date>> {
    collect_values(value, date)
}

pub(crate) fn date(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Date> {
    if value.is_instance_of::<PyDateTime>() {
        return Err(PyTypeError::new_err(
            "expected datetime.date, excluding datetime",
        ));
    }
    let date = value.cast::<PyDate>()?;
    domain(kiyosi::Date::new(
        date.get_year(),
        date.get_month().into(),
        date.get_day().into(),
    ))
}

type MonitoringSchedule = (
    Option<Option<kiyosi::ObservationSchedule>>,
    Option<Vec<kiyosi::Timestamp>>,
);

pub(crate) fn monitoring_schedule(
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<MonitoringSchedule> {
    let Some(value) = value else {
        return Ok((None, None));
    };
    if value.is_none() {
        Ok((Some(None), None))
    } else if let Ok(schedule) = value.extract::<PyRef<'_, crate::schedule::ObservationSchedule>>()
    {
        Ok((Some(Some(schedule.0.clone())), None))
    } else {
        Ok((Some(None), Some(timestamps(value)?)))
    }
}

pub(crate) fn timestamps(value: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::Timestamp>> {
    collect_timestamps_checked(value, |_| Ok(()))
}

fn collect_timestamps_checked(
    value: &Bound<'_, PyAny>,
    check_count: impl Fn(usize) -> PyResult<()>,
) -> PyResult<Vec<kiyosi::Timestamp>> {
    if let Ok(schedule) = value.extract::<PyRef<'_, crate::schedule::ObservationSchedule>>() {
        check_count(schedule.0.len())?;
        return Ok(schedule.0.times().to_vec());
    }
    collect_values_checked(value, timestamp, check_count)
}

pub(crate) fn surface_inputs(
    expiries: &Bound<'_, PyAny>,
    strikes: &Bound<'_, PyAny>,
) -> PyResult<(Vec<kiyosi::Timestamp>, Vec<f64>)> {
    let expiries = collect_timestamps_checked(expiries, |count| {
        domain(kiyosi::binding_support::validate_finite_difference_surface_size(count, 0))
    })?;
    let strikes = collect_values_checked(strikes, real, |count| {
        domain(
            kiyosi::binding_support::validate_finite_difference_surface_size(expiries.len(), count),
        )
    })?;
    Ok((expiries, strikes))
}

pub(crate) fn finite_difference_strikes(value: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    collect_values_checked(value, real, |count| {
        domain(kiyosi::binding_support::validate_finite_difference_strike_batch_size(count))
    })
}

pub(crate) fn heston_strikes(value: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    collect_values_checked(value, real, |count| {
        domain(kiyosi::binding_support::validate_heston_strike_batch_size(
            count,
        ))
    })
}

pub(crate) fn quadrature_strikes(value: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    collect_values_checked(value, real, |count| {
        domain(kiyosi::binding_support::validate_quadrature_strike_batch_size(count))
    })
}

pub(crate) fn clock_time(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::TimeOfDay> {
    let time = value.cast::<PyTime>()?;
    if !value
        .py()
        .get_type::<PyTime>()
        .getattr("tzinfo")?
        .call_method1("__get__", (value,))?
        .is_none()
    {
        return Err(PyTypeError::new_err("expected a timezone-free time"));
    }
    if value.hasattr("nanosecond")? && value.getattr("nanosecond")?.extract::<u32>()? != 0 {
        return Err(PyTypeError::new_err(
            "time precision must not exceed microseconds",
        ));
    }
    domain(kiyosi::TimeOfDay::new(
        time.get_hour().into(),
        time.get_minute().into(),
        time.get_second().into(),
        time.get_microsecond(),
    ))
}

pub(crate) fn python_clock_time(
    py: Python<'_>,
    at: kiyosi::TimeOfDay,
) -> PyResult<Bound<'_, PyAny>> {
    let micros = at.micros_since_midnight();
    py.get_type::<PyTime>().call1((
        micros / 3_600_000_000,
        micros / 60_000_000 % 60,
        micros / 1_000_000 % 60,
        micros % 1_000_000,
    ))
}

pub(crate) fn timestamp(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Timestamp> {
    let time = value.cast::<PyDateTime>()?;
    if !value
        .py()
        .get_type::<PyDateTime>()
        .getattr("tzinfo")?
        .call_method1("__get__", (value,))?
        .is_none()
    {
        return Err(PyTypeError::new_err("expected a timezone-free datetime"));
    }
    if value.hasattr("nanosecond")? && value.getattr("nanosecond")?.extract::<u32>()? != 0 {
        return Err(PyTypeError::new_err(
            "datetime precision must not exceed microseconds",
        ));
    }
    let day = domain(kiyosi::Date::new(
        time.get_year(),
        time.get_month().into(),
        time.get_day().into(),
    ))?;
    domain(kiyosi::Timestamp::new(
        day,
        time.get_hour().into(),
        time.get_minute().into(),
        time.get_second().into(),
        time.get_microsecond(),
    ))
}

pub(crate) fn python_date(py: Python<'_>, date: kiyosi::Date) -> PyResult<Bound<'_, PyDate>> {
    PyDate::new(
        py,
        date.year(),
        u8::try_from(date.month()).expect("month fits in u8"),
        u8::try_from(date.day()).expect("day fits in u8"),
    )
}

pub(crate) fn python_timestamp(
    py: Python<'_>,
    at: kiyosi::Timestamp,
) -> PyResult<Bound<'_, PyAny>> {
    let date = at.date();
    let micros = at.micros_since_midnight();
    py.get_type::<PyDateTime>().call1((
        date.year(),
        date.month(),
        date.day(),
        micros / 3_600_000_000,
        micros / 60_000_000 % 60,
        micros / 1_000_000 % 60,
        micros % 1_000_000,
    ))
}

pub(crate) fn python_timestamps<'py>(
    py: Python<'py>,
    times: &[kiyosi::Timestamp],
) -> PyResult<Bound<'py, PyTuple>> {
    python_tuple(py, times.iter().copied(), |time| python_timestamp(py, time))
}

// Pass conversion errors through PyTuple::new so it can discard a partial tuple.
struct TupleItem<T>(PyResult<T>);

impl<'py, T: IntoPyObject<'py>> IntoPyObject<'py> for TupleItem<T> {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Self::Output> {
        self.0?.into_bound_py_any(py)
    }
}

pub(crate) fn python_tuple<'py, T, U: IntoPyObject<'py>>(
    py: Python<'py>,
    values: impl IntoIterator<Item = T, IntoIter: ExactSizeIterator>,
    mut convert: impl FnMut(T) -> PyResult<U>,
) -> PyResult<Bound<'py, PyTuple>> {
    PyTuple::new(
        py,
        values.into_iter().enumerate().map(|(index, value)| {
            let converted = if index % 1024 == 0 {
                py.check_signals().and_then(|()| convert(value))
            } else {
                convert(value)
            };
            TupleItem(converted)
        }),
    )
}
