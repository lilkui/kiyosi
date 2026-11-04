use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDate, PyDateAccess, PyDateTime, PyFloat, PyTimeAccess};

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

// Ellipsis marks omission; None remains an explicitly supplied value.
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
    let numbers = value.py().import("numbers")?;
    if value.is_instance_of::<PyBool>() || !value.is_instance(&numbers.getattr("Real")?)? {
        return Err(PyTypeError::new_err(
            "expected a real number, excluding bool",
        ));
    }
    let converted: f64 = value.extract()?;
    // Integer scalar equality may round both operands (for example, NumPy integers).
    let exact = if value.is_instance(&numbers.getattr("Integral")?)? {
        integer_index(value)?
    } else {
        value.clone()
    };
    if converted.is_finite() && !value.is_instance_of::<PyFloat>() && !exact.eq(converted)? {
        return Err(PyValueError::new_err(
            "number cannot be represented exactly as a float; convert to float explicitly to round",
        ));
    }
    Ok(converted)
}

pub(crate) fn boolean(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    value.cast::<PyBool>()?.extract()
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
    value.try_iter()?.map(|item| date(&item?)).collect()
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

pub(crate) fn timestamps(value: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::Timestamp>> {
    value.try_iter()?.map(|item| timestamp(&item?)).collect()
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
    let module = py.import("datetime")?;
    let date = at.date();
    let micros = at.micros_since_midnight();
    module.getattr("datetime")?.call1((
        date.year(),
        date.month(),
        date.day(),
        micros / 3_600_000_000,
        micros / 60_000_000 % 60,
        micros / 1_000_000 % 60,
        micros % 1_000_000,
    ))
}
