use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDate, PyDateAccess, PyDateTime, PyTimeAccess};

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
pub(crate) fn supplied<'a, 'py>(
    value: &'a Bound<'py, PyAny>,
) -> PyResult<Option<&'a Bound<'py, PyAny>>> {
    Ok((!value.is_instance_of::<pyo3::types::PyEllipsis>()).then_some(value))
}

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

pub(crate) fn timestamp(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Timestamp> {
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
    let day = domain(kiyosi::Date::new(
        utc.get_year(),
        utc.get_month().into(),
        utc.get_day().into(),
    ))?;
    domain(kiyosi::Timestamp::new(
        day,
        utc.get_hour().into(),
        utc.get_minute().into(),
        utc.get_second().into(),
        utc.get_microsecond(),
    ))
}

pub(crate) fn python_date(py: Python<'_>, date: kiyosi::Date) -> PyResult<Bound<'_, PyDate>> {
    PyDate::new(py, date.year(), date.month() as u8, date.day() as u8)
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
        module.getattr("timezone")?.getattr("utc")?,
    ))
}
