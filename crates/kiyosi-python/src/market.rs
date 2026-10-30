use crate::calendar::TradingCalendar;
use crate::conversions::{check_changes, python_date, python_timestamp, real, timestamp};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDict};

/// Immutable rate curve with log-linear factors and Actual/365 Fixed time.
///
/// Parameters
/// ----------
/// reference_time : datetime.date or datetime.datetime
///     Inclusive curve reference; factors equal one at this instant.
/// times : iterable of datetime.date or datetime.datetime
///     Strictly increasing nodes after the reference.
/// factors : iterable of float
///     Positive finite factors at the nodes; log factors interpolate
///     linearly.
///
/// Raises
/// ------
/// InvalidInputError
///     If nodes or factors are empty or mismatched, times do not increase
///     after the reference, factors are nonpositive/non-finite, or implied
///     forward rates are non-finite.
///
/// Notes
/// -----
/// Dated curves reject times before their reference; node curves also reject
/// times after the last node. Negative rates are allowed.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RateTermStructure(pub(crate) kiyosi::RateTermStructure);

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
            kiyosi::RateTermStructure::new(reference, nodes, factors),
        )?))
    }
    /// Create a continuously compounded flat rate curve.
    ///
    /// Parameters
    /// ----------
    /// rate : float
    ///     Finite annual rate in decimal units; negative rates are allowed.
    /// reference_time : datetime.date or datetime.datetime, optional
    ///     Inclusive lower coverage bound, or None for an undated curve.
    ///
    /// Returns
    /// -------
    /// RateTermStructure
    ///     Owned flat curve with no upper coverage bound.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the rate is non-finite.
    #[staticmethod]
    #[pyo3(signature = (rate, *, reference_time=None))]
    fn flat(
        py: Python<'_>,
        rate: &Bound<'_, PyAny>,
        reference_time: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Self(domain(
            py,
            kiyosi::RateTermStructure::flat(
                real(rate)?,
                reference_time.map(timestamp).transpose()?,
            ),
        )?))
    }
    /// Return whether the rate curve is explicitly flat.
    #[getter]
    fn is_flat(&self) -> bool {
        self.0.is_flat()
    }
    /// Return the inclusive lower coverage bound, or None if undated.
    #[getter]
    fn reference_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .reference_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    /// Return the final node time, or None for a flat curve.
    #[getter]
    fn max_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .max_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    /// Return a copy of the supplied curve node times.
    #[getter]
    fn nodes<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        self.0
            .nodes()
            .iter()
            .map(|&time| python_timestamp(py, time))
            .collect()
    }
    /// Return copied node factors, including any parallel shift.
    #[getter]
    fn factors(&self) -> Vec<f64> {
        self.0.factors()
    }
    /// Return the factor over an ordered, covered interval.
    ///
    /// Parameters
    /// ----------
    /// start, end : datetime.date or datetime.datetime
    ///     Ordered interval endpoints.
    ///
    /// Returns
    /// -------
    /// float
    ///     Interval factor; underflow to zero is allowed.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If endpoints are reversed or outside curve coverage.
    /// ComputationFailureError
    ///     If the log factor is non-finite or exponentiation overflows.
    fn factor(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(py, self.0.factor(timestamp(start)?, timestamp(end)?))
    }
    /// Return the log factor over an ordered, covered interval.
    ///
    /// Parameters
    /// ----------
    /// start, end : datetime.date or datetime.datetime
    ///     Ordered interval endpoints.
    ///
    /// Returns
    /// -------
    /// float
    ///     Log of the interval factor, using Actual/365 Fixed time.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If endpoints are reversed or outside curve coverage.
    /// ComputationFailureError
    ///     If the result is non-finite.
    fn log_factor(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(py, self.0.log_factor(timestamp(start)?, timestamp(end)?))
    }
    /// Return the right-hand instantaneous forward rate.
    ///
    /// Parameters
    /// ----------
    /// time : datetime.date or datetime.datetime
    ///     Query instant. The final node uses its left interval.
    ///
    /// Returns
    /// -------
    /// float
    ///     Annual continuously compounded forward rate in decimal units.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the instant is outside curve coverage.
    fn forward_rate(&self, py: Python<'_>, time: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(py, self.0.forward_rate(timestamp(time)?))
    }
    /// Create a parallel rate shift without changing this curve.
    ///
    /// Parameters
    /// ----------
    /// shift : float
    ///     Finite additive rate shift in decimal units.
    ///
    /// Returns
    /// -------
    /// RateTermStructure
    ///     Independent curve with unchanged coverage.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the shift or resulting rates are non-finite.
    fn shifted(&self, py: Python<'_>, shift: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(domain(py, self.0.shifted(real(shift)?))?))
    }
}

fn rate(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Rate> {
    if let Ok(curve) = value.extract::<PyRef<'_, RateTermStructure>>() {
        Ok(curve.0.clone().into())
    } else {
        Ok(real(value)?.into())
    }
}

/// Immutable BSM rates and strictly positive annual volatility.
///
/// Parameters
/// ----------
/// risk_free_rate : float or RateTermStructure
///     Continuously compounded risk-free rate or owned rate curve.
/// dividend_yield : float or RateTermStructure
///     Continuously compounded dividend yield or owned rate curve.
/// volatility : float
///     Positive finite annual volatility in decimal units.
///
/// Raises
/// ------
/// InvalidInputError
///     If scalar rates are non-finite or volatility is nonpositive/non-finite.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BlackScholesMertonParameters(pub(crate) kiyosi::BlackScholesMertonParameters);

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
            kiyosi::BlackScholesMertonParameters::new(
                rate(risk_free_rate)?,
                rate(dividend_yield)?,
                real(volatility)?,
            ),
        )?))
    }
    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// risk_free_rate : float or RateTermStructure, optional
    ///     Continuously compounded risk-free rate or owned rate curve.
    /// dividend_yield : float or RateTermStructure, optional
    ///     Continuously compounded dividend yield or owned rate curve.
    /// volatility : float, optional
    ///     Positive finite annual volatility in decimal units.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (**changes))]
    fn replace(&self, py: Python<'_>, changes: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        check_changes(changes, &["risk_free_rate", "dividend_yield", "volatility"])?;
        let mut update = kiyosi::BlackScholesMertonParameterChanges::default();
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
    /// Return the scalar risk-free rate or an owned node curve.
    #[getter]
    fn risk_free_rate(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let curve = self.0.risk_free_curve();
        if let Some(rate) = curve.flat_rate() {
            Ok(rate.into_pyobject(py)?.into_any().unbind())
        } else {
            Ok(Py::new(py, RateTermStructure(curve.clone()))?.into_any())
        }
    }
    /// Return the scalar dividend yield or an owned node curve.
    #[getter]
    fn dividend_yield(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let curve = self.0.dividend_curve();
        if let Some(rate) = curve.flat_rate() {
            Ok(rate.into_pyobject(py)?.into_any().unbind())
        } else {
            Ok(Py::new(py, RateTermStructure(curve.clone()))?.into_any())
        }
    }
    /// Return an owned copy of the risk-free curve.
    #[getter]
    fn risk_free_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.risk_free_curve().clone())
    }
    /// Return an owned copy of the dividend curve.
    #[getter]
    fn dividend_curve(&self) -> RateTermStructure {
        RateTermStructure(self.0.dividend_curve().clone())
    }
    /// Return positive finite annual volatility in decimal units.
    #[getter]
    fn volatility(&self) -> f64 {
        self.0.volatility()
    }
}

/// Immutable market snapshot for one valuation instant.
///
/// Parameters
/// ----------
/// model_parameters : BlackScholesMertonParameters
///     Validated BSM rates and volatility.
/// spot_price : float
///     Positive finite underlying spot price.
/// valuation_time : datetime.date or datetime.datetime
///     Valuation instant. Dates mean midnight; aware datetimes convert to
///     UTC.
/// calendar : TradingCalendar, optional
///     Trading calendar. Defaults to weekdays with a 252-day basis.
///
/// Raises
/// ------
/// InvalidInputError
///     If spot_price is nonpositive/non-finite or valuation_time is outside
///     either rate curve. Contract-life checks occur at pricing.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PricingContext(pub(crate) kiyosi::PricingContext);

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
            kiyosi::PricingContext::with_calendar(
                model_parameters.0.clone(),
                real(spot_price)?,
                timestamp(valuation_time)?,
                calendar.0,
            ),
        )?))
    }
    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// model_parameters : BlackScholesMertonParameters, optional
    ///     Validated BSM rates and volatility.
    /// spot_price : float, optional
    ///     Positive finite underlying spot price.
    /// valuation_time : datetime.date or datetime.datetime, optional
    ///     Valuation instant. Dates mean midnight; aware datetimes convert to
    ///     UTC.
    /// calendar : TradingCalendar, optional
    ///     Trading calendar. Defaults to weekdays with a 252-day basis.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
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
        let mut update = kiyosi::PricingContextChanges::default();
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
    /// Return an owned copy of the model parameters.
    #[getter]
    fn model_parameters(&self) -> BlackScholesMertonParameters {
        BlackScholesMertonParameters(self.0.model_parameters().clone())
    }
    /// Return positive finite underlying spot price.
    #[getter]
    fn spot_price(&self) -> f64 {
        self.0.spot_price()
    }
    /// Return the date containing the valuation instant.
    #[getter]
    fn valuation_date<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDate>> {
        python_date(py, self.0.valuation_date())
    }
    /// Return valuation instant. Dates mean midnight; aware datetimes convert to UTC.
    #[getter]
    fn valuation_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.valuation_time())
    }
    /// Return an owned copy of the trading calendar.
    #[getter]
    fn calendar(&self) -> TradingCalendar {
        TradingCalendar(self.0.calendar().clone())
    }
}
