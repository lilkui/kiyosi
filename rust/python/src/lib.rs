//! Python representation adapters; all domain validation lives in kiyosi-core.

use kiyosi_core as core;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDate, PyDateAccess, PyDateTime, PyDict, PyTimeAccess};

pyo3::create_exception!(_native, KiyosiError, PyException);

fn domain<T>(py: Python<'_>, result: core::Result<T>) -> PyResult<T> {
    result.map_err(|error| {
        let exception = KiyosiError::new_err(error.message);
        let category = py
            .import("kiyosi._native")
            .and_then(|module| module.getattr("ErrorCategory"))
            .and_then(|enum_type| enum_type.getattr(error.category.name()));
        match category.and_then(|category| exception.value(py).setattr("category", category)) {
            Ok(()) => exception,
            Err(mapping_error) => mapping_error,
        }
    })
}

fn real(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    if value.is_instance_of::<PyBool>()
        || !value.is_instance(&value.py().import("numbers")?.getattr("Real")?)?
    {
        return Err(PyTypeError::new_err(
            "expected a real number, excluding bool",
        ));
    }
    value.extract()
}

fn date(value: &Bound<'_, PyAny>) -> PyResult<core::Date> {
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

fn timestamp(value: &Bound<'_, PyAny>) -> PyResult<core::Timestamp> {
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

fn python_date(py: Python<'_>, date: core::Date) -> PyResult<Bound<'_, PyDate>> {
    PyDate::new(py, date.year(), date.month() as u8, date.day() as u8)
}

fn python_timestamp(py: Python<'_>, at: core::Timestamp) -> PyResult<Bound<'_, PyAny>> {
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

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
struct EuropeanOption(core::EuropeanOption);

#[pymethods]
impl EuropeanOption {
    #[new]
    #[pyo3(signature = (*, option_type, strike, effective_date, expiry_date))]
    fn new(
        py: Python<'_>,
        option_type: &Bound<'_, PyAny>,
        strike: &Bound<'_, PyAny>,
        effective_date: &Bound<'_, PyAny>,
        expiry_date: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let direction = match option_type.extract::<&str>()? {
            "call" => core::OptionType::Call,
            "put" => core::OptionType::Put,
            _ => return Err(PyValueError::new_err("option_type must be call or put")),
        };
        Ok(Self(domain(
            py,
            core::EuropeanOption::new(
                direction,
                real(strike)?,
                date(effective_date)?,
                date(expiry_date)?,
            ),
        )?))
    }

    #[getter]
    fn option_type(&self) -> &'static str {
        match self.0.option_type() {
            core::OptionType::Call => "call",
            core::OptionType::Put => "put",
        }
    }
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    #[getter]
    fn effective_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.effective_date())
    }
    #[getter]
    fn expiry_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.expiry_date())
    }
}

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
struct BlackScholesMertonParameters(core::BlackScholesMertonParameters);

#[pymethods]
impl BlackScholesMertonParameters {
    #[new]
    #[pyo3(signature = (*, risk_free_rate, dividend_yield, volatility))]
    fn new(
        py: Python<'_>,
        risk_free_rate: &Bound<'_, PyAny>,
        dividend_yield: &Bound<'_, PyAny>,
        volatility: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            core::BlackScholesMertonParameters::new(
                real(risk_free_rate)?,
                real(dividend_yield)?,
                real(volatility)?,
            ),
        )?))
    }
    #[getter]
    fn risk_free_rate(&self) -> f64 {
        self.0.risk_free_rate()
    }
    #[getter]
    fn dividend_yield(&self) -> f64 {
        self.0.dividend_yield()
    }
    #[getter]
    fn volatility(&self) -> f64 {
        self.0.volatility()
    }
}

#[pyclass(frozen, eq, from_py_object, module = "kiyosi._native")]
#[derive(Clone, Default, PartialEq)]
struct TradingCalendar(core::TradingCalendar);

#[pymethods]
impl TradingCalendar {
    #[getter]
    fn trading_days_per_year(&self) -> u32 {
        self.0.trading_days_per_year()
    }

    fn __repr__(&self) -> &'static str {
        "weekdays_calendar()"
    }
}

#[pyfunction]
fn weekdays_calendar() -> TradingCalendar {
    TradingCalendar::default()
}

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
struct PricingContext(core::PricingContext);

#[pymethods]
impl PricingContext {
    #[new]
    #[pyo3(signature = (*, model_parameters, spot_price, valuation_time, calendar = TradingCalendar::default()))]
    fn new(
        py: Python<'_>,
        model_parameters: &BlackScholesMertonParameters,
        spot_price: &Bound<'_, PyAny>,
        valuation_time: &Bound<'_, PyAny>,
        calendar: TradingCalendar,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            core::PricingContext::with_calendar(
                model_parameters.0.clone(),
                real(spot_price)?,
                timestamp(valuation_time)?,
                calendar.0,
            ),
        )?))
    }
    #[getter]
    fn model_parameters(&self) -> BlackScholesMertonParameters {
        BlackScholesMertonParameters(self.0.model_parameters().clone())
    }
    #[getter]
    fn spot_price(&self) -> f64 {
        self.0.spot_price()
    }
    #[getter]
    fn valuation_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.valuation_date())
    }
    #[getter]
    fn valuation_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.valuation_time())
    }
    #[getter]
    fn calendar(&self) -> TradingCalendar {
        TradingCalendar(self.0.calendar())
    }
}

#[pyclass(frozen, module = "kiyosi._native")]
struct AnalyticVanillaEngine;

#[pymethods]
impl AnalyticVanillaEngine {
    #[new]
    fn new() -> Self {
        Self
    }

    fn price(
        &self,
        py: Python<'_>,
        instrument: &EuropeanOption,
        context: &PricingContext,
    ) -> PyResult<f64> {
        // Keep the GIL for the short scalar formula, matching the legacy analytic contract.
        domain(
            py,
            core::AnalyticVanillaEngine.price(&instrument.0, &context.0),
        )
    }
}

#[pymodule]
fn _native(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", core::VERSION)?;
    let categories = PyDict::new(py);
    for category in core::ErrorCategory::ALL {
        categories.set_item(category.name(), category as u8)?;
    }
    let kwargs = PyDict::new(py);
    kwargs.set_item("module", "kiyosi._native")?;
    module.add(
        "ErrorCategory",
        py.import("enum")?
            .getattr("IntEnum")?
            .call(("ErrorCategory", categories), Some(&kwargs))?,
    )?;
    module.add("KiyosiError", py.get_type::<KiyosiError>())?;
    module.add_class::<EuropeanOption>()?;
    module.add_class::<BlackScholesMertonParameters>()?;
    module.add_class::<TradingCalendar>()?;
    module.add_class::<PricingContext>()?;
    module.add_class::<AnalyticVanillaEngine>()?;
    module.add_function(wrap_pyfunction!(weekdays_calendar, module)?)?;
    Ok(())
}
