use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDate, PyDateAccess, PyDateTime, PyDict, PyTimeAccess};

pub(crate) fn instrument<'a, 'py, T>(value: &'a Bound<'py, PyAny>) -> PyResult<T>
where
    T: FromPyObject<'a, 'py>,
{
    pricing_argument(value, |value| {
        use crate::instruments::*;
        value.is_instance_of::<vanilla::EuropeanOption>()
            || value.is_instance_of::<american::AmericanOption>()
            || value.is_instance_of::<digital::CashOrNothingOption>()
            || value.is_instance_of::<digital::AssetOrNothingOption>()
            || value.is_instance_of::<asian::GeometricAveragePriceOption>()
            || value.is_instance_of::<asian::ArithmeticAveragePriceOption>()
            || value.is_instance_of::<barrier::BarrierOption>()
            || value.is_instance_of::<binary_barrier::BinaryBarrierOption>()
            || value.is_instance_of::<binary_barrier::TouchOption>()
            || value.is_instance_of::<double_barrier::DoubleBarrierOption>()
            || value.is_instance_of::<double_touch::DoubleTouchOption>()
            || value.is_instance_of::<autocallable::SnowballOption>()
            || value.is_instance_of::<autocallable::BinarySnowballOption>()
            || value.is_instance_of::<autocallable::TernarySnowballOption>()
            || value.is_instance_of::<autocallable::PhoenixOption>()
            || value.is_instance_of::<accumulator::Accumulator>()
            || value.is_instance_of::<range_accrual::RangeAccrualOption>()
    })
}

pub(crate) fn engine<'a, 'py, T>(value: &'a Bound<'py, PyAny>) -> PyResult<T>
where
    T: FromPyObject<'a, 'py>,
{
    pricing_argument(value, |value| {
        use crate::engines::*;
        value.is_instance_of::<analytic::european::AnalyticEuropeanEngine>()
            || value.is_instance_of::<analytic::american::BjerksundStenslandVanillaEngine>()
            || value.is_instance_of::<analytic::digital::AnalyticDigitalEngine>()
            || value.is_instance_of::<analytic::asian::AnalyticGeometricAveragePriceEngine>()
            || value
                .is_instance_of::<analytic::asian::TurnbullWakemanArithmeticAveragePriceEngine>()
            || value.is_instance_of::<analytic::barrier::AnalyticBarrierEngine>()
            || value.is_instance_of::<analytic::binary_barrier::AnalyticBinaryBarrierEngine>()
            || value.is_instance_of::<analytic::double_barrier::AnalyticDoubleBarrierEngine>()
            || value.is_instance_of::<analytic::range_accrual::AnalyticRangeAccrualEngine>()
            || value.is_instance_of::<binomial::CoxRossRubinsteinVanillaEngine>()
            || value.is_instance_of::<binomial::CoxRossRubinsteinDigitalEngine>()
            || value.is_instance_of::<binomial::CoxRossRubinsteinBarrierEngine>()
            || value.is_instance_of::<quadrature::QuadratureVanillaEngine>()
            || value.is_instance_of::<quadrature::QuadratureDigitalEngine>()
            || value.is_instance_of::<quadrature::QuadratureBarrierEngine>()
            || value.is_instance_of::<quadrature::QuadratureAutocallableEngine>()
            || value.is_instance_of::<quadrature::QuadratureAccumulatorEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceVanillaEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceDigitalEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceBarrierEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceDoubleBarrierEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceBinaryBarrierEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceDoubleTouchEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceAutocallableEngine>()
            || value.is_instance_of::<finite_difference::FiniteDifferenceAccumulatorEngine>()
            || value.is_instance_of::<monte_carlo::MonteCarloVanillaEngine>()
            || value.is_instance_of::<monte_carlo::MonteCarloDigitalEngine>()
            || value.is_instance_of::<monte_carlo::MonteCarloAsianEngine>()
            || value.is_instance_of::<monte_carlo::MonteCarloBarrierEngine>()
            || value.is_instance_of::<monte_carlo::MonteCarloDoubleBarrierEngine>()
            || value.is_instance_of::<monte_carlo::MonteCarloAutocallableEngine>()
            || value.is_instance_of::<monte_carlo::MonteCarloAccumulatorEngine>()
    })
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

// Preserve explicit None separately from an omitted keyword.
pub(crate) fn supplied<'a, 'py>(
    value: &'a Bound<'py, PyAny>,
) -> PyResult<Option<&'a Bound<'py, PyAny>>> {
    Ok(Some(value))
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
    domain(
        value.py(),
        kiyosi::Date::new(
            date.get_year(),
            date.get_month().into(),
            date.get_day().into(),
        ),
    )
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
    let day = domain(
        py,
        kiyosi::Date::new(utc.get_year(), utc.get_month().into(), utc.get_day().into()),
    )?;
    domain(
        py,
        kiyosi::Timestamp::new(
            day,
            utc.get_hour().into(),
            utc.get_minute().into(),
            utc.get_second().into(),
            utc.get_microsecond(),
        ),
    )
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
