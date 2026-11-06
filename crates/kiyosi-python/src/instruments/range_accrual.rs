use crate::calendar::TradingCalendar;
use crate::conversions::{integer, python_timestamp, real, supplied, timestamp};
use crate::error::domain;
use pyo3::prelude::*;

fn history(value: &Bound<'_, PyAny>) -> PyResult<Option<i32>> {
    if value.is_none() {
        Ok(None)
    } else {
        integer(value).map(Some)
    }
}

/// Range coupon paid at expiry without principal repayment.
///
/// Parameters
/// ----------
/// notional : float
///     Positive finite coupon notional, without principal repayment.
/// annual_coupon_rate : float
///     Nonnegative finite annual coupon rate in decimal units.
/// lower_bound : float
///     Inclusive nonnegative lower spot boundary.
/// upper_bound : float
///     Inclusive finite upper spot boundary, above the lower boundary.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// observation_clock_time : datetime.time
///     Required daily observation time, without timezone information.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// realized_hit_count : int or None, optional
///     Hits strictly before valuation, or None for unknown history.
///
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays, independently of PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If amounts, bounds or history are invalid, or expiry is not after
///     effective_time, the expiry date is closed in the calendar, or the
///     observation schedule is empty.
///     Pricing checks the inclusive contract life and history
///     against elapsed observations.
///
/// Notes
/// -----
/// Observe trading times in (effective_time, expiry_time]. Bounds are
/// inclusive. Payment is notional times annual coupon times Actual/365 Fixed
/// life times the fraction of observations inside the range. Payment is at expiry;
/// a completed observation schedule still has a discounted future payment.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RangeAccrualOption(pub kiyosi::RangeAccrualOption);

#[pymethods]
impl RangeAccrualOption {
    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// notional : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// annual_coupon_rate : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// lower_bound : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// upper_bound : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// realized_hit_count : int | None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
    ///
    /// Returns
    /// -------
    /// object
    ///     Independent contract of the same class, preserving omitted history and terms.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation; the original is unchanged.
    /// TypeError
    ///     If a representation or field name is invalid.
    #[pyo3(signature = (*, notional=crate::conversions::OMITTED, annual_coupon_rate=crate::conversions::OMITTED, lower_bound=crate::conversions::OMITTED, upper_bound=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, observation_clock_time=crate::conversions::OMITTED, realized_hit_count=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] notional: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] annual_coupon_rate: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] lower_bound: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] upper_bound: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] observation_clock_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] realized_hit_count: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let changes = kiyosi::RangeAccrualOptionChanges {
            calendar: calendar
                .map(|v| v.extract::<TradingCalendar>().map(|v| v.0))
                .transpose()?,
            notional: notional.map(crate::conversions::real).transpose()?,
            annual_coupon_rate: annual_coupon_rate
                .map(crate::conversions::real)
                .transpose()?,
            lower_bound: lower_bound.map(crate::conversions::real).transpose()?,
            upper_bound: upper_bound.map(crate::conversions::real).transpose()?,
            effective_time: effective_time
                .map(crate::conversions::timestamp)
                .transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            observation_clock_time: observation_clock_time
                .map(crate::conversions::clock_time)
                .transpose()?,
            realized_hit_count: realized_hit_count
                .map(|value| {
                    if value.is_none() {
                        Ok(None)
                    } else {
                        crate::conversions::integer(value).map(Some)
                    }
                })
                .transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
    #[new]
    #[expect(clippy::too_many_arguments, reason = "explicit Python contract terms")]
    #[pyo3(signature = (*, notional, annual_coupon_rate, lower_bound, upper_bound, effective_time, expiry_time, observation_clock_time, realized_hit_count=None, calendar=TradingCalendar::default()))]
    fn new(
        notional: &Bound<'_, PyAny>,
        annual_coupon_rate: &Bound<'_, PyAny>,
        lower_bound: &Bound<'_, PyAny>,
        upper_bound: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        observation_clock_time: &Bound<'_, PyAny>,
        realized_hit_count: Option<&Bound<'_, PyAny>>,
        calendar: TradingCalendar,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::RangeAccrualTerms::new(
            real(notional)?,
            real(annual_coupon_rate)?,
            real(lower_bound)?,
            real(upper_bound)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            crate::conversions::clock_time(observation_clock_time)?,
        );
        if let Some(value) = realized_hit_count {
            terms.realized_hit_count = history(value)?;
        }
        terms.calendar = calendar.0;
        Ok(Self(domain(kiyosi::RangeAccrualOption::new(terms))?))
    }

    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// realized_hit_count : int or None, optional
    ///     Hits strictly before valuation, or None for unknown history.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omitted fields or Ellipsis retain their values.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (*, realized_hit_count=crate::conversions::OMITTED))]
    fn replace_history(
        &self,
        #[pyo3(from_py_with = supplied)] realized_hit_count: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let count = match realized_hit_count {
            Some(value) => history(value)?,
            None => self.0.realized_hit_count(),
        };
        Ok(Self(domain(self.0.replace_history(count))?))
    }
    /// Returns the explicitly specified daily observation clock time.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::conversions::python_clock_time(py, self.0.observation_clock_time())
    }

    /// Returns an owned copy of the contract's observation calendar.
    #[getter]
    fn calendar(&self) -> TradingCalendar {
        TradingCalendar(self.0.calendar().clone())
    }
    /// Return positive finite coupon notional, without principal repayment.
    #[getter]
    fn notional(&self) -> f64 {
        self.0.notional()
    }
    /// Return nonnegative finite annual coupon rate in decimal units.
    #[getter]
    fn annual_coupon_rate(&self) -> f64 {
        self.0.annual_coupon_rate()
    }
    /// Return inclusive nonnegative lower spot boundary.
    #[getter]
    fn lower_bound(&self) -> f64 {
        self.0.lower_bound()
    }
    /// Return inclusive finite upper spot boundary, above the lower boundary.
    #[getter]
    fn upper_bound(&self) -> f64 {
        self.0.upper_bound()
    }
    /// Return inclusive first contract timestamp.
    #[getter]
    fn effective_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.effective_time())
    }
    /// Return inclusive expiry time.
    #[getter]
    fn expiry_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.expiry_time())
    }
    /// Return hits strictly before valuation, or None for unknown history.
    #[getter]
    fn realized_hit_count(&self) -> Option<i32> {
        self.0.realized_hit_count()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "RangeAccrualOption(notional={:?}, annual_coupon_rate={:?}, lower_bound={:?}, upper_bound={:?}, effective_time={}, expiry_time={}, realized_hit_count={}, calendar={})",
            self.notional(),
            self.annual_coupon_rate(),
            self.lower_bound(),
            self.upper_bound(),
            self.effective_time(py)?.repr()?,
            self.expiry_time(py)?.repr()?,
            self.realized_hit_count()
                .map_or_else(|| "None".into(), |n| n.to_string()),
            crate::representation::calendar(py, self.0.calendar())?
        ))
    }
}
