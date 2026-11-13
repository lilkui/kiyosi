use super::{Curve, PricingContext, Result, SpatialGrid, Stepper, TimeGrid, local_volatility};
use crate::{Error, LocalVolatilitySurface, Timestamp};

pub(super) trait Diffusion: Copy {
    const LOCAL: bool;
    fn sigma(self, time: f64, spot: f64) -> f64;

    fn stepper(self, count: usize) -> Stepper {
        if Self::LOCAL {
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
        if Self::LOCAL {
            let [dt, theta, rate, dividend] = key;
            stepper.advance_local(
                old,
                next,
                |i| space.coefficients(i, rate, dividend, self.sigma(middle, space.asset(i))),
                [dt, theta],
                edges,
            )
        } else {
            stepper.advance(old, next, space, self.sigma(0.0, 0.0), key, edges)
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

pub(super) fn time_grid(
    context: &PricingContext,
    horizon: Timestamp,
    count: i32,
    risk: &Curve,
    dividend: &Curve,
    events: &[f64],
) -> Result<TimeGrid> {
    let time = context.valuation_time().years_until(horizon);
    if let Some(model) = local_volatility::local_model(context) {
        model.surface().validate_time(horizon)?;
        let mut knots = events.to_vec();
        knots.extend(
            model
                .surface()
                .times()
                .iter()
                .filter(|&&t| t > context.valuation_time() && t < horizon)
                .map(|&t| context.valuation_time().years_until(t)),
        );
        Ok(TimeGrid::with_events(time, count, risk, dividend, &knots))
    } else {
        Ok(TimeGrid::with_events(time, count, risk, dividend, events))
    }
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
