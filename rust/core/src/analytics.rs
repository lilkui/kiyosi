use crate::{Error, ErrorCategory, EuropeanOption, PricingContext, Result};

/// Spot derivatives are per spot unit; volatility/rate derivatives per percentage
/// point; time derivatives per calendar day of forward valuation time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum Greek {
    Delta,
    Gamma,
    Speed,
    Theta,
    Charm,
    Color,
    Vega,
    Vanna,
    Zomma,
    Rho,
}

impl Greek {
    pub const ALL: [Self; 10] = [
        Self::Delta,
        Self::Gamma,
        Self::Speed,
        Self::Theta,
        Self::Charm,
        Self::Color,
        Self::Vega,
        Self::Vanna,
        Self::Zomma,
        Self::Rho,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Delta => "delta",
            Self::Gamma => "gamma",
            Self::Speed => "speed",
            Self::Theta => "theta",
            Self::Charm => "charm",
            Self::Color => "color",
            Self::Vega => "vega",
            Self::Vanna => "vanna",
            Self::Zomma => "zomma",
            Self::Rho => "rho",
        }
    }
}

/// Selected measures; duplicates are ignored. Empty requests fail at valuation.
#[derive(Clone, Copy, Debug, Default)]
pub struct GreeksRequest([bool; 10]);
impl GreeksRequest {
    pub fn new(greeks: impl IntoIterator<Item = Greek>) -> Self {
        let mut result = Self::default();
        for greek in greeks {
            result.0[greek as usize] = true;
        }
        result
    }
    pub const fn all() -> Self {
        Self([true; 10])
    }
    pub fn has(self, greek: Greek) -> bool {
        self.0[greek as usize]
    }
    pub(crate) fn validate(self) -> Result<()> {
        if !self.0.contains(&true) {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "empty Greeks request",
            ));
        }
        Ok(())
    }
}

/// Finite price and optional finite Greeks. Missing measures are never zero sentinels.
#[derive(Clone, Debug, PartialEq)]
pub struct PricingResult {
    price: f64,
    values: [Option<f64>; 10],
}
impl PricingResult {
    pub fn price(&self) -> f64 {
        self.price
    }
    pub fn get(&self, greek: Greek) -> Option<f64> {
        self.values[greek as usize]
    }
    pub fn has(&self, greek: Greek) -> bool {
        self.get(greek).is_some()
    }
    pub fn require(&self, greek: Greek) -> Result<f64> {
        self.get(greek).ok_or(Error::new(
            ErrorCategory::InvalidResult,
            "requested Greek is unavailable",
        ))
    }
    /// Borrowed immutable storage, valid for this result's lifetime.
    pub fn values(&self) -> &[Option<f64>; 10] {
        &self.values
    }
    pub(crate) fn new(price: f64) -> Result<Self> {
        if !price.is_finite() {
            return Err(Error::new(ErrorCategory::InvalidResult, "non-finite price"));
        }
        Ok(Self {
            price,
            values: [None; 10],
        })
    }
    pub(crate) fn set(&mut self, request: GreeksRequest, greek: Greek, value: f64) -> Result<()> {
        if request.has(greek) {
            if !value.is_finite() {
                return Err(Error::new(ErrorCategory::InvalidResult, "non-finite Greek"));
            }
            self.values[greek as usize] = Some(value);
        }
        Ok(())
    }
}

/// Absolute finite-difference bumps. Positive values are validated even for analytic requests.
#[derive(Clone, Copy, Debug)]
pub struct NumericalShiftSettings {
    pub spot_shift: f64,
    pub volatility_shift: f64,
    pub rate_shift: f64,
    pub time_shift_days: i32,
}
impl Default for NumericalShiftSettings {
    fn default() -> Self {
        Self {
            spot_shift: 1e-2,
            volatility_shift: 1e-4,
            rate_shift: 1e-4,
            time_shift_days: 1,
        }
    }
}
impl NumericalShiftSettings {
    pub(crate) fn validate(self) -> Result<()> {
        if [self.spot_shift, self.volatility_shift, self.rate_shift]
            .iter()
            .any(|x| !x.is_finite() || *x <= 0.0)
            || self.time_shift_days <= 0
        {
            return Err(Error::new(
                ErrorCategory::InvalidParameter,
                "numerical shifts are invalid",
            ));
        }
        Ok(())
    }
}

/// Scalar European pricing boundary reused by numerical analysis and solvers.
/// Implementations must return domain failures through Result, without mutating inputs.
pub trait EuropeanPricingEngine {
    fn price(&self, option: &EuropeanOption, context: &PricingContext) -> Result<f64>;
}
