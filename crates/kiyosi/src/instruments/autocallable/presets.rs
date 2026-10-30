use super::{
    AutocallableBarrierState, AutocallableObservation, KnockInObservationMode, SnowballOption,
    SnowballTerms,
};
use crate::{Date, Result};

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
}

/// Supplies common market terms for the named snowball presets.
///
/// See [`Self::expand`] for conventions, validation and an example.
#[derive(Clone, Debug, PartialEq)]
pub struct SnowballPresetTerms {
    /// Sets the annual coupon, or the initial coupon for a decreasing ladder.
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
        let defaults = SnowballTerms::new(
            coupon_rate,
            initial_spot,
            knock_in_level,
            initial_spot,
            0.0,
            effective_date,
            expiry_date,
            Vec::new(),
        );
        Self {
            coupon_rate,
            initial_spot,
            knock_in_level,
            knock_out_level,
            barrier_state: defaults.barrier_state,
            principal_ratio: defaults.principal_ratio,
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
    ///
    /// # Errors
    ///
    /// Propagates [`SnowballOption::new`] errors for invalid expanded terms,
    /// schedules or life dates. Inputs are consumed on failure.
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
        }
        SnowballOption::new(terms)
    }
}
