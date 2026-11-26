use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use super::super::{BLOCK_SIZE, BlockRunner, PathBlocks, skipped_sample_pair};
use super::{
    AutocallableNote, AutocallableTerms, CashEvent, Error, Moments, MonteCarloAutocallableEngine,
    MonteCarloSettings, PathPlan, PathStep, PricingContext, PricingEngine, Result, Sample,
    SkippingPath, check_path_step, scaled_value, transition, unstable,
};
use crate::{ObservationSchedule, TimeOfDay, Timestamp, TradingCalendar};

// Leave room for the next note's preparation and the request's Sobol table.
const BATCH_BYTES: usize = 256 * 1024 * 1024;
const PLAN_RESERVATION: usize = 16;
const DAILY_GRID_SLOTS: usize = 4;
const DAILY_GRID_BYTES: usize = 2 * 1024 * 1024;
const SHARED_STEP_BYTES: usize =
    2 * size_of::<SharedInterval>() + size_of::<[f64; 2]>() + 4 * size_of::<usize>();

type IndexedPlan<'a, T> = (usize, PathPlan<'a, T>);

pub(super) struct SparseEvents {
    key: u64,
    maturity_on_grid: bool,
    events: Vec<(usize, CashEvent)>,
}

pub(super) struct SparsePath {
    pub steps: Arc<Vec<PathStep>>,
    pub events: SparseEvents,
    pub excursion: f64,
}

struct DailyGrid<'a> {
    expiry: Timestamp,
    calendar: &'a TradingCalendar,
    clock: TimeOfDay,
    dates: Vec<Timestamp>,
    steps: Arc<Vec<PathStep>>,
    key: u64,
    maturity_on_grid: bool,
    excursion: f64,
}

#[derive(Default)]
pub(super) struct DailyGrids<'a>(Vec<DailyGrid<'a>>);

impl<'a> DailyGrids<'a> {
    pub(super) fn prepare<T: AutocallableTerms>(
        &mut self,
        note: &'a AutocallableNote<T>,
        context: &PricingContext,
        settings: MonteCarloSettings,
        [rate, dividend, sigma]: [f64; 3],
    ) -> Result<Option<SparsePath>> {
        let Some(clock) = note.observation_clock_time() else {
            return Ok(None);
        };
        let expiry = note.expiry_time();
        let calendar = note.calendar();
        let valuation = context.valuation_time();
        let events = note.events();
        let first = events.partition_point(|record| record.time <= valuation);
        let events = &events[first..];
        let grid = if let Some(index) = self.0.iter().position(|grid| {
            grid.expiry == expiry && grid.clock == clock && grid.calendar == calendar
        }) {
            self.0.remove(index)
        } else {
            let max_days = DAILY_GRID_BYTES / (size_of::<Timestamp>() + size_of::<PathStep>());
            let days = valuation.date().days_until(expiry.date());
            if usize::try_from(days).unwrap_or(usize::MAX) >= max_days - 1 {
                return Ok(None);
            }
            for record in events {
                crate::cancellation::check()?;
                if record.time != expiry && !note.daily_time(record.time, calendar) {
                    return Ok(None);
                }
            }
            let mut dates =
                ObservationSchedule::daily_times(valuation, expiry, calendar, false, clock)?;
            if dates.is_empty() {
                return Ok(None);
            }
            let maturity_on_grid = dates.last() == Some(&expiry);
            if !maturity_on_grid {
                dates.push(expiry);
            }
            if dates.capacity() * size_of::<Timestamp>() + dates.len() * size_of::<PathStep>()
                > DAILY_GRID_BYTES
            {
                return Ok(None);
            }
            let mut steps = settings.scheduled_steps::<PathStep>(dates.len())?;
            let mut previous = valuation;
            let mut excursion = 0.0;
            for &date in &dates {
                crate::cancellation::check()?;
                let (drift, diffusion, discount) = transition(
                    rate,
                    dividend,
                    sigma,
                    previous.years_until(date),
                    valuation.years_until(date),
                )?;
                excursion += drift.abs() + 20.0 * diffusion;
                steps.push(PathStep {
                    transition: [drift, diffusion, discount],
                    event: None,
                    observe_knock_in: note.daily_time(date, calendar),
                });
                previous = date;
            }
            let key = if maturity_on_grid {
                let mut key = DefaultHasher::new();
                ("daily", clock.micros_since_midnight()).hash(&mut key);
                key.finish()
            } else {
                grid_key(&steps, true, false)?.0
            };
            if self.0.len() == DAILY_GRID_SLOTS {
                self.0.remove(0);
            }
            DailyGrid {
                expiry,
                calendar,
                clock,
                dates,
                steps: Arc::new(steps),
                key,
                maturity_on_grid,
                excursion,
            }
        };
        self.0.push(grid);
        let grid = self.0.last().expect("the daily grid was retained");
        let mut sparse = Vec::with_capacity(events.len());
        for record in events {
            crate::cancellation::check()?;
            let Ok(index) = grid.dates.binary_search(&record.time) else {
                return Ok(None);
            };
            sparse.push((index, CashEvent::new(note, record, context.spot_price())));
        }
        Ok(Some(SparsePath {
            steps: Arc::clone(&grid.steps),
            events: SparseEvents {
                key: grid.key,
                maturity_on_grid: grid.maturity_on_grid,
                events: sparse,
            },
            excursion: grid.excursion,
        }))
    }
}

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
    let mut daily_grids = DailyGrids::default();
    let mut charged_grids = HashSet::new();
    let budget = BATCH_BYTES
        - if share_daily_grid {
            DAILY_GRID_SLOTS * DAILY_GRID_BYTES
        } else {
            0
        };
    let mut bytes = 0;
    for (index, note) in notes.iter().enumerate() {
        crate::cancellation::check()?;
        let plan = match engine.prepare_sample_on_grid::<_, PLAN_RESERVATION>(
            note,
            context,
            share_daily_grid.then_some(&mut daily_grids),
        ) {
            Ok(Sample::Fixed(price)) => {
                prices[index] = price;
                continue;
            }
            Ok(Sample::Path(plan)) => plan,
            Err(Error::InvalidInput(_)) => {
                // A schedule may fit only as a scalar request.
                price_groups(&mut groups, engine, context, &mut prices, share_daily_grid)?;
                index_by_grid.clear();
                charged_grids.clear();
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
        let grid = plan
            .shared_events
            .as_ref()
            .map(|_| Arc::as_ptr(&plan.steps));
        let grid_bytes = plan.steps.len() * (size_of::<PathStep>() + SHARED_STEP_BYTES);
        let mut charge = plan_bytes
            + grid
                .filter(|grid| !charged_grids.contains(grid))
                .map_or(0, |_| grid_bytes);
        if bytes > budget.saturating_sub(charge) {
            price_groups(&mut groups, engine, context, &mut prices, share_daily_grid)?;
            index_by_grid.clear();
            charged_grids.clear();
            bytes = 0;
            charge = plan_bytes + grid.map_or(0, |_| grid_bytes);
        }
        if charge > budget {
            prices[index] = scalar(&plan, engine, context)?;
            continue;
        }
        if let Some(grid) = grid {
            charged_grids.insert(grid);
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
        bytes += charge;
    }
    price_groups(&mut groups, engine, context, &mut prices, share_daily_grid)?;
    Ok(prices)
}

fn grid_key(steps: &[PathStep], shared: bool, bridge: bool) -> Result<(u64, usize)> {
    let mut key = DefaultHasher::new();
    steps.len().hash(&mut key);
    shared.hash(&mut key);
    if !shared {
        bridge.hash(&mut key);
    }
    let mut events = 0;
    for chunk in steps.chunks(1024) {
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
    Ok((key.finish(), events))
}

fn key_and_workspace<T: AutocallableTerms>(
    plan: &PathPlan<'_, T>,
    settings: MonteCarloSettings,
    shared: bool,
) -> Result<(u64, usize)> {
    let (key, events) = if let Some(sparse) = &plan.shared_events {
        (sparse.key, sparse.events.len())
    } else {
        grid_key(&plan.steps, shared, plan.bridge.is_some())?
    };
    let schedule = if let Some(sparse) = &plan.shared_events {
        sparse.events.capacity() * size_of::<(usize, CashEvent)>()
    } else {
        std::mem::size_of_val(plan.steps.as_slice())
            + plan.bridge.as_ref().map_or(0, |bridge| {
                std::mem::size_of_val(bridge.daily.as_slice())
                    + bridge.intervals.capacity() * size_of::<super::BridgeInterval>()
            })
    };
    let blocks = settings.sample_count().div_ceil(BLOCK_SIZE);
    let bytes = schedule
        + if shared {
            // Buckets and packed events coexist while assembling the schedule.
            6 * (events + 1) * size_of::<(usize, SharedAction)>()
                + size_of::<(f64, usize)>()
                + if plan.shared_events.is_none() {
                    plan.steps.len() * SHARED_STEP_BYTES
                } else {
                    0
                }
        } else {
            events * size_of::<Event>()
        }
        + blocks * (size_of::<Moments>() + size_of::<Lane>() + size_of::<usize>())
        + 2 * size_of::<IndexedPlan<'_, T>>();
    Ok((key, bytes))
}

fn scalar<T: AutocallableTerms>(
    plan: &PathPlan<'_, T>,
    engine: MonteCarloAutocallableEngine,
    context: &PricingContext,
) -> Result<f64> {
    if plan.shared_events.is_some() {
        // A singleton uses the ordinary estimator, including its bridge if eligible.
        return engine.price(plan.note, context);
    }
    let ([moment], _, _) = plan.sample(engine.settings, [plan], false, false)?;
    scaled_value(moment.scale, moment.mean, 0.0)
}

fn price_groups<T: AutocallableTerms>(
    groups: &mut Vec<Vec<IndexedPlan<'_, T>>>,
    engine: MonteCarloAutocallableEngine,
    context: &PricingContext,
    prices: &mut [f64],
    share_daily_grid: bool,
) -> Result<()> {
    for mut group in groups.drain(..) {
        crate::cancellation::check()?;
        let (index, base) = &group[0];
        if group.len() == 1 {
            prices[*index] = scalar(base, engine, context)?;
        } else {
            let shared = share_daily_grid && base.shared_daily_grid();
            if !shared || !price_coupon_group(&mut group, engine.settings, prices)? {
                Portfolio::new(&group, shared)?.price(engine.settings, prices)?;
            }
        }
    }
    Ok(())
}

fn price_coupon_group<T: AutocallableTerms>(
    group: &mut [IndexedPlan<'_, T>],
    settings: MonteCarloSettings,
    prices: &mut [f64],
) -> Result<bool> {
    if group.len() < 3 {
        return Ok(false);
    }
    let first = group[0].1.note;
    let mut low = (0, first.portfolio_coupon());
    let mut high = low;
    for (index, (_, plan)) in group.iter().enumerate().skip(1) {
        crate::cancellation::check()?;
        if !first.shares_coupon_terms(plan.note)? {
            return Ok(false);
        }
        let coupon = plan.note.portfolio_coupon();
        if coupon < low.1 {
            low = (index, coupon);
        }
        if coupon > high.1 {
            high = (index, coupon);
        }
    }
    let width = high.1 - low.1;
    if !width.is_finite() {
        return Ok(false);
    }
    // Equal event dates and thresholds preserve the union grid and its draws.
    // Keep two endpoint lanes and interpolate the coupon cash flows afterward.
    group.swap(0, low.0);
    let high_index = if high.0 == 0 { low.0.max(1) } else { high.0 };
    group.swap(1, high_index);
    Portfolio::new(&group[..2], true)?.price(settings, prices)?;
    let [lower_price, upper_price] = [prices[group[0].0], prices[group[1].0]];
    // Restore the original lane order for a numerical fallback.
    group.swap(1, high_index);
    group.swap(0, low.0);
    for (index, plan) in group {
        crate::cancellation::check()?;
        let fraction = if width > 0.0 {
            (plan.note.portfolio_coupon() - low.1) / width
        } else {
            0.0
        };
        let value = [lower_price * (1.0 - fraction), upper_price * fraction];
        let price = value[0] + value[1];
        let magnitude = value[0].abs() + value[1].abs();
        if !price.is_finite() || price.abs() < 1e-12 * magnitude {
            return Ok(false);
        }
        prices[*index] = price;
    }
    Ok(true)
}

struct Portfolio<'a, 'b, T: AutocallableTerms> {
    notes: &'a [IndexedPlan<'b, T>],
    base: &'a PathPlan<'b, T>,
    events: Vec<Event>,
    shared: Option<SharedEvents>,
    expiry_discount: f64,
}

struct SharedEvents {
    events: Vec<(usize, SharedAction)>,
    intervals: Vec<SharedInterval>,
    daily: Vec<[f64; 2]>,
    barriers: Vec<(f64, usize)>,
}

enum SharedAction {
    Cash(Event),
    Maturity(f64),
}

struct SharedInterval {
    step: usize,
    end: usize,
    drift: f64,
    diffusion: f64,
}

impl<T: AutocallableTerms> PathPlan<'_, T> {
    fn shared_daily_grid(&self) -> bool {
        self.shared_events.is_some() || self.steps.iter().any(|step| step.observe_knock_in)
    }

    fn same_daily_grid(&self, other: &Self) -> Result<bool> {
        if Arc::ptr_eq(&self.steps, &other.steps) {
            return Ok(true);
        }
        if self.steps.len() != other.steps.len() {
            let aligned = |plan: &Self| {
                plan.shared_events
                    .as_ref()
                    .is_some_and(|events| events.maturity_on_grid)
            };
            if !aligned(self)
                || !aligned(other)
                || self.note.calendar() != other.note.calendar()
                || self.note.observation_clock_time() != other.note.observation_clock_time()
            {
                return Ok(false);
            }
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
    fn new<T: AutocallableTerms>(
        notes: &[IndexedPlan<'_, T>],
        base: &PathPlan<'_, T>,
    ) -> Result<Self> {
        let steps = &base.steps;
        let mut ends = Vec::with_capacity(steps.len());
        let mut events = Vec::new();
        let mut buckets = Vec::with_capacity(steps.len());
        let mut barriers = Vec::with_capacity(notes.len());
        buckets.resize_with(steps.len(), Vec::new);
        for (lane, (_, note)) in notes.iter().enumerate() {
            check_path_step(lane)?;
            if let Some(level) = note.knock_in.filter(|_| !note.state.knocked_in) {
                barriers.push((level, lane));
            }
            if let Some(sparse) = &note.shared_events {
                for (i, &(step, event)) in sparse.events.iter().enumerate() {
                    check_path_step(i)?;
                    buckets[step].push((
                        lane,
                        SharedAction::Cash(Event::new(event, steps[step].transition[2])),
                    ));
                }
            } else {
                for (i, (step, bucket)) in note.steps.iter().zip(&mut buckets).enumerate() {
                    check_path_step(i)?;
                    if let Some(event) = step.event {
                        bucket.push((
                            lane,
                            SharedAction::Cash(Event::new(event, step.transition[2])),
                        ));
                    }
                }
            }
            let maturity = note.steps.len() - 1;
            buckets[maturity].push((lane, SharedAction::Maturity(steps[maturity].transition[2])));
        }
        for (step, mut bucket) in buckets.into_iter().enumerate() {
            check_path_step(step)?;
            events.append(&mut bucket);
            ends.push(events.len());
        }
        let mut excursion = 0.0;
        for (index, step) in steps.iter().enumerate() {
            check_path_step(index)?;
            excursion += step.transition[0].abs() + 20.0 * step.transition[1];
        }
        let bridge =
            base.safe_returns.contains(&excursion) && base.safe_returns.contains(&-excursion);
        let mut intervals = Vec::new();
        let mut daily = Vec::new();
        let mut first = 0;
        let mut previous = 0;
        for (step, end) in ends.into_iter().enumerate() {
            check_path_step(step)?;
            if end == previous && step + 1 < steps.len() {
                continue;
            }
            if bridge && step - first >= 8 {
                if daily.is_empty() {
                    daily.resize(steps.len(), [0.0; 2]);
                }
                let [mut drift, width, _] = steps[step].transition;
                let mut variance = width * width;
                for index in (first..step).rev() {
                    check_path_step(index)?;
                    let [mean, width, _] = steps[index].transition;
                    let next = width.mul_add(width, variance);
                    daily[index] = [(width * width) / next, width * (variance / next).sqrt()];
                    drift += mean;
                    variance = next;
                }
                let diffusion = variance.sqrt();
                if diffusion.is_normal()
                    && drift.is_finite()
                    && daily[first..step]
                        .iter()
                        .all(|&[weight, width]| weight > 0.0 && width > 0.0)
                {
                    intervals.push(SharedInterval {
                        step,
                        end,
                        drift,
                        diffusion,
                    });
                    first = step + 1;
                    previous = end;
                    continue;
                }
            }
            for (index, transition) in steps.iter().enumerate().take(step + 1).skip(first) {
                check_path_step(index)?;
                let [drift, diffusion, _] = transition.transition;
                intervals.push(SharedInterval {
                    step: index,
                    end: if index == step { end } else { previous },
                    drift,
                    diffusion,
                });
            }
            first = step + 1;
            previous = end;
        }
        barriers.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        crate::cancellation::check()?;
        Ok(Self {
            events,
            intervals,
            daily,
            barriers,
        })
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
            let base = notes
                .iter()
                .map(|(_, plan)| plan)
                .max_by_key(|plan| plan.steps.len())
                .expect("the portfolio has a base note");
            return Ok(Self {
                notes,
                base,
                events: Vec::new(),
                shared: Some(SharedEvents::new(notes, base)?),
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
            base,
            events,
            shared: None,
            expiry_discount,
        })
    }

    fn price(&self, settings: MonteCarloSettings, prices: &mut [f64]) -> Result<()> {
        let base = self.base;
        let dimensions = base.steps.len();
        let blocks = PathBlocks::new(settings, dimensions)?;
        let lanes = self.notes.len();
        let mut partials = vec![Moments::default(); blocks.count() * lanes];
        let mut blocks_out = partials.chunks_exact_mut(lanes).collect::<Vec<_>>();
        let simulate = |block, moments: &mut [Moments]| {
            let mut states = Vec::with_capacity(lanes);
            let mut live = Vec::with_capacity(lanes);
            blocks.sample(
                block,
                base.bridge.is_some() || self.shared.is_some(),
                |draws, paired| {
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
                },
            )
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
        let base = self.base;
        let mut log_return = 0.0;
        let mut minimum = f64::INFINITY;
        let mut first = 0;
        let mut first_step = 0;
        let mut remaining = states.len();
        let mut barriers = schedule.barriers.as_slice();
        for &SharedInterval {
            step,
            end,
            drift,
            diffusion,
        } in &schedule.intervals
        {
            check_path_step(step)?;
            let target = log_return + diffusion.mul_add(normals.normal(), drift);
            if first_step < step {
                let floor = live_floor(&mut barriers, states)?;
                for i in first_step..step {
                    if minimum < floor || floor == f64::INFINITY {
                        normals.skip(step - i);
                        break;
                    }
                    check_path_step(i)?;
                    let [weight, diffusion] = schedule.daily[i];
                    log_return += weight.mul_add(target - log_return, diffusion * normals.normal());
                    if base.steps[i].observe_knock_in {
                        minimum = minimum.min(log_return);
                    }
                }
            }
            log_return = target;
            base.validate_return(log_return)?;
            if base.steps[step].observe_knock_in {
                minimum = minimum.min(log_return);
            }
            for (event_index, (index, action)) in schedule.events[first..end].iter().enumerate() {
                check_path_step(event_index)?;
                let lane = &mut states[*index];
                if lane.terminated {
                    continue;
                }
                let terminated = match action {
                    SharedAction::Cash(event) => event.pay(lane, log_return)?,
                    SharedAction::Maturity(discount) => {
                        let plan = &self.notes[*index].1;
                        let floor = if plan.expiry_knock_in {
                            minimum.min(log_return)
                        } else {
                            minimum
                        };
                        knock_in(lane, floor);
                        lane.cash +=
                            plan.discounted_terminal(log_return, lane.knocked_in, *discount)?;
                        if !lane.cash.is_finite() {
                            return Err(unstable());
                        }
                        true
                    }
                };
                if terminated {
                    lane.terminated = true;
                    remaining -= 1;
                }
            }
            if remaining == 0 {
                return Ok(());
            }
            first = end;
            first_step = step + 1;
        }
        Err(unstable())
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

fn live_floor(barriers: &mut &[(f64, usize)], states: &[Lane]) -> Result<f64> {
    let mut skipped = 0;
    while let Some((&(level, lane), rest)) = barriers.split_first() {
        if !states[lane].terminated {
            return Ok(level);
        }
        check_path_step(skipped)?;
        skipped += 1;
        *barriers = rest;
    }
    Ok(f64::INFINITY)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_barrier_cursor_advances_when_lowest_note_terminates() -> Result<()> {
        let barriers = [(-0.4, 2), (-0.3, 0), (-0.2, 1)];
        let mut remaining = barriers.as_slice();
        let mut states = std::array::from_fn::<_, 3, _>(|_| Lane {
            cash: 0.0,
            monitoring: None,
            knocked_in: false,
            terminated: false,
        });
        states[1].terminated = true;
        assert_eq!(
            live_floor(&mut remaining, &states)?.to_bits(),
            (-0.4_f64).to_bits()
        );
        states[2].terminated = true;
        assert_eq!(
            live_floor(&mut remaining, &states)?.to_bits(),
            (-0.3_f64).to_bits()
        );
        assert_eq!(remaining.len(), 2);
        states[0].terminated = true;
        assert_eq!(
            live_floor(&mut remaining, &states)?.to_bits(),
            f64::INFINITY.to_bits()
        );
        assert_eq!(remaining, []);
        Ok(())
    }
}
