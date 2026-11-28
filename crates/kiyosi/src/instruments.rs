//! Contract presets and traits for working with contracts generically.
//!
//! See [`SnowballPresetTerms::expand`] and [`PhoenixPresetTerms::expand`] for
//! constructing concrete contracts from presets. Custom contracts implement
//! [`OptionContract`] for numerical analysis and implied-parameter solvers.

pub(crate) mod accumulator;
pub(crate) mod asian;
pub(crate) mod autocallable;
pub(crate) mod barrier;
pub(crate) mod binary_barrier;
pub(crate) mod double_barrier;
pub(crate) mod double_touch;
pub(crate) mod monitoring;
pub(crate) mod range_accrual;
pub(crate) mod vanilla;
pub use autocallable::{
    AutocallableNote, AutocallableTerms, PhoenixPreset, PhoenixPresetTerms, SnowballCouponTerms,
    SnowballPreset, SnowballPresetTerms,
};
pub use vanilla::OptionContract;
