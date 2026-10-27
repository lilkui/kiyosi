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
struct RateTermStructure(core::RateTermStructure);

#[pymethods]
impl RateTermStructure {
    #[new]
    #[pyo3(signature = (*, reference_time, times, factors))]
    fn new(
        py: Python<'_>,
        reference_time: &Bound<'_, PyAny>,
        times: &Bound<'_, PyAny>,
        factors: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let reference = timestamp(reference_time)?;
        let nodes = times
            .try_iter()?
            .map(|item| timestamp(&item?))
            .collect::<PyResult<Vec<_>>>()?;
        let factors = factors
            .try_iter()?
            .map(|item| real(&item?))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Self(domain(
            py,
            core::RateTermStructure::new(reference, nodes, factors),
        )?))
    }
    #[staticmethod]
    #[pyo3(signature = (rate, *, reference_time=None))]
    fn flat(
        py: Python<'_>,
        rate: &Bound<'_, PyAny>,
        reference_time: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            core::RateTermStructure::flat(real(rate)?, reference_time.map(timestamp).transpose()?),
        )?))
    }
    #[getter]
    fn is_flat(&self) -> bool {
        self.0.is_flat()
    }
    #[getter]
    fn reference_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .reference_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    #[getter]
    fn max_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .max_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    #[getter]
    fn nodes<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        self.0
            .nodes()
            .iter()
            .map(|&time| python_timestamp(py, time))
            .collect()
    }
    #[getter]
    fn factors(&self) -> Vec<f64> {
        self.0.factors()
    }
    fn factor(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(py, self.0.factor(timestamp(start)?, timestamp(end)?))
    }
    fn log_factor(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(py, self.0.log_factor(timestamp(start)?, timestamp(end)?))
    }
    fn forward_rate(&self, py: Python<'_>, time: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(py, self.0.forward_rate(timestamp(time)?))
    }
    fn shifted(&self, py: Python<'_>, shift: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.shifted(real(shift)?))?))
    }
}

fn rate(value: &Bound<'_, PyAny>) -> PyResult<core::Rate> {
    if let Ok(curve) = value.extract::<PyRef<'_, RateTermStructure>>() {
        Ok(curve.0.clone().into())
    } else {
        Ok(real(value)?.into())
    }
}

fn check_changes(changes: Option<&Bound<'_, PyDict>>, fields: &[&str]) -> PyResult<()> {
    if let Some(changes) = changes {
        for key in changes.keys() {
            if !fields.contains(&key.extract::<&str>()?) {
                return Err(PyTypeError::new_err("unexpected replacement field"));
            }
        }
    }
    Ok(())
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
                rate(risk_free_rate)?,
                rate(dividend_yield)?,
                real(volatility)?,
            ),
        )?))
    }
    #[pyo3(signature = (**changes))]
    fn replace(&self, py: Python<'_>, changes: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(changes, &["risk_free_rate", "dividend_yield", "volatility"])?;
        let mut update = core::BlackScholesMertonParameterChanges::default();
        if let Some(changes) = changes {
            update.risk_free_rate = changes
                .get_item("risk_free_rate")?
                .as_ref()
                .map(rate)
                .transpose()?;
            update.dividend_yield = changes
                .get_item("dividend_yield")?
                .as_ref()
                .map(rate)
                .transpose()?;
            update.volatility = changes
                .get_item("volatility")?
                .as_ref()
                .map(real)
                .transpose()?;
        }
        Ok(Self(domain(py, self.0.replace(update))?))
    }
    #[getter]
    fn risk_free_rate(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let curve = self.0.risk_free_curve();
        if let Some(rate) = curve.flat_rate() {
            Ok(rate.into_pyobject(py)?.into_any().unbind())
        } else {
            Ok(Py::new(py, RateTermStructure(curve.clone()))?.into_any())
        }
    }
    #[getter]
    fn dividend_yield(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let curve = self.0.dividend_curve();
        if let Some(rate) = curve.flat_rate() {
            Ok(rate.into_pyobject(py)?.into_any().unbind())
        } else {
            Ok(Py::new(py, RateTermStructure(curve.clone()))?.into_any())
        }
    }
    #[getter]
    fn risk_free_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.risk_free_curve().clone())
    }
    #[getter]
    fn dividend_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.dividend_curve().clone())
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
    #[pyo3(signature = (**changes))]
    fn replace(&self, py: Python<'_>, changes: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(
            changes,
            &[
                "model_parameters",
                "spot_price",
                "valuation_time",
                "calendar",
            ],
        )?;
        let mut update = core::PricingContextChanges::default();
        if let Some(changes) = changes {
            update.model_parameters = changes
                .get_item("model_parameters")?
                .map(|value| -> PyResult<_> {
                    Ok(value
                        .extract::<PyRef<'_, BlackScholesMertonParameters>>()?
                        .0
                        .clone())
                })
                .transpose()?;
            update.spot_price = changes
                .get_item("spot_price")?
                .as_ref()
                .map(real)
                .transpose()?;
            update.valuation_time = changes
                .get_item("valuation_time")?
                .as_ref()
                .map(timestamp)
                .transpose()?;
            update.calendar = changes
                .get_item("calendar")?
                .map(|value| -> PyResult<_> { Ok(value.extract::<TradingCalendar>()?.0) })
                .transpose()?;
        }
        Ok(Self(domain(py, self.0.replace(update))?))
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
    module.add_class::<RateTermStructure>()?;
    module.add_class::<TradingCalendar>()?;
    module.add_class::<PricingContext>()?;
    module.add_class::<AnalyticVanillaEngine>()?;
    module.add_function(wrap_pyfunction!(weekdays_calendar, module)?)?;
    Ok(())
}
