use crate::calendar::TradingCalendar;
use crate::conversions::{direction, python_timestamp, real, supplied, timestamp, timestamps};
use crate::error::domain;
use pyo3::prelude::*;
use pyo3::types::PyAny;

fn settlement(value: &Bound<'_, PyAny>) -> PyResult<Option<bool>> {
    if value.is_none() {
        Ok(None)
    } else {
        crate::conversions::boolean(value).map(Some)
    }
}

/// Accumulates purchases or sales daily, settling at scheduled knock-out or expiry.
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
/// observation_clock_time : datetime.time
///     Required daily observation time, without timezone information.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// accumulated_quantity : float or None, optional
///     Outstanding quantity strictly before valuation. None means unknown; zero means no units.
///     Explicit history is required after any prior accumulation or knock-out observation.
///     Pricing rejects positive quantity before a strictly prior accumulation unless already settled.
/// knock_out_settled : bool or None, optional
///     Prior knock-out settlement history: None (default) is unknown, False means active,
///     and True means settled. Required after a strictly prior knock-out observation.
///     True requires a strictly prior knock-out; stored quantity is ignored and
///     remaining value is zero.
/// option_type : {'call', 'put'}, optional
///     Call accumulates purchases (the default); put accumulates sales.
/// knock_out_observation_times : iterable of datetime.datetime or None, optional
///     Strictly increasing, unique times within the inclusive contract life.
///     None observes knock-out at the daily accumulation time; an empty sequence disables
///     early knock-out. Times need not include either contract endpoint.
///
/// calendar : TradingCalendar, optional
///     Owned observation calendar; defaults to weekdays, independently of PricingContext.
///
/// history_time : datetime.datetime or None, optional
///     Timestamp of supplied history, covering events strictly before it.
///     Required at pricing whenever history is supplied. Refresh the history
///     payload and timestamp together.
///
/// Raises
/// ------
/// InvalidInputError
///     If strike or knock-out level is invalid, quantities are negative or
///     non-finite, expiry precedes effective_time, or explicit times are invalid.
///     Trading-timestamp compatibility is checked against the calendar on construction.
///
/// Notes
/// -----
/// Daily accumulations occur at observation_clock_time on trading dates. Explicit
/// knock-out observations occur independently at their supplied times. On a knock-out observation
/// timestamp, calls knock out at or above the barrier and puts at or below it.
/// Knock-out precedes accumulation and adds no units. Other trading times
/// accumulate even beyond the barrier. At strike, the base quantity applies.
/// Expiry settles after its event, whether or not it is a knock-out timestamp.
/// Calls pay quantity * (settlement spot - strike); puts pay its negative.
/// Intraday history includes that day's earlier accumulation. Record settled
/// contracts with knock_out_settled=True.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct Accumulator(pub(crate) kiyosi::Accumulator);

#[pymethods]
impl Accumulator {
    /// Return the timestamp through which supplied history is known, or None.
    #[getter]
    fn history_time<'py>(
        &self,
        py: pyo3::Python<'py>,
    ) -> pyo3::PyResult<Option<pyo3::Bound<'py, pyo3::PyAny>>> {
        kiyosi::OptionContract::history_time(&self.0)
            .map(|time| crate::conversions::python_timestamp(py, time))
            .transpose()
    }

    /// Returns a validated replacement without changing this contract.
    ///
    /// Parameters
    /// ----------
    /// option_type : {'call', 'put'}, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// strike : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_out_level : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// daily_quantity : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// acceleration_factor : float, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// accumulated_quantity : float or None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_out_settled : bool or None, optional
    ///     Records a settled knock-out strictly before valuation.
    /// effective_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// expiry_time : datetime.datetime, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// knock_out_observation_times : Iterable[datetime.datetime] | None, optional
    ///     Replacement value; omission or Ellipsis retains the current field.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
    ///
    /// history_time : datetime.datetime or None, optional
    ///     Timestamp of supplied history, covering events strictly before it.
    ///     Required at pricing whenever history is supplied. Refresh the history
    ///     payload and timestamp together.
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
    #[pyo3(signature = (*, option_type=crate::conversions::OMITTED, strike=crate::conversions::OMITTED, knock_out_level=crate::conversions::OMITTED, daily_quantity=crate::conversions::OMITTED, acceleration_factor=crate::conversions::OMITTED, accumulated_quantity=crate::conversions::OMITTED, effective_time=crate::conversions::OMITTED, expiry_time=crate::conversions::OMITTED, observation_clock_time=crate::conversions::OMITTED, knock_out_observation_times=crate::conversions::OMITTED, knock_out_settled=crate::conversions::OMITTED, calendar=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Python replacement fields"
    )]
    fn replace(
        &self,
        #[pyo3(from_py_with = crate::conversions::supplied)] option_type: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] strike: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_out_level: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] daily_quantity: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] acceleration_factor: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] accumulated_quantity: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] effective_time: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = crate::conversions::supplied)] expiry_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] observation_clock_time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = crate::conversions::supplied)] knock_out_observation_times: Option<
            &Bound<'_, PyAny>,
        >,
        #[pyo3(from_py_with = supplied)] knock_out_settled: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] calendar: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<
            &pyo3::Bound<'_, pyo3::PyAny>,
        >,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?
            .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

        let changes = kiyosi::AccumulatorChanges {
            history_time: Some(history_time),
            calendar: calendar
                .map(|v| v.extract::<TradingCalendar>().map(|v| v.0))
                .transpose()?,
            knock_out_settled: knock_out_settled.map(settlement).transpose()?,
            option_type: option_type.map(crate::conversions::direction).transpose()?,
            strike: strike.map(crate::conversions::real).transpose()?,
            knock_out_level: knock_out_level.map(crate::conversions::real).transpose()?,
            daily_quantity: daily_quantity.map(crate::conversions::real).transpose()?,
            acceleration_factor: acceleration_factor
                .map(crate::conversions::real)
                .transpose()?,
            accumulated_quantity: accumulated_quantity
                .map(|value| {
                    if value.is_none() {
                        Ok(None)
                    } else {
                        crate::conversions::real(value).map(Some)
                    }
                })
                .transpose()?,
            effective_time: effective_time
                .map(crate::conversions::timestamp)
                .transpose()?,
            expiry_time: expiry_time.map(crate::conversions::timestamp).transpose()?,
            observation_clock_time: observation_clock_time
                .map(crate::conversions::clock_time)
                .transpose()?,
            knock_out_observation_times: knock_out_observation_times
                .map(|value| {
                    if value.is_none() {
                        Ok(None)
                    } else {
                        crate::conversions::timestamps(value).map(Some)
                    }
                })
                .transpose()?,
        };
        Ok(Self(domain(self.0.replace(changes))?))
    }
    #[new]
    #[pyo3(signature = (*, strike, knock_out_level, daily_quantity, acceleration_factor, effective_time, expiry_time, observation_clock_time, accumulated_quantity=None, option_type=kiyosi::AccumulatorTerms::DEFAULT_OPTION_TYPE, knock_out_observation_times=None, knock_out_settled=kiyosi::AccumulatorTerms::DEFAULT_KNOCK_OUT_SETTLED, calendar=TradingCalendar::default(), history_time=None))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        strike: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        daily_quantity: &Bound<'_, PyAny>,
        acceleration_factor: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        observation_clock_time: &Bound<'_, PyAny>,
        accumulated_quantity: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = direction)] option_type: kiyosi::OptionType,
        knock_out_observation_times: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = settlement)] knock_out_settled: Option<bool>,
        calendar: TradingCalendar,

        #[pyo3(from_py_with = crate::conversions::optional_timestamp)] history_time: Option<
            kiyosi::Timestamp,
        >,
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
        terms.knock_out_observation_times =
            knock_out_observation_times.map(timestamps).transpose()?;
        terms.calendar = calendar.0;
        Ok(Self(domain(kiyosi::OptionContract::with_history_time(
            domain(kiyosi::Accumulator::new(terms))?,
            history_time,
        ))?))
    }
    /// Return a validated replacement without changing this value.
    ///
    /// Parameters
    /// ----------
    /// accumulated_quantity : float or None, optional
    ///     Outstanding quantity strictly before valuation. None means unknown; zero means no units.
    ///     Required after a prior observation unless knock_out_settled is True.
    ///
    /// knock_out_settled : bool or None, optional
    ///     None restores unknown history; omission retains the current state. Pricing
    ///     rejects knock-out settlement before or at the first knock-out observation.
    ///
    /// history_time : datetime.datetime or None, optional
    ///     Timestamp of supplied history, covering events strictly before it.
    ///     Required at pricing whenever history is supplied. Refresh the history
    ///     payload and timestamp together.
    ///
    /// Returns
    /// -------
    /// object
    ///     New instance of the same class; omission or Ellipsis retains history.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If replacement terms fail domain validation. The original is unchanged.
    #[pyo3(signature = (*, accumulated_quantity=crate::conversions::OMITTED, knock_out_settled=crate::conversions::OMITTED, history_time=crate::conversions::OMITTED))]
    fn replace_history(
        &self,
        #[pyo3(from_py_with = supplied)] accumulated_quantity: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] knock_out_settled: Option<&Bound<'_, PyAny>>,

        #[pyo3(from_py_with = crate::conversions::supplied)] history_time: Option<
            &pyo3::Bound<'_, pyo3::PyAny>,
        >,
    ) -> PyResult<Self> {
        let history_time = history_time
            .map(crate::conversions::optional_timestamp)
            .transpose()?
            .unwrap_or(kiyosi::OptionContract::history_time(&self.0));

        let changes = kiyosi::AccumulatorHistoryChanges {
            accumulated_quantity: accumulated_quantity
                .map(|value| {
                    if value.is_none() {
                        Ok(None)
                    } else {
                        real(value).map(Some)
                    }
                })
                .transpose()?,
            knock_out_settled: knock_out_settled.map(settlement).transpose()?,
        };
        Ok(Self(domain(kiyosi::OptionContract::with_history_time(
            domain(self.0.replace_history(changes))?,
            history_time,
        ))?))
    }

    /// Returns the explicitly specified daily observation clock time.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        crate::conversions::python_clock_time(py, self.0.observation_clock_time())
    }

    /// Returns an owned schedule of daily accumulation opportunities.
    ///
    /// Includes the effective instant when it matches the contract clock and
    /// calendar; expiry adds no observation. Coincident knock-out can prevent accumulation.
    #[getter]
    fn accumulation_schedule(&self) -> PyResult<crate::schedule::ObservationSchedule> {
        domain(self.0.accumulation_schedule()).map(crate::schedule::ObservationSchedule)
    }

    /// Returns an owned schedule resolving daily or explicit knock-out monitoring.
    ///
    /// An empty explicit list disables knock-out. Supplied history does not
    /// filter this whole-life schedule.
    #[getter]
    fn knock_out_observation_schedule(&self) -> PyResult<crate::schedule::ObservationSchedule> {
        domain(self.0.knock_out_observation_schedule()).map(crate::schedule::ObservationSchedule)
    }

    /// Returns an owned copy of the contract's observation calendar.
    #[getter]
    fn calendar(&self) -> TradingCalendar {
        TradingCalendar(self.0.calendar().clone())
    }
    /// Returns call for purchases or put for sales.
    #[getter]
    fn option_type(&self) -> &'static str {
        match self.0.option_type() {
            kiyosi::OptionType::Call => "call",
            kiyosi::OptionType::Put => "put",
        }
    }
    /// Returns owned knock-out times, or None for at the daily accumulation time.
    #[getter]
    fn knock_out_observation_times<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Option<Vec<Bound<'py, PyAny>>>> {
        self.0
            .knock_out_observation_times()
            .map(|times| {
                times
                    .iter()
                    .map(|&timestamp| python_timestamp(py, timestamp))
                    .collect()
            })
            .transpose()
    }
    /// Returns the purchase or sale strike in underlying price units.
    #[getter]
    fn strike(&self) -> f64 {
        self.0.strike()
    }
    /// Return positive inclusive knock-out threshold.
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level()
    }
    /// Returns the nonnegative base quantity bought or sold on each trading timestamp.
    #[getter]
    fn daily_quantity(&self) -> f64 {
        self.0.daily_quantity()
    }
    /// Returns the quantity multiplier below strike for calls or above it for puts.
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
}
