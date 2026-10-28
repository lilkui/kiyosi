//! Python representation adapters; all domain validation lives in kiyosi-core.

use kiyosi_core as core;
mod american;
mod analytics;
mod asian;
mod barrier;
mod binary_barrier;
mod binomial;
mod digital;
mod double_barrier;
mod finite_difference;
mod monte_carlo;
mod quadrature;
use pyo3::exceptions::{PyException, PyIndexError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDate, PyDateAccess, PyDateTime, PyDict, PyTimeAccess};

pyo3::create_exception!(_native, KiyosiError, PyException);

fn domain<T>(py: Python<'_>, result: core::Result<T>) -> PyResult<T> {
    result.map_err(|error| {
        if let Some(native) = error.operational_error() {
            return match native.raw_os_error() {
                Some(code) => std::io::Error::from_raw_os_error(code).into(),
                None => std::io::Error::new(native.kind(), native.to_string()).into(),
            };
        }
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

fn direction(value: &Bound<'_, PyAny>) -> PyResult<core::OptionType> {
    match value.extract::<&str>()? {
        "call" => Ok(core::OptionType::Call),
        "put" => Ok(core::OptionType::Put),
        _ => Err(PyValueError::new_err("option_type must be call or put")),
    }
}

fn integer(value: &Bound<'_, PyAny>) -> PyResult<i32> {
    check_integer(value)?;
    value.extract()
}

fn check_integer(value: &Bound<'_, PyAny>) -> PyResult<()> {
    if value.is_instance_of::<PyBool>()
        || !value.is_instance(&value.py().import("numbers")?.getattr("Integral")?)?
    {
        return Err(PyTypeError::new_err("expected an integer, excluding bool"));
    }
    Ok(())
}

fn dates(value: &Bound<'_, PyAny>) -> PyResult<Vec<core::Date>> {
    value.try_iter()?.map(|item| date(&item?)).collect()
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
        let direction = direction(option_type)?;
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
    fn trading_days_per_year(&self) -> i32 {
        self.0.trading_days_per_year()
    }

    fn __repr__(&self) -> String {
        format!(
            "TradingCalendar(trading_days_per_year={})",
            self.0.trading_days_per_year()
        )
    }

    fn is_trading_day(&self, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0.is_trading_day(date(value)?))
    }

    fn adjust<'py>(
        &self,
        py: Python<'py>,
        nominal: &Bound<'_, PyAny>,
        convention: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyDate>> {
        let nominal = date(nominal)?;
        let convention = match convention.extract::<&str>()? {
            "following" => core::BusinessDayConvention::Following,
            "preceding" => core::BusinessDayConvention::Preceding,
            _ => {
                return Err(PyValueError::new_err(
                    "convention must be following or preceding",
                ));
            }
        };
        python_date(py, domain(py, self.0.adjust(nominal, convention))?)
    }

    fn trading_days_between(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<i32> {
        domain(py, self.0.trading_days_between(date(start)?, date(end)?))
    }

    fn trading_year_fraction(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(py, self.0.trading_year_fraction(date(start)?, date(end)?))
    }
}

#[pyfunction]
fn weekdays_calendar() -> TradingCalendar {
    TradingCalendar::default()
}

#[pyfunction]
fn all_days_calendar() -> TradingCalendar {
    TradingCalendar(core::TradingCalendar::all_days())
}

#[pyfunction]
fn sse_calendar() -> TradingCalendar {
    TradingCalendar(core::TradingCalendar::sse())
}

#[pyfunction]
#[pyo3(signature = (*, base, **overrides))]
fn custom_calendar(
    py: Python<'_>,
    base: &TradingCalendar,
    overrides: Option<&Bound<'_, PyDict>>,
) -> PyResult<TradingCalendar> {
    check_changes(
        overrides,
        &["closed_dates", "open_dates", "trading_days_per_year"],
    )?;
    let mut closed = Vec::new();
    let mut open = Vec::new();
    let mut basis = None;
    if let Some(overrides) = overrides {
        closed = overrides
            .get_item("closed_dates")?
            .as_ref()
            .map(dates)
            .transpose()?
            .unwrap_or_default();
        open = overrides
            .get_item("open_dates")?
            .as_ref()
            .map(dates)
            .transpose()?
            .unwrap_or_default();
        basis = overrides
            .get_item("trading_days_per_year")?
            .filter(|value| !value.is_none())
            .as_ref()
            .map(integer)
            .transpose()?;
    }
    Ok(TradingCalendar(domain(
        py,
        core::TradingCalendar::custom(base.0.clone(), closed, open, basis),
    )?))
}

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
struct ObservationSchedule(core::ObservationSchedule);

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
fn fixed_interval_schedule(
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
fn monthly_schedule(
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
        TradingCalendar(self.0.calendar().clone())
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

    #[pyo3(signature = (instrument, context, greeks=None, **settings))]
    fn price_with_greeks(
        &self,
        py: Python<'_>,
        instrument: &EuropeanOption,
        context: &PricingContext,
        greeks: Option<&Bound<'_, PyAny>>,
        settings: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<analytics::PricingResult> {
        let request = analytics::request(greeks, settings)?;
        let shifts = analytics::shifts(settings)?;
        Ok(analytics::PricingResult(domain(
            py,
            core::AnalyticVanillaEngine.price_with_greeks(
                &instrument.0,
                &context.0,
                request,
                shifts,
            ),
        )?))
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
    let names = core::Greek::ALL.map(|g| g.name());
    module.add(
        "Greek",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(pyo3::types::PyTuple::new(py, names)?)?,
    )?;
    module.add_class::<asian::GeometricAveragePriceOption>()?;
    module.add_class::<asian::ArithmeticAveragePriceOption>()?;
    module.add_class::<asian::AnalyticGeometricAveragePriceEngine>()?;
    module.add_class::<asian::TurnbullWakemanArithmeticAveragePriceEngine>()?;
    binary_barrier::register(module)?;
    module.add_class::<double_barrier::DoubleBarrierOption>()?;
    module.add_class::<double_barrier::AnalyticDoubleBarrierEngine>()?;
    module.add_class::<barrier::BarrierOption>()?;
    module.add_class::<barrier::AnalyticBarrierEngine>()?;
    module.add_class::<barrier::QuadratureBarrierEngine>()?;
    module.add_class::<barrier::CoxRossRubinsteinBarrierEngine>()?;
    module.add_class::<EuropeanOption>()?;
    module.add_class::<american::AmericanOption>()?;
    module.add_class::<american::BjerksundStenslandVanillaEngine>()?;
    module.add_class::<digital::CashOrNothingOption>()?;
    module.add_class::<digital::AssetOrNothingOption>()?;
    module.add_class::<digital::AnalyticDigitalEngine>()?;
    module.add_class::<binomial::CoxRossRubinsteinVanillaEngine>()?;
    module.add_class::<binomial::CoxRossRubinsteinDigitalEngine>()?;
    module.add_class::<quadrature::QuadratureVanillaEngine>()?;
    module.add_class::<quadrature::QuadratureDigitalEngine>()?;
    module.add_class::<monte_carlo::MonteCarloVanillaEngine>()?;
    module.add_class::<monte_carlo::MonteCarloDigitalEngine>()?;
    module.add_class::<monte_carlo::MonteCarloAsianEngine>()?;
    module.add_class::<monte_carlo::MonteCarloEstimate>()?;
    module.add(
        "MonteCarloSampling",
        py.import("typing")?
            .getattr("Literal")?
            .get_item(("pseudo_random", "sobol"))?,
    )?;
    module.add_class::<finite_difference::FiniteDifferenceVanillaEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceDigitalEngine>()?;
    module.add_class::<finite_difference::FiniteDifferenceBarrierEngine>()?;
    module.add(
        "FiniteDifferenceScheme",
        py.import("typing")?.getattr("Literal")?.get_item((
            "explicit_euler",
            "implicit_euler",
            "crank_nicolson",
        ))?,
    )?;
    module.add_class::<BlackScholesMertonParameters>()?;
    module.add_class::<RateTermStructure>()?;
    module.add_class::<TradingCalendar>()?;
    module.add_class::<ObservationSchedule>()?;
    module.add_class::<PricingContext>()?;
    module.add_class::<AnalyticVanillaEngine>()?;
    module.add_class::<analytics::PricingResult>()?;
    module.add_function(wrap_pyfunction!(analytics::implied_volatility, module)?)?;
    module.add_function(wrap_pyfunction!(weekdays_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(all_days_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(sse_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(custom_calendar, module)?)?;
    module.add_function(wrap_pyfunction!(fixed_interval_schedule, module)?)?;
    module.add_function(wrap_pyfunction!(monthly_schedule, module)?)?;
    Ok(())
}
