use crate::calendar::TradingCalendar;
use crate::conversions::{check_changes, python_date, python_timestamp, real, timestamp};
use crate::{core, error::domain};
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RateTermStructure(pub(crate) core::RateTermStructure);

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

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BlackScholesMertonParameters(pub(crate) core::BlackScholesMertonParameters);

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

#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PricingContext(pub(crate) core::PricingContext);

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
