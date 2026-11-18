use super::{Curve, PricingContext, Result, SpatialGrid, Stepper, TimeGrid, local_volatility};
use crate::{Error, LocalVolatilitySurface, Timestamp};

// Dispatch outside the solver so each diffusion keeps its compile-time specialization.
macro_rules! with_diffusion {
    ($context:ident, |$diffusion:ident| $price:expr) => {{
        use $crate::engines::finite_difference::{diffusion::LocalDiffusion, local_volatility};
        if let Some(model) = local_volatility::local_model($context) {
            let base = LocalDiffusion::new($context, model.surface());
            if $context
                .model_parameters()
                .variance_time_convention()
                .calendar()
                .is_some()
            {
                let $diffusion =
                    $crate::engines::finite_difference::diffusion::BusinessDiffusion::new(
                        $context, base,
                    );
                $price
            } else {
                let $diffusion = base;
                $price
            }
        } else {
            let base = $context.bsm_parameters()?.volatility();
            if $context
                .model_parameters()
                .variance_time_convention()
                .calendar()
                .is_some()
            {
                let $diffusion =
                    $crate::engines::finite_difference::diffusion::BusinessDiffusion::new(
                        $context, base,
                    );
                $price
            } else {
                let $diffusion = base;
                $price
            }
        }
    }};
}
pub(super) use with_diffusion;

pub(super) trait Diffusion: Copy {
    const LOCAL: bool;
    const SPOT_DEPENDENT: bool = Self::LOCAL;
    fn sigma(self, time: f64, spot: f64) -> f64;

    fn variance_rate(self, _time: f64) -> f64 {
        1.0
    }

    fn stepper(self, count: usize) -> Stepper {
        if Self::SPOT_DEPENDENT {
            Stepper::new_local(count)
        } else {
            Stepper::new(count)
        }
    }

    #[expect(clippy::too_many_arguments, reason = "shared numerical step inputs")]
    fn advance(
        self,
        stepper: &mut Stepper,
        old: &[f64],
        next: &mut [f64],
        space: &SpatialGrid,
        middle: f64,
        key: [f64; 4],
        edges: [f64; 2],
    ) -> Result<()> {
        let scale = self.variance_rate(middle).sqrt();
        if Self::SPOT_DEPENDENT {
            let [dt, theta, rate, dividend] = key;
            stepper.advance_local(
                old,
                next,
                |i| {
                    space.coefficients(
                        i,
                        rate,
                        dividend,
                        self.sigma(middle, space.asset(i)) * scale,
                    )
                },
                [dt, theta],
                edges,
            )
        } else {
            stepper.advance(
                old,
                next,
                space,
                self.sigma(middle, 0.0) * scale,
                key,
                edges,
            )
        }
    }
}

impl Diffusion for f64 {
    const LOCAL: bool = false;
    fn sigma(self, _: f64, _: f64) -> f64 {
        self
    }
}

#[derive(Clone, Copy)]
pub(super) struct LocalDiffusion<'a> {
    surface: &'a LocalVolatilitySurface,
    offset: f64,
}

impl<'a> LocalDiffusion<'a> {
    pub fn new(context: &PricingContext, surface: &'a LocalVolatilitySurface) -> Self {
        Self {
            surface,
            offset: surface.times()[0].years_until(context.valuation_time()),
        }
    }
}

impl Diffusion for LocalDiffusion<'_> {
    const LOCAL: bool = true;
    fn sigma(self, time: f64, spot: f64) -> f64 {
        self.surface.at_year(self.offset + time, spot)
    }
}

#[derive(Clone, Copy)]
pub(super) struct BusinessDiffusion<'a, D> {
    base: D,
    context: &'a PricingContext,
}

impl<'a, D> BusinessDiffusion<'a, D> {
    pub fn new(context: &'a PricingContext, base: D) -> Self {
        Self { base, context }
    }
}

impl<D: Diffusion> Diffusion for BusinessDiffusion<'_, D> {
    const LOCAL: bool = true;
    const SPOT_DEPENDENT: bool = D::SPOT_DEPENDENT;
    fn sigma(self, time: f64, spot: f64) -> f64 {
        self.base.sigma(time, spot)
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "A validated timestamp horizon bounds the elapsed day offset to i64"
    )]
    fn variance_rate(self, time: f64) -> f64 {
        let start = self.context.valuation_time();
        let elapsed_days = (start.date().start_of_day().years_until(start) + time) * 365.0;
        start
            .date()
            .add_days(elapsed_days.floor() as i64)
            .map_or(f64::NAN, |date| {
                self.context
                    .model_parameters()
                    .variance_time_convention()
                    .variance_rate(date)
            })
    }
}

pub(super) fn time_grid(
    context: &PricingContext,
    horizon: Timestamp,
    count: i32,
    risk: &Curve,
    dividend: &Curve,
    events: &[f64],
) -> Result<TimeGrid> {
    let start = context.valuation_time();
    let time = start.years_until(horizon);
    let mut knots = events.to_vec();
    knots.extend(
        context
            .model_parameters()
            .variance_time_convention()
            .transitions(start, horizon)?
            .into_iter()
            .map(|t| start.years_until(t)),
    );
    if let Some(model) = local_volatility::local_model(context) {
        model.surface().validate_time(horizon)?;
        knots.extend(
            model
                .surface()
                .times()
                .iter()
                .filter(|&&t| t > start && t < horizon)
                .map(|&t| start.years_until(t)),
        );
    }
    Ok(TimeGrid::with_events(time, count, risk, dividend, &knots))
}

pub(super) fn observation_times(
    valuation: Timestamp,
    expiry: Timestamp,
    events: &[Timestamp],
) -> Result<Vec<f64>> {
    validate_event_resolution(valuation, expiry, events)?;
    Ok(events
        .iter()
        .filter(|&&time| time > valuation)
        .map(|&time| valuation.years_until(time))
        .collect())
}

pub(super) fn validate_event_resolution(
    valuation: Timestamp,
    expiry: Timestamp,
    events: &[Timestamp],
) -> Result<()> {
    let mut previous = valuation;
    let mut previous_years = 0.0;
    for &time in events
        .iter()
        .chain(std::iter::once(&expiry))
        .filter(|&&time| time > valuation)
    {
        let years = valuation.years_until(time);
        if time != previous && years <= previous_years {
            return Err(Error::ComputationFailure(
                "finite-difference event times are not representably distinct".into(),
            ));
        }
        previous = time;
        previous_years = years;
    }
    Ok(())
}
