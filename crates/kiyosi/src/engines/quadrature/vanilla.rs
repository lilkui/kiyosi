use super::gaussian::{
    CubicInterpolation, Layer, Polynomial, TerminalPiece, TransitionCache, integrate_terminal,
    integration_nodes,
};
use super::{QuadratureSettings, flat_rates, simpson};
use crate::engines::Payoff;
use crate::instruments::monitoring::validate_life;
use crate::numerics::{checked, checked_with_scale, log_ratio, scaled_cash};
use crate::{
    AmericanOption, Error, EuropeanOption, GreeksRequest, NumericalShiftSettings, OptionContract,
    PricingContext, PricingEngine, PricingResult, Result,
};

/// Configures vanilla spatial quadrature and American exercise intervals.
///
/// Counts have no extra cap; pricing remains subject to memory and runtime.
/// See [`QuadratureVanillaEngine::new`] for an example.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadratureVanillaSettings {
    /// Sets at least four spatial steps; defaults to 512. Simpson panels round up to even.
    pub asset_step_count: i32,
    /// Sets at least one American time interval; defaults to 200. European pricing ignores it.
    pub time_step_count: i32,
    /// Sets a finite positive integration extent in standard deviations; defaults to 8.
    pub integration_standard_deviations: f64,
}

impl Default for QuadratureVanillaSettings {
    fn default() -> Self {
        let spatial = QuadratureSettings::default();
        Self {
            asset_step_count: spatial.asset_step_count,
            time_step_count: 200,
            integration_standard_deviations: spatial.integration_standard_deviations,
        }
    }
}

impl QuadratureVanillaSettings {
    /// Validates spatial controls and the time count; see [`QuadratureVanillaEngine::new`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] for fewer than four spatial steps, a nonpositive
    /// or non-finite extent, or fewer than one time interval. Inputs remain unchanged.
    pub fn validate(&self) -> Result<()> {
        QuadratureSettings {
            asset_step_count: self.asset_step_count,
            integration_standard_deviations: self.integration_standard_deviations,
        }
        .validate()?;
        if self.time_step_count < 1 {
            return Err(Error::InvalidInput(
                "vanilla quadrature requires at least one time interval".into(),
            ));
        }
        Ok(())
    }
}

/// Prices European and American vanilla options by flat-rate BSM quadrature.
///
/// European pricing uses composite Simpson integration split at the strike.
/// American pricing integrates cubic continuation curves on a strike-centered
/// log-price grid, with linear interpolation in cells whose cubic control values
/// exceed the endpoint range, and exercise at valuation, uniform time points and expiry.
/// The last interval integrates the terminal payoff with exact Gaussian moments.
/// In-the-money tails extend the edge values linearly in the asset; opposite
/// tails are zero. This boundary approximation requires domain convergence checks.
///
/// Time, space and extent require separate convergence checks; fixed-grid
/// completion provides no error estimate. Numerical Greeks, especially higher
/// derivatives near exercise boundaries, also require bump convergence checks.
/// Owns immutable settings; distinct and shared instances support concurrent
/// pricing with request-local working storage. See [`Self::new`] for usage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadratureVanillaEngine {
    settings: QuadratureVanillaSettings,
}

impl QuadratureVanillaEngine {
    /// Creates an engine after validating all integration and exercise controls.
    ///
    /// # Errors
    ///
    /// Propagates [`QuadratureVanillaSettings::validate`] errors without changing inputs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kiyosi::*;
    /// # fn main() -> Result<()> {
    /// let start = Date::new(2025, 1, 1)?.start_of_day();
    /// let end = Date::new(2026, 1, 1)?.start_of_day();
    /// let option = AmericanOption::new(OptionType::Put, 100.0, start, end)?;
    /// let context = PricingContext::new(
    ///     BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    /// let settings = QuadratureVanillaSettings::default();
    /// settings.validate()?;
    /// let engine = QuadratureVanillaEngine::new(settings)?;
    /// assert_eq!(engine.settings(), settings);
    /// assert!(engine.price(&option, &context)? > 0.0);
    /// let result = engine.price_with_greeks(&option, &context,
    ///     GreeksRequest::new([Greek::Delta]), Default::default())?;
    /// assert!(result.require(Greek::Delta)? < 0.0);
    /// # Ok(()) }
    /// ```
    pub fn new(settings: QuadratureVanillaSettings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings })
    }

    /// Returns owned settings; see [`Self::new`] for an example.
    pub fn settings(&self) -> QuadratureVanillaSettings {
        self.settings
    }

    /// Prices a European or American option, settling intrinsic value at expiry.
    ///
    /// See [`Self::new`] for an example.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedOperation`] for other models or nonconstant rate curves,
    /// before life validation. Returns [`Error::InvalidInput`] outside inclusive
    /// contract life, and [`Error::ComputationFailure`] for unrepresentable grids,
    /// transitions, tails or prices. Inputs and engine settings remain unchanged.
    pub fn price<I>(&self, option: &I, context: &PricingContext) -> Result<f64>
    where
        Self: PricingEngine<I>,
    {
        <Self as PricingEngine<I>>::price(self, option, context)
    }

    /// Computes requested numerical Greeks with shared bump controls.
    ///
    /// Expiry Greeks follow terminal payoffs; time Greeks remain absent. Infeasible stencils leave affected measures
    /// absent; feasible bumped failures propagate. See [`Self::new`] for usage.
    ///
    /// # Errors
    ///
    /// Checks flat-rate BSM applicability before request and shift validation, then
    /// propagates [`crate::numerical_greeks`] and [`Self::price`] errors. Inputs remain unchanged.
    pub fn price_with_greeks<I: OptionContract>(
        &self,
        option: &I,
        context: &PricingContext,
        request: GreeksRequest,
        shifts: NumericalShiftSettings,
    ) -> Result<PricingResult>
    where
        Self: PricingEngine<I>,
    {
        crate::numerical_greeks(self, option, context, request, shifts)
    }
}

macro_rules! vanilla_pricing {
    ($option:ty, $american:expr) => {
        impl PricingEngine<$option> for QuadratureVanillaEngine {
            fn validate_applicability(&self, context: &PricingContext) -> Result<()> {
                flat_rates(context).map(|_| ())
            }
            fn price(&self, option: &$option, context: &PricingContext) -> Result<f64> {
                let (rate, dividend) = flat_rates(context)?;
                validate_life(option, context)?;
                context.validate_curve_horizon(option.expiry_time())?;
                let payoff = Payoff::vanilla(option.option_type(), option.strike(), $american);
                if crate::engines::deterministic::enabled(context) {
                    return crate::engines::deterministic::vanilla(option, payoff, context);
                }
                let time = context
                    .valuation_time()
                    .years_until(option.expiry_time().into());
                if time == 0.0 {
                    return checked(payoff.value(context.spot_price()));
                }
                let sigma = context.bsm_parameters()?.volatility();
                let drift = (rate - dividend - 0.5 * sigma * sigma) * time;
                let width = sigma * time.sqrt();
                if !drift.is_finite() || !width.is_finite() || width <= 0.0 {
                    return Err(invalid_transition());
                }
                if $american {
                    american_price(payoff, context, time, drift, width, rate, self.settings)
                } else {
                    european_price(payoff, context, time, drift, width, rate, self.settings)
                }
            }
        }
    };
}
vanilla_pricing!(EuropeanOption, false);
vanilla_pricing!(AmericanOption, true);

fn invalid_transition() -> Error {
    Error::ComputationFailure(
        "vanilla quadrature grid, transition or tail is not representable".into(),
    )
}

fn european_price(
    payoff: Payoff,
    context: &PricingContext,
    time: f64,
    drift: f64,
    width: f64,
    rate: f64,
    settings: QuadratureVanillaSettings,
) -> Result<f64> {
    let extent = settings.integration_standard_deviations;
    let upper = extent + if payoff.sign > 0.0 { width } else { 0.0 };
    if !upper.is_finite() {
        return Err(invalid_transition());
    }
    let current = log_ratio(context.spot_price(), payoff.strike);
    let kink = (-current - drift) / width;
    let mut breaks = vec![-extent, upper];
    if kink > -extent && kink < upper {
        breaks.insert(1, kink);
    }
    let integrand = |z: f64| {
        let distance = current + drift + width * z;
        if payoff.sign * distance <= 0.0 {
            return 0.0;
        }
        let log_payoff = payoff.strike.ln()
            + if distance > 0.0 {
                distance + (-(-distance).exp_m1()).ln()
            } else {
                (-distance.exp_m1()).ln()
            };
        (log_payoff - rate * time - 0.5 * z * z - 0.918_938_533_204_672_7).exp()
    };
    checked(simpson(&breaks, settings.asset_step_count, integrand))
}

fn american_price(
    payoff: Payoff,
    context: &PricingContext,
    time: f64,
    drift: f64,
    width: f64,
    rate: f64,
    settings: QuadratureVanillaSettings,
) -> Result<f64> {
    let intervals = f64::from(settings.time_step_count);
    let deviation = width / intervals.sqrt();
    let drift_step = drift / intervals;
    let discount = -rate * time / intervals;
    let extent = settings.integration_standard_deviations * width + drift.abs();
    if !extent.is_finite() || !discount.is_finite() || deviation <= 0.0 {
        return Err(invalid_transition());
    }
    let nodes = integration_nodes(-extent, extent, settings.asset_step_count, &[0.0])?;
    let interpolation = CubicInterpolation::new(&nodes);
    let intrinsic: Vec<_> = nodes
        .iter()
        .map(|&x| checked(log_intrinsic(payoff, x)))
        .collect::<Result<_>>()?;
    let mut layer = Layer {
        cells: Vec::new(),
        exact: vec![TerminalPiece {
            lower: if payoff.sign > 0.0 {
                0.0
            } else {
                f64::NEG_INFINITY
            },
            upper: if payoff.sign > 0.0 {
                f64::INFINITY
            } else {
                0.0
            },
            cash: -payoff.sign * payoff.strike,
            asset: payoff.sign,
        }],
    };
    let mut cache = TransitionCache::default();
    let mut values = vec![0.0; nodes.len()];
    for _ in 1..settings.time_step_count {
        if layer.cells.is_empty() {
            values.fill(0.0);
        } else {
            cache
                .get(time / intervals, &nodes, drift_step, deviation)
                .integrate(&layer.cells, discount, &mut values);
        }
        for ((value, &node), &exercise) in values.iter_mut().zip(&nodes).zip(&intrinsic) {
            let continuation = *value
                + integrate_terminal(
                    &layer.exact,
                    node,
                    payoff.strike,
                    drift_step,
                    deviation,
                    discount,
                );
            *value = checked_with_scale(continuation, exercise + payoff.strike)?.max(exercise);
        }
        layer = Layer {
            cells: continuation_curve(&interpolation, &values),
            exact: tails(payoff, &nodes, &values)?,
        };
    }
    let continuation = layer.evaluate(
        &nodes,
        log_ratio(context.spot_price(), payoff.strike),
        payoff.strike,
        drift_step,
        deviation,
        discount,
    );
    Ok(
        checked_with_scale(continuation, context.spot_price() + payoff.strike)?
            .max(payoff.value(context.spot_price())),
    )
}

fn continuation_curve(interpolation: &CubicInterpolation, values: &[f64]) -> Vec<Polynomial> {
    let mut cells = interpolation.interpolate(values);
    for (cell, endpoints) in cells.iter_mut().zip(values.windows(2)) {
        let lower = endpoints[0].min(endpoints[1]);
        let upper = endpoints[0].max(endpoints[1]);
        let controls = [
            cell[0] + cell[1] / 3.0,
            cell[0] + (2.0 * cell[1] + cell[2]) / 3.0,
        ];
        // Bernstein control values bound the entire cubic, including nearly worthless tails.
        if controls
            .iter()
            .any(|&value| !value.is_finite() || value < lower || value > upper)
        {
            *cell = [endpoints[0], endpoints[1] - endpoints[0], 0.0, 0.0];
        }
    }
    cells
}

fn log_intrinsic(payoff: Payoff, x: f64) -> f64 {
    if payoff.sign * x <= 0.0 {
        0.0
    } else if x > 0.0 {
        scaled_cash(payoff.strike, x + (-(-x).exp_m1()).ln())
    } else {
        payoff.strike * -x.exp_m1()
    }
}

fn tails(payoff: Payoff, nodes: &[f64], values: &[f64]) -> Result<Vec<TerminalPiece>> {
    let first = if payoff.sign > 0.0 {
        nodes.len() - 2
    } else {
        0
    };
    let asset = scaled_cash(payoff.strike, nodes[first]);
    let change = scaled_cash(
        payoff.strike,
        nodes[first] + (nodes[first + 1] - nodes[first]).exp_m1().ln(),
    );
    let slope = (values[first + 1] - values[first]) / change;
    let cash = values[first] - slope * asset;
    if !slope.is_finite() || !cash.is_finite() || payoff.sign * slope < 0.0 {
        return Err(invalid_transition());
    }
    Ok(vec![TerminalPiece {
        lower: if payoff.sign > 0.0 {
            nodes[nodes.len() - 1]
        } else {
            f64::NEG_INFINITY
        },
        upper: if payoff.sign > 0.0 {
            f64::INFINITY
        } else {
            nodes[0]
        },
        cash,
        asset: slope,
    }])
}
