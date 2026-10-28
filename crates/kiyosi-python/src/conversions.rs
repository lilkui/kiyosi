use crate::{core, error::domain};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDate, PyDateAccess, PyDateTime, PyDict, PyTimeAccess};

pub(crate) fn real(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    if value.is_instance_of::<PyBool>()
        || !value.is_instance(&value.py().import("numbers")?.getattr("Real")?)?
    {
        return Err(PyTypeError::new_err(
            "expected a real number, excluding bool",
        ));
    }
    value.extract()
}

pub(crate) fn direction(value: &Bound<'_, PyAny>) -> PyResult<core::OptionType> {
    match value.extract::<&str>()? {
        "call" => Ok(core::OptionType::Call),
        "put" => Ok(core::OptionType::Put),
        _ => Err(PyValueError::new_err("option_type must be call or put")),
    }
}

pub(crate) fn integer(value: &Bound<'_, PyAny>) -> PyResult<i32> {
    check_integer(value)?;
    value.extract()
}

pub(crate) fn check_integer(value: &Bound<'_, PyAny>) -> PyResult<()> {
    if value.is_instance_of::<PyBool>()
        || !value.is_instance(&value.py().import("numbers")?.getattr("Integral")?)?
    {
        return Err(PyTypeError::new_err("expected an integer, excluding bool"));
    }
    Ok(())
}

pub(crate) fn dates(value: &Bound<'_, PyAny>) -> PyResult<Vec<core::Date>> {
    value.try_iter()?.map(|item| date(&item?)).collect()
}

pub(crate) fn date(value: &Bound<'_, PyAny>) -> PyResult<core::Date> {
    if value.is_instance_of::<PyDateTime>() {
        return Err(PyTypeError::new_err(
            "expected datetime.date, excluding datetime",
        ));
    }
    let date = value.cast::<PyDate>()?;
    domain(
        value.py(),
        core::Date::new(
            date.get_year(),
            date.get_month().into(),
            date.get_day().into(),
        ),
    )
}

pub(crate) fn timestamp(value: &Bound<'_, PyAny>) -> PyResult<core::Timestamp> {
    if !value.is_instance_of::<PyDateTime>() {
        return Ok(date(value)?.into());
    }
    let py = value.py();
    let module = py.import("datetime")?;
    let datetime_type = module.getattr("datetime")?;
    // Invoke built-in descriptors so subclasses cannot shadow stored components or UTC conversion.
    if datetime_type.call_method1("utcoffset", (value,))?.is_none() {
        return Err(PyTypeError::new_err(
            "valuation datetime must be timezone-aware",
        ));
    }
    let utc = datetime_type.call_method1(
        "astimezone",
        (value, module.getattr("timezone")?.getattr("utc")?),
    )?;
    let utc = utc.cast::<PyDateTime>()?;
    let day = domain(
        py,
        core::Date::new(utc.get_year(), utc.get_month().into(), utc.get_day().into()),
    )?;
    domain(
        py,
        core::Timestamp::new(
            day,
            utc.get_hour().into(),
            utc.get_minute().into(),
            utc.get_second().into(),
            utc.get_microsecond(),
        ),
    )
}

pub(crate) fn python_date(py: Python<'_>, date: core::Date) -> PyResult<Bound<'_, PyDate>> {
    PyDate::new(py, date.year(), date.month() as u8, date.day() as u8)
}

pub(crate) fn python_timestamp(py: Python<'_>, at: core::Timestamp) -> PyResult<Bound<'_, PyAny>> {
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
        module.getattr("timezone")?.getattr("utc")?,
    ))
}

pub(crate) fn check_changes(changes: Option<&Bound<'_, PyDict>>, fields: &[&str]) -> PyResult<()> {
    if let Some(changes) = changes {
        for key in changes.keys() {
            if !fields.contains(&key.extract::<&str>()?) {
                return Err(PyTypeError::new_err("unexpected replacement field"));
            }
        }
    }
    Ok(())
}
