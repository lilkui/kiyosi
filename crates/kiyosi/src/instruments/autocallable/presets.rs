use super::{
    AutocallableBarrierState, AutocallableObservation, DEFAULT_PRINCIPAL_RATIO,
    KnockInObservationMode, PhoenixCoupon, PhoenixOption, PhoenixTerms, SnowballOption,
    SnowballTerms,
};
use crate::error::finite;
use crate::{Date, Error, Result};

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
        /// Sets the signed annual maturity coupon.
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
        first_stage_end_date: Date,
        /// Sets the finite signed second-stage and maturity annual coupon.
        second_coupon_rate: f64,
    },
    /// Uses three annual coupon stages; see [`SnowballPresetTerms::expand`].
    Butterfly {
        /// Sets the inclusive end of the first stage, within life and before expiry.
        first_stage_end_date: Date,
        /// Sets the finite signed second-stage annual coupon.
        second_coupon_rate: f64,
        /// Sets the inclusive end of the second stage, after the first and before expiry.
        second_stage_end_date: Date,
        /// Sets the finite signed third-stage and maturity annual coupon.
        third_coupon_rate: f64,
    },
}

/// Supplies common market terms for the named snowball presets.
///
/// See [`Self::expand`] for conventions, validation and an example.
#[derive(Clone, Debug, PartialEq)]
pub struct SnowballPresetTerms {
    /// Sets the annual coupon, or the first coupon for a ladder or staged preset.
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
    /// Sets the inclusive first date.
    pub effective_date: Date,
    /// Sets expiry at midnight UTC.
    pub expiry_date: Date,
    /// Supplies nonempty, strictly increasing observation dates within life.
    pub observation_dates: Vec<Date>,
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
        effective_date: Date,
        expiry_date: Date,
        observation_dates: Vec<Date>,
    ) -> Self {
        Self {
            coupon_rate,
            initial_spot,
            knock_in_level,
            knock_out_level,
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_date,
            expiry_date,
            observation_dates,
        }
    }
    /// Expands a named convention into an ordinary validated [`SnowballOption`].
    ///
    /// All presets use zero lower strike, initial spot as upper strike and daily
    /// knock-in monitoring unless overridden by the selected convention.
    /// Signed coupons and decrements are accepted when every expanded term is valid.
    /// The returned note owns its terms and uses the usual history and replacement rules.
    /// Early-profit and butterfly presets select the coupon by observation date,
    /// with each stage end included in that stage. Cutoffs need not be observation
    /// or trading dates; stages may have no observations. Knock-out uses the chosen
    /// annual rate over the entire holding period; maturity uses the last stage rate.
    /// All stage rates must be finite, but need not be positive or decreasing.
    ///
    /// # Errors
    ///
    /// Propagates [`SnowballOption::new`] errors for invalid expanded terms,
    /// schedules or life dates. Returns [`Error::InvalidInput`] for non-finite stage
    /// rates or unordered cutoffs outside `[effective_date, expiry_date)`.
    /// Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, SnowballPreset, SnowballPresetTerms};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let end = Date::new(2026, 1, 1)?;
    /// let terms = SnowballPresetTerms::new(0.08, 100.0, 80.0, 110.0,
    ///     start, end, vec![end]);
    /// let note = terms.expand(SnowballPreset::LossCapped { lower_strike: 60.0 })?;
    /// assert_eq!(note.terms().lower_strike, 60.0);
    /// let terms = SnowballPresetTerms::new(0.20, 100.0, 80.0, 110.0,
    ///     start, end, vec![end]);
    /// let note = terms.expand(SnowballPreset::EarlyProfit {
    ///     first_stage_end_date: Date::new(2025, 7, 1)?, second_coupon_rate: 0.05,
    /// })?;
    /// assert_eq!(note.observations()[0].coupon_rate, 0.05);
    /// # Ok(()) }
    /// ```
    pub fn expand(self, preset: SnowballPreset) -> Result<SnowballOption> {
        let observations = self
            .observation_dates
            .into_iter()
            .map(|date| {
                let mut record = AutocallableObservation::new(date, self.knock_out_level);
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
            self.effective_date,
            self.expiry_date,
            observations,
        );
        terms.barrier_state = self.barrier_state;
        terms.principal_ratio = self.principal_ratio;
        match preset {
            SnowballPreset::Standard => {}
            SnowballPreset::European => {
                terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry
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
                first_stage_end_date,
                second_coupon_rate,
            } => staged_coupons(
                &mut terms,
                &[(first_stage_end_date, self.coupon_rate)],
                second_coupon_rate,
            )?,
            SnowballPreset::Butterfly {
                first_stage_end_date,
                second_coupon_rate,
                second_stage_end_date,
                third_coupon_rate,
            } => staged_coupons(
                &mut terms,
                &[
                    (first_stage_end_date, self.coupon_rate),
                    (second_stage_end_date, second_coupon_rate),
                ],
                third_coupon_rate,
            )?,
        }
        SnowballOption::new(terms)
    }
}

fn staged_coupons(
    terms: &mut SnowballTerms,
    stages: &[(Date, f64)],
    final_rate: f64,
) -> Result<()> {
    finite(final_rate, false)?;
    for (i, &(date, rate)) in stages.iter().enumerate() {
        finite(rate, false)?;
        if date < terms.effective_date
            || date >= terms.expiry_date
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
            .find(|(end, _)| record.date <= *end)
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

/// Supplies FCN/DCN inputs; see [`Self::expand`] for conventions and an example.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoenixPresetTerms {
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
    /// Sets the inclusive first date.
    pub effective_date: Date,
    /// Sets expiry at midnight UTC.
    pub expiry_date: Date,
    /// Supplies nonempty, strictly increasing observation dates within life.
    pub observation_dates: Vec<Date>,
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
        effective_date: Date,
        expiry_date: Date,
        observation_dates: Vec<Date>,
    ) -> Self {
        Self {
            coupon_per_period,
            initial_spot,
            knock_in_level,
            knock_out_level,
            barrier_state: None,
            principal_ratio: DEFAULT_PRINCIPAL_RATIO,
            effective_date,
            expiry_date,
            observation_dates,
        }
    }

    /// Expands FCN/DCN terms into an ordinary validated [`PhoenixOption`].
    ///
    /// Observes strict knock-in only at expiry, with the knock-in level as upper
    /// loss strike and zero lower strike. Coupons are fixed per observation,
    /// including a qualifying knock-out observation, with no memory. Coupon and
    /// principal amounts are normalized to initial notional; downside is the
    /// capped price difference divided by initial spot. Observation dates are
    /// payment dates, with no implicit observation added at expiry.
    ///
    /// # Errors
    ///
    /// Propagates [`PhoenixOption::new`] errors for invalid numeric terms, dates
    /// or schedules. Inputs are consumed on failure.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::{Date, PhoenixCoupon, PhoenixPreset, PhoenixPresetTerms};
    /// # fn main() -> kiyosi::Result<()> {
    /// let start = Date::new(2025, 1, 1)?;
    /// let end = Date::new(2026, 1, 1)?;
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
            .observation_dates
            .into_iter()
            .map(|date| {
                let mut record = AutocallableObservation::new(date, self.knock_out_level);
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
            self.effective_date,
            self.expiry_date,
            observations,
        );
        terms.coupon = PhoenixCoupon::PerPeriod(self.coupon_per_period);
        terms.knock_in_observation_mode = KnockInObservationMode::AtExpiry;
        terms.barrier_state = self.barrier_state;
        terms.principal_ratio = self.principal_ratio;
        PhoenixOption::new(terms)
    }
}
