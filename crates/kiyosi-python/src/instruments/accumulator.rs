use crate::calendar::{TradingCalendar, run_date_scan};
use crate::conversions::{direction, python_timestamp, real, timestamp};
use crate::schedule::ObservationSchedule;
use pyo3::prelude::*;
use pyo3::types::PyAny;

fn settlement(value: &Bound<'_, PyAny>) -> PyResult<Option<bool>> {
    if value.is_none() {
        Ok(None)
    } else {
        crate::conversions::boolean(value).map(Some)
    }
}

/// Daily purchases or sales settled at scheduled knock-out or expiry.
///
/// Parameters
/// ----------
/// strike : float
///     Positive finite purchase or sale strike.
/// knock_out_level : float
///     Positive inclusive knock-out threshold.
/// daily_quantity : float
///     Nonnegative finite quantity bought or sold on each trading timestamp.
/// acceleration_factor : float
///     Nonnegative multiplier below strike for calls or above strike for puts.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// observation_clock_time : datetime.time
///     Required daily observation time, without timezone information.
/// accumulated_quantity : float or None, optional
///     Outstanding quantity strictly before valuation. None means unknown;
///     zero means no units. Explicit history is required after any prior
///     accumulation or knock-out observation. Pricing rejects positive
///     quantity before a strictly prior accumulation unless already settled.
/// option_type : {'call', 'put'}, optional
///     Call accumulates purchases (the default); put accumulates sales.
/// knock_out_observation_schedule : ObservationSchedule or iterable of datetime.datetime or None, optional
///     Strictly increasing, unique times within the inclusive contract life.
///     None follows the daily accumulation schedule. An empty schedule disables
///     early knock-out. Times need not include either contract endpoint.
/// knock_out_settled : bool or None, optional
///     Prior knock-out settlement history: None (default) is unknown, False
///     means active, and True means settled. Required after a strictly prior
///     knock-out observation. True requires a strictly prior knock-out; stored
///     quantity is ignored and remaining value is zero.
/// calendar : TradingCalendar, optional
///     Explicit override. Omission or Ellipsis inherits a supplied
///     ObservationSchedule calendar. Otherwise, construction defaults to
///     weekdays and replacement defaults to the instance's calendar.
///
/// Raises
/// ------
/// InvalidInputError
///     If strike or knock-out level is invalid, quantities are negative or
///     non-finite, expiry precedes effective_time, or explicit times are
///     invalid. Trading-timestamp compatibility is checked against the
///     calendar on construction.
///
/// Notes
/// -----
/// Daily accumulations occur at observation_clock_time on trading dates.
/// Explicit knock-out observations occur independently at their supplied
/// times. On a knock-out observation timestamp, calls knock out at or above
/// the barrier and puts at or below it. Knock-out precedes accumulation and
/// adds no units. Other trading times accumulate even beyond the barrier. At
/// strike, the base quantity applies. Expiry settles after its event, whether
/// or not it is a knock-out timestamp. Calls pay quantity * (settlement spot -
/// strike); puts pay its negative. Intraday history includes that day's
/// earlier accumulation. Record settled contracts with knock_out_settled=True.
/// Long schedule validations release the GIL and poll Python signals.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct Accumulator(pub(crate) kiyosi::Accumulator);

impl Accumulator {
    fn replace_with_schedule(
        &self,
        py: Python<'_>,
        changes: kiyosi::AccumulatorChanges,
        times: Option<Vec<kiyosi::Timestamp>>,
    ) -> PyResult<Self> {
        let schedule = changes
            .knock_out_observation_schedule
            .as_ref()
            .map_or_else(|| self.0.knock_out_observation_schedule(), Option::as_ref);
        let work = super::monitoring_work(schedule, times.as_deref(), changes.calendar.as_ref());
        run_date_scan(py, work, || {
            kiyosi::binding_support::replace_accumulator(&self.0, changes, times)
        })
        .map(Self)
    }
}

value_methods!(@documented Accumulator, AccumulatorChanges,
    [option_type, strike, knock_out_level, daily_quantity, acceleration_factor,
     accumulated_quantity, effective_time, expiry_time, observation_clock_time,
     knock_out_observation_schedule, knock_out_settled, calendar], |self| {
        calendar: crate::calendar::explicit_calendar(calendar)?,
        knock_out_settled: knock_out_settled.map(settlement).transpose()?,
        option_type: option_type.map(crate::conversions::direction).transpose()?,
        strike: strike.map(crate::conversions::real).transpose()?,
        knock_out_level: knock_out_level.map(crate::conversions::real).transpose()?,
        daily_quantity: daily_quantity.map(crate::conversions::real).transpose()?,
        acceleration_factor: acceleration_factor.map(crate::conversions::real).transpose()?,
        accumulated_quantity: accumulated_quantity.map(crate::conversions::optional_real).transpose()?,
        effective_time: effective_time.map(crate::conversions::timestamp).transpose()?,
        expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
        observation_clock_time: observation_clock_time.map(crate::conversions::clock_time).transpose()?,
        knock_out_observation_schedule: {
            let (schedule, raw_times) = crate::conversions::monitoring_schedule(knock_out_observation_schedule)?;
            times = raw_times;
            schedule
        },
    }, [
    /// Return a validated replacement.
    ///
    /// Omitted fields and fields set to Ellipsis default to the instance's values.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Call or put payoff direction.
    /// strike, knock_out_level, daily_quantity : float, optional
    ///     Strike, inclusive knock-out threshold, and daily purchase or sale quantity.
    /// acceleration_factor : float, optional
    ///     Nonnegative quantity multiplier below strike for calls or above strike for puts.
    /// accumulated_quantity : float or None, optional
    ///     Quantity strictly before valuation; None clears to unknown.
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// observation_clock_time : datetime.time, optional
    ///     Daily observation clock, without timezone information.
    /// knock_out_observation_schedule : ObservationSchedule or iterable of datetime.datetime or None, optional
    ///     Explicit knock-out schedule. None restores the daily rule and an empty
    ///     schedule disables early knock-out.
    /// knock_out_settled : bool or None, optional
    ///     Records a settled knock-out strictly before valuation; None clears to
    ///     unknown.
    /// calendar : TradingCalendar, optional
    ///     Explicit override. Omission or Ellipsis inherits a supplied
    ///     ObservationSchedule calendar. Otherwise, construction defaults to
    ///     weekdays and replacement defaults to the instance's calendar.
    ///
    /// Returns
    /// -------
    /// Accumulator
    ///     Validated copy of the same class.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation.
    /// TypeError
    ///     If a representation or field name is invalid.
    ///
    /// Notes
    /// -----
    /// Long schedule validations release the GIL and poll Python signals.
    ///
    ], {
    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let value = slf.borrow();
        let py = slf.py();
        crate::representation::format(
            slf.as_any(),
            Self::_STATE_FIELDS,
            &[
                (
                    "calendar",
                    crate::representation::calendar_preview(py, value.0.calendar())?,
                ),
                (
                    "knock_out_observation_schedule",
                    Some(value.0.knock_out_observation_schedule().map_or_else(
                        || Ok("None".into()),
                        |schedule| crate::representation::times(py, schedule.times()),
                    )?),
                ),
            ],
        )
    }

    #[new]
    #[pyo3(signature = (*, strike, knock_out_level, daily_quantity, acceleration_factor, effective_time, expiry_time, observation_clock_time, accumulated_quantity=None, option_type=kiyosi::AccumulatorTerms::DEFAULT_OPTION_TYPE, knock_out_observation_schedule=None, knock_out_settled=kiyosi::AccumulatorTerms::DEFAULT_KNOCK_OUT_SETTLED, calendar=crate::conversions::OMITTED))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        strike: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        daily_quantity: &Bound<'_, PyAny>,
        acceleration_factor: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        observation_clock_time: &Bound<'_, PyAny>,
        accumulated_quantity: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = direction)] option_type: kiyosi::OptionType,
        knock_out_observation_schedule: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = settlement)] knock_out_settled: Option<bool>,
        #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut terms = kiyosi::AccumulatorTerms::new(
            real(strike)?,
            real(knock_out_level)?,
            real(daily_quantity)?,
            real(acceleration_factor)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            crate::conversions::clock_time(observation_clock_time)?,
        );
        terms.knock_out_settled = knock_out_settled;
        terms.accumulated_quantity = accumulated_quantity.map(real).transpose()?;
        terms.option_type = option_type;
        let (schedule, times) =
            crate::conversions::monitoring_schedule(knock_out_observation_schedule)?;
        terms.knock_out_observation_schedule = schedule.flatten();
        terms.calendar = crate::calendar::explicit_calendar(calendar)?;
        let work = super::monitoring_work(
            terms.knock_out_observation_schedule.as_ref(),
            times.as_deref(),
            terms.calendar.as_ref(),
        );
        run_date_scan(py, work, || kiyosi::binding_support::new_accumulator(terms, times))
            .map(Self)
    }
    /// Return the explicitly specified daily observation clock time.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::conversions::python_clock_time(py, self.0.observation_clock_time())
    }

    /// Return an owned schedule of daily accumulation opportunities.
    ///
    /// Includes the effective instant when it matches the contract clock and
    /// calendar; expiry adds no observation. Coincident knock-out can prevent
    /// accumulation. Long scans release the GIL and poll Python signals.
    ///
    #[getter]
    fn accumulation_schedule(&self, py: Python<'_>) -> PyResult<crate::schedule::ObservationSchedule> {
        let days = self.0.effective_time().date().days_until(self.0.expiry_time().date());
        run_date_scan(py, days.unsigned_abs() + 1, || self.0.accumulation_schedule())
            .map(crate::schedule::ObservationSchedule)
    }

    /// Return the explicit knock-out schedule, or None for the daily rule.
    ///
    /// An empty schedule disables early knock-out. Use accumulation_schedule
    /// for the dates used by the daily rule.
    #[getter]
    fn knock_out_observation_schedule(&self) -> Option<ObservationSchedule> {
        self.0.knock_out_observation_schedule().cloned().map(ObservationSchedule)
    }

    /// Return an owned copy of the contract's observation calendar.
    #[getter]
    fn calendar(&self) -> TradingCalendar {
        TradingCalendar(self.0.calendar().clone())
    }
    /// Return call for purchases or put for sales.
    #[getter]
    fn option_type(&self) -> &'static str {
        crate::conversions::direction_name(self.0.option_type())
    }
    /// Return the purchase or sale strike in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    /// Return positive inclusive knock-out threshold.
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level()
    }
    /// Return the nonnegative base quantity per trading timestamp.
    ///
    #[getter]
    fn daily_quantity(&self) -> f64 {
        self.0.daily_quantity()
    }
    /// Return the quantity multiplier below strike for calls or above it for puts.
    #[getter]
    fn acceleration_factor(&self) -> f64 {
        self.0.acceleration_factor()
    }
    /// Return caller-asserted outstanding quantity strictly before valuation.
    #[getter]
    fn accumulated_quantity(&self) -> Option<f64> {
        self.0.accumulated_quantity()
    }
    /// Return prior knock-out settlement history, or None if unknown.
    #[getter]
    fn knock_out_settled(&self) -> Option<bool> {
        self.0.knock_out_settled()
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
}, py => Self::replace_with_schedule, times);
