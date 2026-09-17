# Static performance analysis

Date: 2026-09-17. Inspected revision: `113db152e524863bfd21b1b9601e24cf78fdd53f`.

This report uses source inspection only. No benchmarks, profiling, builds, or tests were set up or executed. Operation counts and storage estimates below follow from the code; they are not measured speedups. Confidence refers to identifiable redundant work, not its share of application runtime. Production code was not changed.

Inspection focused on pricing engines, shared numerical helpers, calendars, numerical analytics, scenario and implied solvers, and their Python entry points. Searches covered engine loops and allocations; detailed tracing concentrated on Monte Carlo and finite-difference paths. Existing tests were inspected for relevant contracts. This is a targeted performance review, not an exhaustive correctness audit.

## Prioritized opportunities

| Priority | Opportunity | Statically identifiable benefit | Implementation scope |
| --- | --- | --- | --- |
| 1 | Retain only European Monte Carlo terminal values | Simulation storage falls from O(P S) to O(P) | Vanilla simulation helper and European caller |
| 2 | Prepare structured Monte Carlo schedules once per price | Removes per-path date enumeration, schedule allocation, and repeated time calculations | Structured and accumulator engines; calendar caveat below |
| 3 | Index structured finite-difference events | Replaces repeated linear event scans with logarithmic lookups | Structured finite-difference engine |
| 4 | Reuse the matrix factorization across paired finite-difference layers | Removes one coefficient assembly and factorization per paired step | Unconstrained two-layer solver path |
| 5 | Eliminate a duplicate finite-difference finiteness scan | Removes an interior-vector traversal per implicit step | Shared finite-difference helper |
| 6 | Seed structured engines lazily | Avoids entropy-source construction and invocation for explicit seeds | Two Monte Carlo entry points |

Here P is the effective path count, S the number of simulation grid points, N the number of interior asset nodes, G the actual number of finite-difference intervals, D the number of calendar dates examined, and O the observation count. Priority balances scope and identifiable work; it is not a measured ranking.

### 1. Retain only terminal values for European Monte Carlo

**Evidence:** [vanilla/monte_carlo.cpp](../src/pricing/engines/vanilla/monte_carlo.cpp), `simulate_paths` (lines 29–83) and `price_european` (lines 129–148). The simulator allocates `path_count * step_count` doubles and writes every step. The European caller reads only the last value in each row. Its sibling `price_american` genuinely needs intermediate values for regression, so changing both callers to terminal-only storage would be incorrect.

**Recommendation:** Keep the existing grid, antithetic pairing, and random draws, but allow the European path to retain only P terminal values. Keep the shared simulation logic and validation in the core rather than duplicating them in a second implementation. Aggregate terminal payoffs in the existing order: all positive paths followed by all negative paths.

With the [default settings](../include/kiyosi/pricing/settings/monte_carlo.hpp) of 100,000 paths and 50 grid points, the path payload is currently 5,000,000 doubles: 40,000,000 bytes on an eight-byte-double platform. Terminal storage would be 800,000 bytes. These are allocation-size calculations, excluding allocator overhead. Simulation arithmetic remains O(P S), but intermediate storage writes and the later full-matrix validation read can disappear.

**Preserve:** Check every generated intermediate spot for finiteness and positivity even when it is no longer stored. Preserve odd-path rounding, step-count validation, expiry behavior, random draw order, and the existing summation order. A one-draw terminal-distribution shortcut would also reduce arithmetic, but would change seeded results and the meaning of `step_count`; it is outside this recommendation.

**Correctness checks for implementation:** Extend [vanilla Monte Carlo tests](../tests/pricing/engines/vanilla/monte_carlo_tests.cpp) with odd/even path counts and multiple step counts, comparing the same seeded stream to the old full-path calculation. Retain American regression and immediate-exercise coverage, plus extreme-input rejection checks.

### 2. Prepare structured Monte Carlo schedules and time factors once

**Evidence:** [structured/monte_carlo.cpp](../src/pricing/engines/structured/monte_carlo.cpp), `path_payoff` (lines 18–68) is called inside the path loop at lines 89–90. It constructs `observation_schedule` for each path and calls `trading_dates` inside that path. [accumulator/monte_carlo.cpp](../src/pricing/engines/accumulator/monte_carlo.cpp), lines 14–49 and 68–69, repeats trading-date construction in the same way. [calendar_dates.hpp](../src/pricing/detail/calendar_dates.hpp), lines 12–21, walks the entire calendar range and builds a vector before path traversal begins, even if that path later knocks out early. [autocallable_traits.hpp](../src/pricing/detail/autocallable_traits.hpp), `observation_schedule`, scans all observation dates and allocates another vector.

**Recommendation:** After existing validation, prepare the observation indices and, for stable calendars, trading dates once per pricing invocation. Precompute each interval's year fraction, drift, volatility times square root of time, and discount factor. Pass these local immutable values into the payoff loop. Preserve the existing product-specific settlement rules in the traits.

For paths reaching the simulation loop, calendar enumeration drops from O(P D) predicate calls to O(D), and structured schedule construction from O(P O) to O(O). Time factors become O(D) preparation instead of repeated calculations on every simulated path. Random evolution and payoff evaluation remain proportional to the total number of visited path steps. The default structured path count is 20,000, so repeated setup is present in the default configuration.

**Calendar qualification:** [TradingCalendar](../include/kiyosi/market/calendar.hpp) accepts an arbitrary C++ `std::function` predicate. Its documented concurrency requirements do not establish purity or stable return values. Hoisting queries is safe for built-in deterministic calendars, but changes callback counts and potentially results for stateful custom calendars. Establish a stable-calendar contract or preserve a fallback for such calendars before applying this universally. Observation-index preparation alone does not have that caveat.

**Preserve:** Midnight-only valuation events, intraday first intervals, holiday gaps, empty future schedules, early knockout, and expiry handling. In particular, retain the current per-path lifetime of `std::normal_distribution`: moving it outside the path loop can carry a cached draw across paths and change seeded outcomes. Avoid generating additional draws after early termination. Keep preparation local to each price call to avoid cache invalidation and shared mutable state.

**Correctness checks for implementation:** Use [structured tests](../tests/pricing/engines/structured/autocallable_tests.cpp), [accumulator tests](../tests/pricing/engines/accumulator/engine_tests.cpp), and [intraday architecture tests](../tests/pricing/analytics/architecture_tests.cpp). Add seeded before/after comparisons for early knockout and holiday gaps. A counting calendar with stable answers can verify reduced query counts without timing anything.

### 3. Replace repeated structured finite-difference event scans

**Evidence:** [structured/finite_difference.cpp](../src/pricing/engines/structured/finite_difference.cpp), lines 112–125, defines `event_index` using `find_if` over observation indices and `daily_event` using `any_of` over trading dates. Both recalculate `actual_365` while searching. Lines 156–157 invoke these searches at every backward time step. For daily-monitoring products this adds O(G (O + D)) search work, separate from the O(G N) PDE work.

**Recommendation:** Prepare sorted observation times with their original indices and sorted trading times once, then use `lower_bound` and `binary_search`. The existing [accumulator finite-difference engine](../src/pricing/engines/accumulator/finite_difference.cpp), lines 76–83 and 105, already demonstrates the trading-times approach. This reduces lookup work to O(G (log O + log D)) after linear preparation and removes date conversion from the searches. A reverse merge cursor could make lookup linear overall, but binary search is the smaller established solution.

**Preserve:** Dates are strictly ordered by [schedule validation](../include/kiyosi/market/schedule.hpp). Use the same year-fraction calculation and exact equality semantics as today; retain original indices into coupon and knockout arrays. The grid includes inserted event anchors and may contain more than `settings.time_steps` intervals. Keep valuation-time and expiry events, and do not introduce approximate date matching.

**Correctness checks for implementation:** Exercise daily versus expiry-only monitoring, intraday valuation, and observation anchors between uniform grid points using the existing structured refinement and architecture cases.

### 4. Factor once for two finite-difference right-hand sides

**Evidence:** [structured/finite_difference.cpp](../src/pricing/engines/structured/finite_difference.cpp), lines 147–155, advances knocked-in and alive layers consecutively with the same `LinearBoundaryStepper` and `dt`. [accumulator/finite_difference.cpp](../src/pricing/engines/accumulator/finite_difference.cpp), lines 97–103, does the same for slope and intercept. Each call to [FiniteDifferenceStep::advance](../src/pricing/detail/fd_scheme.hpp), lines 36–82, reconstructs the tridiagonal coefficients and performs Thomas elimination. In these unconstrained calls the matrix depends on grid size, `dt`, rate, dividend, volatility, and theta, not on the value layer. Layer-specific boundary values affect the right-hand side.

**Recommendation:** Provide a small internal paired-layer operation that assembles and factors the matrix once for the current step, then solves both right-hand sides. Retain the existing single-layer constrained operation. Store elimination factors alongside the factored diagonal so the second right-hand side can use the same arithmetic factors.

This removes one O(N) coefficient assembly and matrix factorization per paired implicit or Crank–Nicolson step. Both O(N) right-hand-side constructions and solves remain; total complexity stays O(G N). Explicit Euler has no factorization to reuse. This is a larger change than findings 3, 5, and 6 and should remain narrowly scoped.

**Preserve:** Separate extrapolated boundaries, pivot/finiteness checks, event application order, and error categories. Reuse within a paired step avoids assuming successive `dt` values are identical: event insertion and floating-point subtraction can produce different intervals. Do not reuse a matrix across changing constraint masks in barrier engines or across shifted market contexts.

**Correctness checks for implementation:** Compare paired and independent advances for all three schemes, nonuniform intervals, and differing layer boundaries; retain structured and accumulator refinement and instability-rejection cases.

### 5. Remove the redundant implicit-step finiteness traversal

**Evidence:** [fd_scheme.hpp](../src/pricing/detail/fd_scheme.hpp), lines 80–82, scans all of `rhs` for finiteness, copies it unchanged into the interior of `next`, then scans all of `next` for finiteness. No arithmetic occurs between the two scans. Thus every interior value is checked twice on successful implicit steps.

**Recommendation:** Keep the `rhs` scan and validate only `next.front()` and `next.back()` after copying. This retains early return before copying an invalid interior solution, while removing N redundant checks on successful steps. The explicit-Euler branch has only one scan and should keep it.

This applies through `march_backward` to vanilla, digital, and barrier engines, and through `LinearBoundaryStepper` to structured and accumulator engines. Buffer allocation is already reused across steps; adding another allocation cache is unnecessary.

**Preserve:** All pivot checks and boundary checks. The observation is about duplicate validation of identical values, not permission to remove validation of numerical results.

**Correctness checks for implementation:** Check finite interiors with invalid lower/upper boundaries, invalid interior solutions, and stable results across the shared callers.

### 6. Avoid eager entropy acquisition when a seed exists

**Evidence:** [structured/monte_carlo.cpp](../src/pricing/engines/structured/monte_carlo.cpp), line 87, and [accumulator/monte_carlo.cpp](../src/pricing/engines/accumulator/monte_carlo.cpp), line 66, use `settings_.seed.value_or(std::random_device{}())`. The argument is evaluated before `value_or`, even when the optional contains a seed. Structured settings default to seed 1, so default calls perform an unnecessary entropy-source operation.

**Recommendation:** Select lazily, for example `settings_.seed ? *settings_.seed : std::random_device{}()`. The vanilla simulator already uses an explicit seeded/unseeded branch. Keep the existing single-draw unseeded behavior in these engines rather than silently adopting vanilla's different seed-sequence construction.

This removes one entropy-source construction and draw per seeded pricing invocation, not per path. It is a small, certain work reduction; its runtime significance is unknown. It also removes an unnecessary dependency on entropy-source availability for deterministic pricing.

**Correctness checks for implementation:** Preserve exact seeded outputs for seed zero and the maximum `uint64_t` value, and verify the absent-seed path remains usable. Do not assert that two unseeded prices must differ.

## Additional opportunity requiring a semantic decision

[scenario.hpp](../include/kiyosi/pricing/scenario.hpp), `scenario_grid` (lines 59–85), calls full `numerical_analytics` for each spot and retains only price, delta, and gamma. [numerical_greeks.hpp](../include/kiyosi/pricing/numerical_greeks.hpp) makes 19 engine-price calls on its successful full path: five spot points, six volatility-related points, two rate points, and six time-related points. Only the base spot and its two immediate bumps are required for the three scenario columns.

A core spot-only calculation could reduce calls from 19 to 3 per successful scenario row. This is an operation-count reduction, not a promise of a 19/3 runtime speedup. Python's scenario binding routes through this same core function.

Do not present this as a transparent substitution. Today scenario calculation can fail because an unused volatility bump is invalid, a time shift is unavailable, or an unused Greek is non-finite. Skipping those calculations changes accepted inputs and observable errors. Generic user-supplied engines may also be stateful, so removing calls changes their invocation sequence. Decide and document whether scenario grids should promise only spot sensitivities, retain appropriate core validation, and cover the chosen behavior in both languages. Existing [architecture tests](../tests/pricing/analytics/architecture_tests.cpp) and [Python scenario tests](../tests/python/test_kiyosi.py) are starting points, not proof of unchanged failure semantics.

## Boundaries of these recommendations

- Keep domain validation, defaults, and optimized numerical work in C++; Python remains an adapter. Public copies should not become borrowed views merely to avoid allocation without the required lifetime guarantees.
- Do not add cross-call pricing caches or parallel paths based on this inspection. Custom calendars, seeded random streams, shifted contexts, and concurrency contracts need explicit treatment first.
- Do not reduce path counts, grid resolution, solver iterations, or tolerances as a performance fix; those change requested numerical behavior.
- American Monte Carlo's strided regression reads, algebraic simplifications in analytic engines, and alternative implied solvers may merit future investigation, but static inspection alone does not justify a layout rewrite or algorithm replacement here.
- The correctness checks above are proposed follow-up work. None were executed for this report, and no empirical performance claims are made.
