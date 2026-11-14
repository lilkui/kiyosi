use crate::calendar::TradingCalendar;
use crate::conversions::{
    integer_usize, python_timestamp, real, reals, supplied, timestamp, timestamps,
};
use crate::error::domain;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyTuple;

// Signature order and conversion order are separate to preserve boundary failures.
macro_rules! autocallable_properties {
    ($name:ident,
     @replace $changes:ident [$($parameter:ident),+]
     {$($field:ident => $convert:expr),+ $(,)?}
     [$(#[$replacement_doc:meta])*],
     $($extra:item),* $(,)?) => {
        value_methods!(@documented $name, $changes, [$($parameter),+],
            {$($field: $field.map($convert).transpose()?),+}, [$(#[$replacement_doc])*], {
            $($extra)*

            fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
                let value = slf.borrow();
                let py = slf.py();
                crate::representation::format(slf.as_any(), Self::_STATE_FIELDS, &[
                    ("calendar", Some(crate::representation::calendar_preview(py, value.0.calendar())?
                        .unwrap_or_else(|| "weekdays_calendar()".into()))),
                    ("autocall_observations", Some(crate::representation::observations(py, value.0.autocall_observations())?)),
                ])
            }

            /// Return an owned copy of the contract's monitoring calendar.
            #[getter]
            fn calendar(&self) -> TradingCalendar { TradingCalendar(self.0.calendar().clone()) }
            /// Return owned copies of the contractual knock-out and coupon observations.
            ///
            /// Phoenix records include coupon observations during knock-out lockout.
            /// Knock-in monitoring is available through knock_in_observation_schedule.
            #[getter]
            fn autocall_observations<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                PyTuple::new(py, self.0.autocall_observations().iter().copied().map(AutocallableObservation))
            }
            /// Return an owned whole-life knock-in schedule, or None without knock-in.
            ///
            /// Daily monitoring includes the effective instant at the contractual clock on
            /// open dates. Expiry-only monitoring returns just expiry. Observations for
            /// knock-out and coupons remain available through autocall_observations.
            ///
            #[getter]
            fn knock_in_observation_schedule(&self) -> PyResult<Option<crate::schedule::ObservationSchedule>> {
                Ok(domain(self.0.knock_in_observation_schedule())?.map(crate::schedule::ObservationSchedule))
            }
            /// Return a copy of the contractual knock-out and coupon observation times.
            ///
            /// Includes Phoenix coupon times during knock-out lockout; excludes
            /// independently scheduled knock-in monitoring.
            #[getter]
            fn autocall_times<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                PyTuple::new(py, self.0.autocall_observations().iter()
                    .map(|r| python_timestamp(py, r.time()))
                    .collect::<PyResult<Vec<_>>>()?)
            }
            /// Return copied knock-out thresholds in observation order.
            #[getter]
            fn knock_out_levels<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                PyTuple::new(py, self.0.autocall_observations().iter().map(|r| r.knock_out_level()))
            }
            /// Return nonnegative normalized principal, independent of coupons and losses.
            #[getter]
            fn principal_redemption_ratio(&self) -> f64 {
                self.0.terms().principal_redemption_ratio
            }
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
        });
    };
}

macro_rules! snowball_properties {
    ($name:ident,
     @replace $changes:ident $parameters:tt $conversions:tt $documentation:tt,
     $($extra:item),* $(,)?) => {
        autocallable_properties! {
            $name,
            @replace $changes $parameters $conversions $documentation,
            $($extra,)*
            /// Return copied annual knock-out coupons in observation order.
            #[getter]
            fn knock_out_coupon_rates<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
                PyTuple::new(py, self.0.autocall_observations().iter().map(|r| r.coupon_rate().unwrap_or_default()))
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
                first_observation_coupon: &Bound<'_, PyAny>,
                quote_convention: &Bound<'_, PyAny>,
            ) -> PyResult<Self> {
                Ok(Self(domain(self.0.with_coupon(real(first_observation_coupon)?, coupon_convention(quote_convention)?))?))
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
/// AutocallableObservation.
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

pub(crate) fn records(values: &Bound<'_, PyAny>) -> PyResult<Vec<kiyosi::AutocallableObservation>> {
    values
        .try_iter()?
        .map(|item| Ok(item?.extract::<PyRef<'_, AutocallableObservation>>()?.0))
        .collect()
}

/// Knock-out and coupon observation with validated numeric terms.
///
/// Notes
/// -----
/// Knock-in monitoring is defined independently by KnockInObservation. Phoenix
/// coupon observations remain active during knock-out lockout.
///
/// Construct with snowball(..., coupon_rate=...) or phoenix(...,
/// coupon_barrier_level=...). A zero knock-out coupon or unconditional Phoenix
/// coupon requires an explicit zero. Observations retain their snowball or
/// Phoenix kind, including at zero. Notes reject observations of the other
/// kind; replacements preserve the kind. Numeric terms are validated in Rust
/// on construction. Note construction also validates observation ordering,
/// life bounds and product-specific constraints. Invalid numeric terms raise
/// InvalidInputError.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct AutocallableObservation(pub kiyosi::AutocallableObservation);
#[pymethods]
impl AutocallableObservation {
    /// Create a snowball observation with an annual knock-out coupon.
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
    #[staticmethod]
    #[pyo3(signature = (time, knock_out_level, *, coupon_rate))]
    fn snowball(
        time: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        coupon_rate: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::AutocallableObservation::snowball(
            timestamp(time)?,
            real(knock_out_level)?,
            real(coupon_rate)?,
        ))?))
    }
    /// Create a Phoenix observation with a coupon threshold.
    ///
    /// The note supplies the coupon amount.
    ///
    /// Parameters
    /// ----------
    /// time : datetime.datetime
    ///     Exact observation timestamp.
    /// knock_out_level : float
    ///     Positive finite inclusive knock-out threshold.
    /// coupon_barrier_level : float
    ///     Nonnegative finite coupon threshold; supply zero explicitly for
    ///     unconditional coupons.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If the knock-out level is nonpositive or non-finite, or the coupon
    ///     threshold is negative or non-finite.
    ///
    #[staticmethod]
    #[pyo3(signature = (time, knock_out_level, *, coupon_barrier_level))]
    fn phoenix(
        time: &Bound<'_, PyAny>,
        knock_out_level: &Bound<'_, PyAny>,
        coupon_barrier_level: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self(domain(kiyosi::AutocallableObservation::phoenix(
            timestamp(time)?,
            real(knock_out_level)?,
            real(coupon_barrier_level)?,
        ))?))
    }
    /// Return a validated observation with the supplied fields replaced.
    ///
    /// Omitted fields or Ellipsis retain their values and observation kind.
    ///
    /// Parameters
    /// ----------
    /// time : datetime.datetime, optional
    ///     Observation timestamp.
    /// knock_out_level : float, optional
    ///     Positive finite knock-out threshold.
    /// coupon_rate : float or None, optional
    ///     Finite annual snowball coupon. None only for Phoenix observations.
    /// coupon_barrier_level : float or None, optional
    ///     Nonnegative Phoenix threshold. None only for snowball observations.
    ///
    /// Returns
    /// -------
    /// AutocallableObservation
    ///     Validated copy; note construction checks schedule constraints.
    ///
    /// Raises
    /// ------
    /// InvalidInputError
    ///     If numeric terms are invalid or belong to the other observation kind,
    ///     including zero values.
    /// TypeError
    ///     If the active coupon field is None or representations are invalid.
    ///
    #[pyo3(signature = (*, time=crate::conversions::OMITTED, knock_out_level=crate::conversions::OMITTED, coupon_rate=crate::conversions::OMITTED, coupon_barrier_level=crate::conversions::OMITTED))]
    fn replace(
        &self,
        #[pyo3(from_py_with = supplied)] time: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] knock_out_level: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] coupon_rate: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = supplied)] coupon_barrier_level: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let active_coupon = match self.0.kind() {
            kiyosi::AutocallableObservationKind::Snowball => coupon_rate,
            kiyosi::AutocallableObservationKind::Phoenix => coupon_barrier_level,
        };
        if active_coupon.is_some_and(Bound::is_none) {
            return Err(PyTypeError::new_err(
                "the active coupon field cannot be None",
            ));
        }
        Ok(Self(domain(
            self.0.replace(kiyosi::AutocallableObservationChanges {
                time: time.map(timestamp).transpose()?,
                knock_out_level: knock_out_level.map(real).transpose()?,
                coupon_rate: coupon_rate.filter(|v| !v.is_none()).map(real).transpose()?,
                coupon_barrier_level: coupon_barrier_level
                    .filter(|v| !v.is_none())
                    .map(real)
                    .transpose()?,
            }),
        )?))
    }
    /// Return snowball or phoenix, independently of coupon values.
    #[getter]
    pub(crate) fn kind(&self) -> &'static str {
        match self.0.kind() {
            kiyosi::AutocallableObservationKind::Snowball => "snowball",
            kiyosi::AutocallableObservationKind::Phoenix => "phoenix",
        }
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
    /// Return the annual snowball coupon rate, or None for Phoenix observations.
    #[getter]
    fn coupon_rate(&self) -> Option<f64> {
        self.0.coupon_rate()
    }
    /// Return the Phoenix coupon threshold, or None for snowball observations.
    #[getter]
    fn coupon_barrier_level(&self) -> Option<f64> {
        self.0.coupon_barrier_level()
    }
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        crate::representation::observation(py, &self.0)
    }
}

/// Combine parallel sequences into validated observation records.
///
/// Parameters
/// ----------
/// autocall_times : iterable of datetime.datetime
///     Knock-out and coupon observation times; the note checks ordering and life
///     bounds.
/// knock_out_levels : iterable of float
///     Positive inclusive knock-out levels, one per observation timestamp.
/// knock_out_coupon_rates : iterable of float or None, optional
///     Explicit annual snowball coupons, including zero for no coupon.
/// coupon_barrier_levels : iterable of float or None, optional
///     Explicit Phoenix coupon thresholds, including zero for unconditional
///     coupons.
///
/// Returns
/// -------
/// list of AutocallableObservation
///     Observation records.
///
/// Raises
/// ------
/// InvalidInputError
///     If neither or both coupon choices are supplied, sequence lengths
///     differ, or numeric levels or coupons are invalid.
///
/// Notes
/// -----
/// The note constructor validates ordering, life bounds and product-specific
/// constraints. Supply exactly one of knock_out_coupon_rates and
/// coupon_barrier_levels.
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
/// >>> calendar = market.custom_calendar(
/// ...     base=market.weekdays_calendar(),
/// ...     closed_dates=[datetime(2025, 1, 6).date()],
/// ... )
/// >>> schedule = market.ObservationSchedule(
/// ...     times=[end], start_time=start, end_time=end, calendar=calendar,
/// ... )
/// >>> records = ins.autocallable_observations(
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
#[pyfunction]
#[pyo3(signature = (autocall_times, knock_out_levels, *, knock_out_coupon_rates=None, coupon_barrier_levels=None))]
pub(crate) fn autocallable_observations(
    autocall_times: &Bound<'_, PyAny>,
    knock_out_levels: &Bound<'_, PyAny>,
    knock_out_coupon_rates: Option<&Bound<'_, PyAny>>,
    coupon_barrier_levels: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<AutocallableObservation>> {
    let times = timestamps(autocall_times)?;
    let levels = reals(knock_out_levels)?;
    let coupons = knock_out_coupon_rates.map(reals).transpose()?;
    let barriers = coupon_barrier_levels.map(reals).transpose()?;
    Ok(domain(kiyosi::autocallable_observations(
        &times,
        &levels,
        coupons.as_deref(),
        barriers.as_deref(),
    ))?
    .into_iter()
    .map(AutocallableObservation)
    .collect())
}

macro_rules! snowball_preset {
    ($name:ident, [$($clock:ident)?], $coupon:ident => $coupon_doc:literal, $level:ident => $level_doc:literal,
     [$($extra:ident => $extra_doc:literal => $convert:ident),*], $preset:expr,
     $summary:literal, $monitoring:literal, $notes:literal) => {
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
        #[pyfunction]
        #[pyo3(signature = (*, $coupon, initial_spot, knock_in_level, $level, $($extra,)* autocall_times, effective_time, expiry_time, $($clock,)? barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, calendar=crate::conversions::OMITTED))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
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
            #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

        ) -> PyResult<SnowballOption> {
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
            terms.calendar = crate::calendar::observation_calendar(calendar, Some(autocall_times))?.unwrap_or_default();

            Ok(SnowballOption(domain(terms.expand($preset))?))
        }
    };
    (@clock $clock:ident) => { Some(crate::conversions::clock_time($clock)?) };
    (@clock) => { None };
    (@type real) => { "float" };
    (@type timestamp) => { "datetime.datetime" };
}
snowball_preset!(
    standard_snowball,
    [knock_in_observation_clock_time],
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [],
    kiyosi::SnowballPreset::Standard,
    "Create constant knock-out coupons and levels with daily knock-in monitoring.",
    "on every trading day",
    ""
);
snowball_preset!(
    european_snowball,
    [],
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [],
    kiyosi::SnowballPreset::European,
    "Create a snowball with knock-in observed only at expiry.",
    "only at expiry",
    ""
);
snowball_preset!(
    step_down_snowball,
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
value. Signed coupons and decrements are accepted if all expanded terms
remain valid."
);
snowball_preset!(
    both_down_snowball,
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
value. Signed coupons and decrements are accepted if all expanded terms
remain valid. Maturity uses the final observation coupon."
);
snowball_preset!(
    dual_coupon_snowball,
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
);
snowball_preset!(
    parachute_snowball,
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
);
snowball_preset!(
    otm_snowball,
    [knock_in_observation_clock_time],
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [upper_strike => "Positive upper terminal settlement strike." => real],
    kiyosi::SnowballPreset::Otm { upper_strike },
    "Create a snowball with a replaced upper terminal strike.",
    "on every trading day",
    ""
);
snowball_preset!(
    loss_capped_snowball,
    [knock_in_observation_clock_time],
    coupon_rate => "Finite annual coupon rate in decimal units.",
    knock_out_level => "Positive inclusive knock-out threshold.",
    [lower_strike => "Nonnegative lower terminal strike, at most the upper strike." => real],
    kiyosi::SnowballPreset::LossCapped { lower_strike },
    "Create a snowball whose lower terminal strike caps downside loss.",
    "on every trading day",
    ""
);

snowball_preset!(
    early_profit_snowball,
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
);
snowball_preset!(
    butterfly_snowball,
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
/// autocall_observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered knock-out and coupon observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
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
snowball_properties! {
    SnowballOption,
    @replace SnowballOptionChanges [maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, barrier_state, principal_redemption_ratio, effective_time, expiry_time, autocall_observations, calendar]
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
        effective_time => crate::conversions::timestamp,
        expiry_time => crate::conversions::timestamp,
        autocall_observations => crate::instruments::autocallable::records,
    }
    [
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
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
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// autocall_observations : iterable of AutocallableObservation, optional
    ///     Product-specific knock-out and coupon observations, in time order.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
    ///
    /// Returns
    /// -------
    /// SnowballOption
    ///     Independent contract of the same class, preserving omitted history and
    ///     terms.
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
    #[pyo3(signature = (*, maturity_coupon_rate, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, effective_time, expiry_time, autocall_observations, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, calendar))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
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

        Ok(Self(domain(kiyosi::SnowballOption::new(terms))?))
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
/// autocall_observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered knock-out and coupon observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
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
    @replace BinarySnowballOptionChanges [maturity_coupon_rate, barrier_state, principal_redemption_ratio, effective_time, expiry_time, autocall_observations, calendar]
    {
        calendar => |v| v.extract::<TradingCalendar>().map(|v| v.0),
        maturity_coupon_rate => crate::conversions::real,
        barrier_state => crate::instruments::autocallable::state,
        principal_redemption_ratio => crate::conversions::real,
        effective_time => crate::conversions::timestamp,
        expiry_time => crate::conversions::timestamp,
        autocall_observations => crate::instruments::autocallable::records,
    }
    [
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
    ///
    /// Parameters
    /// ----------
    /// maturity_coupon_rate : float, optional
    ///     Finite annual maturity coupon rate in decimal units.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Strictly prior event history; None clears it to unknown.
    /// principal_redemption_ratio : float, optional
    ///     Finite nonnegative principal redemption per unit of original notional.
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// autocall_observations : iterable of AutocallableObservation, optional
    ///     Product-specific knock-out and coupon observations, in time order.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
    ///
    /// Returns
    /// -------
    /// BinarySnowballOption
    ///     Independent contract of the same class, preserving omitted history and
    ///     terms.
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
    #[pyo3(signature = (*, maturity_coupon_rate, effective_time, expiry_time, autocall_observations, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, calendar))]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        autocall_observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
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

        Ok(Self(domain(kiyosi::BinarySnowballOption::new(terms))?))
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
/// autocall_observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered knock-out and coupon observations within life.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
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
    @replace TernarySnowballOptionChanges [maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation, barrier_state, principal_redemption_ratio, effective_time, expiry_time, autocall_observations, calendar]
    {
        calendar => |v| v.extract::<TradingCalendar>().map(|v| v.0),
        maturity_coupon_rate => crate::conversions::real,
        minimum_coupon_rate => crate::conversions::real,
        knock_in_level => crate::conversions::real,
        knock_in_observation => |value| value.extract::<PyRef<'_, KnockInObservation>>().map(|value| value.0),
        barrier_state => crate::instruments::autocallable::state,
        principal_redemption_ratio => crate::conversions::real,
        effective_time => crate::conversions::timestamp,
        expiry_time => crate::conversions::timestamp,
        autocall_observations => crate::instruments::autocallable::records,
    }
    [
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
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
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// autocall_observations : iterable of AutocallableObservation, optional
    ///     Product-specific knock-out and coupon observations, in time order.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
    ///
    /// Returns
    /// -------
    /// TernarySnowballOption
    ///     Independent contract of the same class, preserving omitted history and
    ///     terms.
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
    #[pyo3(signature = (*, maturity_coupon_rate, minimum_coupon_rate, knock_in_level, knock_in_observation, effective_time, expiry_time, autocall_observations, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, calendar))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        maturity_coupon_rate: &Bound<'_, PyAny>,
        minimum_coupon_rate: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        knock_in_observation: &KnockInObservation,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        autocall_observations: &Bound<'_, PyAny>,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
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

        Ok(Self(domain(kiyosi::TernarySnowballOption::new(terms))?))
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
/// autocall_observations : iterable of AutocallableObservation
///     Nonempty, strictly ordered knock-out and coupon observations within life.
/// lockout_periods : int, optional
///     Initial observations excluded from knock-out; defaults to zero and must
///     leave at least one eligible observation. Coupons and knock-in remain active.
/// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
///     Events strictly before valuation. None is unknown; no_event means no prior
///     knock-in or knock-out; knocked_in means an outstanding knocked-in note;
///     knocked_out means settled with no remaining value. The engine handles
///     observations at valuation using current spot. Callers supply applicable state.
/// principal_redemption_ratio : float, optional
///     Nonnegative normalized principal, independent of coupons and losses.
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
///     If amounts, strikes, life times or observations are invalid. Records
///     must be Phoenix observations with nonnegative coupon barriers;
///     snowball observations are rejected even when their coupon is zero.
///
/// Notes
/// -----
/// Periodic coupons pay once per qualifying observation, including at the
/// effective timestamp, without time accrual. None is not a coupon value.
/// Coupon and knock-out barriers are inclusive; knock-in is strict.
/// Past coupons are settled. There is no coupon memory or maturity coupon.
/// The initial lockout disables only knock-out; coupons and knock-in remain active.
///
#[pyclass(frozen, eq, module = "kiyosi._native")]
#[derive(Clone, PartialEq)]
pub(crate) struct PhoenixOption(pub kiyosi::PhoenixOption);
autocallable_properties! {
    PhoenixOption,
    @replace PhoenixOptionChanges [coupon_per_period, lockout_periods, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, barrier_state, principal_redemption_ratio, effective_time, expiry_time, autocall_observations, calendar]
    {
        lockout_periods => integer_usize,
        calendar => |v| v.extract::<TradingCalendar>().map(|v| v.0),
        coupon_per_period => crate::conversions::real,
        initial_spot => crate::conversions::real,
        knock_in_level => crate::conversions::real,
        upper_strike => crate::conversions::real,
        lower_strike => crate::conversions::real,
        knock_in_observation => |value| value.extract::<PyRef<'_, KnockInObservation>>().map(|value| value.0),
        barrier_state => crate::instruments::autocallable::state,
        principal_redemption_ratio => crate::conversions::real,
        effective_time => crate::conversions::timestamp,
        expiry_time => crate::conversions::timestamp,
        autocall_observations => crate::instruments::autocallable::records,
    }
    [
    /// Return a validated replacement.
    ///
    /// Omission or Ellipsis retains a field. The original value is unchanged.
    ///
    /// Parameters
    /// ----------
    /// coupon_per_period : float, optional
    ///     Finite signed periodic coupon rate; omission or Ellipsis retains it.
    /// lockout_periods : int, optional
    ///     Initial observations excluded from knock-out. Does not regenerate barriers.
    /// initial_spot, knock_in_level, upper_strike, lower_strike : float, optional
    ///     Reference spot, strict downside barrier, and loss interval strikes.
    /// knock_in_observation : KnockInObservation, optional
    ///     Daily or expiry-only knock-in monitoring rule.
    /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
    ///     Strictly prior event history; None clears it to unknown.
    /// principal_redemption_ratio : float, optional
    ///     Finite nonnegative principal redemption per unit of original notional.
    /// effective_time, expiry_time : datetime.datetime, optional
    ///     Inclusive contract start and expiry timestamps.
    /// autocall_observations : iterable of AutocallableObservation, optional
    ///     Product-specific knock-out and coupon observations, in time order.
    /// calendar : TradingCalendar, optional
    ///     Owned contractual observation calendar; omission retains it.
    ///
    /// Returns
    /// -------
    /// PhoenixOption
    ///     Independent contract of the same class, preserving omitted history and
    ///     terms.
    ///
    /// Raises
    /// ------
    /// KiyosiError
    ///     If the resulting contract fails core validation.
    /// TypeError
    ///     If a representation or field name is invalid, including a None coupon.
    ///
    ],
    /// Return the number of initial observations excluded from knock-out.
    #[getter]
    fn lockout_periods(&self) -> usize { self.0.lockout_periods() },

    #[new]
    #[pyo3(signature = (*, coupon_per_period, initial_spot, knock_in_level, upper_strike, lower_strike, knock_in_observation, effective_time, expiry_time, autocall_observations, lockout_periods=kiyosi::DEFAULT_PHOENIX_LOCKOUT_PERIODS, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, calendar))]
    #[expect(clippy::too_many_arguments, reason = "keyword-only contract terms")]
    fn new(
        #[pyo3(from_py_with = real)] coupon_per_period: f64,
        initial_spot: &Bound<'_, PyAny>,
        knock_in_level: &Bound<'_, PyAny>,
        upper_strike: &Bound<'_, PyAny>,
        lower_strike: &Bound<'_, PyAny>,
        knock_in_observation: &KnockInObservation,
        effective_time: &Bound<'_, PyAny>,
        expiry_time: &Bound<'_, PyAny>,
        autocall_observations: &Bound<'_, PyAny>,
        #[pyo3(from_py_with = integer_usize)] lockout_periods: usize,
        barrier_state: Option<&Bound<'_, PyAny>>,
        #[pyo3(from_py_with = real)] principal_redemption_ratio: f64,
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
            records(autocall_observations)?,
            knock_in_observation.0,
            calendar.0,
        );
        terms.lockout_periods = lockout_periods;
        if let Some(value) = barrier_state {
            terms.barrier_state = state(value)?;
        }
        terms.principal_redemption_ratio = principal_redemption_ratio;

        Ok(Self(domain(kiyosi::PhoenixOption::new(terms))?))
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
        #[pyo3(from_py_with = real)] coupon_per_period: f64,
    ) -> PyResult<Self> {
        Ok(Self(domain(self.0.with_coupon(coupon_per_period))?))
    },
    /// Return copied Phoenix coupon thresholds in observation order.
    #[getter]
    fn coupon_barrier_levels<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, self.0.autocall_observations().iter().map(|r| r.coupon_barrier_level().unwrap_or_default()))
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
    },
}

macro_rules! phoenix_preset {
    ($name:ident, $knock_out:ident, [$($clock:ident)?], [$($extra:ident => $extra_doc:literal),*], $preset:expr, $summary:literal, $convention:literal) => {
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
        ///     leave at least one eligible observation. Coupons and knock-in remain active.
        /// barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        ///     State strictly before valuation. None is unknown; no_event means no prior
        ///     knock-in or knock-out, knocked_in means outstanding after knock-in, and
        ///     knocked_out means settled. Current observations use current spot.
        /// principal_redemption_ratio : float, optional
        ///     Nonnegative normalized principal, independent of coupon and loss
        ///     components.
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
        #[pyfunction]
        #[pyo3(signature = (*, coupon_per_period, initial_spot, knock_in_level, $knock_out, $($extra,)* $($clock,)? autocall_times, effective_time, expiry_time, lockout_periods=kiyosi::DEFAULT_PHOENIX_LOCKOUT_PERIODS, barrier_state=None, principal_redemption_ratio=kiyosi::DEFAULT_PRINCIPAL_REDEMPTION_RATIO, calendar=crate::conversions::OMITTED))]
        #[expect(clippy::too_many_arguments, reason = "keyword-only preset terms")]
        pub(crate) fn $name(
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
            #[pyo3(from_py_with = crate::conversions::supplied)] calendar: Option<&Bound<'_, PyAny>>,

        ) -> PyResult<PhoenixOption> {
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
            terms.calendar = crate::calendar::observation_calendar(calendar, Some(autocall_times))?.unwrap_or_default();

            Ok(PhoenixOption(domain(terms.expand($preset))?))
        }
    };
}
phoenix_preset!(
    fixed_coupon_note,
    knock_out_level,
    [],
    [],
    kiyosi::PhoenixPreset::FixedCouponNote,
    "Create a fixed coupon note with unconditional surviving coupons.",
    "Validated owned contract with expiry-only knock-in, knock-in level as upper strike and zero lower strike."
);
phoenix_preset!(
    digital_coupon_note,
    knock_out_level,
    [],
    [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on knock-out times."],
    kiyosi::PhoenixPreset::DigitalCouponNote { coupon_barrier_level },
    "Create a digital coupon note with barrier-conditional fixed coupons.",
    "Validated owned contract with expiry-only knock-in, knock-in level as upper strike and zero lower strike."
);

phoenix_preset!(
    standard_phoenix,
    knock_out_level,
    [knock_in_observation_clock_time],
    [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on knock-out times."],
    kiyosi::PhoenixPreset::Standard { coupon_barrier_level },
    "Create a standard Phoenix with a constant knock-out barrier and optional initial lockout.",
    "Validated owned contract with daily knock-in, initial spot as upper strike and zero lower strike."
);
phoenix_preset!(
    step_down_phoenix,
    initial_knock_out_level,
    [knock_in_observation_clock_time],
    [coupon_barrier_level => "Finite nonnegative inclusive coupon threshold, also tested on knock-out times.",
     knock_out_level_decrement => "Finite nonnegative absolute decrement after each eligible observation; lockout does not consume ladder steps."],
    kiyosi::PhoenixPreset::StepDown { coupon_barrier_level, knock_out_level_decrement },
    "Create a step-down Phoenix whose first eligible knock-out uses the initial level.",
    "Validated owned contract with daily knock-in, initial spot as upper strike and zero lower strike."
);

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
