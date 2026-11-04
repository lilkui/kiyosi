use super::{
    AutocallableBarrierState, AutocallableObservation, DEFAULT_PRINCIPAL_RATIO,
    KnockInObservationMode, PhoenixCoupon, PhoenixOption, PhoenixTerms, SnowballOption,
    SnowballTerms,
};
use crate::Timestamp;
use crate::TradingCalendar;
use crate::error::finite;
use crate::{Error, Result};

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
    pub principal_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly increasing observation times within life.
    pub observation_times: Vec<Timestamp>,
}
impl SnowballPresetTerms {
    /// Creates input terms with the core snowball history and principal defaults.
    ///
    /// Validation occurs on [`Self::expand`]; see its example.
    pub fn new(
        coupon_rate: f64,
        initial_spot: f64,
        knock_in_level: f64,
        knock_out_level: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        observation_times: Vec<Timestamp>,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            coupon_rate,
            initial_spot,
            knock_in_level,
            knock_out_level,
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_time,
            expiry_time,
            observation_times,
        }
    }
    /// Expands a named convention into an ordinary validated [`SnowballOption`].
    ///
    /// All presets use zero lower strike, initial spot as upper strike and daily
    /// knock-in monitoring unless overridden by the selected convention.
    /// Signed coupons and decrements are accepted when every expanded term is valid.
    /// The returned note owns its terms and uses the usual history and replacement rules.
    /// Early-profit and butterfly presets select the coupon by observation time,
    /// with each stage end included in that stage. Cutoffs need not be observation
    /// or trading dates; stages may have no observations. Knock-out uses the chosen
    /// annual rate over the entire holding period; maturity uses the last stage rate.
    /// All stage rates must be finite, but need not be positive or decreasing.
    ///
    /// # Errors
    ///
    /// Propagates [`SnowballOption::new`] errors for invalid expanded terms,
    /// schedules or life times. Returns [`Error::InvalidInput`] for non-finite stage
    /// rates or unordered cutoffs outside `[effective_time, expiry_time)`.
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
    ///     start, end, vec![end]);
    /// let note = terms.expand(SnowballPreset::LossCapped { lower_strike: 60.0 })?;
    /// assert_eq!(note.terms().lower_strike, 60.0);
    /// let terms = SnowballPresetTerms::new(0.20, 100.0, 80.0, 110.0,
    ///     start, end, vec![end]);
    /// let note = terms.expand(SnowballPreset::EarlyProfit {
    ///     first_stage_end_time: Date::new(2025, 7, 1)?.start_of_day(), second_coupon_rate: 0.05,
    /// })?;
    /// assert_eq!(note.observations()[0].coupon_rate, 0.05);
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::cast_precision_loss,
        reason = "Observation indices are bounded by the supported calendar range and fit exactly in f64."
    )]
    pub fn expand(self, preset: SnowballPreset) -> Result<SnowballOption> {
        let observations = self
            .observation_times
            .into_iter()
            .map(|date| {
                let mut record = AutocallableObservation {
                    time: date,
                    knock_out_level: self.knock_out_level,
                    coupon_rate: AutocallableObservation::DEFAULT_COUPON_RATE,
                    coupon_barrier_level: AutocallableObservation::DEFAULT_COUPON_BARRIER_LEVEL,
                };
                record.coupon_rate = self.coupon_rate;
                record
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
        );
        terms.barrier_state = self.barrier_state;
        terms.calendar = self.calendar;
        terms.principal_ratio = self.principal_ratio;
        match preset {
            SnowballPreset::Standard => {}
            SnowballPreset::European => {
                terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
            }
            SnowballPreset::StepDown {
                knock_out_level_decrement,
            } => {
                for (i, r) in terms.observations.iter_mut().enumerate() {
                    r.knock_out_level -= i as f64 * knock_out_level_decrement;
                }
            }
            SnowballPreset::BothDown {
                coupon_rate_decrement,
                knock_out_level_decrement,
            } => {
                for (i, r) in terms.observations.iter_mut().enumerate() {
                    r.knock_out_level -= i as f64 * knock_out_level_decrement;
                    r.coupon_rate -= i as f64 * coupon_rate_decrement;
                }
                terms.maturity_coupon_rate =
                    terms.observations.last().map_or(0.0, |r| r.coupon_rate);
            }
            SnowballPreset::DualCoupon {
                maturity_coupon_rate,
            } => terms.maturity_coupon_rate = maturity_coupon_rate,
            SnowballPreset::Parachute {
                final_knock_out_level,
            } => {
                if let Some(last) = terms.observations.last_mut() {
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
    for record in &mut terms.observations {
        record.coupon_rate = stages
            .iter()
            .find(|(end, _)| record.time <= *end)
            .map_or(final_rate, |(_, rate)| *rate);
    }
    terms.maturity_coupon_rate = final_rate;
    Ok(())
}

/// Selects a fixed-payment Phoenix convention; see [`PhoenixPresetTerms::expand`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhoenixPreset {
    /// Pays a fixed coupon unconditionally at each surviving observation.
    FixedCouponNote,
    /// Pays a fixed coupon only when the inclusive coupon barrier is met.
    DigitalCouponNote {
        /// Sets the finite nonnegative coupon threshold for every observation.
        coupon_barrier_level: f64,
    },
}

/// Supplies fixed coupon note (FCN) and digital coupon note (DCN) inputs; see [`Self::expand`] for conventions and an example.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoenixPresetTerms {
    /// Owns the contractual observation calendar; defaults to weekdays.
    pub calendar: TradingCalendar,
    /// Sets the finite signed proportion paid per qualifying observation, without day accrual.
    pub coupon_per_period: f64,
    /// Sets the positive reference spot used to normalize downside losses.
    pub initial_spot: f64,
    /// Sets the positive strict expiry knock-in threshold and upper settlement strike.
    pub knock_in_level: f64,
    /// Sets the positive inclusive knock-out threshold.
    pub knock_out_level: f64,
    /// Records prior events; defaults to unknown.
    pub barrier_state: Option<AutocallableBarrierState>,
    /// Sets nonnegative normalized principal, independently of coupon and loss components.
    pub principal_ratio: f64,
    /// Sets the inclusive effective time.
    pub effective_time: Timestamp,
    /// Sets the inclusive expiry timestamp.
    pub expiry_time: Timestamp,
    /// Supplies nonempty, strictly increasing observation times within life.
    pub observation_times: Vec<Timestamp>,
}
impl PhoenixPresetTerms {
    /// Creates FCN/DCN input terms with unknown history and unit principal.
    ///
    /// Validation occurs on [`Self::expand`]; see its example.
    pub fn new(
        coupon_per_period: f64,
        initial_spot: f64,
        knock_in_level: f64,
        knock_out_level: f64,
        effective_time: Timestamp,
        expiry_time: Timestamp,
        observation_times: Vec<Timestamp>,
    ) -> Self {
        Self {
            calendar: TradingCalendar::default(),
            coupon_per_period,
            initial_spot,
            knock_in_level,
            knock_out_level,
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_time,
            expiry_time,
            observation_times,
        }
    }

    /// Expands FCN/DCN terms into an ordinary validated [`PhoenixOption`].
    ///
    /// Observes strict knock-in only at expiry, with the knock-in level as upper
    /// loss strike and zero lower strike. Coupons are fixed per observation,
    /// including a qualifying knock-out observation, with no memory. Coupon and
    /// principal amounts are normalized to initial notional; downside is the
    /// capped price difference divided by initial spot. Observation times are
    /// payment times, with no implicit observation added at expiry.
    ///
    /// # Errors
    ///
    /// Propagates [`PhoenixOption::new`] errors for invalid numeric terms, times
    /// or schedules. Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, PhoenixCoupon, PhoenixPreset, PhoenixPresetTerms};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let terms = PhoenixPresetTerms::new(0.01, 100.0, 80.0, 110.0,
    ///     start, end, vec![end]);
    /// let note = terms.expand(PhoenixPreset::FixedCouponNote)?;
    /// assert_eq!(note.terms().coupon, PhoenixCoupon::PerPeriod(0.01));
    /// # Ok(()) }
    /// ```
    pub fn expand(self, preset: PhoenixPreset) -> Result<PhoenixOption> {
        let coupon_barrier = match preset {
            PhoenixPreset::FixedCouponNote => AutocallableObservation::DEFAULT_COUPON_BARRIER_LEVEL,
            PhoenixPreset::DigitalCouponNote {
                coupon_barrier_level,
            } => coupon_barrier_level,
        };
        let observations = self
            .observation_times
            .into_iter()
            .map(|date| {
                let mut record = AutocallableObservation {
                    time: date,
                    knock_out_level: self.knock_out_level,
                    coupon_rate: AutocallableObservation::DEFAULT_COUPON_RATE,
                    coupon_barrier_level: AutocallableObservation::DEFAULT_COUPON_BARRIER_LEVEL,
                };
                record.coupon_barrier_level = coupon_barrier;
                record
            })
            .collect();
        let mut terms = PhoenixTerms::new(
            0.0,
            self.initial_spot,
            self.knock_in_level,
            self.knock_in_level,
            0.0,
            self.effective_time,
            self.expiry_time,
            observations,
        );
        terms.coupon = PhoenixCoupon::PerPeriod(self.coupon_per_period);
        terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
        terms.barrier_state = self.barrier_state;
        terms.calendar = self.calendar;
        terms.principal_ratio = self.principal_ratio;
        PhoenixOption::new(terms)
    }
}
