use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use super::super::{BLOCK_SIZE, BlockRunner, PathBlocks, skipped_sample_pair};
use super::{
    AutocallableNote, AutocallableTerms, CashEvent, Error, Moments, MonteCarloAutocallableEngine,
    MonteCarloSettings, PathPlan, PricingContext, PricingEngine, Result, Sample, SkippingPath,
    check_path_step, scaled_value, unstable,
};

// Leave room for the next note's preparation and the request's Sobol table.
const BATCH_BYTES: usize = 256 * 1024 * 1024;
const PLAN_RESERVATION: usize = 16;

type IndexedPlan<'a, T> = (usize, PathPlan<'a, T>);

pub(super) fn price_many<T: AutocallableTerms>(
    mut engine: MonteCarloAutocallableEngine,
    notes: &[&AutocallableNote<T>],
    context: &PricingContext,
    share_daily_grid: bool,
) -> Result<Vec<f64>> {
    let _sampling = super::super::sobol::RequestCache::new();
    let mut prices = Vec::new();
    prices
        .try_reserve_exact(notes.len())
        .map_err(|_| Error::ComputationFailure("cannot reserve Monte Carlo batch output".into()))?;
    prices.resize(notes.len(), 0.0);
    let mut groups: Vec<Vec<IndexedPlan<'_, T>>> = Vec::new();
    let mut index_by_grid: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut bytes = 0;
    for (index, note) in notes.iter().enumerate() {
        crate::cancellation::check()?;
        let plan = match engine.prepare_sample::<_, PLAN_RESERVATION>(note, context) {
            Ok(Sample::Fixed(price)) => {
                prices[index] = price;
                continue;
            }
            Ok(Sample::Path(plan)) => plan,
            Err(Error::InvalidInput(_)) => {
                // A schedule may fit only as a scalar request.
                price_groups(&mut groups, engine.settings, &mut prices, share_daily_grid)?;
                index_by_grid.clear();
                bytes = 0;
                let (price, seed) = engine.price_with_seed(note, context)?;
                engine.settings.seed = seed.or(engine.settings.seed);
                prices[index] = price;
                continue;
            }
            Err(error) => return Err(error),
        };
        engine = engine.with_resolved_seed()?;
        let shared = share_daily_grid && plan.shared_daily_grid();
        let (key, plan_bytes) = key_and_workspace(&plan, engine.settings, shared)?;
        if bytes > BATCH_BYTES.saturating_sub(plan_bytes) {
            price_groups(&mut groups, engine.settings, &mut prices, share_daily_grid)?;
            index_by_grid.clear();
            bytes = 0;
        }
        if plan_bytes > BATCH_BYTES {
            prices[index] = scalar(&plan, engine.settings)?;
            continue;
        }
        let candidates = index_by_grid.entry(key).or_default();
        let mut matched = None;
        for &candidate in candidates.iter() {
            let base = &groups[candidate][0].1;
            if if shared {
                base.same_daily_grid(&plan)?
            } else {
                base.shares_portfolio_grid(&plan)?
            } {
                matched = Some(candidate);
                break;
            }
        }
        let group = matched.unwrap_or_else(|| {
            candidates.push(groups.len());
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push((index, plan));
        bytes += plan_bytes;
    }
    price_groups(&mut groups, engine.settings, &mut prices, share_daily_grid)?;
    Ok(prices)
}

fn key_and_workspace<T: AutocallableTerms>(
    plan: &PathPlan<'_, T>,
    settings: MonteCarloSettings,
    shared: bool,
) -> Result<(u64, usize)> {
    let mut key = DefaultHasher::new();
    plan.steps.len().hash(&mut key);
    shared.hash(&mut key);
    if !shared {
        plan.bridge.is_some().hash(&mut key);
    }
    let mut events = 0;
    for chunk in plan.steps.chunks(1024) {
        crate::cancellation::check()?;
        for step in chunk {
            step.transition.map(f64::to_bits).hash(&mut key);
            step.observe_knock_in.hash(&mut key);
            if !shared {
                step.event.is_some().hash(&mut key);
            }
            events += usize::from(step.event.is_some());
        }
    }
    let schedule = std::mem::size_of_val(plan.steps.as_slice())
        + plan.bridge.as_ref().map_or(0, |bridge| {
            std::mem::size_of_val(bridge.daily.as_slice())
                + bridge.intervals.capacity() * size_of::<super::BridgeInterval>()
        });
    let blocks = settings.sample_count().div_ceil(BLOCK_SIZE);
    let bytes = schedule
        + if shared {
            events * size_of::<(usize, Event)>() + plan.steps.len() * size_of::<usize>()
        } else {
            events * size_of::<Event>()
        }
        + blocks * (size_of::<Moments>() + size_of::<Lane>() + size_of::<usize>())
        + 2 * size_of::<IndexedPlan<'_, T>>();
    Ok((key.finish(), bytes))
}

fn scalar<T: AutocallableTerms>(
    plan: &PathPlan<'_, T>,
    settings: MonteCarloSettings,
) -> Result<f64> {
    let ([moment], _, _) = plan.sample(settings, [plan], false, false)?;
    scaled_value(moment.scale, moment.mean, 0.0)
}

fn price_groups<T: AutocallableTerms>(
    groups: &mut Vec<Vec<IndexedPlan<'_, T>>>,
    settings: MonteCarloSettings,
    prices: &mut [f64],
    share_daily_grid: bool,
) -> Result<()> {
    for group in groups.drain(..) {
        crate::cancellation::check()?;
        let (index, base) = &group[0];
        if group.len() == 1 {
            prices[*index] = scalar(base, settings)?;
        } else {
            Portfolio::new(&group, share_daily_grid && base.shared_daily_grid())?
                .price(settings, prices)?;
        }
    }
    Ok(())
}

struct Portfolio<'a, 'b, T: AutocallableTerms> {
    notes: &'a [IndexedPlan<'b, T>],
    events: Vec<Event>,
    shared: Option<SharedEvents>,
    expiry_discount: f64,
}

struct SharedEvents {
    events: Vec<(usize, Event)>,
    ends: Vec<usize>,
}

impl<T: AutocallableTerms> PathPlan<'_, T> {
    fn shared_daily_grid(&self) -> bool {
        self.steps.iter().any(|step| step.observe_knock_in)
    }

    fn same_daily_grid(&self, other: &Self) -> Result<bool> {
        if self.steps.len() != other.steps.len() {
            return Ok(false);
        }
        for (left, right) in self.steps.chunks(1024).zip(other.steps.chunks(1024)) {
            crate::cancellation::check()?;
            if left.iter().zip(right).any(|(a, b)| {
                a.transition.map(f64::to_bits) != b.transition.map(f64::to_bits)
                    || a.observe_knock_in != b.observe_knock_in
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl SharedEvents {
    fn new<T: AutocallableTerms>(notes: &[IndexedPlan<'_, T>]) -> Result<Self> {
        let steps = &notes[0].1.steps;
        let mut ends = Vec::with_capacity(steps.len());
        let mut events = Vec::new();
        for (step, base) in steps.iter().enumerate() {
            check_path_step(step)?;
            for (lane, (_, note)) in notes.iter().enumerate() {
                check_path_step(lane)?;
                if let Some(event) = note.steps[step].event {
                    events.push((lane, Event::new(event, base.transition[2])));
                }
            }
            ends.push(events.len());
        }
        Ok(Self { events, ends })
    }
}

struct Event {
    knock_out: Option<f64>,
    coupon_barrier: f64,
    payments: [[f64; 2]; 2],
}

impl Event {
    fn new(event: CashEvent, discount: f64) -> Self {
        Self {
            knock_out: event.knock_out,
            coupon_barrier: event.coupon_barrier,
            // Defer unrepresentable cashflows until a path actually pays them.
            payments: [false, true].map(|hit| {
                [false, true].map(|due| {
                    event
                        .discounted_payment(due, hit, discount)
                        .unwrap_or(f64::NAN)
                })
            }),
        }
    }

    fn pay(&self, lane: &mut Lane, log_return: f64) -> Result<bool> {
        let hit = self.knock_out.is_some_and(|level| log_return >= level);
        let due = log_return >= self.coupon_barrier;
        lane.cash += self.payments[usize::from(hit)][usize::from(due)];
        if !lane.cash.is_finite() {
            return Err(unstable());
        }
        Ok(hit)
    }
}

#[derive(Clone, Copy)]
struct Lane {
    cash: f64,
    monitoring: Option<f64>,
    knocked_in: bool,
    terminated: bool,
}

impl<'a, 'b, T: AutocallableTerms> Portfolio<'a, 'b, T> {
    fn new(notes: &'a [IndexedPlan<'b, T>], shared: bool) -> Result<Self> {
        let base = &notes[0].1;
        let expiry_discount = base
            .steps
            .last()
            .expect("an active path has an expiry step")
            .transition[2];
        if shared {
            return Ok(Self {
                notes,
                events: Vec::new(),
                shared: Some(SharedEvents::new(notes)?),
                expiry_discount,
            });
        }
        let count = base
            .steps
            .iter()
            .filter(|step| step.event.is_some())
            .count()
            * notes.len();
        let mut events = Vec::with_capacity(count);
        for (i, step) in base.steps.iter().enumerate() {
            check_path_step(i)?;
            if step.event.is_some() {
                for (index, (_, plan)) in notes.iter().enumerate() {
                    check_path_step(index)?;
                    let event = plan.steps[i]
                        .event
                        .expect("compatible grids share event dates");
                    events.push(Event::new(event, step.transition[2]));
                }
            }
        }
        Ok(Self {
            notes,
            events,
            shared: None,
            expiry_discount,
        })
    }

    fn price(&self, settings: MonteCarloSettings, prices: &mut [f64]) -> Result<()> {
        let base = &self.notes[0].1;
        let dimensions = base.steps.len();
        let blocks = PathBlocks::new(settings, dimensions)?;
        let lanes = self.notes.len();
        let mut partials = vec![Moments::default(); blocks.count() * lanes];
        let mut blocks_out = partials.chunks_exact_mut(lanes).collect::<Vec<_>>();
        let simulate = |block, moments: &mut [Moments]| {
            let mut states = Vec::with_capacity(lanes);
            let mut live = Vec::with_capacity(lanes);
            blocks.sample(block, base.bridge.is_some(), |draws, paired| {
                skipped_sample_pair(draws, dimensions, paired, |path, _| {
                    if let Some(events) = &self.shared {
                        self.shared_payoffs(path, &mut states, events)?;
                    } else {
                        self.payoffs(path, &mut states, &mut live)?;
                    }
                    for (moment, lane) in moments.iter_mut().zip(&states) {
                        moment.add(lane.cash, false);
                    }
                    Ok(())
                })
            })
        };
        BlockRunner::new(blocks.count(), settings.max_active_workers)?
            .run(&mut blocks_out, |block, moments| simulate(block, moments))?;
        let mut total = vec![Moments::default(); lanes];
        for partial in partials.chunks_exact(lanes) {
            crate::cancellation::check()?;
            for (total, &moment) in total.iter_mut().zip(partial) {
                total.merge(moment, false);
            }
        }
        for ((index, _), moment) in self.notes.iter().zip(total) {
            prices[*index] = scaled_value(moment.scale, moment.mean, 0.0)?;
        }
        Ok(())
    }

    fn reset_states(&self, states: &mut Vec<Lane>) -> Result<()> {
        states.clear();
        for (index, (_, plan)) in self.notes.iter().enumerate() {
            check_path_step(index)?;
            states.push(Lane {
                cash: plan.state.coupon,
                monitoring: (!plan.state.knocked_in).then_some(plan.knock_in).flatten(),
                knocked_in: plan.state.knocked_in,
                terminated: false,
            });
        }
        Ok(())
    }

    fn shared_payoffs(
        &self,
        normals: &mut SkippingPath<'_, '_>,
        states: &mut Vec<Lane>,
        schedule: &SharedEvents,
    ) -> Result<()> {
        self.reset_states(states)?;
        let base = &self.notes[0].1;
        let mut log_return = 0.0;
        let mut minimum = f64::INFINITY;
        let mut first = 0;
        let mut remaining = states.len();
        for (index, (step, &end)) in base.steps.iter().zip(&schedule.ends).enumerate() {
            check_path_step(index)?;
            let [drift, diffusion, _] = step.transition;
            log_return += diffusion.mul_add(normals.normal(), drift);
            base.validate_return(log_return)?;
            if step.observe_knock_in {
                minimum = minimum.min(log_return);
            }
            for (event_index, (lane, event)) in schedule.events[first..end].iter().enumerate() {
                check_path_step(event_index)?;
                let lane = &mut states[*lane];
                if !lane.terminated && event.pay(lane, log_return)? {
                    lane.terminated = true;
                    remaining -= 1;
                }
            }
            if remaining == 0 {
                return Ok(());
            }
            first = end;
        }
        for (i, (lane, (_, plan))) in states.iter_mut().zip(self.notes).enumerate() {
            check_path_step(i)?;
            if !lane.terminated {
                let floor = if plan.expiry_knock_in {
                    minimum.min(log_return)
                } else {
                    minimum
                };
                knock_in(lane, floor);
                lane.cash +=
                    plan.discounted_terminal(log_return, lane.knocked_in, self.expiry_discount)?;
                if !lane.cash.is_finite() {
                    return Err(unstable());
                }
            }
        }
        Ok(())
    }

    fn payoffs(
        &self,
        normals: &mut SkippingPath<'_, '_>,
        states: &mut Vec<Lane>,
        live: &mut Vec<usize>,
    ) -> Result<()> {
        self.reset_states(states)?;
        live.clear();
        live.extend(0..states.len());
        let base = &self.notes[0].1;
        let mut events = self.events.chunks_exact(states.len());
        let mut log_return = 0.0;
        let mut minimum = f64::INFINITY;
        if let Some(bridge) = &base.bridge {
            let mut start = 0;
            for (index, interval) in bridge.intervals.iter().enumerate() {
                check_path_step(index)?;
                let target =
                    log_return + interval.diffusion.mul_add(normals.normal(), interval.drift);
                minimum = f64::INFINITY;
                if let Some(floor) = live
                    .iter()
                    .filter_map(|&index| states[index].monitoring)
                    .reduce(f64::min)
                {
                    for i in start..interval.end {
                        if minimum < floor {
                            normals.skip(interval.end - i);
                            break;
                        }
                        check_path_step(i)?;
                        let [weight, diffusion] = bridge.daily[i];
                        log_return +=
                            weight.mul_add(target - log_return, diffusion * normals.normal());
                        minimum = minimum.min(log_return);
                    }
                } else {
                    normals.skip(interval.end - start);
                }
                log_return = target;
                start = interval.end + 1;
                let step = &base.steps[interval.end];
                if step.observe_knock_in {
                    minimum = minimum.min(log_return);
                }
                if let Some(events) = step.event.and_then(|_| events.next()) {
                    observe(states, live, events, log_return, minimum)?;
                } else {
                    for &index in &*live {
                        knock_in(&mut states[index], minimum);
                    }
                }
                if live.is_empty() {
                    break;
                }
            }
            minimum = f64::INFINITY;
        } else {
            for (index, step) in base.steps.iter().enumerate() {
                check_path_step(index)?;
                let [drift, diffusion, _] = step.transition;
                log_return += diffusion.mul_add(normals.normal(), drift);
                base.validate_return(log_return)?;
                if step.observe_knock_in {
                    minimum = minimum.min(log_return);
                }
                if let Some(events) = step.event.and_then(|_| events.next()) {
                    observe(states, live, events, log_return, f64::INFINITY)?;
                    if live.is_empty() {
                        break;
                    }
                }
            }
        }
        for (position, &index) in live.iter().enumerate() {
            check_path_step(position)?;
            let plan = &self.notes[index].1;
            let lane = &mut states[index];
            let floor = if plan.expiry_knock_in {
                minimum.min(log_return)
            } else {
                minimum
            };
            knock_in(lane, floor);
            lane.cash +=
                plan.discounted_terminal(log_return, lane.knocked_in, self.expiry_discount)?;
            if !lane.cash.is_finite() {
                return Err(unstable());
            }
        }
        Ok(())
    }
}

fn knock_in(lane: &mut Lane, minimum: f64) {
    if lane.monitoring.is_some_and(|level| minimum < level) {
        lane.monitoring = None;
        lane.knocked_in = true;
    }
}

fn observe(
    states: &mut [Lane],
    live: &mut Vec<usize>,
    events: &[Event],
    log_return: f64,
    minimum: f64,
) -> Result<()> {
    let mut survivors = 0;
    for position in 0..live.len() {
        check_path_step(position)?;
        let index = live[position];
        let lane = &mut states[index];
        let event = &events[index];
        knock_in(lane, minimum);
        if !event.pay(lane, log_return)? {
            if survivors != position {
                live[survivors] = index;
            }
            survivors += 1;
        }
    }
    live.truncate(survivors);
    Ok(())
}
