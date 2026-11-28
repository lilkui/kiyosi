use crate::calendar::{TradingCalendar, run_date_scan};
use crate::conversions::{
    collect_values, integer_usize, python_timestamp, python_tuple, real, reals, supplied,
    timestamp, timestamps,
};
use crate::error::domain;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

// Signature order and conversion order are separate to preserve boundary failures.
macro_rules! autocallable_properties {
    ($name:ident,
     @replace $changes:ident [$($parameter:ident),+] [$($observations:ident),+]
     {$($field:ident => $convert:expr),+ $(,)?}
     [$(#[$replacement_doc:meta])*],
     $($extra:item),* $(,)?) => {
        impl $name {
            fn replace_observations(&self, py: Python<'_>, changes: kiyosi::$changes) -> PyResult<Self> {
                let count: usize = [$(
                    changes.$observations.as_deref().unwrap_or_else(|| self.0.$observations()).len()
                ),+].into_iter().sum();
                run_date_scan(py, count as u64, || self.0.replace(changes)).map(Self)
            }
        }

        value_methods!(@documented $name, $changes, [$($parameter),+],
            {$($field: $field.map($convert).transpose()?),+}, [$(#[$replacement_doc])*], {
            $($extra)*

            /// Return an owned copy of the contract's monitoring calendar.
            #[getter]
            fn calendar(&self) -> TradingCalendar { TradingCalendar(self.0.calendar().clone()) }
            /// Return an owned whole-life knock-in schedule, or None without knock-in.
            ///
            /// Daily monitoring includes the effective instant at the contractual clock on
            /// open dates. Expiry-only monitoring returns just expiry. Observations for
            /// knock-out and coupons are supplied by the product observation lists.
            /// Long scans release the GIL and poll Python signals.
            ///
            #[getter]
            fn knock_in_observation_schedule(&self, py: Python<'_>) -> PyResult<Option<crate::schedule::ObservationSchedule>> {
                let count = self.0.observation_clock_time().map_or(0, |_| {
                    self.0.effective_time().date().days_until(self.0.expiry_time().date()).unsigned_abs() + 1
                });
                Ok(run_date_scan(py, count, || self.0.knock_in_observation_schedule())?
                    .map(crate::schedule::ObservationSchedule))
            }
            /// Return nonnegative normalized principal, independent of coupons and losses.
            #[getter]
            fn principal_redemption_ratio(&self) -> f64 {
                self.0.terms().principal_redemption_ratio
            }
            /// Return the flat rebate proportion paid once at knock-out or maturity.
            #[getter]
            fn rebate_rate(&self) -> f64 { self.0.terms().rebate_rate }
            /// Return the annualized rebate rate accrued from effective time to exit.
            #[getter]
            fn annual_rebate_rate(&self) -> f64 { self.0.terms().annual_rebate_rate }
            /// Return strictly prior autocallable history, or None if unknown.
            #[getter]
            fn barrier_state(&self) -> Option<&'static str> {
                state_name(self.0.barrier_state())
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
        }, py => Self::replace_observations);
    };
}

macro_rules! snowball_properties {
    ($name:ident,
     @replace $changes:ident $parameters:tt $conversions:tt $documentation:tt,
     $($extra:item),* $(,)?) => {
        autocallable_properties! {
            $name,
            @replace $changes $parameters [autocall_observations] $conversions $documentation,
            $($extra,)*
            fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
                let value = slf.borrow();
                let py = slf.py();
                crate::representation::format(slf.as_any(), Self::_STATE_FIELDS, &[
                    ("calendar", Some(crate::representation::calendar_preview(py, value.0.calendar())?
.unwrap_or_else(|| "TradingCalendar.weekdays()".into()))),
                    ("autocall_observations", Some(crate::representation::observations(py, value.0.autocall_observations())?)),
                ])
            },

            /// Return owned copies of the contractual knock-out and coupon observations.
            ///
            /// Knock-in monitoring is available through knock_in_observation_schedule.
            #[getter]
            fn autocall_observations<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                python_tuple(py, self.0.autocall_observations(), |&r| Ok(SnowballObservation(r)))
            },
            /// Return a copy of the contractual knock-out and coupon observation times.
            ///
            /// Excludes independently scheduled knock-in monitoring.
            #[getter]
            fn autocall_times<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                python_tuple(py, self.0.autocall_observations(), |r| python_timestamp(py, r.time()))
            },
            /// Return copied knock-out thresholds in observation order.
            #[getter]
            fn knock_out_levels<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                python_tuple(py, self.0.autocall_observations(), |r| Ok(r.knock_out_level()))
            },

            /// Return copied annual knock-out coupons in observation order.
            #[getter]
            fn knock_out_coupon_rates<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                python_tuple(py, self.0.autocall_observations(), |r| Ok(r.coupon_rate()))
            },
            /// Return finite annual maturity coupon rate in decimal units.
            #[getter]
            fn maturity_coupon_rate(&self) -> f64 {
                self.0.terms().maturity_coupon_rate
            },
            /// Return a note with the quoted first coupon and preserved coupon spreads.
            ///
            /// Parameters
            /// ----------
            /// first_observation_coupon : float
            ///     Absolute annual first observation coupon, not an additive shift. All
            ///     observation coupons shift equally to preserve their spreads.
            /// quote_convention : CouponQuoteConvention
            ///     Whether to shift the maturity coupon or preserve it.
            ///
            /// Returns
            /// -------
            #[doc = stringify!($name)]
            ///     New note of the same type, preserving history and other terms.
            ///
            /// Raises
            /// ------
            /// InvalidInputError
            ///     If shifted coupons are non-finite or violate contractual constraints.
            ///
            #[pyo3(signature = (first_observation_coupon, *, quote_convention))]
            fn with_coupon(
                &self,
                py: Python<'_>,
                first_observation_coupon: &Bound<'_, PyAny>,
                quote_convention: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                let coupon = real(first_observation_coupon)?;
                let convention = coupon_convention(quote_convention)?;
                run_date_scan(py, self.0.autocall_observations().len() as u64, || {
                    self.0.with_coupon(coupon, convention)
                }).map(Self)
            }
        }
    };
}

pub(crate) fn coupon_convention(
    value: &Bound<'_, PyAny>,
) -> PyResult<kiyosi::CouponQuoteConvention> {
    match value.extract::<&str>()? {
        "shift_maturity_coupon" => Ok(kiyosi::CouponQuoteConvention::ShiftMaturityCoupon),
        "preserve_maturity_coupon" => Ok(kiyosi::CouponQuoteConvention::PreserveMaturityCoupon),
        _ => Err(PyValueError::new_err("unknown coupon quote convention")),
    }
}

pub(crate) fn state(
    value: &Bound<'_, PyAny>,
) -> PyResult<Option<kiyosi::AutocallableBarrierState>> {
    if value.is_none() {
        return Ok(None);
    }
    Ok(Some(match value.extract::<&str>()? {
        "no_event" => kiyosi::AutocallableBarrierState::NoEvent,
        "knocked_in" => kiyosi::AutocallableBarrierState::KnockedIn,
        "knocked_out" => kiyosi::AutocallableBarrierState::KnockedOut,
        _ => return Err(PyValueError::new_err("unknown autocallable barrier_state")),
    }))
}
fn state_name(value: Option<kiyosi::AutocallableBarrierState>) -> Option<&'static str> {
    value.map(|s| match s {
        kiyosi::AutocallableBarrierState::NoEvent => "no_event",
        kiyosi::AutocallableBarrierState::KnockedIn => "knocked_in",
        kiyosi::AutocallableBarrierState::KnockedOut => "knocked_out",
    })
}

/// Knock-in observation rule with an owned daily clock when applicable.
///
/// Notes
/// -----
/// Construct with every_trading_day(observation_clock_time) or at_expiry().
/// Knock-out and coupon timestamps are supplied independently through
/// the corresponding product observation records.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct KnockInObservation(pub kiyosi::KnockInObservation);

#[pymethods]
impl KnockInObservation {
    /// Create daily knock-in observation on trading days.
    ///
    /// Parameters
    /// ----------
    /// observation_clock_time : datetime.time
    ///     Timezone-free daily knock-in clock with microsecond precision.
    ///
    /// Returns
    /// -------
    /// KnockInObservation
    ///     Immutable daily observation rule.
    ///
    /// Raises
    /// ------
    /// TypeError
    ///     If the clock representation is invalid or timezone-aware.
    ///
    #[staticmethod]
    fn every_trading_day(observation_clock_time: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(kiyosi::KnockInObservation::EveryTradingDay {
            observation_clock_time: crate::conversions::clock_time(observation_clock_time)?,
        }))
    }

    /// Create knock-in observation only at the contract's expiry timestamp.
    ///
    /// Returns
    /// -------
    /// KnockInObservation
    ///     Immutable expiry-only observation rule without a daily clock.
    ///
    #[staticmethod]
    fn at_expiry() -> Self {
        Self(kiyosi::KnockInObservation::AtExpiry)
    }

    /// Return every_trading_day or at_expiry.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.0 {
            kiyosi::KnockInObservation::EveryTradingDay { .. } => "every_trading_day",
            kiyosi::KnockInObservation::AtExpiry => "at_expiry",
        }
    }

    /// Return the owned daily clock, or None for expiry-only observation.
    #[getter]
    fn observation_clock_time<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        match self.0 {
            kiyosi::KnockInObservation::EveryTradingDay {
                observation_clock_time,
            } => crate::conversions::python_clock_time(py, observation_clock_time).map(Some),
            kiyosi::KnockInObservation::AtExpiry => Ok(None),
        }
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        match self.observation_clock_time(py)? {
            Some(clock) => Ok(format!(
                "KnockInObservation.every_trading_day({})",
                clock.repr()?
            )),
            None => Ok("KnockInObservation.at_expiry()".into()),
        }
    }
}

pub(crate) fn records(values: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::SnowballObservation>> {
    collect_values(values, |item| {
        Ok(item.extract::<PyRef<'_, SnowballObservation>>()?.0)
    })
}

/// Snowball knock-out observation with a finite annual coupon.
///
/// Parameters
/// ----------
/// time : datetime.datetime
///     Exact observation timestamp.
/// knock_out_level : float
///     Positive finite inclusive knock-out threshold.
/// coupon_rate : float
///     Finite signed annual knock-out coupon; supply zero explicitly for no
///     coupon.
///
/// Raises
/// ------
/// InvalidInputError
///     If the level is nonpositive or non-finite, or the coupon is non-finite.
///
/// Notes
/// -----
/// Numeric terms are validated in Rust; the note validates ordering, life bounds
/// and calendar compatibility. Phoenix uses KnockOutObservation and CouponObservation.
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct SnowballObservation(pub kiyosi::SnowballObservation);
value_methods!(@documented SnowballObservation, SnowballObservationChanges,
    [time, knock_out_level, coupon_rate],
    {
        time: time.map(timestamp).transpose()?,
        knock_out_level: knock_out_level.map(real).transpose()?,
        coupon_rate: coupon_rate.map(real).transpose()?,
    },
    [
        /// Return a validated observation with the supplied fields replaced.
        ///
        /// Omitted fields and fields set to Ellipsis default to the instance's values.
        ///
        /// Parameters
        /// ----------
        /// time : datetime.datetime, optional
        ///     Observation timestamp.
        /// knock_out_level : float, optional
        ///     Positive finite knock-out threshold.
        /// coupon_rate : float, optional
        ///     Finite annual snowball coupon.
        ///
        /// Returns
        /// -------
        /// SnowballObservation
        ///     Validated copy. The note checks schedule constraints.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If the level is nonpositive or non-finite, or the coupon is non-finite.
        /// TypeError
        ///     If a field has an invalid representation.
    ], {
    #[new]
    #[pyo3(signature = (time, knock_out_level, *, coupon_rate))]
    fn new(
        time: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        coupon_rate: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::SnowballObservation::new(
            timestamp(time)?,
            real(knock_out_level)?,
            real(coupon_rate)?,
        ))?))
    }
    /// Combine parallel sequences into validated snowball observation records.
    ///
    /// Parameters
    /// ----------
    /// autocall_times : iterable of datetime.datetime
    ///     Knock-out and coupon observation times; the note checks ordering and life
    ///     bounds.
    /// knock_out_levels : iterable of float
    ///     Positive inclusive knock-out levels, one per observation timestamp.
    /// knock_out_coupon_rates : iterable of float
    ///     Explicit annual snowball coupons, including zero for no coupon.
    ///
    /// Returns
    /// -------
    /// list of SnowballObservation
    ///     Observation records.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If sequence lengths differ or numeric levels or coupons are invalid.
    /// TypeError
    ///     If an argument is missing or a value has an invalid representation.
    ///
    /// Notes
    /// -----
    /// The note constructor validates ordering, life bounds and calendar compatibility.
    ///
    /// Passing an ObservationSchedule copies its timestamps into the records;
    /// the records carry no calendar. Pass calendar=schedule.calendar to the note
    /// constructor to preserve its observation calendar. Raw note constructors require
    /// an explicit calendar, including for daily knock-in monitoring.
    ///
    /// Examples
    /// --------
    /// Forward a schedule's custom calendar when constructing a snowball:
    ///
    /// >>> from datetime import datetime, time
    /// >>> from kiyosi import instruments as ins, market
    /// >>> start, end = datetime(2025, 1, 2, 15), datetime(2025, 1, 10, 15)
    /// >>> calendar = market.TradingCalendar.custom(
    /// ...     base=market.TradingCalendar.weekdays(),
    /// ...     closed_dates=[datetime(2025, 1, 6).date()],
    /// ... )
    /// >>> schedule = market.ObservationSchedule(
    /// ...     times=[end], start_time=start, end_time=end, calendar=calendar,
    /// ... )
    /// >>> records = ins.SnowballObservation.from_columns(
    /// ...     schedule, [110], knock_out_coupon_rates=[0.1],
    /// ... )
    /// >>> note = ins.SnowballOption(
    /// ...     maturity_coupon_rate=0.1, initial_spot=100, knock_in_level=80,
    /// ...     upper_strike=100, lower_strike=0,
    /// ...     knock_in_observation=ins.KnockInObservation.every_trading_day(time(15)),
    /// ...     effective_time=start, expiry_time=end, autocall_observations=records,
    /// ...     calendar=schedule.calendar,
    /// ... )
    /// >>> note.calendar == schedule.calendar
    /// True
    /// >>> len(note.knock_in_observation_schedule)
    /// 6
    ///
    #[staticmethod]
    #[pyo3(signature = (autocall_times, knock_out_levels, *, knock_out_coupon_rates))]
    fn from_columns(
        autocall_times: &Bound<'_, PyAny>,
        knock_out_levels: &Bound<'_, PyAny>,
        knock_out_coupon_rates: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<Self>> {
        let times = timestamps(autocall_times)?;
        let levels = reals(knock_out_levels)?;
        let coupons = reals(knock_out_coupon_rates)?;
        Ok(
            domain(kiyosi::SnowballObservation::from_columns(&times, &levels, &coupons))?
                .into_iter()
                .map(Self)
                .collect(),
        )
    }
    /// Return exact observation time.
    #[getter]
    fn time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        python_timestamp(py, self.0.time())
    }
    /// Return the positive finite knock-out threshold.
    #[getter]
    fn knock_out_level(&self) -> f64 {
        self.0.knock_out_level()
    }
    /// Return the finite annual snowball coupon rate.
    #[getter]
    fn coupon_rate(&self) -> f64 {
        self.0.coupon_rate()
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        crate::representation::observation(py, &self.0)
    }
});

macro_rules! phoenix_observation {
    ($name:ident, $level:ident, $records:ident, $description:literal) => {
        #[doc = $description]
        ///
        /// Parameters
        /// ----------
        /// time : datetime.datetime
        ///     Exact timezone-free observation timestamp.
        #[doc = concat!(stringify!($level), " : float")]
        ///     Finite observation threshold; knock-out levels are positive and
        ///     coupon barriers are nonnegative. A zero coupon barrier is unconditional.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If the threshold is outside its domain.
        /// TypeError
        ///     If the timestamp or number representation is invalid.
        #[pyclass(frozen, eq, module = "kiyosi._native")]
        #[derive(Clone, Copy, PartialEq)]
        pub(crate) struct $name(pub kiyosi::$name);
        #[pymethods]
        impl $name {
            #[classattr]
            const _STATE_FIELDS: &'static [&'static str] = &["time", stringify!($level)];
            #[new]
            #[pyo3(signature = (time, $level))]
            fn new(time: &Bound<'_, PyAny>, $level: &Bound<'_, PyAny>) -> PyResult<Self> {
                Ok(Self(domain(kiyosi::$name::new(timestamp(time)?, real($level)?))?))
            }
            /// Return the exact observation timestamp.
            #[getter]
            fn time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                python_timestamp(py, self.0.time())
            }
            /// Return the validated observation threshold.
            #[getter]
            fn $level(&self) -> f64 { self.0.$level() }
            /// Return a validated copy with the supplied fields applied.
            ///
            /// Omitted fields and fields set to Ellipsis default to the instance's values.
            ///
            /// Parameters
            /// ----------
            /// time : datetime.datetime, optional
            ///     Observation timestamp, defaulting to the instance's timestamp.
            #[doc = concat!(stringify!($level), " : float, optional")]
            ///     Threshold, defaulting to the instance's threshold.
            ///
            /// Returns
            /// -------
            #[doc = stringify!($name)]
            ///     Independent validated observation.
            ///
            /// Raises
            /// ------
            /// InvalidInputError
            ///     If the threshold is outside its domain.
            /// TypeError
            ///     If the timestamp or number representation is invalid.
            #[pyo3(signature = (*, time=crate::conversions::OMITTED, $level=crate::conversions::OMITTED))]
            fn replace(&self,
                #[pyo3(from_py_with = supplied)] time: Option<&Bound<'_, PyAny>>,
                #[pyo3(from_py_with = supplied)] $level: Option<&Bound<'_, PyAny>>,
            ) -> PyResult<Self> {
                Ok(Self(domain(self.0.replace(time.map(timestamp).transpose()?, $level.map(real).transpose()?))?))
            }
            fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                Ok(format!("{}(time={}, {}={:?})", stringify!($name), python_timestamp(py, self.0.time())?.repr()?, stringify!($level), self.0.$level()))
            }
        }
        fn $records(values: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::$name>> {
            collect_values(values, |item| Ok(item.extract::<PyRef<'_, $name>>()?.0))
        }
    };
}
phoenix_observation!(
    KnockOutObservation,
    knock_out_level,
    knock_out_records,
    "One Phoenix knock-out observation with a positive inclusive threshold."
);
phoenix_observation!(
    CouponObservation,
    coupon_barrier_level,
    coupon_records,
    "One Phoenix coupon observation with a nonnegative inclusive threshold."
);

/// Snowball with knock-out coupons and capped knock-in downside loss.
///
/// Parameters
/// ----------
/// maturity_coupon_rate : float
///     Finite annual maturity coupon rate in decimal units.
/// initial_spot : float
///     Positive reference spot used to normalize downside losses.
/// knock_in_level : float
///     Positive strict downside threshold.
/// upper_strike : float
///     Positive upper terminal settlement strike.
/// lower_strike : float
///     Nonnegative lower terminal strike, at most the upper strike.
/// knock_in_observation : KnockInObservation
///     Observe knock-in on every trading day or only at expiry.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// autocall_observations : iterable of SnowballObservation
///     Nonempty, strictly ordered knock-out and coupon observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
/// rebate_rate : float, optional
///     Finite nonnegative flat rebate per unit of original notional, default 0.
/// annual_rebate_rate : float, optional
///     Finite nonnegative annualized rebate rate, default 0. Accrues from
///     effective time to actual knock-out or expiry using Actual/365 Fixed,
///     including intraday time and nontrading days. Both rebates are additive
///     to coupons and principal gains or losses and pay once at termination.
///     A note knocked out before valuation is settled and has zero value.
/// calendar : TradingCalendar
///     Required owned observation calendar. Records carry no calendar; pass
///     schedule.calendar when constructing from a schedule. Controls contractual
///     observations independently of PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If amounts, strikes, life times or observation terms are invalid.
///     Calendar compatibility is checked on construction; prior barrier
///     history is checked during pricing.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct SnowballOption(pub kiyosi::SnowballOption);
macro_rules! snowball_preset {
    ([$($properties:tt)*], $(
        ($name:ident, [$($clock:ident)?], $coupon:ident => $coupon_doc:literal, $level:ident => $level_doc:literal,
        [$($extra:ident => $extra_doc:literal => $convert:ident),*], $preset:expr,
        $summary:literal, $monitoring:literal, $notes:literal)
    ),+ $(,)?) => {
        snowball_properties! {
            $($properties)*
            $(
                #[doc = $summary]
                ///
                /// Parameters
                /// ----------
                #[doc = concat!(stringify!($coupon), " : float")]
                #[doc = concat!("    ", $coupon_doc)]
                /// initial_spot : float
                ///     Positive reference spot used to normalize downside losses.
                /// knock_in_level : float
                ///     Positive strict downside threshold.
                #[doc = concat!(stringify!($level), " : float")]
                #[doc = concat!("    ", $level_doc)]
                $(
                    #[doc = concat!(stringify!($extra), " : ", snowball_preset!(@type $convert))]
                    #[doc = concat!("    ", $extra_doc)]
                )*
                /// autocall_times : iterable of datetime.datetime
                ///     Strictly increasing knock-out and coupon observation times within life.
                /// effective_time : datetime.datetime
                ///     Inclusive contract start.
                /// expiry_time : datetime.datetime
                ///     Inclusive expiry time.
                $(
                    #[doc = concat!(stringify!($clock), " : datetime.time")]
                    ///     Daily knock-in observation clock on trading days.
                )?
                /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
                ///     Events strictly before valuation. None is unknown; no_event means no prior
                ///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
                ///     knocked_out means settled with no remaining value. The engine handles
                ///     observations at valuation using current spot. Callers supply applicable state.
                /// principal_redemption_ratio : float, optional
                ///     Nonnegative normalized principal, independent of coupons and losses.
                /// rebate_rate : float, optional
                ///     Finite nonnegative flat rebate per unit of original notional, default 0.
                /// annual_rebate_rate : float, optional
                ///     Finite nonnegative annualized rebate rate, default 0. Accrues from
                ///     effective time to actual knock-out or expiry using Actual/365 Fixed,
                ///     including intraday time and nontrading days. Both rebates are additive
                ///     to coupons and principal gains or losses and pay once at termination.
                ///     A note knocked out before valuation is settled and has zero value.
                /// calendar : TradingCalendar, optional
                ///     Explicit calendar override; omission or Ellipsis inherits an
                ///     ObservationSchedule calendar, otherwise weekdays.
                ///
                /// Returns
                /// -------
                /// SnowballOption
                ///     Validated owned contract.
                ///
                /// Raises
                /// ------
                /// InvalidInputError
                ///     If contractual amounts, life times or observation terms are invalid.
                ///
                /// Notes
                /// -----
                #[doc = concat!("Knock-in is observed ", $monitoring, ".", $notes, "\n")]
                /// The returned SnowballOption stores observation and maturity coupons separately.
                /// To edit its first annual observation coupon while preserving coupon spreads,
                /// use the returned note's quoting operation:
                ///
                /// .. code-block:: python
                ///
                ///     edited = note.with_coupon(
                ///         first_observation_coupon=0.15,
                ///         quote_convention="shift_maturity_coupon",
                ///     )
                ///
                /// This convention shifts the maturity coupon by the same amount. Use
                /// preserve_maturity_coupon to keep maturity fixed. Replacing only
                /// maturity_coupon_rate edits only the maturity coupon.
                ///
                #[staticmethod]
                #[pyo3(signature = (*, $coupon, initial_spot, knock_in_level, $level, $($extra,)* autocall_times, effective_time, expiry_time, $($clock,)? barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, annual_rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, calendar=crate::conversions::OMITTED))]
                #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
                fn $name(
                    py: Python<'_>,
                    $coupon: &Bound<'_, PyAny>,
                    initial_spot: &Bound<'_, PyAny>,
                    knock_in_level: &Bound<'_, PyAny>,
                    $level: &Bound<'_, PyAny>,
                    $($extra: &Bound<'_, PyAny>,)*
                    autocall_times: &Bound<'_, PyAny>,
                    effective_time: &Bound<'_, PyAny>,
                    expiry_time: &Bound<'_, PyAny>,
                    $($clock: &Bound<'_, PyAny>,)?
                    barrier_state: Option<&Bound<'_, PyAny>>,
                    #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
                    #[pyo3(from_py_with = real)] rebate_rate: f64,
                    #[pyo3(from_py_with = real)] annual_rebate_rate: f64,
                    #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

                ) -> PyResult<Self> {
                    let mut terms = kiyosi::SnowballPresetTerms::new(
                        real($coupon)?, real(initial_spot)?, real(knock_in_level)?, real($level)?,
                        timestamp(effective_time)?, timestamp(expiry_time)?, timestamps(autocall_times)?,
                        snowball_preset!(@clock $($clock)?),
                    );
                    $(let $extra = $convert($extra)?;)*
                    if let Some(value) = barrier_state {
                        terms.barrier_state = state(value)?;
                    }
                    terms.principal_redemption_ratio = principal_redemption_ratio;
                    terms.rebate_rate = rebate_rate;
                    terms.annual_rebate_rate = annual_rebate_rate;
                    terms.calendar = crate::calendar::observation_calendar(calendar, Some(autocall_times))?.unwrap_or_default();

                    run_date_scan(py, terms.autocall_times.len() as u64, || terms.expand($preset)).map(Self)
                },
            )+
        }
    };
    (@clock $clock:ident) => { Some(crate::conversions::clock_time($clock)?) };
    (@clock) => { None };
    (@type real) => { "float" };
    (@type timestamp) => { "datetime.datetime" };
}
snowball_preset! {
    [
        SnowballOption,
        @replace SnowballOptionChanges [maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, barrier_state, principal_redemption_ratio, rebate_rate, annual_rebate_rate, effective_time, expiry_time, autocall_observations, calendar]
        {
            calendar => |v| v.extract::<TradingCalendar>().map(|v| v.0),
            maturity_coupon_rate => crate::conversions::real,
            initial_spot => crate::conversions::real,
            knock_in_level => crate::conversions::real,
            upper_strike => crate::conversions::real,
            lower_strike => crate::conversions::real,
            knock_in_observation => |value| value.extract::<PyRef<'_, KnockInObservation>>().map(|value| value.0),
            barrier_state => crate::instruments::autocallable::state,
            principal_redemption_ratio => crate::conversions::real,
            rebate_rate => crate::conversions::real,
            annual_rebate_rate => crate::conversions::real,
            effective_time => crate::conversions::timestamp,
            expiry_time => crate::conversions::timestamp,
            autocall_observations => crate::instruments::autocallable::records,
        }
        [
        /// Return a validated replacement.
        ///
        /// Omitted fields and fields set to Ellipsis default to the instance's values.
        ///
        /// Parameters
        /// ----------
        /// maturity_coupon_rate, initial_spot, knock_in_level : float, optional
        ///     Annual maturity coupon, reference spot, and strict downside barrier.
        /// upper_strike, lower_strike : float, optional
        ///     Upper and lower strikes defining the downside loss interval.
        /// knock_in_observation : KnockInObservation, optional
        ///     Daily or expiry-only knock-in monitoring rule.
        /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        ///     Strictly prior event history; None clears it to unknown.
        /// principal_redemption_ratio : float, optional
        ///     Finite nonnegative principal redemption per unit of original notional.
        /// rebate_rate : float, optional
        ///     Finite nonnegative flat rebate per unit of original notional.
        ///     Defaults to the instance's value.
        /// annual_rebate_rate : float, optional
        ///     Finite nonnegative annualized rebate rate. Defaults to the instance's
        ///     value. Accrues from effective time to actual knock-out or expiry using
        ///     Actual/365 Fixed,
        ///     including intraday time and nontrading days. Both rebates are additive
        ///     to coupons and principal gains or losses and pay once at termination.
        ///     A note knocked out before valuation is settled and has zero value.
        /// effective_time, expiry_time : datetime.datetime, optional
        ///     Inclusive contract start and expiry timestamps.
        /// autocall_observations : iterable of SnowballObservation, optional
        ///     Product-specific knock-out and coupon observations, in time order.
        /// calendar : TradingCalendar, optional
        ///     Owned contractual observation calendar, defaulting to the instance's calendar.
        ///
        /// Returns
        /// -------
        /// SnowballOption
        ///     Validated copy of the same class.
        ///
        /// Raises
        /// ------
        /// KiyosiError
        ///     If the resulting contract fails core validation.
        /// TypeError
        ///     If a representation or field name is invalid.
        ///
        ],

        #[new]
        #[pyo3(signature = (*, maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, effective_time, expiry_time, autocall_observations, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, annual_rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, calendar))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        fn new(
            py: Python<'_>,
            maturity_coupon_rate: &Bound<'_, PyAny>,
            initial_spot: &Bound<'_, PyAny>,
            knock_in_level: &Bound<'_, PyAny>,
            upper_strike: &Bound<'_, PyAny>,
            lower_strike: &Bound<'_, PyAny>,
            knock_in_observation: &KnockInObservation,
            effective_time: &Bound<'_, PyAny>,
            expiry_time: &Bound<'_, PyAny>,
            autocall_observations: &Bound<'_, PyAny>,
            barrier_state: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
            #[pyo3(from_py_with = real)] rebate_rate: f64,
            #[pyo3(from_py_with = real)] annual_rebate_rate: f64,
            calendar: TradingCalendar,

        ) -> PyResult<Self> {
            let mut terms = kiyosi::SnowballTerms::new(
                real(maturity_coupon_rate)?,
                real(initial_spot)?,
                real(knock_in_level)?,
                real(upper_strike)?,
                real(lower_strike)?,
                timestamp(effective_time)?,
                timestamp(expiry_time)?,
                records(autocall_observations)?,
                knock_in_observation.0,
                calendar.0,
            );
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_redemption_ratio = principal_redemption_ratio;
            terms.rebate_rate = rebate_rate;
            terms.annual_rebate_rate = annual_rebate_rate;

            let count = terms.autocall_observations.len();
            run_date_scan(py, count as u64, || kiyosi::SnowballOption::new(terms)).map(Self)
        },
        /// Return positive reference spot used to normalize downside losses.
        #[getter]
        fn initial_spot(&self) -> f64 {
            self.0.terms().initial_spot
        },
        /// Return positive strict downside threshold.
        #[getter]
        fn knock_in_level(&self) -> f64 {
            self.0.terms().knock_in_level
        },
        /// Return positive upper terminal settlement strike.
        #[getter]
        fn upper_strike(&self) -> f64 {
            self.0.terms().upper_strike
        },
        /// Return nonnegative lower terminal strike, at most the upper strike.
        #[getter]
        fn lower_strike(&self) -> f64 {
            self.0.terms().lower_strike
        },
        /// Return an owned knock-in observation rule.
        #[getter]
        fn knock_in_observation(&self) -> KnockInObservation {
            KnockInObservation(self.0.terms().knock_in_observation)
        },
    ],
    (
        standard,
        [knock_in_observation_clock_time],
        coupon_rate => "Finite annual coupon rate in decimal units.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [],
        kiyosi::SnowballPreset::Standard,
        "Create constant knock-out coupons and levels with daily knock-in monitoring.",
        "on every trading day",
        ""
    ),
    (
        european,
        [],
        coupon_rate => "Finite annual coupon rate in decimal units.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [],
        kiyosi::SnowballPreset::European,
        "Create a snowball with knock-in observed only at expiry.",
        "only at expiry",
        ""
    ),
    (
        step_down,
        [knock_in_observation_clock_time],
        coupon_rate => "Finite annual coupon rate in decimal units.",
        initial_knock_out_level => "Positive knock-out threshold at the first observation.",
        [knock_out_level_decrement => "Absolute price decrement after each observation; may be negative." => real],
        kiyosi::SnowballPreset::StepDown {
            knock_out_level_decrement
        },
        "Create a snowball with decreasing knock-out levels.",
        "on every trading day",
        r" Decrements apply after each observation; the first uses the initial
value. Signed coupons and decrements are accepted if all expanded terms are
valid."
    ),
    (
        both_down,
        [knock_in_observation_clock_time],
        initial_coupon_rate => "Annual coupon rate at the first observation.",
        initial_knock_out_level => "Positive knock-out threshold at the first observation.",
        [
            coupon_rate_decrement => "Absolute decimal-rate decrement after each observation." => real,
            knock_out_level_decrement => "Absolute price decrement after each observation; may be negative." => real
        ],
        kiyosi::SnowballPreset::BothDown {
            coupon_rate_decrement,
            knock_out_level_decrement
        },
        "Create a snowball with decreasing coupons and knock-out levels.",
        "on every trading day",
        r" Decrements apply after each observation; the first uses the initial
value. Signed coupons and decrements are accepted if all expanded terms are
valid. Maturity uses the final observation coupon."
    ),
    (
        dual_coupon,
        [knock_in_observation_clock_time],
        knock_out_coupon_rate => "Annual coupon rate for knock-out settlement.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [maturity_coupon_rate => "Finite annual maturity coupon rate in decimal units." => real],
        kiyosi::SnowballPreset::DualCoupon {
            maturity_coupon_rate
        },
        "Create a snowball with separate knock-out and maturity coupons.",
        "on every trading day",
        ""
    ),
    (
        parachute,
        [knock_in_observation_clock_time],
        coupon_rate => "Finite annual coupon rate in decimal units.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [final_knock_out_level => "Positive threshold replacing only the final knock-out level." => real],
        kiyosi::SnowballPreset::Parachute {
            final_knock_out_level
        },
        "Create a snowball with a distinct final knock-out level.",
        "on every trading day",
        ""
    ),
    (
        otm,
        [knock_in_observation_clock_time],
        coupon_rate => "Finite annual coupon rate in decimal units.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [upper_strike => "Positive upper terminal settlement strike." => real],
        kiyosi::SnowballPreset::Otm { upper_strike },
        "Create a snowball with a replaced upper terminal strike.",
        "on every trading day",
        ""
    ),
    (
        loss_capped,
        [knock_in_observation_clock_time],
        coupon_rate => "Finite annual coupon rate in decimal units.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [lower_strike => "Nonnegative lower terminal strike, at most the upper strike." => real],
        kiyosi::SnowballPreset::LossCapped { lower_strike },
        "Create a snowball whose lower terminal strike caps downside loss.",
        "on every trading day",
        ""
    ),
    (
        early_profit,
        [knock_in_observation_clock_time],
        first_coupon_rate => "Finite signed annual coupon rate through first_stage_end_time.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [
            second_coupon_rate => "Finite signed annual coupon rate after the cutoff and at maturity." => real,
            first_stage_end_time => "Inclusive first-stage end within life and strictly before expiry." => timestamp
        ],
        kiyosi::SnowballPreset::EarlyProfit { first_stage_end_time, second_coupon_rate },
        "Create a two-stage early-profit snowball.",
        "on every trading day",
        r" Knock-out uses the observation's stage rate over the whole holding period.
Maturity uses the final stage rate. Cutoffs need not be observation or trading
times, stages may have no autocall_observations, and rates need not decrease."
    ),
    (
        butterfly,
        [knock_in_observation_clock_time],
        first_coupon_rate => "Finite signed annual coupon rate through first_stage_end_time.",
        knock_out_level => "Positive inclusive knock-out threshold.",
        [
            second_coupon_rate => "Finite signed annual coupon in the second stage." => real,
            third_coupon_rate => "Finite signed annual coupon in the third stage and at maturity." => real,
            first_stage_end_time => "Inclusive first-stage end within life and before second_stage_end_time." => timestamp,
            second_stage_end_time => "Inclusive second-stage end strictly before expiry." => timestamp
        ],
        kiyosi::SnowballPreset::Butterfly {
            first_stage_end_time, second_coupon_rate, second_stage_end_time, third_coupon_rate
        },
        "Create a three-stage butterfly snowball.",
        "on every trading day",
        r" Knock-out uses the observation's stage rate over the whole holding period.
Maturity uses the final stage rate. Cutoffs need not be observation or trading
times, stages may have no autocall_observations, and rates need not decrease."
    ),
}

/// Snowball with knock-out coupons and no knock-in or downside loss.
///
/// Parameters
/// ----------
/// maturity_coupon_rate : float
///     Finite annual maturity coupon rate in decimal units.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// autocall_observations : iterable of SnowballObservation
///     Nonempty, strictly ordered knock-out and coupon observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
/// rebate_rate : float, optional
///     Finite nonnegative flat rebate per unit of original notional, default 0.
/// annual_rebate_rate : float, optional
///     Finite nonnegative annualized rebate rate, default 0. Accrues from
///     effective time to actual knock-out or expiry using Actual/365 Fixed,
///     including intraday time and nontrading days. Both rebates are additive
///     to coupons and principal gains or losses and pay once at termination.
///     A note knocked out before valuation is settled and has zero value.
/// calendar : TradingCalendar
///     Required owned observation calendar. Records carry no calendar; pass
///     schedule.calendar when constructing from a schedule. Controls contractual
///     observations independently of PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If coupons, principal, times or observations are invalid, or
///     barrier_state is knocked_in; this contract has no knock-in barrier.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct BinarySnowballOption(pub kiyosi::BinarySnowballOption);
snowball_properties! {
    BinarySnowballOption,
    @replace BinarySnowballOptionChanges [maturity_coupon_rate, barrier_state, principal_redemption_ratio, rebate_rate, annual_rebate_rate, effective_time, expiry_time, autocall_observations, calendar]
    {
        calendar => |v| v.extract::<TradingCalendar>().map(|v| v.0),
        maturity_coupon_rate => crate::conversions::real,
        barrier_state => crate::instruments::autocallable::state,
        principal_redemption_ratio => crate::conversions::real,
        rebate_rate => crate::conversions::real,
        annual_rebate_rate => crate::conversions::real,
        effective_time => crate::conversions::timestamp,
        expiry_time => crate::conversions::timestamp,
        autocall_observations => crate::instruments::autocallable::records,
    }
    [
    /// Return a validated replacement.
    ///
    /// Omitted fields and fields set to Ellipsis default to the instance's values.
    ///
    /// Parameters
    /// ----------
    /// maturity_coupon_rate : float, optional
    ///     Finite annual maturity coupon rate in decimal units.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Strictly prior event history; None clears it to unknown.
    /// principal_redemption_ratio : float, optional
    ///     Finite nonnegative principal redemption per unit of original notional.
    /// rebate_rate : float, optional
    ///     Finite nonnegative flat rebate per unit of original notional.
    ///     Defaults to the instance's value.
    /// annual_rebate_rate : float, optional
    ///     Finite nonnegative annualized rebate rate. Defaults to the instance's
    ///     value. Accrues from effective time to actual knock-out or expiry using
    ///     Actual/365 Fixed,
    ///     including intraday time and nontrading days. Both rebates are additive
    ///     to coupons and principal gains or losses and pay once at termination.
    ///     A note knocked out before valuation is settled and has zero value.
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// autocall_observations : iterable of SnowballObservation, optional
    ///     Product-specific knock-out and coupon observations, in time order.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar, defaulting to the instance's calendar.
    ///
    /// Returns
    /// -------
    /// BinarySnowballOption
    ///     Validated copy of the same class.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation.
    /// TypeError
    ///     If a representation or field name is invalid.
    ///
    ],

    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, effective_time, expiry_time, autocall_observations, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, annual_rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, calendar))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        maturity_coupon_rate: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        autocall_observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
        #[pyo3(from_py_with = real)] rebate_rate: f64,
        #[pyo3(from_py_with = real)] annual_rebate_rate: f64,
        calendar: TradingCalendar,

    ) -> PyResult<Self> {
        let mut terms = kiyosi::BinarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            records(autocall_observations)?,
            calendar.0,
        );
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_redemption_ratio = principal_redemption_ratio;
        terms.rebate_rate = rebate_rate;
        terms.annual_rebate_rate = annual_rebate_rate;

        let count = terms.autocall_observations.len();
        run_date_scan(py, count as u64, || kiyosi::BinarySnowballOption::new(terms)).map(Self)
    },
}

/// Snowball with distinct intact and knocked-in maturity coupons.
///
/// Parameters
/// ----------
/// maturity_coupon_rate : float
///     Finite annual maturity coupon rate in decimal units.
/// minimum_coupon_rate : float
///     Knocked-in maturity coupon, at most the intact maturity coupon.
/// knock_in_level : float
///     Positive strict downside threshold.
/// knock_in_observation : KnockInObservation
///     Observe knock-in on every trading day or only at expiry.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// autocall_observations : iterable of SnowballObservation
///     Nonempty, strictly ordered knock-out and coupon observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
/// rebate_rate : float, optional
///     Finite nonnegative flat rebate per unit of original notional, default 0.
/// annual_rebate_rate : float, optional
///     Finite nonnegative annualized rebate rate, default 0. Accrues from
///     effective time to actual knock-out or expiry using Actual/365 Fixed,
///     including intraday time and nontrading days. Both rebates are additive
///     to coupons and principal gains or losses and pay once at termination.
///     A note knocked out before valuation is settled and has zero value.
/// calendar : TradingCalendar
///     Required owned observation calendar. Records carry no calendar; pass
///     schedule.calendar when constructing from a schedule. Controls contractual
///     observations independently of PricingContext.
///
/// Raises
/// ------
/// InvalidInputError
///     If coupons, principal, barriers, times or observations are invalid.
///     minimum_coupon_rate must not exceed maturity_coupon_rate.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct TernarySnowballOption(pub kiyosi::TernarySnowballOption);
snowball_properties! {
    TernarySnowballOption,
    @replace TernarySnowballOptionChanges [maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation, barrier_state, principal_redemption_ratio, rebate_rate, annual_rebate_rate, effective_time, expiry_time, autocall_observations, calendar]
    {
        calendar => |v| v.extract::<TradingCalendar>().map(|v| v.0),
        maturity_coupon_rate => crate::conversions::real,
        minimum_coupon_rate => crate::conversions::real,
        knock_in_level => crate::conversions::real,
        knock_in_observation => |value| value.extract::<PyRef<'_, KnockInObservation>>().map(|value| value.0),
        barrier_state => crate::instruments::autocallable::state,
        principal_redemption_ratio => crate::conversions::real,
        rebate_rate => crate::conversions::real,
        annual_rebate_rate => crate::conversions::real,
        effective_time => crate::conversions::timestamp,
        expiry_time => crate::conversions::timestamp,
        autocall_observations => crate::instruments::autocallable::records,
    }
    [
    /// Return a validated replacement.
    ///
    /// Omitted fields and fields set to Ellipsis default to the instance's values.
    ///
    /// Parameters
    /// ----------
    /// maturity_coupon_rate, minimum_coupon_rate, knock_in_level : float, optional
    ///     Annual maturity and minimum coupons, and the strict downside barrier.
    /// knock_in_observation : KnockInObservation, optional
    ///     Daily or expiry-only knock-in monitoring rule.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Strictly prior event history; None clears it to unknown.
    /// principal_redemption_ratio : float, optional
    ///     Finite nonnegative principal redemption per unit of original notional.
    /// rebate_rate : float, optional
    ///     Finite nonnegative flat rebate per unit of original notional.
    ///     Defaults to the instance's value.
    /// annual_rebate_rate : float, optional
    ///     Finite nonnegative annualized rebate rate. Defaults to the instance's
    ///     value. Accrues from effective time to actual knock-out or expiry using
    ///     Actual/365 Fixed,
    ///     including intraday time and nontrading days. Both rebates are additive
    ///     to coupons and principal gains or losses and pay once at termination.
    ///     A note knocked out before valuation is settled and has zero value.
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// autocall_observations : iterable of SnowballObservation, optional
    ///     Product-specific knock-out and coupon observations, in time order.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar, defaulting to the instance's calendar.
    ///
    /// Returns
    /// -------
    /// TernarySnowballOption
    ///     Validated copy of the same class.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation.
    /// TypeError
    ///     If a representation or field name is invalid.
    ///
    ],

    #[new]
    #[pyo3(signature = (*, maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation, effective_time, expiry_time, autocall_observations, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, annual_rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, calendar))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        py: Python<'_>,
        maturity_coupon_rate: &Bound<'_, PyAny>,
        minimum_coupon_rate: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        knock_in_observation: &KnockInObservation,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        autocall_observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
        #[pyo3(from_py_with = real)] rebate_rate: f64,
        #[pyo3(from_py_with = real)] annual_rebate_rate: f64,
        calendar: TradingCalendar,

    ) -> PyResult<Self> {
        let mut terms = kiyosi::TernarySnowballTerms::new(
            real(maturity_coupon_rate)?,
            real(minimum_coupon_rate)?,
            real(knock_in_level)?,
            timestamp(effective_time)?,
            timestamp(expiry_time)?,
            records(autocall_observations)?,
            knock_in_observation.0,
            calendar.0,
        );
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_redemption_ratio = principal_redemption_ratio;
        terms.rebate_rate = rebate_rate;
        terms.annual_rebate_rate = annual_rebate_rate;

        let count = terms.autocall_observations.len();
        run_date_scan(py, count as u64, || kiyosi::TernarySnowballOption::new(terms)).map(Self)
    },
    /// Return positive strict downside threshold.
    #[getter]
    fn knock_in_level(&self) -> f64 {
        self.0.terms().knock_in_level
    },
    /// Return the annual knocked-in maturity coupon.
    #[getter]
    fn minimum_coupon_rate(&self) -> f64 {
        self.0.terms().minimum_coupon_rate
    },
    /// Return an owned knock-in observation rule.
    #[getter]
    fn knock_in_observation(&self) -> KnockInObservation {
        KnockInObservation(self.0.terms().knock_in_observation)
    },
}

/// Autocallable with conditional period coupons and capped downside loss.
///
/// Parameters
/// ----------
/// coupon_per_period : float
///     Finite signed coupon proportion per qualifying observation, independent
///     of period length.
/// initial_spot : float
///     Positive reference spot used to normalize downside losses.
/// knock_in_level : float
///     Positive strict downside threshold.
/// upper_strike : float
///     Positive upper terminal settlement strike.
/// lower_strike : float
///     Nonnegative lower terminal strike, at most the upper strike.
/// knock_in_observation : KnockInObservation
///     Observe knock-in on every trading day or only at expiry.
/// effective_time : datetime.datetime
///     Inclusive contract start.
/// expiry_time : datetime.datetime
///     Inclusive expiry time.
/// knock_out_observations : iterable of KnockOutObservation
///     Nonempty, strictly ordered knock-out observations within life.
/// coupon_observations : iterable of CouponObservation
///     Independent nonempty, strictly ordered coupon observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
/// rebate_rate : float, optional
///     Finite nonnegative flat rebate per unit of original notional, default 0.
/// annual_rebate_rate : float, optional
///     Finite nonnegative annualized rebate rate, default 0. Accrues from
///     effective time to actual knock-out or expiry using Actual/365 Fixed,
///     including intraday time and nontrading days. Both rebates are additive
///     to coupons and principal gains or losses and pay once at termination.
///     A note knocked out before valuation is settled and has zero value.
/// calendar : TradingCalendar
///     Required owned observation calendar. Records carry no calendar; pass
///     schedule.calendar when constructing from a schedule. Controls contractual
///     observations independently of PricingContext.
///
/// Raises
/// ------
/// TypeError
///     If a required value is missing or a supplied representation is invalid.
/// InvalidInputError
///     If amounts, strikes, life times or either observation list are invalid.
///
/// Notes
/// -----
/// Periodic coupons pay once per qualifying observation, including at the
/// effective timestamp, without time accrual. None is not a coupon value.
/// Coupon and knock-out barriers are inclusive; knock-in is strict.
/// Past coupons are settled. There is no coupon memory or maturity coupon.
/// Knock-out and coupon schedules are independent; neither implies an observation
/// in the other. Expiry adds no implicit coupon or knock-out observation.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PhoenixOption(pub kiyosi::PhoenixOption);
macro_rules! phoenix_preset {
    ([$($properties:tt)*], $(
        ($name:ident, $knock_out:ident, [$($clock:ident)?], [$($extra:ident => $extra_doc:literal),*], $preset:expr, $summary:literal, $convention:literal)
    ),+ $(,)?) => {
        autocallable_properties! {
            $($properties)*
            $(
                #[doc = $summary]
                ///
                /// Parameters
                /// ----------
                /// coupon_per_period : float
                ///     Finite signed coupon proportion per qualifying observation; not
                ///     annualized.
                /// initial_spot : float
                ///     Positive reference spot used to normalize downside losses.
                /// knock_in_level : float
                ///     Positive strict knock-in threshold.
                #[doc = concat!(stringify!($knock_out), " : float")]
                ///     Positive inclusive threshold at the first eligible knock-out observation.
                $(
                    #[doc = concat!(stringify!($extra), " : float")]
                    #[doc = concat!("    ", $extra_doc)]
                )*
                $(
                    #[doc = concat!(stringify!($clock), " : datetime.time")]
                    ///     Daily knock-in observation clock on trading days.
                )?
                /// autocall_times : iterable of datetime.datetime
                ///     Strictly increasing observation and payment times within life.
                /// effective_time : datetime.datetime
                ///     Inclusive contract start.
                /// expiry_time : datetime.datetime
                ///     Inclusive expiry time.
                /// lockout_periods : int, optional
                ///     Initial observations excluded from knock-out; defaults to zero and must
                ///     leave at least one eligible observation. Coupon and knock-in observations
                ///     apply during the lockout.
                /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
                ///     State strictly before valuation. None is unknown; no_event means no prior
                ///     knock-in or knock-out, knocked_in means outstanding after knock-in, and
                ///     knocked_out means settled. Current observations use current spot.
                /// principal_redemption_ratio : float, optional
                ///     Nonnegative normalized principal, independent of coupon and loss
                ///     components.
                /// rebate_rate : float, optional
                ///     Finite nonnegative flat rebate per unit of original notional, default 0.
                /// annual_rebate_rate : float, optional
                ///     Finite nonnegative annualized rebate rate, default 0. Accrues from
                ///     effective time to actual knock-out or expiry using Actual/365 Fixed,
                ///     including intraday time and nontrading days. Both rebates are additive
                ///     to coupons and principal gains or losses and pay once at termination.
                ///     A note knocked out before valuation is settled and has zero value.
                /// calendar : TradingCalendar, optional
                ///     Explicit calendar override; omission or Ellipsis inherits an
                ///     ObservationSchedule calendar, otherwise weekdays.
                ///
                /// Returns
                /// -------
                /// PhoenixOption
                #[doc = concat!("    ", $convention)]
                ///
                /// Raises
                /// ------
                /// InvalidInputError
                ///     If numeric terms, lockout, times or observations are invalid.
                /// TypeError
                ///     If a supplied representation is invalid, including Boolean or fractional counts.
                /// OverflowError
                ///     If a count is negative or outside the native unsigned integer range.
                ///
                /// Notes
                /// -----
                /// Coupons pay once per qualifying observation, including a qualifying
                /// knock-out, with no day accrual or memory. Knock-out and coupon barriers are
                /// inclusive; knock-in is strict. Observation times are payment times; no
                /// observation is implicitly added at expiry. Calendar compatibility is
                /// checked on construction; history consistency is checked during pricing.
                /// Past coupons are settled.
                ///
                #[staticmethod]
                #[pyo3(signature = (*, coupon_per_period, initial_spot, knock_in_level, $knock_out, $($extra,)* $($clock,)? autocall_times, effective_time, expiry_time, lockout_periods=kiyosi::DEFAULT_PHOENIX_LOCKOUT_PERIODS, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, annual_rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, calendar=crate::conversions::OMITTED))]
                #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
                fn $name(
                    py: Python<'_>,
                    coupon_per_period: &Bound<'_, PyAny>,
                    initial_spot: &Bound<'_, PyAny>,
                    knock_in_level: &Bound<'_, PyAny>,
                    $knock_out: &Bound<'_, PyAny>,
                    $($extra: &Bound<'_, PyAny>,)*
                    $($clock: &Bound<'_, PyAny>,)?
                    autocall_times: &Bound<'_, PyAny>,
                    effective_time: &Bound<'_, PyAny>,
                    expiry_time: &Bound<'_, PyAny>,
                    #[pyo3(from_py_with = integer_usize)] lockout_periods: usize,
                    barrier_state: Option<&Bound<'_, PyAny>>,
                    #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
                    #[pyo3(from_py_with = real)] rebate_rate: f64,
                    #[pyo3(from_py_with = real)] annual_rebate_rate: f64,
                    #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

                ) -> PyResult<Self> {
                    let mut terms = kiyosi::PhoenixPresetTerms::new(
                        real(coupon_per_period)?, real(initial_spot)?, real(knock_in_level)?,
                        real($knock_out)?, timestamp(effective_time)?, timestamp(expiry_time)?,
                        timestamps(autocall_times)?,
                    );
                    $(let $extra = real($extra)?;)*
                    $(terms.knock_in_observation_clock_time = Some(crate::conversions::clock_time($clock)?);)?
                    terms.lockout_periods = lockout_periods;
                    if let Some(value) = barrier_state {
                        terms.barrier_state = state(value)?;
                    }
                    terms.principal_redemption_ratio = principal_redemption_ratio;
                    terms.rebate_rate = rebate_rate;
                    terms.annual_rebate_rate = annual_rebate_rate;
                    terms.calendar = crate::calendar::observation_calendar(calendar, Some(autocall_times))?.unwrap_or_default();

                    run_date_scan(py, terms.autocall_times.len() as u64, || terms.expand($preset)).map(Self)
                },
            )+
        }
    };
}
phoenix_preset! {
    [
        PhoenixOption,
        @replace PhoenixOptionChanges [coupon_per_period, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, barrier_state, principal_redemption_ratio, rebate_rate, annual_rebate_rate, effective_time, expiry_time, knock_out_observations, coupon_observations, calendar]
        [knock_out_observations, coupon_observations]
        {
            calendar => |v| v.extract::<TradingCalendar>().map(|v| v.0),
            coupon_per_period => crate::conversions::real,
            initial_spot => crate::conversions::real,
            knock_in_level => crate::conversions::real,
            upper_strike => crate::conversions::real,
            lower_strike => crate::conversions::real,
            knock_in_observation => |value| value.extract::<PyRef<'_, KnockInObservation>>().map(|value| value.0),
            barrier_state => crate::instruments::autocallable::state,
            principal_redemption_ratio => crate::conversions::real,
            rebate_rate => crate::conversions::real,
            annual_rebate_rate => crate::conversions::real,
            effective_time => crate::conversions::timestamp,
            expiry_time => crate::conversions::timestamp,
            knock_out_observations => crate::instruments::autocallable::knock_out_records,
            coupon_observations => crate::instruments::autocallable::coupon_records,
        }
        [
        /// Return a validated replacement.
        ///
        /// Omitted fields and fields set to Ellipsis default to the instance's values.
        ///
        /// Parameters
        /// ----------
        /// coupon_per_period : float, optional
        ///     Finite signed periodic coupon rate.
        /// initial_spot, knock_in_level, upper_strike, lower_strike : float, optional
        ///     Reference spot, strict downside barrier, and loss interval strikes.
        /// knock_in_observation : KnockInObservation, optional
        ///     Daily or expiry-only knock-in monitoring rule.
        /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        ///     Strictly prior event history; None clears it to unknown.
        /// principal_redemption_ratio : float, optional
        ///     Finite nonnegative principal redemption per unit of original notional.
        /// rebate_rate : float, optional
        ///     Finite nonnegative flat rebate per unit of original notional.
        ///     Defaults to the instance's value.
        /// annual_rebate_rate : float, optional
        ///     Finite nonnegative annualized rebate rate. Defaults to the instance's
        ///     value. Accrues from effective time to actual knock-out or expiry using
        ///     Actual/365 Fixed,
        ///     including intraday time and nontrading days. Both rebates are additive
        ///     to coupons and principal gains or losses and pay once at termination.
        ///     A note knocked out before valuation is settled and has zero value.
        /// effective_time, expiry_time : datetime.datetime, optional
        ///     Inclusive contract start and expiry timestamps.
        /// knock_out_observations : iterable of KnockOutObservation, optional
        ///     Knock-out observations in time order.
        /// coupon_observations : iterable of CouponObservation, optional
        ///     Independent coupon observations in time order.
        /// calendar : TradingCalendar, optional
        ///     Owned contractual observation calendar, defaulting to the instance's calendar.
        ///
        /// Returns
        /// -------
        /// PhoenixOption
        ///     Validated copy of the same class.
        ///
        /// Raises
        /// ------
        /// KiyosiError
        ///     If the resulting contract fails core validation.
        /// TypeError
        ///     If a representation or field name is invalid, including a None coupon.
        ///
        ],
        #[new]
        #[pyo3(signature = (*, coupon_per_period, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, effective_time, expiry_time, knock_out_observations, coupon_observations, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, annual_rebate_rate=kiyosi::DEFAULT_AUTOCALLABLE_REBATE_RATE, calendar))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
        fn new(
            py: Python<'_>,
            #[pyo3(from_py_with = real)] coupon_per_period: f64,
            initial_spot: &Bound<'_, PyAny>,
            knock_in_level: &Bound<'_, PyAny>,
            upper_strike: &Bound<'_, PyAny>,
            lower_strike: &Bound<'_, PyAny>,
            knock_in_observation: &KnockInObservation,
            effective_time: &Bound<'_, PyAny>,
            expiry_time: &Bound<'_, PyAny>,
            knock_out_observations: &Bound<'_, PyAny>,
            coupon_observations: &Bound<'_, PyAny>,
            barrier_state: Option<&Bound<'_, PyAny>>,
            #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
            #[pyo3(from_py_with = real)] rebate_rate: f64,
            #[pyo3(from_py_with = real)] annual_rebate_rate: f64,
            calendar: TradingCalendar,

        ) -> PyResult<Self> {
            let mut terms = kiyosi::PhoenixTerms::new(
                coupon_per_period,
                real(initial_spot)?,
                real(knock_in_level)?,
                real(upper_strike)?,
                real(lower_strike)?,
                timestamp(effective_time)?,
                timestamp(expiry_time)?,
                knock_out_records(knock_out_observations)?,
                coupon_records(coupon_observations)?,
                knock_in_observation.0,
                calendar.0,
            );
            if let Some(value) = barrier_state {
                terms.barrier_state = state(value)?;
            }
            terms.principal_redemption_ratio = principal_redemption_ratio;
            terms.rebate_rate = rebate_rate;
            terms.annual_rebate_rate = annual_rebate_rate;

            let count = terms.knock_out_observations.len() + terms.coupon_observations.len();
            run_date_scan(py, count as u64, || kiyosi::PhoenixOption::new(terms)).map(Self)
        },
        /// Return a note with the supplied periodic coupon rate.
        ///
        /// History and other terms are preserved.
        ///
        /// Parameters
        /// ----------
        /// coupon_per_period : float
        ///     Finite signed payment per observation, as a proportion of initial notional.
        ///
        /// Returns
        /// -------
        /// PhoenixOption
        ///     Note using the supplied periodic coupon rate.
        ///
        /// Raises
        /// ------
        /// InvalidInputError
        ///     If the coupon is non-finite.
        /// TypeError
        ///     If the supplied representation is invalid.
        ///
        #[pyo3(signature = (*, coupon_per_period))]
        fn with_coupon(
            &self,
            py: Python<'_>,
            #[pyo3(from_py_with = real)] coupon_per_period: f64,
        ) -> PyResult<Self> {
            let count = self.0.knock_out_observations().len() + self.0.coupon_observations().len();
            run_date_scan(py, count as u64, || self.0.with_coupon(coupon_per_period)).map(Self)
        },
        /// Return copied Phoenix coupon thresholds in observation order.
        #[getter]
        fn coupon_barrier_levels<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
            python_tuple(py, self.0.coupon_observations(), |r| Ok(r.coupon_barrier_level()))
        },
        /// Return the finite signed coupon proportion per qualifying observation.
        #[getter]
        fn coupon_per_period(&self) -> f64 {
            self.0.terms().coupon_per_period
        },
        /// Return positive reference spot used to normalize downside losses.
        #[getter]
        fn initial_spot(&self) -> f64 {
            self.0.terms().initial_spot
        },
        /// Return positive strict downside threshold.
        #[getter]
        fn knock_in_level(&self) -> f64 {
            self.0.terms().knock_in_level
        },
        /// Return positive upper terminal settlement strike.
        #[getter]
        fn upper_strike(&self) -> f64 {
            self.0.terms().upper_strike
        },
        /// Return nonnegative lower terminal strike, at most the upper strike.
        #[getter]
        fn lower_strike(&self) -> f64 {
            self.0.terms().lower_strike
        },
        /// Return an owned knock-in observation rule.
        #[getter]
        fn knock_in_observation(&self) -> KnockInObservation {
            KnockInObservation(self.0.terms().knock_in_observation)
        },    /// Return owned copies of the knock-out observations.
        #[getter]
        fn knock_out_observations<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
            python_tuple(py, self.0.knock_out_observations(), |&r| Ok(KnockOutObservation(r)))
        },
        /// Return copied exact knock-out observation times.
        #[getter]
        fn knock_out_times<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
            python_tuple(py, self.0.knock_out_observations(), |r| python_timestamp(py, r.time()))
        },
        /// Return owned copies of the coupon observations.
        #[getter]
        fn coupon_observations<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
            python_tuple(py, self.0.coupon_observations(), |&r| Ok(CouponObservation(r)))
        },
        /// Return copied exact coupon observation times.
        #[getter]
        fn coupon_times<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
            python_tuple(py, self.0.coupon_observations(), |r| python_timestamp(py, r.time()))
        },
        /// Return copied knock-out thresholds in knock-out observation order.
        #[getter]
        fn knock_out_levels<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
            python_tuple(py, self.0.knock_out_observations(), |r| Ok(r.knock_out_level()))
        },
        fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
            let value = slf.borrow();
            let py = slf.py();
            crate::representation::format(slf.as_any(), Self::_STATE_FIELDS, &[
                ("calendar", Some(crate::representation::calendar_preview(py, value.0.calendar())?.unwrap_or_else(|| "TradingCalendar.weekdays()".into()))),
                ("knock_out_observations", Some(crate::representation::preview(value.0.knock_out_observations(), |r| KnockOutObservation(*r).__repr__(py))?)),
                ("coupon_observations", Some(crate::representation::preview(value.0.coupon_observations(), |r| CouponObservation(*r).__repr__(py))?)),
            ])
        },

    ],
    (
        fixed_coupon,
        knock_out_level,
        [],
        [],
        kiyosi::PhoenixPreset::FixedCouponNote,
        "Create a fixed coupon note with unconditional surviving coupons.",
        "Validated owned contract with expiry-only knock-in, knock-in level as upper strike and zero lower strike."
    ),
    (
        digital_coupon,
        knock_out_level,
        [],
        [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on knock-out times."],
        kiyosi::PhoenixPreset::DigitalCouponNote { coupon_barrier_level },
        "Create a digital coupon note with barrier-conditional fixed coupons.",
        "Validated owned contract with expiry-only knock-in, knock-in level as upper strike and zero lower strike."
    ),
    (
        standard,
        knock_out_level,
        [knock_in_observation_clock_time],
        [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on knock-out times."],
        kiyosi::PhoenixPreset::Standard { coupon_barrier_level },
        "Create a standard Phoenix with a constant knock-out barrier and optional initial lockout.",
        "Validated owned contract with daily knock-in, initial spot as upper strike and zero lower strike."
    ),
    (
        step_down,
        initial_knock_out_level,
        [knock_in_observation_clock_time],
        [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on knock-out times.",
         knock_out_level_decrement => "Finite nonnegative absolute decrement after each eligible observation; lockout does not consume ladder steps."],
        kiyosi::PhoenixPreset::StepDown { coupon_barrier_level, knock_out_level_decrement },
        "Create a step-down Phoenix whose first eligible knock-out uses the initial level.",
        "Validated owned contract with daily knock-in, initial spot as upper strike and zero lower strike."
    ),
}

#[derive(FromPyObject)]
pub(crate) enum AutocallableInstrument<'py> {
    Phoenix(PyRef<'py, PhoenixOption>),
    Binary(PyRef<'py, BinarySnowballOption>),
    Ternary(PyRef<'py, TernarySnowballOption>),
    Snowball(PyRef<'py, SnowballOption>),
}

macro_rules! with_note {
    ($instrument:expr, $note:ident, $body:expr) => {{
        use $crate::instruments::autocallable::AutocallableInstrument;
        match $instrument {
            AutocallableInstrument::Phoenix(note) => {
                let $note = &note.0;
                $body
            }
            AutocallableInstrument::Binary(note) => {
                let $note = &note.0;
                $body
            }
            AutocallableInstrument::Ternary(note) => {
                let $note = &note.0;
                $body
            }
            AutocallableInstrument::Snowball(note) => {
                let $note = &note.0;
                $body
            }
        }
    }};
}
pub(crate) use with_note;
