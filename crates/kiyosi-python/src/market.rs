use crate::conversions::{
    python_date, python_timestamp, python_timestamps, python_tuple, real, timestamp,
};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyTuple};

macro_rules! rate_properties {
    ($name:ident, @replace $changes:ident [$($parameter:ident),+]
     {$($field:ident: $value:expr),+ $(,)?}, [$(#[$extra_error:meta])*], $($extra:item),* $(,)?) => {
        value_methods!(@documented $name, $changes, [$($parameter),+, variance_time_convention, calendar], |self|
            {$($field: $value),+, variance_time_convention: Some($crate::market::updated_variance_time_convention(self.0.variance_time_convention(), variance_time_convention, calendar)?)}, [
                /// Return a validated copy with the supplied fields applied.
                ///
                /// Parameters
                /// ----------
                /// **fields : object
                ///     Constructor fields to replace. Omitted fields and fields set to
                ///     Ellipsis default to the instance's values. None clears the calendar.
                ///     A replacement clock uses the supplied or defaulted volatility value
                ///     directly. See the class parameters for units.
                ///
                /// Returns
                /// -------
                /// object
                ///     Validated copy of the same class.
                ///
                /// Raises
                /// ------
                /// InvalidInputError
                ///     If the merged fields fail model validation.
                $(#[$extra_error])*
                /// TypeError
                ///     If a representation or keyword is invalid.
                /// ValueError
                ///     If a time convention choice is unknown.
            ], {
            $($extra)*

            /// Return the model volatility annualization choice.
            #[getter]
            fn variance_time_convention(&self) -> &'static str {
                $crate::market::variance_time_convention_name(self.0.variance_time_convention())
            }
            /// Return an owned model calendar, or None for A/365F.
            #[getter]
            fn calendar(&self) -> Option<$crate::calendar::TradingCalendar> {
                self.0.variance_time_convention().calendar().cloned().map($crate::calendar::TradingCalendar)
            }


            /// Return the scalar rate for dated or undated constant curves.
            ///
            /// Use ``risk_free_curve`` to preserve coverage when passing rates to constructors
            /// or replacements.
            ///
            /// Raises
            /// ------
            /// UnsupportedOperationError
            ///     If the curve is nonconstant.
            ///
            #[getter]
            fn flat_risk_free_rate(&self) -> pyo3::PyResult<f64> {
                $crate::error::domain(self.0.flat_risk_free_rate())
            }
            /// Return the scalar yield for dated or undated constant curves.
            ///
            /// Use ``dividend_curve`` to preserve coverage when passing yields to constructors
            /// or replacements.
            ///
            /// Raises
            /// ------
            /// UnsupportedOperationError
            ///     If the curve is nonconstant.
            ///
            #[getter]
            fn flat_dividend_yield(&self) -> pyo3::PyResult<f64> {
                $crate::error::domain(self.0.flat_dividend_yield())
            }
            /// Return an owned copy of the risk-free curve.
            #[getter]
            fn risk_free_curve(&self) -> $crate::market::RateTermStructure {
                $crate::market::RateTermStructure(self.0.risk_free_curve().clone())
            }
            /// Return an owned copy of the dividend curve.
            #[getter]
            fn dividend_curve(&self) -> $crate::market::RateTermStructure {
                $crate::market::RateTermStructure(self.0.dividend_curve().clone())
            }
        });
    };
}
pub(crate) use rate_properties;

pub(crate) fn variance_time_convention_name(
    value: &kiyosi::VarianceTimeConvention,
) -> &'static str {
    match value {
        kiyosi::VarianceTimeConvention::Actual365Fixed => "actual_365_fixed",
        kiyosi::VarianceTimeConvention::Business(_) => "business",
    }
}

pub(crate) fn updated_variance_time_convention(
    current: &kiyosi::VarianceTimeConvention,
    choice: Option<&Bound<'_, PyAny>>,
    calendar: Option<&Bound<'_, PyAny>>,
) -> PyResult<kiyosi::VarianceTimeConvention> {
    let business = match choice {
        Some(value) => match value.extract::<&str>()? {
            "actual_365_fixed" => false,
            "business" => true,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "variance_time_convention must be actual_365_fixed or business",
                ));
            }
        },
        None => matches!(current, kiyosi::VarianceTimeConvention::Business(_)),
    };
    let calendar = match calendar {
        None => current.calendar().cloned(),
        Some(value) if value.is_none() => None,
        Some(value) => Some(
            value
                .extract::<PyRef<'_, crate::calendar::TradingCalendar>>()?
                .0
                .clone(),
        ),
    };
    domain(kiyosi::VarianceTimeConvention::from_inputs(
        business, calendar,
    ))
}

/// Rate term structure with log-linear discount factors.
///
/// Parameters
/// ----------
/// reference_time : datetime.datetime
///     Inclusive curve reference; factors equal one at this instant.
/// times : iterable of datetime.datetime
///     Strictly increasing nodes after the reference.
/// factors : iterable of float
///     Positive finite factors at the nodes; log factors interpolate linearly.
///
/// Raises
/// ------
/// InvalidInputError
///     If nodes or factors are empty or mismatched, times do not increase
///     after the reference, factors are nonpositive or non-finite, or implied
///     forward rates are non-finite.
///
/// Notes
/// -----
/// Uses Actual/365 Fixed by default. Factor nodes interpolate log-linearly.
/// A bounded flat quote can accrue on a calendar. Dated curves reject times
/// outside their coverage. Negative rates are allowed.
/// Constructing a large node curve releases the GIL and polls Python signals
/// during core validation.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RateTermStructure(pub(crate) kiyosi::RateTermStructure);

#[pymethods]
impl RateTermStructure {
    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        crate::representation::curve(slf.py(), &slf.borrow().0)
    }

    #[new]
    #[pyo3(signature = (*, reference_time, times, factors))]
    fn new(
        py: Python<'_>,
        reference_time: &Bound<'_, PyAny>,
        times: &Bound<'_, PyAny>,
        factors: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let reference = timestamp(reference_time)?;
        let nodes = crate::conversions::timestamps(times)?;
        let factors = crate::conversions::reals(factors)?;
        crate::calendar::run_date_scan(py, nodes.len() as u64, || {
            kiyosi::RateTermStructure::new(reference, nodes, factors)
        })
        .map(Self)
    }
    /// Create a continuously compounded flat rate curve.
    ///
    /// Parameters
    /// ----------
    /// rate : float
    ///     Finite annual rate in decimal units; negative rates are allowed.
    /// reference_time : datetime.datetime, optional
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
    ///
    #[staticmethod]
    #[pyo3(signature = (rate, *, reference_time=None))]
    fn flat(rate: &Bound<'_, PyAny>, reference_time: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::RateTermStructure::flat(
            real(rate)?,
            reference_time.map(timestamp).transpose()?,
        ))?))
    }
    /// Create a constant annual quote with inclusive coverage bounds.
    ///
    /// Parameters
    /// ----------
    /// rate : float
    ///     Finite continuously compounded annual rate; negative rates are allowed.
    /// reference_time, max_time : datetime.datetime
    ///     Inclusive coverage bounds. Equal bounds select a single instant.
    /// calendar : TradingCalendar or None, optional
    ///     None uses Actual/365 Fixed. With a calendar, accrued time is the
    ///     elapsed fraction of each open date divided by its annualization
    ///     basis. Closed dates contribute zero.
    ///
    /// Returns
    /// -------
    /// RateTermStructure
    ///     Owned curve retaining the quote and coverage through shifts. Calendar
    ///     transitions within the bounds appear in nodes.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the rate or an implied forward is non-finite, or max_time precedes
    ///     reference_time.
    ///
    /// Notes
    /// -----
    /// Long calendar scans release the GIL and poll Python signals.
    ///
    #[staticmethod]
    #[pyo3(signature = (rate, *, reference_time, max_time, calendar=None))]
    fn bounded_flat(
        py: Python<'_>,
        rate: &Bound<'_, PyAny>,
        reference_time: &Bound<'_, PyAny>,
        max_time: &Bound<'_, PyAny>,
        calendar: Option<crate::calendar::TradingCalendar>,
    ) -> PyResult<Self> {
        let rate = real(rate)?;
        let reference = timestamp(reference_time)?;
        let end = timestamp(max_time)?;
        let curve = match calendar {
            Some(calendar) => crate::calendar::run_date_scan(
                py,
                reference.date().days_until(end.date()).unsigned_abs(),
                || {
                    kiyosi::RateTermStructure::bounded_flat_with_calendar(
                        rate, reference, end, calendar.0,
                    )
                },
            )?,
            None => domain(kiyosi::RateTermStructure::bounded_flat(
                rate, reference, end,
            ))?,
        };
        Ok(Self(curve))
    }
    /// Return the scalar rate when all implied forwards are exactly constant.
    ///
    /// This read-only query returns None for nonconstant curves.
    ///
    #[getter]
    fn exact_flat_rate(&self) -> Option<f64> {
        self.0.exact_flat_rate()
    }
    /// Return the inclusive lower coverage bound, or None if undated.
    #[getter]
    fn reference_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .reference_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    /// Return the inclusive upper bound, or None for an unbounded flat curve.
    #[getter]
    fn max_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        self.0
            .max_time()
            .map(|time| python_timestamp(py, time))
            .transpose()
    }
    /// Return a copy of factor nodes or calendar transition times.
    #[getter]
    fn nodes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        python_timestamps(py, self.0.nodes())
    }
    /// Return owned node factors including shifts, allowing underflow to zero.
    ///
    /// Raises
    /// ------
    /// ComputationFailureError
    ///     If a log factor is non-finite or exponentiation overflows.
    ///
    /// Notes
    /// -----
    /// Calendar curves return factors at their transition times.
    /// Reconstruct factor-node curves from base_factors, then apply
    /// shifted(parallel_shift).
    /// Large factor calculations release the GIL and poll Python signals.
    ///
    #[getter]
    fn factors<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let factors = if self.0.nodes().len() > 1024 {
            crate::cancellation::run(py, || self.0.factors())?
        } else {
            self.0.factors()
        };
        python_tuple(py, domain(factors)?, Ok)
    }
    /// Return original node factors without parallel shifts.
    ///
    /// Quote-based curves have no supplied factors. For factor-node curves, pass these factors with
    /// reference_time and nodes to RateTermStructure, then call
    /// shifted(parallel_shift).
    ///
    #[getter]
    fn base_factors<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        python_tuple(py, self.0.base_factors(), Ok)
    }
    /// Return the accumulated node-curve parallel shift in decimal units.
    ///
    /// Quote-based curves include shifts in their rate and return zero here.
    ///
    #[getter]
    fn parallel_shift(&self) -> f64 {
        self.0.parallel_shift()
    }
    /// Return the factor over an ordered, covered interval.
    ///
    /// Parameters
    /// ----------
    /// start_time, end_time : datetime.datetime
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
    ///
    fn factor(&self, start_time: &Bound<'_, PyAny>, end_time: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(self.0.factor(timestamp(start_time)?, timestamp(end_time)?))
    }
    /// Return the log factor over an ordered, covered interval.
    ///
    /// Parameters
    /// ----------
    /// start_time, end_time : datetime.datetime
    ///     Ordered interval endpoints.
    ///
    /// Returns
    /// -------
    /// float
    ///     Log of the interval factor, using the curve's accrual convention.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If endpoints are reversed or outside curve coverage.
    /// ComputationFailureError
    ///     If the result is non-finite.
    ///
    fn log_factor(
        &self,
        start_time: &Bound<'_, PyAny>,
        end_time: &Bound<'_, PyAny>,
    ) -> PyResult<f64> {
        domain(
            self.0
                .log_factor(timestamp(start_time)?, timestamp(end_time)?),
        )
    }
    /// Return the right-hand instantaneous forward rate.
    ///
    /// Parameters
    /// ----------
    /// time : datetime.datetime
    ///     Query instant. The coverage endpoint uses its left interval.
    ///
    /// Returns
    /// -------
    /// float
    ///     Continuously compounded forward rate in Actual/365 Fixed decimal units.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the instant is outside curve coverage.
    ///
    fn forward_rate(&self, time: &Bound<'_, PyAny>) -> PyResult<f64> {
        domain(self.0.forward_rate(timestamp(time)?))
    }
    /// Return a parallel-shifted copy of this curve.
    ///
    /// Parameters
    /// ----------
    /// shift : float
    ///     Finite additive rate shift in the curve's annual quote units.
    ///
    /// Returns
    /// -------
    /// RateTermStructure
    ///     Independent curve with the source curve's coverage bounds.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the shift or resulting rates are non-finite.
    ///
    /// Notes
    /// -----
    /// Shifting a large node curve releases the GIL and polls Python signals.
    ///
    fn shifted(&self, py: Python<'_>, shift: &Bound<'_, PyAny>) -> PyResult<Self> {
        let shift = real(shift)?;
        crate::calendar::run_date_scan(py, self.0.base_factors().len() as u64, || {
            self.0.shifted(shift)
        })
        .map(Self)
    }
}

pub(crate) fn rate(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::Rate> {
    if let Ok(curve) = value.extract::<PyRef<'_, RateTermStructure>>() {
        Ok(curve.0.clone().into())
    } else {
        Ok(real(value)?.into())
    }
}

/// Black-Scholes-Merton (BSM) model parameters.
///
/// Parameters
/// ----------
/// risk_free_rate : float or RateTermStructure
///     Continuously compounded risk-free rate or owned rate curve.
/// dividend_yield : float or RateTermStructure
///     Continuously compounded dividend yield or owned rate curve.
/// volatility : float
///     Nonnegative finite annualized log-return volatility in decimal units.
///     Zero selects the exact deterministic path implied by the rate curves.
///
/// variance_time_convention : {"actual_365_fixed", "business"}, optional
///     Volatility annualization and variance clock. The Rust default is
///     actual_365_fixed. The rate curves define rate and dividend accrual.
/// calendar : TradingCalendar or None, optional
///     Required for business time and rejected for actual_365_fixed. Business
///     time accumulates uniformly over each trading date's 24 hours, divided
///     by the calendar's fixed annualization basis; closed dates contribute zero.
///     Contract observation calendars are independent. Replacement preserves
///     omitted volatility values without unit conversion when changing the clock.
///
/// Raises
/// ------
/// InvalidInputError
///     If scalar rates are non-finite or volatility is negative or non-finite.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BlackScholesMertonParameters(pub(crate) kiyosi::BlackScholesMertonParameters);

rate_properties!(
    BlackScholesMertonParameters,
    @replace BlackScholesMertonParameterChanges [risk_free_rate, dividend_yield, volatility] {
        risk_free_rate: risk_free_rate.map(rate).transpose()?,
        dividend_yield: dividend_yield.map(rate).transpose()?,
        volatility: volatility.map(real).transpose()?,
    },
    [],
    #[new]
    #[pyo3(signature = (*, risk_free_rate, dividend_yield, volatility, variance_time_convention=crate::conversions::OMITTED, calendar=None))]
    fn new(
        risk_free_rate: &Bound<'_, PyAny>,
        dividend_yield: &Bound<'_, PyAny>,
        volatility: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = crate::conversions::supplied)] variance_time_convention: Option<&Bound<'_, PyAny>>,
        calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::BlackScholesMertonParameters::new_with_variance_time_convention(
            rate(risk_free_rate)?,
            rate(dividend_yield)?,
            real(volatility)?,
            updated_variance_time_convention(&kiyosi::VarianceTimeConvention::default(), variance_time_convention, calendar)?,
        ))?))
    },
    /// Return nonnegative finite annualized volatility in decimal units.
    #[getter]
    fn volatility(&self) -> f64 {
        self.0.volatility()
    },
);

/// Market snapshot for one valuation instant.
///
/// Parameters
/// ----------
/// model_parameters : BlackScholesMertonParameters, LocalVolatilityParameters or HestonParameters
///     Validated rates and the selected volatility or variance model.
/// spot_price : float
///     Positive finite underlying spot price.
/// valuation_time : datetime.datetime
///     Valuation time.
///
/// Raises
/// ------
/// InvalidInputError
///     If spot_price is nonpositive or non-finite or valuation_time is outside
///     either rate curve or the local-volatility surface. Contract-life and
///     expiry coverage checks occur at pricing.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PricingContext(pub(crate) kiyosi::PricingContext);

value_methods!(PricingContext, PricingContextChanges,
    [model_parameters, spot_price, valuation_time], {
        model_parameters: model_parameters.map(model_parameters_value).transpose()?,
        spot_price: spot_price.map(real).transpose()?,
        valuation_time: valuation_time.map(timestamp).transpose()?,
    }, {
    #[new]
    #[pyo3(signature = (*, model_parameters, spot_price, valuation_time))]
    fn new(
        model_parameters: &Bound<'_, PyAny>,
        spot_price: &Bound<'_, PyAny>,
        valuation_time: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::PricingContext::new(
            model_parameters_value(model_parameters)?,
            real(spot_price)?,
            timestamp(valuation_time)?,
        ))?))
    }

    /// Return an owned copy of the model parameters.
    #[getter]
    fn model_parameters(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.0.model_parameters() {
            kiyosi::ModelParameters::Heston(m) => {
                Ok(Py::new(py, crate::heston::HestonParameters(m.clone()))?.into_any())
            }
            kiyosi::ModelParameters::BlackScholesMerton(m) => {
                Ok(Py::new(py, BlackScholesMertonParameters(m.clone()))?.into_any())
            }
            kiyosi::ModelParameters::LocalVolatility(m) => Ok(Py::new(
                py,
                crate::local_volatility::LocalVolatilityParameters(m.clone()),
            )?
            .into_any()),
        }
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
    /// Return valuation time.
    #[getter]
    fn valuation_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.valuation_time())
    }
});

fn model_parameters_value(value: &Bound<'_, PyAny>) -> PyResult<kiyosi::ModelParameters> {
    if let Ok(m) = value.extract::<PyRef<'_, crate::heston::HestonParameters>>() {
        return Ok(m.0.clone().into());
    }
    if let Ok(m) = value.extract::<PyRef<'_, BlackScholesMertonParameters>>() {
        return Ok(m.0.clone().into());
    }
    if let Ok(m) = value.extract::<PyRef<'_, crate::local_volatility::LocalVolatilityParameters>>()
    {
        return Ok(m.0.clone().into());
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "expected BlackScholesMertonParameters, LocalVolatilityParameters or HestonParameters",
    ))
}
