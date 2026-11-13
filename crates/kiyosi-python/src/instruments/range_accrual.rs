use crate::calendar::TradingCalendar;
use crate::conversions::{integer, python_timestamp, real, timestamp};
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
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// observation_clock_time : datetime.time
///     Required daily observation time, without timezone information.
/// realized_hit_count : int or None, optional
///     Hits strictly before valuation, or None for unknown history.
///     Cannot exceed the number of observations strictly before expiry.
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays, independently of
///     PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If amounts, bounds or history are invalid, or expiry is not after
///     effective_time, or the observation schedule is empty. Pricing checks
///     the inclusive contract life and history against elapsed observations.
///
/// Notes
/// -----
/// Observe trading times in (effective_time, expiry_time]. Bounds are
/// inclusive. Settlement may fall on a closed date without adding an
/// observation. Payment is notional times annual coupon times Actual/365 Fixed
/// life times the fraction of observations inside the range. Payment is at
/// expiry; a completed observation schedule still has a discounted future
/// payment.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct RangeAccrualOption(pub kiyosi::RangeAccrualOption);

value_methods!(RangeAccrualOption, RangeAccrualOptionChanges,
    [notional, annual_coupon_rate, lower_bound, upper_bound, effective_time, expiry_time,
     observation_clock_time, realized_hit_count, calendar], {
        notional: notional.map(real).transpose()?,
        annual_coupon_rate: annual_coupon_rate.map(real).transpose()?,
        lower_bound: lower_bound.map(real).transpose()?,
        upper_bound: upper_bound.map(real).transpose()?,
        effective_time: effective_time.map(timestamp).transpose()?,
        expiry_time: expiry_time.map(timestamp).transpose()?,
        observation_clock_time: observation_clock_time.map(crate::conversions::clock_time).transpose()?,
        realized_hit_count: realized_hit_count.map(history).transpose()?,
        calendar: calendar.map(|value| value.extract::<TradingCalendar>().map(|value| value.0)).transpose()?,
    }, {
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

    /// Return the explicitly specified daily observation clock time.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::conversions::python_clock_time(py, self.0.observation_clock_time())
    }

    /// Return the daily range observation schedule.
    ///
    /// Observations exclude the effective instant.
    ///
    /// Applies the contract calendar and clock through expiry, independently of
    /// supplied history. Payment adds no observation.
    ///
    #[getter]
    fn observation_schedule(&self) -> crate::schedule::ObservationSchedule {
        crate::schedule::ObservationSchedule(self.0.observation_schedule().clone())
    }

    /// Return an owned copy of the contract's observation calendar.
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
});
