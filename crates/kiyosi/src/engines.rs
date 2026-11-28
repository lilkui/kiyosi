use crate::{
    BinaryPayoff, DigitalOption, Error, Greek, GreeksRequest, NumericalShiftSettings,
    OptionContract, OptionType, PricingContext, PricingEngine, PricingResult, Result,
};
use std::sync::{
    OnceLock,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

pub(crate) mod analytic;
pub(crate) mod binomial;
mod curve;
pub(crate) mod deterministic;
pub(crate) mod finite_difference;
pub(crate) mod monte_carlo;
pub(crate) mod quadrature;

/// Checks the input count for CRR and autocallable Monte Carlo contract batches.
///
/// See [`crate::CoxRossRubinsteinVanillaEngine::price_many`] for a batch example.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] above 1,048,576 contracts.
#[doc(hidden)]
pub fn validate_contract_batch_size(count: usize) -> Result<()> {
    if count > 1 << 20 {
        return Err(Error::InvalidInput(
            "contract batch exceeds 1,048,576 instruments".into(),
        ));
    }
    Ok(())
}

fn validate_rollback_work(spatial_steps: u64, time_steps: u64) -> Result<()> {
    if spatial_steps.saturating_mul(time_steps) > 1_000_000_000 {
        return Err(Error::InvalidInput(
            "rollback exceeds one billion grid updates; reduce grid step counts".into(),
        ));
    }
    Ok(())
}

#[derive(Default)]
struct RollbackWork(AtomicU64);

impl RollbackWork {
    fn charge(&self, work: u64) -> Result<()> {
        let mut used = self.0.load(Ordering::Relaxed);
        loop {
            let total = used.saturating_add(work);
            validate_rollback_work(1, total)?;
            match self
                .0
                .compare_exchange_weak(used, total, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return Ok(()),
                Err(current) => used = current,
            }
        }
    }
}

fn rollback_pool() -> Option<(&'static rayon::ThreadPool, usize)> {
    static PROCESS: AtomicU32 = AtomicU32::new(0);
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    let available = std::thread::available_parallelism().ok()?.get().min(4);
    if available < 2 {
        return None;
    }
    let pid = std::process::id();
    if PROCESS
        .compare_exchange(0, pid, Ordering::Relaxed, Ordering::Relaxed)
        .is_err_and(|owner| owner != pid)
    {
        // A forked child can price serially even if the parent was initializing the pool.
        return None;
    }
    let pool = POOL
        .get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(available)
                .thread_name(|index| format!("kiyosi-rollback-{index}"))
                .build()
                .ok()
        })
        .as_ref()?;
    Some((pool, available.min(pool.current_num_threads())))
}

fn flat_rates(context: &PricingContext) -> Result<(f64, f64)> {
    let model = context.natural_bsm_parameters()?;
    Ok((model.flat_risk_free_rate()?, model.flat_dividend_yield()?))
}

fn greeks<I: OptionContract>(
    engine: &impl PricingEngine<I>,
    option: &I,
    context: &PricingContext,
    request: GreeksRequest,
    shifts: NumericalShiftSettings,
    native: impl FnOnce() -> Result<PricingResult>,
) -> Result<PricingResult> {
    engine.validate_applicability(context)?;
    shifts.validate()?;
    let supported = GreeksRequest::new([Greek::Delta, Greek::Gamma]);
    if deterministic::enabled(context) {
        let supported_request = GreeksRequest::new(
            [Greek::Delta, Greek::Gamma]
                .into_iter()
                .filter(|&greek| request.has(greek)),
        );
        return crate::numerical_greeks(engine, option, context, supported_request, shifts)
            .map(|result| result.with_request(request, supported));
    }
    let result = native()?.with_request(request, supported);
    crate::risk::numerical::expiry_greeks(option, context, shifts, result, |context| {
        engine.price(option, context)
    })
}

#[derive(Clone, Copy, PartialEq)]
struct Payoff {
    strike: f64,
    sign: f64,
    kind: PayoffKind,
}

#[derive(Clone, Copy, PartialEq)]
enum PayoffKind {
    European,
    American,
    Digital(BinaryPayoff),
}

impl Payoff {
    fn vanilla(option_type: OptionType, strike: f64, american: bool) -> Self {
        Self {
            strike,
            sign: if option_type == OptionType::Call {
                1.0
            } else {
                -1.0
            },
            kind: if american {
                PayoffKind::American
            } else {
                PayoffKind::European
            },
        }
    }

    fn digital(option: &DigitalOption) -> Self {
        Self {
            kind: PayoffKind::Digital(
                option
                    .payout()
                    .map_or(BinaryPayoff::Asset, BinaryPayoff::Cash),
            ),
            ..Self::vanilla(option.option_type(), option.strike(), false)
        }
    }

    fn is_american(self) -> bool {
        matches!(self.kind, PayoffKind::American)
    }

    fn is_digital(self) -> bool {
        matches!(self.kind, PayoffKind::Digital(_))
    }

    fn payout(self) -> Option<f64> {
        match self.kind {
            PayoffKind::Digital(BinaryPayoff::Cash(amount)) => Some(amount),
            _ => None,
        }
    }

    fn value(self, spot: f64) -> f64 {
        match self.kind {
            PayoffKind::Digital(payment) => {
                if self.sign * (spot - self.strike) > 0.0 {
                    match payment {
                        BinaryPayoff::Cash(amount) => amount,
                        BinaryPayoff::Asset => spot,
                    }
                } else {
                    0.0
                }
            }
            PayoffKind::European | PayoffKind::American => {
                (self.sign * (spot - self.strike)).max(0.0)
            }
        }
    }
}
