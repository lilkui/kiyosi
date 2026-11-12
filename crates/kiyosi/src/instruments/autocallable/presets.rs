use super::{
    AutocallableBarrierState, AutocallableObservation, AutocallableObservationKind,
    DEFAULT_PHOENIX_LOCKOUT_PERIODS, DEFAULT_PRINCIPAL_REDEMPTION_RATIO, KnockInObservation,
    PhoenixOption, PhoenixTerms, SnowballOption, SnowballTerms,
};
use crate::TradingCalendar;
use crate::error::finite;
use crate::{Error, Result};
use crate::{TimeOfDay, Timestamp};

/// Selects a named convention expanded by [`SnowballPresetTerms::expand`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnowballPreset {
    /// Uses constant coupons and knock-out levels, with daily knock-in monitoring.
    Standard,
    /// Uses constant coupons and levels, observing knock-in only at expiry.
    European,
    /// Decreases knock-out levels by an absolute underlying-price amount per observation.
    StepDown {
        /// Sets the decrement after each observation; the first uses the supplied level.
        knock_out_level_decrement: f64,
    },
    /// Decreases coupons and knock-out levels; maturity uses the final observation coupon.
    BothDown {
        /// Sets the absolute decimal-rate decrement (0.01 is one percentage point).
        coupon_rate_decrement: f64,
        /// Sets the absolute underlying-price decrement.
        knock_out_level_decrement: f64,
    },
    /// Uses a separate maturity coupon and constant knock-out coupons.
    DualCoupon {
        /// Sets the signed annual maturity coupon rate.
        maturity_coupon_rate: f64,
    },
    /// Replaces only the final observation's knock-out level.
    Parachute {
        /// Sets the positive final knock-out threshold.
        final_knock_out_level: f64,
    },
    /// Replaces the upper terminal strike.
    Otm {
        /// Sets the positive upper terminal strike.
        upper_strike: f64,
    },
    /// Caps downside participation at a lower terminal strike.
    LossCapped {
        /// Sets the nonnegative lower strike, at most the initial spot.
        lower_strike: f64,
    },
    /// Uses two annual coupon stages; see [`SnowballPresetTerms::expand`].
    EarlyProfit {
        /// Sets the inclusive end of the first stage, within life and before expiry.
        first_stage_end_time: Timestamp,
        /// Sets the finite signed annual coupon rate for the second stage and maturity.
        second_coupon_rate: f64,
    },
    /// Uses three annual coupon stages; see [`SnowballPresetTerms::expand`].
    Butterfly {
        /// Sets the inclusive end of the first stage, within life and before expiry.
        first_stage_end_time: Timestamp,
        /// Sets the finite signed annual coupon rate for the second stage.
        second_coupon_rate: f64,
        /// Sets the inclusive end of the second stage, after the first and before expiry.
        second_stage_end_time: Timestamp,
        /// Sets the finite signed annual coupon rate for the third stage and maturity.
        third_coupon_rate: f64,
    },
}

/// Supplies common market terms for the named snowball presets.
///
/// See [`Self::expand`] for conventions, validation and an example.
#[derive(Clone, Debug, PartialEq)]
pub struct SnowballPresetTerms {
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Sets the annual coupon rate, or the first rate for a ladder or staged preset.
    pub coupon_rate: f64,
    /// Sets the positive reference spot and default upper strike.
    pub initial_spot: f64,
    /// Sets the positive strict knock-in threshold.
    pub knock_in_level: f64,
    /// Sets the positive knock-out level, or the initial level for a decreasing ladder.
    pub knock_out_level: f64,
    /// Records prior history; defaults to unknown.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets nonnegative normalized principal, independently of coupon and loss components.
    pub principal_redemption_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly increasing observation times within life.
    pub autocall_times: Vec<Timestamp>,
    /// Supplies the daily observation clock, absent for European monitoring; see [`TimeOfDay::new`].
    pub knock_in_observation_clock_time: Option<TimeOfDay>,
}
impl SnowballPresetTerms {
    /// Creates input terms with the core snowball history and principal defaults.
    ///
    /// Validation occurs on [`Self::expand`]; see its example.
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit contractual input terms"
    )]
    pub fn new(
        coupon_rate: f64,
        initial_spot: f64,
        knock_in_level: f64,
        knock_out_level: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        autocall_times: Vec<Timestamp>,
        knock_in_observation_clock_time: Option<TimeOfDay>,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            coupon_rate,
            initial_spot,
            knock_in_level,
            knock_out_level,
            barrier_state: None,
            principal_redemption_ratio: DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
            effective_time,
            expiry_time,
            autocall_times,
            knock_in_observation_clock_time,
        }
    }
    /// Expands a named convention into an ordinary validated [`SnowballOption`].
    ///
    /// All presets use zero lower strike, initial spot as upper strike and daily
    /// knock-in monitoring unless overridden by the selected convention.
    /// Signed coupons and decrements are accepted when every expanded term is valid.
    /// The returned note owns its terms and uses the usual history and replacement rules.
    /// Observation and maturity coupons are separate terms. Edit the first observation
    /// coupon with [`SnowballOption::with_coupon`] to preserve observation spreads
    /// and explicitly choose whether the maturity coupon shifts with them.
    /// Early-profit and butterfly presets select the coupon by observation time,
    /// with each stage end included in that stage. Cutoffs need not be observation
    /// or trading dates; stages may have no observations. Knock-out uses the chosen
    /// annual rate over the entire holding period; maturity uses the last stage rate.
    /// All stage rates must be finite, but need not be positive or decreasing.
    ///
    /// # Errors
    ///
    /// Propagates [`SnowballOption::new`] errors for invalid expanded terms,
    /// schedules or life times. Returns [`Error::InvalidInput`] for a missing daily
    /// knock-in clock, a clock on the European preset, non-finite stage rates or
    /// unordered cutoffs outside `[effective_time, expiry_time)`.
    /// Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, SnowballPreset, SnowballPresetTerms};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let terms = SnowballPresetTerms::new(0.08, 100.0, 80.0, 110.0,
    ///     start, end, vec![end], Some(kiyosi::TimeOfDay::new(0, 0, 0, 0)?));
    /// let note = terms.expand(SnowballPreset::LossCapped { lower_strike: 60.0 })?;
    /// assert_eq!(note.terms().lower_strike, 60.0);
    /// let edited = note.with_coupon(0.15, kiyosi::CouponQuoteConvention::ShiftMaturityCoupon)?;
    /// assert!((edited.terms().maturity_coupon_rate - 0.15).abs() < 1e-14);
    /// assert!(edited.autocall_observations()[0].coupon_rate().is_some_and(|coupon| (coupon - 0.15).abs() < 1e-14));
    /// assert_eq!(note.terms().maturity_coupon_rate, 0.08);
    /// let terms = SnowballPresetTerms::new(0.20, 100.0, 80.0, 110.0,
    ///     start, end, vec![end], Some(kiyosi::TimeOfDay::new(0, 0, 0, 0)?));
    /// let note = terms.expand(SnowballPreset::EarlyProfit {
    ///     first_stage_end_time: Date::new(2025, 7, 1)?.start_of_day(), second_coupon_rate: 0.05,
    /// })?;
    /// assert_eq!(note.autocall_observations()[0].coupon_rate(), Some(0.05));
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::cast_precision_loss,
        reason = "Observation indices are bounded by the supported calendar range and fit exactly in f64."
    )]
    pub fn expand(self, preset: SnowballPreset) -> Result<SnowballOption> {
        let knock_in_observation = match (preset, self.knock_in_observation_clock_time) {
            (SnowballPreset::European, None) => KnockInObservation::AtExpiry,
            (SnowballPreset::European, Some(_)) | (_, None) => {
                return Err(Error::InvalidInput(
                    "knock_in_observation_clock_time is required only for daily knock-in observation".into(),
                ));
            }
            (_, Some(observation_clock_time)) => KnockInObservation::EveryTradingDay {
                observation_clock_time,
            },
        };
        let observations = self
            .autocall_times
            .into_iter()
            .map(|date| AutocallableObservation {
                kind: AutocallableObservationKind::Snowball,
                time: date,
                knock_out_level: self.knock_out_level,
                coupon_rate: self.coupon_rate,
                coupon_barrier_level: 0.0,
            })
            .collect();
        let mut terms = SnowballTerms::new(
            self.coupon_rate,
            self.initial_spot,
            self.knock_in_level,
            self.initial_spot,
            0.0,
            self.effective_time,
            self.expiry_time,
            observations,
            knock_in_observation,
            self.calendar,
        );
        terms.barrier_state = self.barrier_state;
        terms.principal_redemption_ratio = self.principal_redemption_ratio;
        match preset {
            SnowballPreset::Standard | SnowballPreset::European => {}
            SnowballPreset::StepDown {
                knock_out_level_decrement,
            } => {
                for (i, r) in terms.autocall_observations.iter_mut().enumerate() {
                    r.knock_out_level -= i as f64 * knock_out_level_decrement;
                }
            }
            SnowballPreset::BothDown {
                coupon_rate_decrement,
                knock_out_level_decrement,
            } => {
                for (i, r) in terms.autocall_observations.iter_mut().enumerate() {
                    r.knock_out_level -= i as f64 * knock_out_level_decrement;
                    r.coupon_rate -= i as f64 * coupon_rate_decrement;
                }
                terms.maturity_coupon_rate = terms
                    .autocall_observations
                    .last()
                    .map_or(0.0, |r| r.coupon_rate);
            }
            SnowballPreset::DualCoupon {
                maturity_coupon_rate,
            } => terms.maturity_coupon_rate = maturity_coupon_rate,
            SnowballPreset::Parachute {
                final_knock_out_level,
            } => {
                if let Some(last) = terms.autocall_observations.last_mut() {
                    last.knock_out_level = final_knock_out_level;
                }
            }
            SnowballPreset::Otm { upper_strike } => terms.upper_strike = upper_strike,
            SnowballPreset::LossCapped { lower_strike } => terms.lower_strike = lower_strike,
            SnowballPreset::EarlyProfit {
                first_stage_end_time,
                second_coupon_rate,
            } => staged_coupons(
                &mut terms,
                &[(first_stage_end_time, self.coupon_rate)],
                second_coupon_rate,
            )?,
            SnowballPreset::Butterfly {
                first_stage_end_time,
                second_coupon_rate,
                second_stage_end_time,
                third_coupon_rate,
            } => staged_coupons(
                &mut terms,
                &[
                    (first_stage_end_time, self.coupon_rate),
                    (second_stage_end_time, second_coupon_rate),
                ],
                third_coupon_rate,
            )?,
        }
        SnowballOption::new(terms)
    }
}

fn staged_coupons(
    terms: &mut SnowballTerms,
    stages: &[(Timestamp, f64)],
    final_rate: f64,
) -> Result<()> {
    finite(final_rate, false, "final_coupon_rate")?;
    for (i, &(date, rate)) in stages.iter().enumerate() {
        finite(rate, false, "coupon_rate")?;
        if date < terms.effective_time
            || date >= terms.expiry_time
            || i > 0 && date <= stages[i - 1].0
        {
            return Err(Error::InvalidInput(
                "stage cutoffs must be ordered within life and before expiry".into(),
            ));
        }
    }
    for record in &mut terms.autocall_observations {
        record.coupon_rate = stages
            .iter()
            .find(|(end, _)| record.time <= *end)
            .map_or(final_rate, |(_, rate)| *rate);
    }
    terms.maturity_coupon_rate = final_rate;
    Ok(())
}

/// Selects a Phoenix convention; see [`PhoenixPresetTerms::expand`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhoenixPreset {
    /// Uses a constant knock-out level and daily knock-in monitoring.
    Standard {
        /// Sets the finite nonnegative inclusive coupon threshold.
        coupon_barrier_level: f64,
    },
    /// Decreases knock-out levels from the first eligible observation, with daily knock-in.
    StepDown {
        /// Sets the finite nonnegative inclusive coupon threshold.
        coupon_barrier_level: f64,
        /// Sets the finite nonnegative absolute decrement per eligible observation.
        knock_out_level_decrement: f64,
    },
    /// Pays a fixed coupon unconditionally at each surviving observation.
    FixedCouponNote,
    /// Pays a fixed coupon only when the inclusive coupon barrier is met.
    DigitalCouponNote {
        /// Sets the finite nonnegative coupon threshold for every observation.
        coupon_barrier_level: f64,
    },
}

/// Supplies Phoenix preset inputs; see [`Self::expand`] for conventions and examples.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoenixPresetTerms {
    /// Disables knock-out for initial observations; defaults to [`DEFAULT_PHOENIX_LOCKOUT_PERIODS`].
    pub lockout_periods: usize,
    /// Supplies the daily clock for standard and step-down presets; absent for FCN/DCN.
    pub knock_in_observation_clock_time: Option<TimeOfDay>,
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Sets the finite signed proportion paid per qualifying observation, without day accrual.
    pub coupon_per_period: f64,
    /// Sets the positive reference spot used to normalize downside losses.
    pub initial_spot: f64,
    /// Sets the positive strict knock-in threshold; also the FCN/DCN upper settlement strike.
    pub knock_in_level: f64,
    /// Sets the positive inclusive knock-out threshold.
    pub knock_out_level: f64,
    /// Records prior events; defaults to unknown.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets nonnegative normalized principal, independently of coupon and loss components.
    pub principal_redemption_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly increasing observation times within life.
    pub autocall_times: Vec<Timestamp>,
}
impl PhoenixPresetTerms {
    /// Creates input terms with no lockout, no daily clock, unknown history and unit principal.
    ///
    /// Validation occurs on [`Self::expand`]; see its example.
    pub fn new(
        coupon_per_period: f64,
        initial_spot: f64,
        knock_in_level: f64,
        knock_out_level: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        autocall_times: Vec<Timestamp>,
    ) -> Self {
        Self {
            lockout_periods: DEFAULT_PHOENIX_LOCKOUT_PERIODS,
            knock_in_observation_clock_time: None,
            calendar: TradingCalendar::default(),
            coupon_per_period,
            initial_spot,
            knock_in_level,
            knock_out_level,
            barrier_state: None,
            principal_redemption_ratio: DEFAULT_PRINCIPAL_REDEMPTION_RATIO,
            effective_time,
            expiry_time,
            autocall_times,
        }
    }

    /// Expands preset terms into an ordinary validated [`PhoenixOption`].
    ///
    /// Standard and step-down presets observe strict daily knock-in at the supplied
    /// clock and use initial spot as the upper loss strike. FCN/DCN observe only at
    /// expiry and use the knock-in level as upper strike. All use zero lower strike.
    /// The lockout disables only knock-out; coupons and knock-in remain active.
    /// Step-down starts at the supplied level on the first eligible observation,
    /// then subtracts the nonnegative absolute decrement per eligible observation.
    /// Coupons are fixed per observation,
    /// including a qualifying knock-out observation, with no memory. Coupon and
    /// principal amounts are normalized to initial notional; downside is the
    /// capped price difference divided by initial spot. Observation times are
    /// payment times, with no implicit observation added at expiry.
    ///
    /// # Errors
    ///
    /// Propagates [`PhoenixOption::new`] errors for invalid numeric terms, times
    /// or schedules, lockout covering every observation, a missing daily clock or
    /// a daily clock on FCN/DCN. Rejects negative or non-finite step-down decrements.
    /// Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, PhoenixPreset, PhoenixPresetTerms};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let terms = PhoenixPresetTerms::new(0.01, 100.0, 80.0, 110.0,
    ///     start, end, vec![end]);
    /// let note = terms.expand(PhoenixPreset::FixedCouponNote)?;
    /// assert_eq!(note.terms().coupon_per_period, 0.01);
    /// let mut terms = PhoenixPresetTerms::new(0.01, 100.0, 80.0, 110.0,
    ///     start, end, vec![start.add_days(1)?, end]);
    /// terms.lockout_periods = 1;
    /// terms.knock_in_observation_clock_time = Some(start.time_of_day());
    /// let note = terms.expand(PhoenixPreset::StepDown {
    ///     coupon_barrier_level: 90.0, knock_out_level_decrement: 2.0,
    /// })?;
    /// assert_eq!(note.lockout_periods(), 1);
    /// assert_eq!(note.autocall_observations()[1].knock_out_level(), 110.0);
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::cast_precision_loss,
        reason = "Observation indices are bounded by the supported calendar range and fit exactly in f64."
    )]
    pub fn expand(self, preset: PhoenixPreset) -> Result<PhoenixOption> {
        let (coupon_barrier, decrement, daily) = match preset {
            PhoenixPreset::Standard {
                coupon_barrier_level,
            } => (coupon_barrier_level, 0.0, true),
            PhoenixPreset::StepDown {
                coupon_barrier_level,
                knock_out_level_decrement,
            } => {
                finite(
                    knock_out_level_decrement,
                    false,
                    "knock_out_level_decrement",
                )?;
                if knock_out_level_decrement < 0.0 {
                    return Err(Error::InvalidInput(
                        "negative knock_out_level_decrement".into(),
                    ));
                }
                (coupon_barrier_level, knock_out_level_decrement, true)
            }
            PhoenixPreset::FixedCouponNote => (0.0, 0.0, false),
            PhoenixPreset::DigitalCouponNote {
                coupon_barrier_level,
            } => (coupon_barrier_level, 0.0, false),
        };
        let knock_in_observation = match (daily, self.knock_in_observation_clock_time) {
            (false, None) => KnockInObservation::AtExpiry,
            (true, Some(observation_clock_time)) => KnockInObservation::EveryTradingDay {
                observation_clock_time,
            },
            _ => return Err(Error::InvalidInput(
                "knock_in_observation_clock_time is required only for daily knock-in observation"
                    .into(),
            )),
        };
        let observations = self
            .autocall_times
            .into_iter()
            .enumerate()
            .map(|(i, date)| AutocallableObservation {
                time: date,
                knock_out_level: self.knock_out_level
                    - i.saturating_sub(self.lockout_periods) as f64 * decrement,
                coupon_rate: 0.0,
                coupon_barrier_level: coupon_barrier,
                kind: AutocallableObservationKind::Phoenix,
            })
            .collect();
        let mut terms = PhoenixTerms::new(
            self.coupon_per_period,
            self.initial_spot,
            self.knock_in_level,
            if daily {
                self.initial_spot
            } else {
                self.knock_in_level
            },
            0.0,
            self.effective_time,
            self.expiry_time,
            observations,
            knock_in_observation,
            self.calendar,
        );
        terms.lockout_periods = self.lockout_periods;
        terms.barrier_state = self.barrier_state;
        terms.principal_redemption_ratio = self.principal_redemption_ratio;
        PhoenixOption::new(terms)
    }
}
