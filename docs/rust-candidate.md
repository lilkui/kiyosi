# Rust pricing and analytics candidate

Tickets 01–35 provide an isolated candidate for European vanilla/digital, American approximation, CRR, quadrature, finite-difference and vanilla/digital/Asian/single-barrier/double-barrier/autocallable CPU Monte Carlo prices and analytics with
flat BSM rates or configured risk-free and dividend term structures, continuous-average Asian analytics, trading
calendars, observation schedules and single/double/binary-barrier and touch analytic pricing with event history, plus single-barrier CRR, quadrature and finite differences, binary-barrier/single-touch finite differences, double-barrier finite differences, cash double-touch/no-touch finite differences, analytic range-accrual coupons, and snowball variants/presets and Phoenix quadrature/finite differences with implied-coupon solving, plus accumulator quadrature/finite differences. The root
`pyproject.toml`, CMake delivery, and legacy Python package remain usable. The
candidate also installs as `kiyosi`: use separate virtual environments or
processes. There is no runtime implementation selector.

The workspace version in `Cargo.toml` supplies both crates and Python's
`__version__` and wheel metadata. It currently matches the legacy version.
The workspace defaults to the core; ordinary `cargo build`, `cargo test`, and
`cargo doc` never build PyO3 or discover Python.

## Rust API

Requires Rust 1.85 or newer. A downstream crate depends on
`kiyosi-core = { path = "/path/to/kiyosi/rust/core" }`.

```rust
use kiyosi_core::{
    AnalyticVanillaEngine, BlackScholesMertonParameters, Date, EuropeanOption, OptionType,
    PricingContext,
};

fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let expiry = Date::new(2026, 1, 1)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, expiry)?;
    let model = BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?;
    let context = PricingContext::new(model, 100.0, start.into())?;
    let price = AnalyticVanillaEngine.price(&option, &context)?;
    assert!((price - 9.227005508154036).abs() < 1e-12);
    Ok(())
}
```

`Date::new` validates Gregorian components and the existing core year range
(-32767 through 32767). `Timestamp::new` adds validated intraday components with
microsecond resolution. Time to expiry uses Actual/365 Fixed. A context is valid
independently of a contract: pricing checks that valuation lies between the
effective date at midnight and expiry at midnight, inclusive. At expiry the price
is intrinsic value; even one microsecond after expiry is rejected.

Strike, spot, and volatility must be finite and strictly positive; both rates
must be finite and may be negative. Constructors validate before returning a
value. Accessors borrow or copy from owned data. `PricingContext::new` uses
`TradingCalendar::default()` (weekdays, annualization 252); the Python default is
derived from the same core value. Other exotic instruments and engines arrive in later tickets.
The vanilla engine accepts only `EuropeanOption`: unsupported instrument types are
excluded by Rust's signature and Python's native argument conversion.

## Greeks and implied volatility

Rust `AnalyticVanillaEngine::price_with_greeks` accepts a `GreeksRequest` and
`NumericalShiftSettings`. `GreeksRequest::new([Greek::Delta, Greek::Vega])`
selects measures, ignoring duplicates; `GreeksRequest::all()` selects all ten.
Empty requests return `InvalidParameter` before pricing. Every `PricingResult`
owns a finite price and optional finite Greeks. `get` returns `Option<f64>`,
`has` reports availability, and `require` returns `InvalidResult` for unavailable
measures. `values()` borrows immutable storage for the result's lifetime.
Unrequested or undefined measures are absent, including every Greek at expiry.
A zero derivative is an available value.

The analytic engine computes all ten Greeks directly. Delta, gamma and speed
are first, second and third spot derivatives. Vega, vanna and zomma are changes
in price, delta and gamma per volatility percentage point (0.01); rho is price
change per interest-rate percentage point, shifting the full risk-free curve in
parallel. Theta, charm and color are changes in price, delta and gamma per
calendar day of forward valuation time. Curve references and nodes stay fixed;
time derivatives use right-hand instantaneous rates at knots. Requested
non-finite derivatives fail the operation; unrequested derivatives cannot spoil
an otherwise finite result. Tail density weights are applied in log space.

The core exposes `numerical_greeks` for scalar implementations of
`PricingEngine`. Absolute bump defaults are spot 0.01, volatility
0.0001, rate 0.0001 and time one calendar day. All must be positive (and numeric
bumps finite), even when the analytic engine does not use them. Numerical
stencils use representable symmetric spacing; collapsed, asymmetric or
nonpositive stencils leave affected measures absent. Time stencils stay within
contract life, curve coverage and neighboring knots, using one-sided differences
at boundaries. Feasible bumped pricing failures fail the whole operation.
Finite bumps introduce truncation and rounding error; compare step sizes,
especially near expiry. Python exposes the same shift validation contract;
the vanilla analytic engine always returns analytic derivatives.

```rust
use kiyosi_core::{AnalyticVanillaEngine, BlackScholesMertonParameters, Date,
    EuropeanOption, OptionType, PricingContext, Greek, GreeksRequest,
    NumericalShiftSettings, ImpliedVolatilitySettings, implied_volatility};
fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let option = EuropeanOption::new(OptionType::Call, 100.0, start, Date::new(2026, 1, 1)?)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    let result = AnalyticVanillaEngine.price_with_greeks(&option, &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]), NumericalShiftSettings::default())?;
    assert!(result.require(Greek::Delta)? > 0.0);
    let sigma = implied_volatility(&AnalyticVanillaEngine, &option, &context,
        result.price(), ImpliedVolatilitySettings::default())?;
    assert!((sigma - 0.2).abs() < 1e-8);
    Ok(())
}
```

`implied_volatility` reuses the scalar engine boundary and safeguarded Brent
interpolation with bisection fallback. Defaults are bounds `[0.0001, 4.0]`,
absolute price tolerance `1e-8`, absolute volatility-interval tolerance `1e-8`
and 100 iterations, excluding endpoint evaluations. Either tolerance can stop
iteration. Bounds must be finite, positive and ordered, tolerances finite and
positive, and the iteration limit positive. Observed prices must be finite;
finite unattainable quotes, including negative prices, return
`UnbracketedVolatility`. Settings and quote validation precede endpoint pricing;
pricing validates the contract and curve horizon before expiry returns
`UnsupportedOperation` because volatility is unidentifiable. Endpoint matches
return their bounds. An exhausted solve returns `SolverNonConvergence`;
non-finite scalar prices or residuals return `SolverNonFinite`. Engine domain
failures propagate, including `InvalidResult` from analytic pricing. Analysis
and solving preserve the original market and engine on success and failure.

Python exports the `Greek` Literal choice alias and owning `PricingResult` at
package level. Results have read-only Greek properties and `require(name)`.
Request one lowercase name, an iterable of names, or `all_greeks=True`,
exclusively. Missing requests, wrong protocols and conflicting forms raise
native `TypeError`; unknown names raise `ValueError`; empty iterables reach core
validation. Numeric controls use the strict real-number protocol below.
`time_shift_days` and `max_iterations` use the integer index protocol, excluding
booleans and floats, with native `OverflowError` outside signed 32-bit range.
Representation errors precede domain validation. Defaults come from Rust.

```python
from kiyosi.pricing import AnalyticVanillaEngine, implied_volatility

engine = AnalyticVanillaEngine()
result = engine.price_with_greeks(option, context, ["delta", "vega"])
assert result.gamma is None
sigma = implied_volatility(engine, option, context, result.price)
assert abs(sigma - context.model_parameters.volatility) < 1e-8
```

## Digital contracts and analytic pricing

Rust `DigitalOption::cash_or_nothing(direction, strike, payout, start, end)` and
`DigitalOption::asset_or_nothing(direction, strike, start, end)` own validated
European contracts. Cash payout is required, finite and strictly positive; invalid
payout returns `InvalidParameter` before strike and date validation. Strike and
life dates follow the vanilla contract rules. `payout()` returns `Some(amount)`
for cash and `None` for asset settlement. Both settle only **strictly** in the
money: a call needs spot greater than strike, a put needs spot less than strike,
and equality at expiry pays zero. Expiry is midnight UTC; all Greeks are absent
at expiry, while implied volatility returns `UnsupportedOperation`.

`AnalyticDigitalEngine` accepts only `DigitalOption`. It computes price, delta
and gamma analytically, applying discounts, settlement amounts and normal-tail
weights in log space when necessary. Non-finite prices or requested derivatives
return `InvalidResult`; unrequested derivatives cannot spoil finite results.
Other requested Greeks use the shared finite-difference completion and the same
Rust `NumericalShiftSettings` defaults as vanilla numerical analysis. Speed
differences native gamma; vanna/zomma and charm/color use native delta/gamma when
both stencil legs provide the measure. If either lacks it, both legs use scalar
differences, including time bumps that reach expiry. A speed stencil needs only positive symmetric ±h spots when native
gamma is available. Pure scalar `numerical_greeks` still requires ±2h for speed.
Collapsed or infeasible stencils leave affected measures absent. Bumped pricing
errors fail the operation. Finite-difference accuracy depends on the bump and
distance to expiry; the one-day QuantLib scenarios validate price and native
delta/gamma under the existing acceptance criteria.

`OptionContract` exposes inclusive life dates without changing payoff semantics.
`PricingEngine<I>` selects supported instrument types, defaulting to
`EuropeanOption`; the shared `numerical_greeks` and `implied_volatility` also
accept digital contracts. Digital prices can be non-monotone in volatility.
The solver retains the caller's bracket, may select one of several roots, and
returns `UnbracketedVolatility` when the endpoint residuals have the same sign,
even if the quote is attainable elsewhere. No root search or new algorithm is added.

```rust
use kiyosi_core::{AnalyticDigitalEngine, BlackScholesMertonParameters, Date,
    DigitalOption, DigitalOptionChanges, OptionType, PricingContext,
    Greek, GreeksRequest, ImpliedVolatilitySettings, implied_volatility};

fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let option = DigitalOption::cash_or_nothing(OptionType::Call, 100.0, 10.0,
        start, Date::new(2026, 1, 1)?)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.02, 0.2)?, 100.0, start.into())?;
    let result = AnalyticDigitalEngine.price_with_greeks(&option, &context,
        GreeksRequest::new([Greek::Delta, Greek::Vega]), Default::default())?;
    assert!((result.price() - 4.945810910532236).abs() < 1e-12);
    let sigma = implied_volatility(&AnalyticDigitalEngine, &option, &context,
        result.price(), ImpliedVolatilitySettings {
            lower_bound: 0.1, upper_bound: 0.22, ..Default::default()
        })?;
    assert!((sigma - 0.2).abs() < 1e-7);
    let updated = option.replace(DigitalOptionChanges {
        payout: Some(7.0), ..Default::default()
    })?;
    assert_eq!(updated.payout(), Some(7.0));
    assert_eq!(option.payout(), Some(10.0));
    Ok(())
}
```

Python exports `CashOrNothingOption` and `AssetOrNothingOption` from
`kiyosi.instruments`, and `AnalyticDigitalEngine` from `kiyosi.pricing`.
Constructors retain the existing keyword-only arguments; cash also requires
`payout`. Numeric and date representations use the strict protocols described
below. Analytic price and Greek calls retain the GIL; implied-volatility solves
release it using owned Rust inputs.

```python
from datetime import date
from kiyosi.instruments import CashOrNothingOption, AssetOrNothingOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticDigitalEngine, implied_volatility

terms = dict(option_type="call", strike=100,
             effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
cash = CashOrNothingOption(**terms, payout=10)
asset = AssetOrNothingOption(**terms)
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2),
    spot_price=100, valuation_time=date(2025, 1, 1))
engine = AnalyticDigitalEngine()
result = engine.price_with_greeks(cash, context, ["delta", "vega"])
assert abs(result.price - 4.945810910532236) < 1e-12
assert abs(engine.price(asset, context) - 58.6851146134764) < 1e-12
sigma = implied_volatility(engine, cash, context, result.price,
                           lower_bound=0.1, upper_bound=0.22)
assert abs(sigma - 0.2) < 1e-7
assert cash.replace(payout=7).payout == 7
assert cash.payout == 10
```

Rust `DigitalOption::replace(DigitalOptionChanges)` and Python `.replace(**fields)`
return owning validated contracts; failure leaves originals intact. Omitted fields
retain their values. Settlement kind cannot change. Rust rejects a cash payout
replacement on asset settlement with `UnsupportedOperation`; Python's asset
contract has no payout parameter and rejects it with native `TypeError`.
Digital contract replacement is a candidate interface; the legacy digital API
provides construction only. Python properties are read-only and equality compares
contract values. Vanilla/digital engine mismatches use Rust type checking or
Python's native argument-conversion errors.

## Vanilla and digital quadrature pricing

`QuadratureVanillaEngine` supports `EuropeanOption` and `QuadratureDigitalEngine`
supports European cash-or-nothing and asset-or-nothing contracts. Both are
stateless: Rust uses unit values and Python uses no-argument constructors.
They retain the existing Simpson integration with **2,048 panels**, compensated
summation, a tail endpoint density of `exp(-72)` relative to the starting density,
and log scaling to preserve representable prices when probabilities or discount
factors underflow. Completing the square keeps high-variance asset tails in the
integration domain. Vanilla retains the existing small-width analytic
stabilization for `volatility * sqrt(time) < 1e-5` with finite log discounts to
avoid cancellation of time value. Infinite log discounts retain the quadrature
limits. No other engine fallback is selected.

These terminal engines expose no integration settings. The legacy
`QuadratureSettings` spatial grid and extent configure path-dependent engines,
including single-barrier quadrature described below. No such controls are added to terminal engines.

Both require explicitly flat rates, including dated flat curves. Node curves
return `UnsupportedOperation` before life or Greek-request validation, even at
expiry. Valuation must lie within inclusive contract life; expiry returns vanilla
intrinsic value or digital payout strictly in the money (zero at equality).
Non-finite widths, indeterminate thresholds and non-finite prices return
`InvalidResult`. Negative rates and dividends and infinite standardized-forward
limits are accepted when the resulting price is finite.

All ten requested Greeks use shared scalar finite differences with the core
`NumericalShiftSettings` defaults; no native Greeks are substituted. At expiry
all Greeks are absent. Empty requests and invalid shifts return `InvalidParameter`
after flat-rate applicability; infeasible or collapsed stencils leave measures
absent, and feasible bumped failures propagate without changing inputs. Sensitivity
accuracy depends on bump size and distance to expiry. The existing QuantLib
profiles validate smooth Greeks and one-day digital prices; the latter exclude
default-bump numerical Greeks. Public API tests also check spot-bump convergence.

`implied_volatility` uses the shared solver and the caller's bracket unchanged,
including its validation order, expiry rejection and failure-state guarantees.
Digital prices may have multiple roots or same-sign endpoints for an attainable
quote; supply a bracket around the desired root. Python uses the same strict
numeric and integer protocols, exception mapping and owning results as the other
engines. Price, Greek and solve calls release the GIL around owned Rust inputs;
shared and distinct engines support concurrent pricing. Signal handling resumes
when control returns to Python.

```python
from kiyosi.pricing import (
    QuadratureVanillaEngine, QuadratureDigitalEngine, implied_volatility,
)

engine = QuadratureDigitalEngine()
result = engine.price_with_greeks(cash, context, ["delta", "vega"])
assert abs(result.price - 4.945810910532236) < 1e-9
sigma = implied_volatility(engine, cash, context, result.price,
                           lower_bound=0.1, upper_bound=0.22)
assert abs(sigma - 0.2) < 1e-7
assert abs(QuadratureVanillaEngine().price(option, context) - 9.227005508154036) < 1e-9
```

The Rust equivalents are `QuadratureVanillaEngine.price(&option, &context)` and
`QuadratureDigitalEngine.price(&digital, &context)`. Both implement
`PricingEngine<I>` for shared numerical analysis and solving.

## Vanilla and digital CRR pricing

`CoxRossRubinsteinVanillaEngine` supports `EuropeanOption` and `AmericanOption`;
`CoxRossRubinsteinDigitalEngine` supports European cash-or-nothing and
asset-or-nothing contracts. Rust constructs engines with
`::new(BinomialSettings { step_count: 1024 })?` or `::default()`; Python uses
`CoxRossRubinsteinVanillaEngine(step_count=1024)`. The core defines the default
of **256** steps. Counts outside **1–1,000,000** return `InvalidParameter` at
construction. Settings are immutable and owned; Rust `settings()` returns a
copy and Python exposes read-only `step_count`. Python accepts integral inputs
through the existing integer protocol, rejecting booleans, floats and `None`
with `TypeError`, and values outside signed 32-bit range with `OverflowError`.
Candidate constructor settings are keyword-only.

Both engines require explicitly flat rates, including dated flat curves.
Node curves return `UnsupportedOperation` before life or Greek request validation,
including at expiry. Valuation must lie within inclusive contract life.
Before expiry the risk-neutral probability must be finite and in `[0, 1]`;
collapsed up/down factors, overflowing or underflowed asset trees and non-finite
values return `InvalidResult`. Negative rates, dividends and carry are allowed
when the tree remains admissible. CRR uses the existing recombining tree,
American early exercise and direct European binomial expectation for larger
grids; no smoothing or alternative tree is selected. Digital terminal nodes
settle strictly in the money, including zero payout at the central strike node.

Both engines return requested node delta and gamma. Digital gamma is absent
below two steps, and all other digital Greeks remain absent. Digital shifts are
validated but unused. Vanilla completes missing requested Greeks with the shared
numerical analysis, including one-step gamma. Speed and mixed derivatives use
native node delta/gamma where available. Immediate American exercise returns
payoff delta and zero gamma. All expiry Greeks are absent. Finite differences
can cross the tree's admissible domain; feasible bumped failures propagate and
leave inputs unchanged. `PricingEngine<I>::price_with_greeks` provides the same
engine-specific analysis through the generic Rust boundary; scalar-only custom
engines default to shared finite differences.

Implied volatility intersects the supplied bracket with
`volatility > abs((r - q) * dt) / sqrt(dt)`, rounding the boundary upward by one
representable value. An empty intersection returns `UnbracketedVolatility`.
The remaining solver validation, tolerances, expiry behavior, digital multiple-root
limitations and failure-state guarantees are shared with analytic pricing.
The original context's volatility does not need to be tree-admissible for solving.

```python
from kiyosi.pricing import (
    CoxRossRubinsteinVanillaEngine, CoxRossRubinsteinDigitalEngine, implied_volatility,
)

tree = CoxRossRubinsteinVanillaEngine(step_count=1024)
result = tree.price_with_greeks(option, context, ["delta", "vega"])
sigma = implied_volatility(tree, option, context, result.price)
digital_tree = CoxRossRubinsteinDigitalEngine()
digital_result = digital_tree.price_with_greeks(cash, context, all_greeks=True)
assert digital_result.delta is not None
assert digital_result.vega is None
```

CRR calls are synchronous. Python price and Greek calls retain the GIL; solves
release it around owned Rust inputs. Distinct and shared instances support the
library-wide concurrency contract. Large American trees use quadratic rollback;
European grids use direct expectations when their discounted terminal bound is
finite. No performance gate is added.

## American contracts and approximation pricing

`AmericanOption::new(direction, strike, start, end)` owns vanilla terms with
exercise permitted at any time from the effective date through expiry at midnight
UTC, inclusive. Positive finite strike and ordered dates use the same core
validation as European vanilla terms. Equal dates are valid. Rust
`AmericanOption::replace(AmericanOptionChanges)` and Python `.replace(**fields)`
return validated owning copies, preserving omitted fields and the original on
success or failure. Python replacement is a candidate interface; the legacy
American API provides construction only. Python properties are read-only.

`BjerksundStenslandVanillaEngine` ports the existing **2002** approximation,
including its bivariate quadrature, put-call symmetry, normalized powers and
small-volatility guards. Prices are bounded below by intrinsic and European
continuation values. Calls with nonpositive dividend yield (and symmetric puts
with nonpositive interest rate) take the existing European limit, subject to the
transformed-rate restriction below. No alternative American algorithm is selected.

The engine accepts scalar rates and explicitly flat curves, including dated flat
curves covering valuation. Node curves return `UnsupportedOperation`, even if
their nodes describe a constant rate, and even at expiry. This check precedes
contract-life validation and Greek request/shift validation. Before expiry, a
negative transformed interest rate (call: risk-free rate; put: dividend yield)
returns `UnsupportedOperation`. Exercise boundaries at or below strike also
return `UnsupportedOperation`, including the existing rejection when transformed
carry times maturity plus twice volatility times square-root maturity is
nonpositive. Unstable variance/exponents and non-finite results return
`InvalidResult`. At expiry, flat-rate contracts return intrinsic value even with
negative rates; all Greeks are absent and implied volatility is unidentifiable.

All ten supported Greeks use the shared scalar finite differences and Rust bump
defaults. There are no analytic American Greeks. Infeasible stencils leave a
measure absent; a feasible bumped price that crosses the approximation's
applicability boundary fails the whole operation. For example, rho at zero call
interest rate can fail because its negative-rate bump is unsupported. Inputs
remain unchanged. Prices and Greek calls retain the GIL; volatility solves release
it using owned Rust inputs and the library-wide concurrency contract.

The shared Rust boundaries are now `OptionContract` and `PricingEngine<I>`
(formerly `EuropeanContract` and `EuropeanPricingEngine<I>`). The instrument type
selects supported combinations; the default remains `EuropeanOption`. Numerical
analysis and volatility solving reuse the same implementations for American,
European and digital contracts. Rust signatures and Python argument conversion
reject engine/contract mismatches. The solver retains its supplied bracket:
unsupported low-volatility endpoints propagate errors, even when the target
volatility would be supported. Supply an appropriate bracket in that case.

```rust
use kiyosi_core::{AmericanOption, AmericanOptionChanges, BjerksundStenslandVanillaEngine,
    BlackScholesMertonParameters, Date, OptionType, PricingContext, Greek, GreeksRequest,
    ImpliedVolatilitySettings, implied_volatility};
fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?;
    let option = AmericanOption::new(OptionType::Put, 100.0, start, Date::new(2026, 1, 1)?)?;
    let context = PricingContext::new(
        BlackScholesMertonParameters::new(0.05, 0.03, 0.2)?, 100.0, start.into())?;
    let engine = BjerksundStenslandVanillaEngine;
    let result = engine.price_with_greeks(&option, &context,
        GreeksRequest::new([Greek::Delta]), Default::default())?;
    assert!((result.price() - 6.932464292493037).abs() < 1e-11);
    let sigma = implied_volatility(&engine, &option, &context, result.price(),
        ImpliedVolatilitySettings { lower_bound: 0.1, upper_bound: 0.4, ..Default::default() })?;
    assert!((sigma - 0.2).abs() < 1e-6);
    assert_eq!(option.replace(AmericanOptionChanges {
        strike: Some(105.0), ..Default::default()
    })?.strike(), 105.0);
    Ok(())
}
```

```python
from datetime import date
from kiyosi.instruments import AmericanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import BjerksundStenslandVanillaEngine, implied_volatility

option = AmericanOption(option_type="put", strike=100,
                        effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.05, dividend_yield=0.03, volatility=0.2),
    spot_price=100, valuation_time=date(2025, 1, 1))
engine = BjerksundStenslandVanillaEngine()
result = engine.price_with_greeks(option, context, ["delta", "vega"])
assert abs(result.price - 6.932464292493037) < 1e-11
assert abs(implied_volatility(engine, option, context, result.price,
                             lower_bound=0.1, upper_bound=0.4) - 0.2) < 1e-6
assert option.replace(strike=105).strike == 105
assert option.strike == 100
```

Existing approximation regressions retain absolute `1e-11` price tolerance.
Independent checks reuse every scenario and the Bjerksund–Stensland profile in
`tests/python/quantlib_reference/american.json`. QuantLib finite-difference grids
refine from 800 through 1600 to 3200, with independent refinement and bump-stability
checks before comparison. Approximation price tolerance is **0.15**; numerical
Greek budgets come from that profile plus measured reference uncertainty.
These tolerances measure an approximation against a refined American reference,
not exact equality to an American solution. No performance gate is added.

## Term structures and replacement

`RateTermStructure::flat(rate, reference)` creates a finite continuous annual
rate. `None` covers the full supported timestamp range; `Some(reference)` sets an
inclusive lower bound with no upper bound. `RateTermStructure::new(reference,
nodes, factors)` requires equally sized, nonempty arrays, strictly increasing
nodes after the reference, and positive finite factors. An implicit factor of one
at the reference anchors log-linear interpolation. Factors may increase when
rates are negative. Node curves have inclusive coverage from reference through
last node and never extrapolate.

`factor(start, end)` and `log_factor(start, end)` require ordered, covered
intervals. Equal endpoints return one and zero. Underflow of a factor to zero is
allowed; non-finite query results return `InvalidResult`. `forward_rate(time)`
uses the right-hand interval at knots and the left-hand interval at the last node.
`shifted(shift)` adds a finite parallel continuous-rate spread and returns an owning
curve with the same coverage. `nodes()` borrows stable storage for the curve's
lifetime; `factors()` returns an owned vector including shifts. As in the legacy
API, shifted factors can underflow or overflow even when forward rates remain
finite; interval queries still validate their numerical results.

Both model rate arguments accept `f64` or `RateTermStructure` independently.
`risk_free_curve()` and `dividend_curve()` borrow the configured curves.
The Rust scalar accessors `risk_free_rate()` and `dividend_yield()` now return
`Result<f64>`: a node curve returns `UnsupportedOperation`, even when its nodes
describe a constant rate. Python keeps its existing scalar-or-curve properties;
explicit flat curves return a scalar from these properties and an owning curve
from the `*_curve` properties.

```rust
use kiyosi_core::{
    BlackScholesMertonParameterChanges, BlackScholesMertonParameters, Date, PricingContext,
    PricingContextChanges, RateTermStructure,
};
fn main() -> kiyosi_core::Result<()> {
    let start = Date::new(2025, 1, 1)?.start_of_day();
    let end = Date::new(2026, 1, 1)?.start_of_day();
    let curve = RateTermStructure::new(start, vec![end], vec![(-0.05_f64).exp()])?;
    let model = BlackScholesMertonParameters::new(curve, 0.02, 0.2)?;
    let bumped = model.replace(BlackScholesMertonParameterChanges {
        risk_free_rate: Some(model.risk_free_curve().shifted(0.01)?.into()),
        ..Default::default()
    })?;
    let context = PricingContext::new(model, 100.0, start)?;
    let updated = context.replace(PricingContextChanges {
        model_parameters: Some(bumped),
        spot_price: Some(105.0),
        ..Default::default()
    })?;
    assert_eq!(context.spot_price(), 100.0);
    assert_eq!(updated.spot_price(), 105.0);
    Ok(())
}
```

`Rate::Flat` and `Rate::Curve` support mixed rate replacements; `From<f64>` and
`From<RateTermStructure>` let callers use `.into()`. Absent change fields retain
their values. Model replacement reuses constructor validation; context replacement
reuses context validation, including spot and valuation coverage in both curves.
All replacements return new owned values, and failures leave originals intact.
A model need not have overlapping curve coverage; context creation requires a
covered valuation. Pricing separately validates contract life and coverage through
expiry in both curves, including dated flats. Unresolvable knots after conversion
to floating-point pricing time return `InvalidResult`; timestamp queries still
preserve microsecond intervals by subtracting timestamps before conversion.
The analytic engine consumes both integrated curve rates over valuation to expiry.

## Calendars and schedules

`TradingCalendar::all_days()`, `::weekdays()` (also `::default()`) and `::sse()`
use annualization bases 365, 252 and 243 respectively. `is_trading_day` queries
a validated `Date`. `trading_days_between` counts the half-open interval
`[start, end)`; `trading_year_fraction` divides that count by the calendar's basis.
Equal endpoints return zero; reversed intervals return `InvalidTimeRange`.
`adjust(date, BusinessDayConvention::Following / Preceding)` includes the nominal
date and returns `InvalidDate` if no trading date exists before the supported
date boundary. Contracts themselves never adjust dates implicitly.

`TradingCalendar::custom(base, closed, open, basis)` owns its base and date vectors,
sorts and deduplicates overrides, and rejects overlapping open/closed dates with
`InvalidCalendar`. Weekend opening overrides are allowed. `None` inherits the
base's annualization basis; overrides never recalculate it. Nonpositive bases
are rejected at construction. Clones retain owning lifetimes and immutable data.
`TradingCalendar::new(predicate, basis)` retains Rust-only custom predicates:
closures must be `Send + Sync + 'static`, deterministic throughout each operation,
and independent of query order or count. User predicate panics remain native
panics. Built-ins compare by kind; predicate calendars compare by shared predicate
identity. `PricingContext::calendar()` now borrows; clone it to retain an owning value.

`ObservationSchedule::fixed_interval(start, end, interval_days, &calendar)`
generates `start + n * interval_days` for positive `n`.
`::monthly(start, end, first_observation_month, &calendar)` generates monthly
candidates from the positive first-month offset, anchoring to the start day and
clamping to each target month's last day. Both exclude start, include end as an
upper bound, use following adjustment, merge duplicate adjusted dates, and stop
before crossing end. End need not appear; empty schedules are valid. Reversed
life is checked before nonpositive intervals/offsets (`InvalidSchedule`).
`::new(dates, start, end, &calendar)` first validates life, then strict ordering
and inclusive bounds (`InvalidSchedule`), then trading-day status (`InvalidDate`).
`dates()` borrows the schedule's immutable storage for its lifetime; newly built
schedules own their data. Rust fixed intervals accept `i64` calendar days;
monthly offsets and annualization bases use `i32`.

```rust
use kiyosi_core::{Date, ObservationSchedule, TradingCalendar};
fn main() -> kiyosi_core::Result<()> {
    let calendar = TradingCalendar::default();
    let schedule = ObservationSchedule::fixed_interval(
        Date::new(2025, 1, 3)?, Date::new(2025, 1, 7)?, 1, &calendar,
    )?;
    assert_eq!(schedule.dates(), &[Date::new(2025, 1, 6)?, Date::new(2025, 1, 7)?]);
    Ok(())
}
```

Python exports the same calendar factories as `all_days_calendar()`,
`weekdays_calendar()`, `sse_calendar()` and keyword-only `custom_calendar(base=...,
closed_dates=(), open_dates=(), trading_days_per_year=None)`. Overrides accept
date iterables and are copied; `None` is valid only for the optional basis.
No Python predicate callbacks are exposed. Adjustment choices are the strings
`"following"` and `"preceding"`. Dates exclude datetimes, using the same stored
component conversion as contract dates. Basis, interval and month arguments accept
`numbers.Integral` with the integer index protocol, excluding bool, floats and strings, with native
`OverflowError` outside the signed 32-bit range. Representation errors occur
before core domain validation. Result conversion beyond Python's year 1–9999
range preserves Python's native date error.

```python
from datetime import date
from kiyosi.market import custom_calendar, fixed_interval_schedule, weekdays_calendar

calendar = custom_calendar(
    base=weekdays_calendar(), open_dates=[date(2026, 10, 10)],
)
schedule = fixed_interval_schedule(
    start=date(2026, 10, 9), end=date(2026, 10, 11), interval_days=1,
    calendar=calendar,
)
assert schedule.dates == [date(2026, 10, 10)]
assert calendar.trading_days_per_year == 252
```

`monthly_schedule` takes named `start`, `end`, `first_observation_month`, and an
optional calendar. Both Python builders default to Rust's default calendar.
Schedules expose read-only owning `.dates` copies, `len`, integer/negative
indexing, equality and independent lazy iterators retaining the schedule owner.
Invalid indices raise `IndexError`. Python has no direct schedule constructor.
See [SSE calendar maintenance](sse-calendar.md) for forecast limitations,
upstream sources, generator commands and licensing.

## Python candidate installation and usage

Use a dedicated environment with CPython 3.11–3.15 and a Rust toolchain:

```sh
python -m venv .venv/candidate
.venv/candidate/bin/python -m pip install './rust/python[test]'
.venv/candidate/bin/python -m unittest discover -s rust/python/tests -p 'test_*.py'
```

On Windows use `.venv\candidate\Scripts\python.exe` in place of the POSIX paths.
The [PyO3 guide](https://pyo3.rs/v0.30.1/) and
[maturin layout guide](https://www.maturin.rs/project_layout.html) describe the
binding and packaging tools. Wheels use each CPython version's ordinary ABI;
ABI3 and free-threaded builds are outside this migration's delivery contract.

```python
from datetime import date
from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine

option = EuropeanOption(
    option_type="call", strike=100,
    effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
)
model = BlackScholesMertonParameters(
    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2,
)
context = PricingContext(
    model_parameters=model, spot_price=100, valuation_time=date(2025, 1, 1),
)
assert abs(AnalyticVanillaEngine().price(option, context) - 9.227005508154036) < 1e-12
```

Constructors require keyword arguments. Choices are `"call"` and `"put"`.
Numeric fields accept `numbers.Real` (including integers) and reject booleans,
strings and complex values. Conversion errors propagate, including integer to
float overflow. Contract dates accept `datetime.date`, excluding datetimes.
Valuation accepts dates (midnight UTC) or timezone-aware datetimes normalized to
UTC. Naive datetimes are rejected. Date subclasses use stored components and
built-in UTC conversion. UTC normalization outside Python's year 1–9999 range
raises its native conversion error. No floating-point epoch conversion is used.
All exposed properties are read-only; returned model parameters, calendar and
date/time values are owned and remain valid after their source is released.
The installed package includes `py.typed` and native API stubs.

Python exposes the same curve operations and supports date/datetime iterables for
nodes and `numbers.Real` iterables for factors. Iterables are consumed and copied;
returned node/factor lists and curves own their data. Dates mean midnight UTC;
datetime normalization uses the same strict rules as valuation time. For example:

```python
from datetime import date
from kiyosi.market import RateTermStructure

curve = RateTermStructure(
    reference_time=date(2025, 1, 1),
    times=[date(2025, 6, 30), date(2026, 1, 1)],
    factors=[0.99, 0.95],
)
curved_model = model.replace(risk_free_rate=curve)
curved_context = context.replace(model_parameters=curved_model, spot_price=105)
assert context.spot_price == 100
price = AnalyticVanillaEngine().price(option, curved_context)
```

Python `.replace()` methods accept only named fields from their constructors.
Omitted fields retain their values; explicit `None` is rejected. Numeric booleans,
strings, overflow and invalid datetime representations use native conversion
exceptions before core domain validation. All properties remain read-only.

## Errors and concurrency

| Failure | Rust | Python |
| --- | --- | --- |
| Invalid domain input or non-finite price | `Err(Error { category, message })` | `KiyosiError.category`, an `ErrorCategory` member |
| Wrong representation, protocol or signature | Excluded by types, or checked date constructor | Native `TypeError`, `ValueError`, `OverflowError`; user conversion exceptions propagate |
| Allocation exhaustion | Native allocator behavior, normally process abort | Python allocations may raise `MemoryError`; Rust allocator exhaustion may terminate the process |
| Internal bug/panic | Native panic behavior | PyO3 `PanicException` if unwinding reaches the binding; aborts still terminate the process |

Categories implemented here are `INVALID_STRIKE` (2), `INVALID_VOLATILITY` (3),
`INVALID_RISK_FREE_RATE` (4), `INVALID_DIVIDEND_YIELD` (5), `INVALID_SPOT_PRICE` (6),
`INVALID_DATE` (7), `INVALID_TIME_RANGE` (8), `INVALID_RESULT` (9),
`INVALID_SCHEDULE` (10), `INVALID_CALENDAR` (11), `INVALID_PARAMETER` (12),
`UNBRACKETED_VOLATILITY` (13), `SOLVER_NON_CONVERGENCE` (14),
`SOLVER_NON_FINITE` (15), `UNBRACKETED_COUPON` (16), `BACKEND_FAILURE` (18, reported operational failures), and `UNSUPPORTED_OPERATION` (19).
Malformed curves and non-finite flat rates or
shifts use `INVALID_PARAMETER`; scalar model rate validation retains its
field-specific categories.
These identifiers and numbers match the legacy categories. Messages are diagnostic
text and must not be parsed. Resource failures and panics are not domain rejections.
Monte Carlo resource failures retain a native I/O error in Rust and map to Python
`OSError` (or its native subclass), with the OS error code when available; they
do not map to `KiyosiError`.

The crate-level rustdoc states the default concurrency contract: all current
values are `Send + Sync`, distinct and shared instances permit concurrent reads
and pricing, and Rust mutation requires exclusive access. Python wrappers are
immutable. Short analytic prices and Greek formulas retain the GIL, as in the
legacy binding. Solves release it around core work with owned Rust inputs and
no Python callbacks or Python state. User-supplied Rust engines must uphold
their own concurrency contract. Calls are synchronous; there is no cancellation API or core signal
polling. Python signal handling (including `KeyboardInterrupt` on the main thread)
waits until control returns to Python and is not mapped to `KiyosiError`.

## Vanilla and digital finite differences

`FiniteDifferenceVanillaEngine` supports European and American vanilla contracts,
including node curves. `FiniteDifferenceDigitalEngine` supports European cash and
asset digitals with explicitly flat rates; node curves return `UnsupportedOperation`
before life and Greek-request validation, even at expiry.

Both engines own `FiniteDifferenceSettings`. Rust defaults are 800 asset steps,
200 time steps, `FiniteDifferenceScheme::CrankNicolson` and no explicit upper
boundary. Python derives these defaults from Rust and exposes read-only settings
as `asset_step_count`, `time_step_count`, `scheme` and `asset_upper_boundary`.
`FiniteDifferenceScheme` in `kiyosi.pricing` is the Literal alias for
`"explicit_euler"`, `"implicit_euler"` and `"crank_nicolson"`.

Construction validates at least three asset steps, at least one time step and a
finite positive explicit boundary. Counts use signed 32-bit integers without an
additional cap; construction allocates no grid. Python uses the existing strict
integer and real protocols, rejecting booleans, fractional counts and out-of-range
integers before core validation. Unknown schemes raise `ValueError`; malformed
representations and unknown keywords raise native `TypeError`.

Pricing validates inclusive contract life, then curve coverage and grid feasibility.
At expiry it returns intrinsic value or the digital's strict-in-the-money payoff,
without constructing a grid; all Greeks are absent. The upper domain must exceed
spot and strike, and spacing must remain representable. Grid failures and unstable
explicit steps return `InvalidParameter`; non-finite systems and digital payoff-bound
violations return `InvalidResult`. Inputs and stored settings remain unchanged.

The existing theta schemes, Thomas solve, one-sided drift stabilization, spline
interpolation and two initial Rannacher intervals are retained. Crank-Nicolson's
first two intervals each use two implicit half-steps, with boundaries and American
exercise updated after each half-step. European and digital grids are uniform in
spot with automatic upper boundary `4 * max(spot, strike)`. American automatic
domains expand with volatility, maturity and maximum accumulated drift, using
`asinh(spot / strike)` coordinates to retain resolution. Vanilla time grids include
risk-free and dividend knots. American rollback projects onto intrinsic value;
its zero-spot put boundary retains the best future exercise value across rate knots.
High boundaries retain analytic continuation value. Digital terminal cells average
the discontinuous payoff; prices respect discounted payoff bounds, allowing the
known scalar carry truncation before clamping. Explicit boundaries still require
domain convergence checks.

`price_with_greeks` returns spline delta/gamma and speed from gamma differences on
the same mesh. Numerical completion reuses native spot Greeks for mixed measures;
every bumped evaluation shares the central upper boundary. It validates all shifts,
preserves selective requests, leaves infeasible stencils absent and propagates
feasible bumped failures. Other sensitivities use the existing units and defaults.
`numerical_greeks` and `implied_volatility` also accept both engines. Solves retain
the caller's bracket: explicit grids may reject endpoint volatility as unstable;
digital quotes can have multiple roots. No bracket search or algorithm switch occurs.

```python
from kiyosi.pricing import FiniteDifferenceVanillaEngine, implied_volatility

engine = FiniteDifferenceVanillaEngine(asset_step_count=800, time_step_count=400)
result = engine.price_with_greeks(option, context, ["delta", "gamma", "vega"])
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.1, upper_bound=0.3)
assert abs(sigma - context.model_parameters.volatility) < 1e-7
```

Rust usage mirrors `CoxRossRubinsteinVanillaEngine` with
`FiniteDifferenceVanillaEngine::new(FiniteDifferenceSettings { ..Default::default() })`.
Both Python engines release the GIL for scalar pricing, analysis and solves using
owned Rust inputs. Shared and distinct engines support concurrent calls; there is
no core cancellation polling, so Python signals are handled after a call returns.

The candidate FD tests migrate applicable Rannacher, mixed-Greek, scaled-discount,
interpolation, large-grid and configuration regressions. They use the existing
independent QuantLib European, refined American and digital reference profiles,
plus grid-refinement checks through public APIs. Legacy suites remain separate.

## European CPU Monte Carlo

`MonteCarloVanillaEngine` prices `EuropeanOption` with flat BSM rates using the
exact terminal lognormal distribution. It validates `step_count` without using
intermediate time steps. Node curves return `UnsupportedOperation` before life
and Greek-request validation, including at expiry; explicitly flat curve objects
remain supported. Contract life is inclusive; expiry returns intrinsic value.
The same applicability check precedes solver quote/settings validation and the
public Rust numerical-Greek helper through `PricingEngine::validate_applicability`.

Rust `MonteCarloSettings` owns the defaults: 100,000 paths, 50 steps, no seed,
automatic concurrency (`thread_count = 0`), `sampling = PseudoRandom` and
`antithetic = false`. Paths accept 1 through 10,000,000,
steps 2 through 10,000, and thread counts any nonnegative `i32`. Construction
validates these settings before committing an engine. Python derives the same
defaults and exposes immutable settings. Counts require integral representations,
excluding bool; fractions and floating-point counts are rejected with `TypeError`.
Out-of-range integer conversions raise `OverflowError`. Seeds are `None` or an
unsigned 64-bit integer, including zero and `2**64 - 1`.

Rust `MonteCarloSampling::{PseudoRandom, Sobol}` maps to Python's
`MonteCarloSampling` Literal choices `"pseudo_random"` and `"sobol"`.
Wrong Python protocols raise `TypeError`, unknown strings raise `ValueError`,
and `antithetic` accepts only actual booleans. Representation checks precede
core domain validation.

Sobol requires an explicit power-of-two path count (including 1); choosing it
with the default 100,000 paths returns `InvalidParameter` at construction.
Each call uses one seeded 52-bit lower-triangular matrix scramble and digital
shift, with globally indexed Gray-code points and midpoint inverse-normal
conversion. European terminal pricing uses exactly one dimension regardless of
`step_count`. The port retains the Joe–Kuo 21,201-dimension direction table and
its BSD license, plus the Cephes/SciPy inverse-normal notices, in standalone
Cargo and Python source distributions; binary distributions carry the notices.

Antithetic sampling pairs each normal draw with its negative. `path_count`
always counts actual payoff paths: pseudorandom sampling accepts odd counts,
leaving the final base draw unpaired. With antithetic enabled, Sobol uses
`ceil(path_count / 2)` points, still requires power-of-two total paths, and
accepts a single unpaired path when `path_count = 1`.

```python
from kiyosi.pricing import MonteCarloVanillaEngine

sobol = MonteCarloVanillaEngine(
    path_count=65536, sampling="sobol", antithetic=True, seed=42)
estimate = sobol.price_with_statistics(option, context)
assert estimate.standard_error is None
assert estimate.confidence_interval is None
```

The Rust equivalent uses `MonteCarloSettings { path_count: 65536,
sampling: MonteCarloSampling::Sobol, antithetic: true, seed: Some(42),
..Default::default() }` with `MonteCarloVanillaEngine::new`.

```rust
use kiyosi_core::{MonteCarloSettings, MonteCarloVanillaEngine, Greek, GreeksRequest,
    ImpliedVolatilitySettings, implied_volatility};
fn example(option: &kiyosi_core::EuropeanOption, context: &kiyosi_core::PricingContext) -> kiyosi_core::Result<()> {
let engine = MonteCarloVanillaEngine::new(MonteCarloSettings {
    seed: Some(42), thread_count: 4, ..Default::default()
})?;
let estimate = engine.price_with_statistics(option, context)?;
let delta = engine.price_with_greeks(option, context,
    GreeksRequest::new([Greek::Delta]), Default::default())?;
let sigma = implied_volatility(&engine, option, context, estimate.price(),
    ImpliedVolatilitySettings { lower_bound: 0.1, upper_bound: 0.4, ..Default::default() })?;
let _ = (delta, sigma);
Ok(())
}
```

With the European `option` and `context` from the Python example above:

```python
from kiyosi.pricing import MonteCarloVanillaEngine, implied_volatility

engine = MonteCarloVanillaEngine(seed=42, thread_count=4)
estimate = engine.price_with_statistics(option, context)
print(estimate.price, estimate.standard_error, estimate.confidence_interval)
analysis = engine.price_with_greeks(option, context, ["delta", "vega"])
sigma = implied_volatility(engine, option, context, estimate.price,
                           lower_bound=0.1, upper_bound=0.4)
```

`price` returns the scalar mean. `price_with_statistics` returns an owned
`MonteCarloEstimate`: the same price, optional `standard_error` in price units,
and an optional `(lower, upper)` `confidence_interval`. The standard error is
the square root of unbiased sample variance divided by the exact path count
for plain pseudorandom draws. Even pseudorandom antithetic counts instead use
the unbiased variance of independent pair means divided by the number of pairs;
at least two pairs are needed. Odd antithetic counts before expiry leave
uncertainty absent. Sobol always returns `None` for both uncertainty fields,
including at expiry; one randomized net does not supply an independent-sample
error estimate.
The two-sided asymptotic 95% normal interval uses 1.959963984540054 standard errors
without clipping to payoff bounds. One path before expiry cannot estimate
uncertainty and returns `None`; a known pseudorandom expiry payoff has zero error and a
degenerate interval, even with one configured path. Small samples, rare payoffs
and heavy tails can make the interval unreliable. These statistics describe
price uncertainty, not uncertainty in Greeks or solved volatility.

Moment accumulation is scaled to avoid overflowing sums or squared payoffs.
Discounting follows accumulation and combines logarithmic scales when the
discount alone underflows or overflows. Unstable simulation parameters,
nonpositive or non-finite terminal prices, non-finite requested results, and
non-finite confidence endpoints return `InvalidResult`. A failed request leaves
the engine, contract and context unchanged.

Greeks use the existing scalar finite-difference stencils, units, defaults and
availability rules. Every bump replays the same draws. An analysis or implied
volatility solve chooses one request-local seed when none is configured, without
changing the original engine. The public Rust `numerical_greeks` and
`implied_volatility` functions use `PricingEngine::prepare_request` for the same
behavior. No samples persist after a call. Sparse sampled volatility curves
need not be monotone or bracket a quote; use a suitable local bracket. Solvers
retain existing validation, expiry identifiability and failure categories.

Threads apply per call: 0 uses available CPU parallelism, 1 computes serially,
and positive values cap workers. Workers are also bounded by available CPUs and
the number of sample blocks. Each fixed block of 256 base draws owns a PCG32
stream and Box–Muller normal draws, or a cursor into the call's immutable Sobol
scramble; block order and reduction order are independent of
worker count. Within the same version and build, fixed seeds reproduce prices,
statistics, Greeks and solves across thread counts. Random streams and last-bit
results may differ across versions, builds, platforms and the legacy engine.
No process-wide thread settings or shared mutable engine state are changed.

All Python price, statistics, Greek and solve calls release the GIL around core
work, capturing owned Rust inputs and accessing no Python objects or callbacks
while detached. Shared and distinct engines support concurrent calls. Calls
join their workers and finish synchronously; there is no cancellation API or
signal polling. Python handles signals, including main-thread `KeyboardInterrupt`,
after core work returns. Recoverable entropy and thread-creation failures retain
`std::io::Error` details via `Error::operational_error()` and `Error::source()`;
Python maps them to `OSError`. Worker panics retain Rust unwinding and PyO3's
`PanicException` behavior. Allocation exhaustion follows Rust's allocator
(normally process abort), rather than producing a domain rejection.

The MC tests reuse the existing QuantLib numerical profile and its original
wrapper matrix and budgets, plus independent scalar stencils, statistical
coverage, numerical scaling, block boundaries, shared-instance calls and GIL
release checks. Sampling cases retain legacy Sobol price/Greek precision budgets,
exact odd-path and single-point regressions, cross-thread analytics and solves,
and the mixed timing tool's bias/RMSE checks without timing pass conditions.
They do not compare thread speed.

## American CPU Monte Carlo

`MonteCarloVanillaEngine.price` and `price_with_greeks` also accept
`AmericanOption`; the shared `implied_volatility` solver supports it too.
The engine retains the existing Longstaff–Schwartz method: `step_count` uniform
grid points include valuation and expiry, with exact BSM increments on each
interval. Continuation is fitted on in-the-money paths using the basis
`1, spot/strike, (spot/strike)^2` and strike-scaled discounted cash flows.
Gaussian elimination uses the existing relative pivot threshold `1e-14`,
reducing quadratic to linear to constant fits when samples do not identify
the higher basis. Empty fits skip that exercise date. Each pricing or bumped
evaluation refits its exercise policy; immediate exercise bounds the final price.

All European settings, strict Python conversions and immutable ownership rules
apply. American pricing before expiry requires at least three grid points;
this product check follows flat-rate applicability and inclusive life validation.
Two points remain valid for engine construction and for intrinsic value at expiry.
At expiry all Greeks are absent and implied volatility is unidentifiable.
Node curves return `UnsupportedOperation`, including at expiry and before
Greek-request or solver-setting validation. Failed requests leave inputs intact.

All four pseudorandom/Sobol and plain/antithetic combinations are supported.
Sobol consumes `step_count - 1` dimensions, except for terminal-only calls.
Path counts still count actual paths, including the final unpaired path for odd
pseudorandom antithetic counts. Fixed block streams and deterministic regression
order preserve seeded results across thread caps within the same build.
Path generation uses per-call workers; exercise regression runs serially.
Buffers belong to each evaluation, with storage proportional to paths times
intervals. Shared and distinct engines support concurrent calls, and Python
releases the GIL around owned Rust inputs for prices, Greeks and solves.

Calls with nonpositive dividends and nonnegative risk-free rates use the same
terminal estimator as European calls, with the immediate-exercise floor.
Minimum American steps are still checked first. For a requested rho stencil
crossing zero rates, both rate legs use the full exercise grid to avoid mixing
the terminal and regression estimators; the reported base price retains its
usual estimator.

Every feasible Greek bump and volatility trial replays the same normal draws
under one request-local seed, even when no seed is configured. No state persists
on the engine. Python's omitted or `None` American `spot_shift` uses **8% of
spot**, derived from Rust's
`MonteCarloVanillaEngine::default_american_spot_shift(context)`; explicit shifts
are absolute. Rust passes this value in `NumericalShiftSettings`; other shifts
retain their shared defaults. Small-bump exercise-policy noise and finite-bump
bias remain, so check seed, path, grid and bump convergence. The public scalar
Rust `numerical_greeks` helper uses its explicitly supplied shifts and scalar
prices; the engine's Greek method additionally handles the rho branch boundary.

`price_with_statistics` retains its European contract signature. Regression
cash flows share an estimated exercise policy, so they do not provide the
independent observations required by the terminal estimator's standard error
and confidence interval. No American sampling-error guarantee is added.
Solver tolerances control the sampled price residual or volatility interval;
they do not guarantee accuracy against the true price or volatility. Sparse
sampled curves can be non-monotone or discontinuous; supply a suitable local
bracket. Existing solver errors and failure-state guarantees apply.

```python
from kiyosi.instruments import AmericanOption
from kiyosi.pricing import MonteCarloVanillaEngine, implied_volatility

put = AmericanOption(option_type="put", strike=100,
                     effective_date=option.effective_date,
                     expiry_date=option.expiry_date)
engine = MonteCarloVanillaEngine(path_count=32768, step_count=50,
                                sampling="sobol", antithetic=True, seed=42)
result = engine.price_with_greeks(put, context, ["delta", "gamma"])
sigma = implied_volatility(engine, put, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
```

The candidate tests migrate the existing American MC QuantLib scenario/profile
with unchanged price and Greek budgets and independent grid/bump stability
checks. Public Rust and Python tests also cover immediate exercise, sparse and
rank-deficient fits, currency scaling, odd paths, all sampling combinations,
thread invariance, shared samples, solver recovery and input failures.

## Digital CPU Monte Carlo

`MonteCarloDigitalEngine` accepts cash-or-nothing and asset-or-nothing European
contracts, using the same Rust `MonteCarloSettings` and Python keyword settings
as the vanilla engine. All four pseudorandom/Sobol and plain/antithetic combinations
are supported, including exact odd pseudorandom path counts and thread caps.
It samples the exact terminal distribution in one dimension; `step_count` is
validated but unused. Defaults, seed and thread reproducibility, strict Python
conversions, ownership, GIL release and operational failure rules match vanilla MC.

Exercise is tested in standardized lognormal coordinates, preserving strict
in-the-money settlement when tiny volatility makes a terminal spot round to the
strike. At expiry, spot strictly above/below strike pays cash or spot for calls/puts;
exact equality pays zero. Expiry Greeks are absent and volatility is unidentifiable.
Node curves return `UnsupportedOperation` before life, request or solver validation.

`price_with_greeks` returns requested likelihood-ratio **delta and gamma** from
the same normal draws as price. Other Greeks remain absent. All numerical shifts
are validated but unused by this method. The Rust `numerical_greeks` helper and
Python scalar-price difference tools can compute separate finite-bump sensitivities;
these can be noisy or zero for discontinuous sampled cash payoffs. The helper
pins a request-local seed so bumps replay draws, without mutating the engine.
Digital MC retains its existing support matrix: no `price_with_statistics` method
or implied independent-sample uncertainty guarantee is added.

Discounted payoffs and likelihood weights use logarithmic scaling when required;
scaled moment accumulation avoids overflowing sums and squares. Non-finite paying
samples or requested Greek samples return `InvalidResult`, even if their average
might be representable. Unrequested overflowing Greeks do not spoil a finite price.
This preserves the terminal estimator's failure semantics.

`implied_volatility` reuses one seed throughout each solve, including unseeded
engines. Sampled digital prices can have plateaus, jumps or multiple volatility
roots. Supply a local bracket; solver tolerances describe the sampled residual
or volatility interval, not accuracy against the true model volatility.

```python
from kiyosi.instruments import CashOrNothingOption
from kiyosi.pricing import MonteCarloDigitalEngine, implied_volatility

cash = CashOrNothingOption(option_type="call", strike=100, payout=10,
                           effective_date=option.effective_date,
                           expiry_date=option.expiry_date)
engine = MonteCarloDigitalEngine(path_count=65536, sampling="sobol",
                                antithetic=True, seed=42, thread_count=4)
result = engine.price_with_greeks(cash, context, ["delta", "gamma"])
sigma = implied_volatility(engine, cash, context, result.price,
                           lower_bound=0.1, upper_bound=0.22)
```

The Rust equivalent uses `DigitalOption`, `MonteCarloDigitalEngine::new` and
`MonteCarloSettings`; the public methods accept borrowed contracts and contexts
and return owned results. The candidate tests migrate the existing digital CPU
price/likelihood error budgets, negative-carry, tiny-volatility, odd-path,
thread-independence and failure regressions. They also compare all 40 existing
live QuantLib digital scenarios and check multi-seed price RMSE against the
independent cash-payoff variance. Legacy implementations remain available.

## Continuous-average Asian analytic pricing

Ticket 14 adds owning `GeometricAveragePriceOption` and
`ArithmeticAveragePriceOption` contracts, `AnalyticGeometricAveragePriceEngine`
and `TurnbullWakemanArithmeticAveragePriceEngine`. Rust constructors take
`(option_type, strike, effective_date, averaging_start_date, expiry_date,
realized_average)`. Use the core `DEFAULT_REALIZED_AVERAGE` (zero) for a new
averaging period; Python derives its omitted default from that constant.

Constructors check positive finite strike, finite nonnegative realized average,
ordered life dates, then `effective_date <= averaging_start_date <= expiry_date`,
returning `InvalidStrike`, `InvalidParameter`, `InvalidTimeRange` or
`InvalidSchedule`, in that order. Both contracts' `replace(realized_average)`
(Rust) and `.replace(realized_average=...)` (Python) replace history only and
return validated owning copies. Python `.replace()` preserves the old history.
Failure leaves the original unchanged. Properties are read-only and copies own
their data. Numeric and date arguments use the strict boundary protocols above.

Pricing validates inclusive life first, then requires zero history at or before
the averaging start and positive history strictly afterward. Missing or premature
history returns `InvalidParameter`. History stays fixed during sensitivities;
no observation is inferred from the current spot. At expiry, nonzero averaging
windows settle their realized average. A zero-length window fixes once at expiry,
requires zero prior history, and uses vanilla pricing before expiry and terminal
spot at expiry. Forward-starting continuous averaging retains the lead-time
Brownian variance.

Geometric pricing integrates weighted carry across deterministic curve knots.
Arithmetic pricing retains the existing moment-matched approximation and its
stable divided moments, small-variance and scaled-tail treatment. It is compared
to QuantLib's **same Levy approximation**, not an exact arithmetic-average value.
Before expiry it requires explicitly flat curves, even for a single fixing;
node curves return `UnsupportedOperation` after life/history validation. Expiry
settlement precedes curve applicability and coverage checks. Before expiry both
engines validate curve coverage; non-finite numerical results use `InvalidResult`.

Both engines expose all ten Greeks through shared scalar finite differences,
using the Rust `NumericalShiftSettings` defaults. Empty requests and invalid
shifts fail before pricing. Collapsed or infeasible stencils leave measures absent;
feasible bumped pricing errors propagate. Theta, charm and color are absent if
the full time stencil crosses the averaging-start history boundary. All Greeks
are absent at expiry. `numerical_greeks` supports both Rust contract types.
The shared implied-volatility solver validates its controls and prices bracket
endpoints before rejecting unidentifiable payoffs with `UnsupportedOperation`:
expiry and arithmetic contracts whose accrued average makes adjusted strike
nonpositive. It does not price at the context's original volatility.

```python
from datetime import date
from kiyosi.instruments import ArithmeticAveragePriceOption, GeometricAveragePriceOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import (
    AnalyticGeometricAveragePriceEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
    implied_volatility,
)

start, end = date(2025, 1, 1), date(2026, 1, 1)
terms = dict(option_type="call", strike=100, effective_date=start,
             averaging_start_date=start, expiry_date=end)
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start)
geometric = GeometricAveragePriceOption(**terms)
assert abs(AnalyticGeometricAveragePriceEngine().price(geometric, context)
           - 5.035867546515384) < 1e-12
arithmetic = ArithmeticAveragePriceOption(**terms)
engine = TurnbullWakemanArithmeticAveragePriceEngine()
result = engine.price_with_greeks(arithmetic, context, ["delta", "vega"])
assert abs(result.price - 5.246501860362777) < 1e-12
assert abs(implied_volatility(engine, arithmetic, context, result.price) - 0.2) < 1e-7
seasoned = arithmetic.replace(realized_average=105)
assert arithmetic.realized_average == 0
```

Python prices, Greek requests and solves release the GIL around owned Rust inputs,
with no Python state or callbacks accessed. Distinct and shared instances support
concurrent reads and pricing; signals resume on return to Python. Rust follows
the crate-wide `Send + Sync` contract. Public suites retain the applicable legacy
regressions and all 24 live QuantLib geometric/Levy scenarios with existing
accuracy and oracle uncertainty budgets. Asian CPU Monte Carlo is described below.

## Continuous-average Asian CPU Monte Carlo

`MonteCarloAsianEngine` prices both Asian contract types with the shared
`MonteCarloSettings` in Rust and the same Python keywords as vanilla MC.
Defaults, strict representation conversions, exact path counts, seeded thread
reproducibility, owning results and operational failures follow vanilla MC.
There is no Asian `price_with_statistics`: the existing support matrix is
preserved, with no added confidence-interval guarantee.

Geometric averages sample the exact lognormal distribution, including elapsed
history weights and the lead-time variance of a forward start. Single-fixing
contracts also sample their exact terminal distribution. Both use one normal
dimension and ignore the validated `step_count`. Arithmetic averaging keeps
`step_count` uniform trapezoidal intervals over the remaining averaging window.
It integrates out the first interval conditionally, samples the remaining
`step_count - 1` increments, and adds one separate normal dimension before a
forward-starting window. Sobol consumes exactly these dimensions; antithetic
sampling negates the whole normal vector and keeps an unpaired final path for
odd pseudorandom counts. No full path matrix is retained.

Pricing checks inclusive life and history consistency first. Expiry settles
history (or spot for single fixings) before curve applicability. Before expiry,
geometric MC requires explicitly flat rates, including for a single fixing;
node curves return `UnsupportedOperation`. Arithmetic MC supports deterministic
rate curves, integrating carry across knots on each interval. Both require
curve coverage. Arithmetic accrued history that makes adjusted strike
nonpositive uses the exact discounted expected payoff and zero gamma, avoiding
sampling; implied volatility is then unidentifiable (`UnsupportedOperation`).
Non-finite calculations and requested Greek samples return `InvalidResult`.

`price_with_greeks` preserves native **delta and gamma**: pathwise delta and
likelihood-ratio gamma for geometric samples, and conditional delta/gamma for
arithmetic integration. Only requested native measures are returned; other
Greeks and all expiry Greeks remain absent. Numerical shifts are validated
before pricing but unused. Rust's `numerical_greeks` provides scalar differences
with the unchanged absolute shift defaults and history-boundary availability
rules. Each scalar analysis or implied-volatility solve pins one request-local
seed even when none is configured, replaying draws without mutating the engine.
Solving tolerances describe the sampled price, not accuracy against the true
model price; check paths, integration steps and seed convergence.

```python
from kiyosi.pricing import MonteCarloAsianEngine, implied_volatility

# Reuse arithmetic and context from the Asian example above.
engine = MonteCarloAsianEngine(path_count=65536, step_count=64,
                               sampling="sobol", antithetic=True, seed=42)
result = engine.price_with_greeks(arithmetic, context, ["delta", "gamma"])
sigma = implied_volatility(engine, arithmetic, context, result.price,
                           lower_bound=0.1, upper_bound=0.4)
assert abs(sigma - 0.2) < 1e-6
```

The Rust equivalent uses `MonteCarloAsianEngine::new(MonteCarloSettings { ... })`
and borrowed Asian contracts and contexts. Shared and distinct instances support
concurrent calls; Python prices, analyses and solves release the GIL around owned
Rust inputs. Cancellation, signal handling, I/O failures, worker panics and
allocation exhaustion follow the CPU MC contract above.

Public Rust and isolated Python tests migrate the applicable existing Asian MC
contracts and numerical budgets. They cover geometric analytic references,
published arithmetic convergence cases, independent raw-path statistical
comparisons, native sensitivities, history boundaries, tiny-volatility and
monetary scaling, curves, threading, strict representations and solver failures.
The legacy implementation remains available; no timing threshold is added.

## Single-barrier analytics and history

Rust constructs `BarrierOption::new(BarrierOptionTerms::new(direction, strike,
start, end, barrier_level, barrier_type))?`. Modify the public input terms before
construction to configure a rebate, monitoring or history. `BarrierType` provides
`UpAndIn`, `UpAndOut`, `DownAndIn` and `DownAndOut`; equality touches. Core defaults
are zero rebate, `RebateTiming::AtExpiry`, `ObservationMode::Continuous`, an empty
schedule and unknown history (`None`). Knock-ins reject `AtHit`, even for zero
rebates, with `InvalidOption` (stable code 1). Levels must be finite and positive;
rebates finite and nonnegative. Construction validates barrier level, life,
schedule structure, then strike and rebate. Scheduled dates must be nonempty,
strictly increasing and within inclusive life; continuous monitoring rejects dates.
Trading-day compatibility is checked against the pricing context's calendar.

`BarrierTouchState::{Untouched,Touched}` describes observations **strictly before**
valuation; it does not include a fixing at valuation. Unknown history is accepted
only until monitoring has previously occurred. Later missing history returns
`InvalidParameter`; a claimed prior touch before the first observation returns
`InvalidOption`. Continuous monitoring begins at the effective date's midnight.
Scheduled observations occur only at midnight UTC; they do not recur later that
day. `replace(Some(state))?` returns an owning copy; `replace(None)?` restores
unknown history. Rust validates construction again and defers time consistency to
pricing. Python `.replace(touch_state=...)` accepts `"untouched"`, `"touched"` or
`None`; omitted history retains its value. Invalid replacements preserve originals.
Rust schedule accessors borrow owner storage; Python returns owned schedules/lists.

`AnalyticBarrierEngine` requires explicitly flat risk-free and dividend curves,
including for resolved or expired contracts. Node curves return
`UnsupportedOperation` before other pricing/analysis validation. Pricing then
validates inclusive valuation life, scheduled trading dates and prior history.
It resolves the current observation before future pricing: touched knock-ins
become vanilla; touched knock-outs pay their rebate at expiry, or pay at hit only
if the hit occurs now. Prior at-hit rebates are already paid and have value zero.
After the final scheduled fixing, an untouched knock-out becomes vanilla and an
untouched knock-in receives its expiry rebate. Expiry settles intrinsic value or
rebate with the same event order. Zero-rebate terminal payoffs that necessarily
require a touch resolve to vanilla (knock-in) or zero (knock-out).

Live continuous prices retain the Reiner–Rubinstein branches, logarithmic tail
scaling and negative-rate first-hit integration. Live scheduled prices retain the
BGK approximation, shifting the barrier by
`exp(±0.5825971579390107 * volatility * sqrt(mean_spacing_years))`.
Mean spacing is effective date to final fixing divided by observation count in
Actual/365 Fixed years. Unresolved schedules must end at expiry, and spot must
lie strictly on the unbreached side of the shifted barrier; otherwise the engine
returns `UnsupportedOperation`. Resolved contracts settle before those restrictions.
There is no engine fallback. Non-finite prices return `InvalidResult`.

All ten Greeks use the shared scalar numerical differences and core bump defaults.
All are absent at expiry or a current barrier equality without a prior touch.
Spot stencils reaching a current barrier omit affected spot/mixed Greeks; time
stencils crossing scheduled fixings or invalid history regimes omit time Greeks.
Other feasible pricing failures propagate. Scheduled calendar validation is cached
only within each analysis or solve request. Requests share no mutable engine state.
`implied_volatility` retains the supplied bracket and shared solver validation;
barrier prices can be non-monotone, so choose a bracket around the desired root.
Fixed remaining cashflows and excluded zero payoffs return `UnsupportedOperation`
because volatility is unidentifiable. Inputs remain unchanged on every outcome.

Python exports `BarrierOption` from `kiyosi.instruments` and
`AnalyticBarrierEngine` from `kiyosi.pricing`. Choices use snake_case strings.
Constructors, Greek controls and solver controls retain strict representation
validation: booleans are rejected as numbers, and integer controls reject floats
and out-of-range values. Domain errors map to `KiyosiError.category`; wrong
protocols and unknown choices keep native exceptions. Price, analysis and solve
calls release the GIL around owned core inputs. Shared and distinct engines allow
concurrent pricing; signals resume when Python regains control.

```python
from datetime import date
from kiyosi.instruments import BarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticBarrierEngine, implied_volatility

option = BarrierOption(option_type="call", strike=100,
    effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
    barrier_level=80, barrier_type="down_and_out", touch_state="untouched")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2),
    spot_price=100, valuation_time=date(2025, 1, 1))
engine = AnalyticBarrierEngine()
result = engine.price_with_greeks(option, context, ["delta", "vega"])
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-7
assert option.replace(touch_state="touched").touch_state == "touched"
assert option.touch_state == "untouched"
```

## Single-barrier CRR

Rust `CoxRossRubinsteinBarrierEngine::new(BinomialSettings { step_count: 1024 })?`
and Python `kiyosi.pricing.CoxRossRubinsteinBarrierEngine(step_count=1024)` price
`BarrierOption` using the existing CRR rollback. `Default::default()` and Python's
no-argument constructor derive **256** steps from Rust; valid counts are
1 through 1,000,000. Invalid counts return `InvalidParameter` at construction.
Python uses the same strict integer protocol as the other CRR engines, exposes
read-only `step_count`, and rejects booleans, floats and out-of-range integers.

All four barrier types, calls and puts, continuous and scheduled monitoring, zero
or nonnegative rebates, and applicable at-hit/at-expiry payment timings follow
the shared barrier terms above. Explicitly flat risk-free and dividend curves
are required before other domain validation, including settlement and expiry.
Pricing then validates inclusive life, calendar and strictly prior history.
Resolved cashflows and vanilla payoffs settle before tree/grid validation;
resolved vanilla uses analytic pricing and delta/gamma. At-hit rebates already
paid remain zero. The final fixing can precede expiry, and fixings occur only
at their original midnight, including a fixing exactly at valuation.

Continuous monitoring checks every tree level and converges toward the analytic
continuous limit; finite-step prices can oscillate as the barrier crosses nodes.
Each unresolved future scheduled fixing must coincide **exactly** with a step,
using microsecond integer arithmetic relative to valuation. Off-grid dates return
`UnsupportedOperation`; the engine never moves dates or adjusts the step count.
Inclusive node hits preserve barrier equality. Unstable risk-neutral probabilities,
non-normal terminal asset nodes and non-finite prices return `InvalidResult`.

`price_with_greeks` returns only requested node delta/gamma. One-step gamma and
all expiry Greeks are absent. Current barrier equality also suppresses Greeks
without a prior touch. Other measures remain absent even when requested. Numerical shift controls are validated but unused.
Rust callers can explicitly use `numerical_greeks` for scalar sensitivities with
the shared stencils; feasible bumped pricing failures propagate, including a
schedule becoming off-grid after a time bump. `implied_volatility` intersects the
supplied bracket with the existing admissible CRR volatility range and rejects
volatility-independent payoffs. Barrier prices can be non-monotone or discontinuous
in volatility; a solve may return a different root or stop by interval tolerance.
Reprice its result when choosing brackets near tree barrier transitions.

```python
from kiyosi.pricing import CoxRossRubinsteinBarrierEngine, implied_volatility

# Reuses the down-and-out option and context from the preceding example.
engine = CoxRossRubinsteinBarrierEngine(step_count=1024)
result = engine.price_with_greeks(option, context, ["delta", "gamma"])
assert result.delta is not None and result.gamma is not None
assert result.vega is None
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(engine.price(option, context.replace(model_parameters=
    context.model_parameters.replace(volatility=sigma))) - result.price) < 2e-7
```

Python price, analysis and solve calls release the GIL around owned Rust inputs.
Shared and distinct instances support concurrent pricing and leave inputs unchanged
on success or failure. Signals resume when Python regains control. Public tests
migrate the relevant legacy event and numerical regressions, compare small trees
with independent exhaustive path enumeration, check continuous-grid refinement,
and compare price/delta/gamma with live QuantLib 1.43. Legacy implementations stay
available; no performance threshold or report is added.

## Single-barrier quadrature

Rust `QuadratureBarrierEngine` and Python `kiyosi.pricing.QuadratureBarrierEngine`
price the same `BarrierOption` contracts with explicitly flat rates, including dated
flat curves. All four barrier types, calls/puts, continuous and scheduled monitoring,
and supported rebates reuse the single-barrier history and settlement rules above.
Scheduled observations retain their original midnight UTC timestamps and may end
before expiry; there is no grid-alignment restriction or BGK approximation.

Rust `QuadratureSettings::default()` and Python's keyword-only constructor use
`asset_step_count=512` and `integration_standard_deviations=8.0`. The budget must
be at least four and the extent finite and positive (`InvalidParameter`). Python
accepts `numbers.Integral` with the integer-index protocol for the budget, excluding booleans/floats
and raising native `OverflowError` outside signed 32-bit range. The extent uses
the strict real-number protocol. Properties are read-only; Rust `settings()`
returns an owned copy. These settings do not configure terminal vanilla/digital engines.

Continuous monitoring retains Simpson integration split at strike and barrier,
rounding an odd panel budget up to even, Brownian-bridge survival, and the shared
first-hit rebate integral. Scheduled monitoring retains cubic interpolation of
continuation values and conditional Gaussian cell moments, exact affine terminal
pieces and tail limits, compensated sums and log scaling. Contractual breaks are
inserted into the log-asset grid. Expiry-only monitoring uses exact digital moments,
independent of grid settings. Check spatial resolution and integration extent
separately; small budgets or narrow extents can give inaccurate finite results.
No new integration algorithm or performance requirement is introduced.

Flat-rate applicability precedes life, calendar and history validation. Node curves
return `UnsupportedOperation`, including at expiry and for resolved payments.
Scheduled trading-day validation precedes settlement. Missing strictly prior
history returns `InvalidParameter`; impossible prior touches return `InvalidOption`.
Current observations include barrier equality. Prior at-hit rebates are already
paid; current at-hit rebates pay immediately, while expiry rebates are discounted
to expiry. Activated or monitoring-completed vanilla payoffs use analytic pricing.
Unrepresentable grids, transitions, continuation values or prices return `InvalidResult`.
Inputs and engine settings remain unchanged on success and failure.

All ten requested Greeks use shared scalar finite differences and Rust bump defaults;
Rust also supports `numerical_greeks`. At expiry, or current barrier equality without
a prior touch, all Greeks are absent. Stencils crossing observations or barriers
leave affected measures absent; feasible bumped errors propagate. An analysis request
reuses its prepared scheduled continuation curve across spot bumps, with no cache
shared between requests. Accuracy depends on resolution and bump size, especially
for higher derivatives and near events. Empty requests or invalid shifts return
`InvalidParameter` after flat-rate applicability.

The shared implied-volatility solver preserves its caller-supplied bracket and
validation order. Volatility-independent cashflows return `UnsupportedOperation`;
barrier prices need not be monotone in volatility. Python price, Greek and solve
calls release the GIL around owned Rust inputs. Shared and distinct instances
support concurrent requests; signal handling resumes on return to Python.

```python
from datetime import date
from kiyosi.instruments import BarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import QuadratureBarrierEngine, implied_volatility

option = BarrierOption(option_type="call", strike=100,
    effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
    barrier_level=80, barrier_type="down_and_out")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=date(2025, 1, 1))
engine = QuadratureBarrierEngine(asset_step_count=1024,
                                integration_standard_deviations=10)
result = engine.price_with_greeks(option, context, ["delta", "gamma", "vega", "rho"])
assert result.delta > 0
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-7
```

Rust constructs `QuadratureBarrierEngine::new(QuadratureSettings { ..Default::default() })?`
and calls `price`, `price_with_greeks`, `numerical_greeks` or `implied_volatility`.
Public API tests cover live QuantLib continuous prices/Greeks, independent digital
and bivariate-normal scheduled payoffs, short maturity, near-barrier values,
spatial/extent convergence, event history, validation, solving and concurrency.

## Single-barrier CPU Monte Carlo

`MonteCarloBarrierEngine` prices `BarrierOption` with flat risk-free and dividend
rates. It uses the shared Rust `MonteCarloSettings` and Python keyword settings:
100,000 paths, 50 steps, no seed, automatic workers, pseudorandom sampling and no
antithetic pairing. Steps are validated but unused. Continuous monitoring samples
one terminal normal and weights payoffs by conditional Brownian-bridge survival.
Scheduled monitoring samples each original future fixing and, when monitoring ends
early, one unobserved expiry leg. It never adds monitoring dates or moves fixings.

Pseudorandom and scrambled Sobol sampling support plain and antithetic paths,
including exact odd pseudorandom counts. Sobol requires power-of-two paths and at
most 21,201 dimensions; dimensions equal the number of simulation legs, independent
of `step_count`. Pairing uses `ceil(path_count / 2)` points. Even paths that knock
out early retain their full normal vector, preserving the remaining streams and
thread-count-independent seeded results. The existing barrier support matrix has
no `price_with_statistics` method; no new standard-error or confidence-interval
claim is introduced. Existing vanilla uncertainty rules remain unchanged.

Life, calendar and strictly prior touch-history validation reuse the barrier core.
Current inclusive hits, known history, completed monitoring and expiry resolve
before simulation. Resolved vanilla payoffs use analytic pricing. Prior at-hit
rebates are already paid. Scheduled rebates discount to the actual hit fixing or
expiry as contracted. Continuous at-hit rebates use the existing analytic hit
value, including its negative-rate integration fallback. Non-finite paths,
requested Greek samples or results return `InvalidResult`; failed calls preserve
inputs and settings. Node curves return `UnsupportedOperation` before request,
life or solver validation.

`price_with_greeks` returns requested likelihood-ratio delta/gamma from the price's
normal draws, with analytic survival derivatives for continuous monitoring. Other
Greeks remain absent. All Greeks are absent at expiry or when the configured spot
stencil crosses a current monitored barrier without a prior touch. Shifts are
validated and also control numerical derivatives of the continuous at-hit rebate.
The Rust `numerical_greeks` helper offers separate scalar sensitivities and pins
one request-local seed. Likelihood estimates can be noisy near expiry.

`implied_volatility` pins one seed for the whole solve, even for unseeded engines,
and rejects volatility-independent remaining payments with `UnsupportedOperation`.
Barrier prices may be nonmonotone in volatility and discrete sampled prices may
jump; provide a bracket isolating the intended root. Solver tolerances apply to
the sampled curve. Engines and returned results own their data, permit concurrent
calls on shared or distinct instances, and change no global thread settings.
Python releases the GIL around owned core work and reuses strict numeric/index
conversions and native representation errors. Operational errors and worker panics
follow the existing CPU Monte Carlo mapping described above.

```python
from datetime import date
from kiyosi.instruments import BarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import MonteCarloBarrierEngine, implied_volatility

start = date(2025, 1, 1)
option = BarrierOption(option_type="call", strike=100, effective_date=start,
    expiry_date=date(2026, 1, 1), barrier_level=80, barrier_type="down_and_out")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start)
engine = MonteCarloBarrierEngine(path_count=32768, seed=42, sampling="sobol",
                                 antithetic=True, thread_count=4)
result = engine.price_with_greeks(option, context, ["delta", "gamma"])
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-5
```

See `MonteCarloBarrierEngine::price_with_greeks` for a Rust example. Candidate
acceptance tests retain continuous analytic and scheduled quadrature comparisons,
rebates, early monitoring, odd counts, shared-sample Greeks, solver behavior,
thread reproducibility, event boundaries and failure categories. The legacy
implementation remains available for migration comparison.

## Single-barrier finite differences

Rust `FiniteDifferenceBarrierEngine` and Python
`kiyosi.pricing.FiniteDifferenceBarrierEngine` price `BarrierOption` with continuous
or scheduled monitoring, all four barrier types, expiry rebates and knock-out
at-hit rebates. Both flat and node rate/dividend curves are supported. The engine
uses the shared `FiniteDifferenceSettings`: 800 asset steps, 200 time steps,
Crank-Nicolson and an automatic upper boundary. The automatic boundary is
`max(4 * max(spot, strike), barrier + max(spot, strike))`; an explicit boundary
must exceed spot, strike and barrier before a time grid is allocated.

Continuous barriers form exact edges of the live domain; their level is never
rounded to a node. Scheduled dates and curve knots are inserted unchanged into
the time grid. Current observations resolve against actual spot before projection;
only strictly future dates become grid events. At each scheduled fixing the
barrier cell projects the discontinuity using piecewise two-point Gaussian
integration, preserving smooth continuation elsewhere. Crank-Nicolson replaces
the first two rollback intervals with implicit half-steps and restarts that damping
after each fixing, without introducing additional observations. Knock-ins use a
separately marched vanilla reference plus discounted rebate parity. Scheduled
outer boundaries discount at-hit rebates to the next original observation.

Life, calendar, strictly prior history, completed monitoring, expiry settlement
and already-paid at-hit rebates follow the single-barrier contract above.
Resolved cashflows or vanilla claims precede grid validation. Settings validate
at construction; clipped or degenerate domains and unstable explicit grids return
`InvalidParameter`, numerical failures return `InvalidResult`, and life/curve
coverage failures return `InvalidTimeRange`. Failed pricing or analysis preserves
inputs and immutable settings.

Requested delta/gamma use the shared spot spline; speed differences spline gamma
on the same grid using `spot_shift`. Other Greeks, including vanna/zomma and
charm/color, use shared numerical completion and all four Rust shift settings.
Analysis fixes the central spatial domain. Collapsed bumps omit dependent
numerical Greeks while spline delta/gamma remain available. Stencils crossing
barriers or observation/history regimes omit affected measures; all Greeks are
absent at expiry or a current barrier equality without a prior touch. Feasible
bump failures propagate. The shared implied-volatility solver keeps the supplied
bracket; fixed cashflows return `UnsupportedOperation`. Barrier prices can be
non-monotone in volatility, so callers must bracket the intended root.

Python settings use strict real/integer protocols, rejecting booleans, fractional
integer controls and integer overflow with native conversion exceptions. Defaults
come from Rust, results own their data, and pricing, Greeks and solving release
the GIL around owned core inputs. Shared and distinct engines support concurrent
pricing and solving; signal handling resumes when control returns to Python.

```python
from datetime import date
from kiyosi.instruments import BarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext, all_days_calendar
from kiyosi.pricing import FiniteDifferenceBarrierEngine, implied_volatility

start, end = date(2025, 1, 1), date(2026, 1, 1)
option = BarrierOption(option_type="call", strike=100,
    effective_date=start, expiry_date=end, barrier_level=80,
    barrier_type="down_and_out", observation_mode="scheduled",
    observation_dates=[date(2025, 4, 2), date(2025, 10, 1)],
    touch_state="untouched")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start, calendar=all_days_calendar())
engine = FiniteDifferenceBarrierEngine(asset_upper_boundary=400)
result = engine.price_with_greeks(option, context, ["delta", "gamma", "vanna"])
assert result.delta is not None and result.vanna is not None
assert result.price == engine.price(option, context)
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-7
```

Public Rust/Python tests cover settlement and failure contracts, exact continuous
edges, event projection across grid placements, Rannacher restarts, interpolation
near extrema, large rebates, vanishing volatility, node-curve discounts, mixed
sensitivities and volatility recovery. Independent live QuantLib comparisons,
analytic binary decompositions and two-observation conditional integrals check
accuracy; spatial and temporal refinements check convergence. Legacy delivery
remains available, with no new dependency or performance gate.

## Binary-barrier and single-touch analytics

Rust constructs `BinaryBarrierOption::cash(BinaryBarrierTerms::new(direction,
strike, start, end, barrier_level, barrier_type), payout)?` or
`BinaryBarrierOption::asset(terms)?`. The binary pays at expiry only when its
barrier is active and spot is **strictly** in the money; equality at strike pays
zero. Construction validates positive finite cash payout first (when present),
then strike and life, then barrier and schedule. Error categories match the
digital and single-barrier contracts above.

`TouchOption::cash(TouchOptionTerms::new(start, end, barrier_level, barrier_type),
payout)?` and `TouchOption::asset(terms)?` have no strike or option direction.
`UpAndIn`/`DownAndIn` select one-touch, `UpAndOut`/`DownAndOut` select no-touch.
Both input-term types default to continuous monitoring, empty dates and unknown
history. Touch terms also default to `SettlementTiming::AtExpiry`. One-touch can
use `AtHit`; no-touch at-hit is representable in Rust but returns `InvalidOption`
at construction. Cash payout is positive and finite; touch construction checks
payout, settlement timing, barrier, life and schedule, in that order.

`AnalyticBinaryBarrierEngine` prices both contract types with the existing
binary-barrier formulas and single-barrier first-passage calculation, including
negative rates, scaled normal tails and very small positive volatility. It
requires explicitly flat risk-free and dividend rates before life, history or
Greek-request validation, even for resolved or expired contracts. Non-finite
results return `InvalidResult`. Continuous and scheduled monitoring share the
single-barrier history contract above: unknown is distinct from untouched,
equality touches, fixings are at midnight UTC, and history is strictly prior.
Completed scheduled monitoring resolves activation even if its last fixing is
before expiry. Unresolved scheduled analytics use the existing BGK approximation
and require the last fixing at expiry; a breached shifted barrier or an early
last fixing returns `UnsupportedOperation`. Resolved events and payoffs that
mathematically require a touch settle before this approximation restriction.

An at-hit one-touch observed now pays cash payout or **current spot**, including
a scheduled overshoot and a hit exactly at expiry. A strictly prior at-hit touch
is already paid and has zero remaining value. At-expiry one-touch activation
retains its discounted cash or asset payoff. An untouched no-touch after its
last observation retains its expiry payoff; a touched no-touch pays zero.
Future continuous asset hits transfer the barrier amount. Scheduled asset
at-hit pricing retains the existing shifted-barrier approximation and payout
convention. `replace(history)?` and Python `.replace(touch_state=...)` preserve
all other terms and the original owner; calendar and history-time consistency
are checked during pricing. Rust schedule accessors borrow; Python copies own
their data.

All ten requested Greeks use shared scalar differences and Rust bump defaults;
at expiry or a current barrier equality all Greeks are absent. Stencils crossing
a barrier or fixing suppress affected measures. The shared implied-volatility
solver retains the caller's bracket and error ordering. Fixed remaining cash
or asset flows, zero settlements and paid historical hits return
`UnsupportedOperation` because volatility is unidentifiable. Activated strike
binaries still have digital volatility exposure; binaries can be non-monotone,
so a bracket may contain several roots or fail to bracket an attainable quote.

Python retains `cash_binary_barrier_option` and `asset_binary_barrier_option`,
returning `BinaryBarrierOption`, plus `cash_one_touch_up/down`,
`asset_one_touch_up/down`, `cash_no_touch_up/down` and `asset_no_touch_up/down`,
returning `TouchOption`. Factories are keyword-only. One-touch accepts
`settlement_timing="at_hit"` or `"at_expiry"`; no-touch factories omit that keyword.
The same strict real, choice, date and iterable protocols apply, with native
representation errors before core domain validation. Domain errors preserve
structured categories. Pricing, Greeks and solves release the GIL around owned
Rust data, following the crate-wide concurrency and failure contracts.

```python
from datetime import date
from kiyosi.instruments import cash_binary_barrier_option, asset_one_touch_up
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticBinaryBarrierEngine, implied_volatility

start, end = date(2025, 1, 1), date(2026, 1, 1)
binary = cash_binary_barrier_option(
    option_type="call", strike=100, effective_date=start, expiry_date=end,
    barrier_level=120, barrier_type="up_and_out", payout=10,
    touch_state="untouched")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start)
engine = AnalyticBinaryBarrierEngine()
result = engine.price_with_greeks(binary, context, ["delta", "vega"])
sigma = implied_volatility(engine, binary, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-7
touch = asset_one_touch_up(effective_date=start, expiry_date=end,
                          barrier_level=120, settlement_timing="at_hit")
assert engine.price(touch, context.replace(spot_price=130)) == 130
assert engine.price(touch.replace(touch_state="touched"),
                    context.replace(valuation_time=date(2025, 7, 1))) == 0
```

Public Rust/Python tests migrate settlement, history, strict strike boundaries,
scheduled monitoring, variance-underflow and extreme-scale regressions. All 268
existing independent QuantLib binary/touch scenarios retain their price, Greek
and reference-uncertainty budgets. The legacy implementation remains available;
no performance threshold is added.

## Double-barrier analytic pricing

Rust `DoubleBarrierOption::new(DoubleBarrierOptionTerms::new(direction, strike,
start, end, lower, upper, barrier_type))?` constructs a European vanilla payoff
activated or terminated by the first inclusive touch of either fixed boundary.
`DoubleBarrierType::{KnockIn, KnockOut}` selects activation behavior. Finite
`0 < lower < upper` is required (`InvalidParameter`), including when the payoff
is already determined. Both boundaries share one schedule and strictly prior
`BarrierTouchState`. Rebate, timing, monitoring and history defaults come from
Rust's single-barrier terms: zero, expiry, continuous, empty dates and unknown.
Validation order, schedule/calendar rules, history replacement, ownership and
concurrency follow the single-barrier contract above. At-hit knock-in rebates
remain invalid even for zero amounts (`InvalidOption`).

`AnalyticDoubleBarrierEngine` requires explicitly flat risk-free and dividend
curves before validating life, scheduled trading dates and prior history. It
resolves current touches, past touches, finished monitoring, expiry and excluded
zero-rebate terminal payoffs before rejecting unresolved scheduled monitoring
with `UnsupportedOperation`. There is no discrete approximation or engine fallback.
A touched knock-in becomes vanilla; a touched knock-out pays at expiry or at
hit, with prior at-hit payments already paid. An untouched knock-in after the
last fixing receives its expiry rebate; an untouched knock-out becomes vanilla.

Continuous pricing retains the existing absorbing-density image/spectral series
switch at `volatility * sqrt(time) / log(upper/lower) = 0.5`, compensated summation
and logarithmic Gaussian tails. Knock-ins use vanilla-minus-knock-out parity.
Expiry rebates use surviving or exiting probability; at-hit rebates integrate
discounted first-exit probability, including negative rates and narrow boundary
layers. Unrepresentable calculations or failure to converge return `InvalidResult`.

All ten requested Greeks use shared scalar numerical differences and Rust bump
defaults. At expiry or a current boundary equality without a prior touch, all
Greeks are absent. Stencils reaching either boundary omit affected spot/mixed
Greeks; time stencils preserve the monitoring/history regime. Pricing failures
on feasible bumps propagate. `implied_volatility` retains the supplied bracket;
prices can be non-monotone, so bracket the desired root. Fixed cashflows and
excluded zero payoffs return `UnsupportedOperation`. Pricing, analysis and
solving preserve inputs on success and failure.

Python exports `DoubleBarrierOption` from `kiyosi.instruments` and
`AnalyticDoubleBarrierEngine` from `kiyosi.pricing`, with typed signatures.
`barrier_type` accepts `"knock_in"` or `"knock_out"`; all other choices and strict
representation protocols match the single-barrier API. Booleans are rejected as
numbers; integer controls reject floats and overflow. Domain errors use
`KiyosiError.category`, while representation failures retain native exceptions.
Schedule accessors return owned copies. Price, Greeks and solve calls release
the GIL around owned Rust inputs.

```python
from datetime import date
from kiyosi.instruments import DoubleBarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticDoubleBarrierEngine, implied_volatility

option = DoubleBarrierOption(option_type="call", strike=100,
    effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
    lower_barrier=80, upper_barrier=120, barrier_type="knock_out")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=date(2025, 1, 1))
engine = AnalyticDoubleBarrierEngine()
result = engine.price_with_greeks(option, context, ["delta", "gamma"])
assert abs(result.price - 1.0838814592583166) < 1e-9
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-7
known = option.replace(touch_state="untouched")
assert option.touch_state is None
assert known.upper_barrier == option.upper_barrier
```

## Binary-barrier and single-touch finite differences

Rust `FiniteDifferenceBinaryBarrierEngine` and Python
`kiyosi.pricing.FiniteDifferenceBinaryBarrierEngine` accept `BinaryBarrierOption`
and `TouchOption`: cash or asset payment, up/down knock-in or knock-out, and
continuous or scheduled observations. Binary barriers pay strictly in the money
at expiry. One-touch pays at hit or expiry; no-touch pays only at expiry.
At-hit no-touch remains an `InvalidOption` construction error in Rust; Python
no-touch factories reject the inapplicable keyword with native `TypeError`.

The engine uses the shared `FiniteDifferenceSettings` defaults (800 asset steps,
200 time steps, damped Crank–Nicolson, automatic upper boundary). It supports
flat and node risk-free/dividend curves. Curve knots and observation dates are
inserted into the time grid. Continuous barriers are exact spatial boundaries
using all configured asset steps; fitting an up barrier makes its grid independent
of the validated upper-boundary setting. Scheduled events average the trigger cell and restart implicit damping. Binary
knock-ins use a vanilla digital layer minus the surviving knock-out layer.
Explicit Euler rejects unstable grids with `InvalidParameter`; invalid or
clipped spatial domains use the same category. Refine the spatial/time grid and
upper boundary to verify accuracy, especially for short maturities and Greeks.

History, current observations and expiry use the existing core rules: unknown
strictly prior history is distinct from `untouched`, equality triggers a barrier,
and scheduled monitoring stops at the final fixing. Prior at-hit payments have
zero remaining value. Resolved touch cash flows use risk-free discounts for cash
and dividend discounts for asset delivery. Resolved binary payoffs use the
analytic digital engine. Resolution precedes spatial-grid validation and requires
only the curves relevant to the remaining payment. Unresolved scheduled at-hit
one-touch needs curve coverage only through the final observation; expiry
payments require expiry coverage. Life/calendar/history errors retain their
existing categories and leave contracts, contexts and settings unchanged.

`price_with_greeks` returns spline delta/gamma and fixed-grid speed, then uses
shared numerical completion for other requested Greeks. Bumped calculations
retain the central upper boundary. Greeks are absent at expiry and current
barrier equality without a prior touch; stencils crossing observations or
barriers leave affected measures absent. `implied_volatility` uses the existing
caller-supplied bracket and rejects volatility-independent cash flows with
`UnsupportedOperation`. Binary prices can have multiple volatility roots.
Python releases the GIL around pricing, Greek requests and solves using owned
Rust inputs. Engines are immutable and support concurrent calls.

```python
from datetime import date
from kiyosi.instruments import cash_one_touch_up
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import FiniteDifferenceBinaryBarrierEngine, implied_volatility

start = date(2025, 1, 1)
option = cash_one_touch_up(effective_date=start, expiry_date=date(2026, 1, 1),
                           barrier_level=120.13, payout=10,
                           settlement_timing="at_hit")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start)
engine = FiniteDifferenceBinaryBarrierEngine(asset_upper_boundary=400)
result = engine.price_with_greeks(option, context, ["delta", "vega"])
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.1, upper_bound=0.3)
assert result.delta > 0
assert abs(sigma - 0.2) < 1e-7
```

The Rust example on `FiniteDifferenceBinaryBarrierEngine::price_with_greeks`
shows the equivalent core API. All settings defaults and domain validation live
in Rust; Python uses the existing strict numeric/index conversions, owning
results and exception mapping.

## Double-barrier finite differences

Rust `FiniteDifferenceDoubleBarrierEngine` and Python
`kiyosi.pricing.FiniteDifferenceDoubleBarrierEngine` price existing
`DoubleBarrierOption` contracts with flat or node risk-free/dividend curves,
continuous or scheduled monitoring, knock-in/out payoffs and supported rebates.
The shared settings default to 800 asset steps, 200 time steps, damped
Crank–Nicolson and an automatic upper boundary; implicit and explicit Euler are
also available. Unstable explicit steps return `InvalidParameter`.

Continuous grids fit both physical barriers exactly and use all configured asset
steps between them. An explicit upper boundary must exceed spot, strike and both
barriers; it sets the spatial domain only for scheduled monitoring. Scheduled
grids run from zero to that upper boundary, insert all original observation dates
and curve knots, and project both boundary events simultaneously, including
barriers sharing a cell. Observation events restart implicit damping. Refine
the grid to verify numerical accuracy, particularly near boundaries and in narrow bands.

Equality at either monitored barrier touches. Strictly prior history remains
separate from the current observation, scheduled observations occur only at
midnight, and monitoring may finish before expiry. Known history and current or
expiry events resolve before grid validation. Previously paid at-hit rebates
have zero remaining value; remaining cash flows use exact curve discounts.
Life, calendar, history and curve-coverage errors use the same categories and
validation timing as single-barrier FD pricing. Failed operations preserve inputs.

`price_with_greeks` returns spline delta/gamma and fixed-grid speed, completing
other requested sensitivities with the shared numerical shifts on the central
spatial domain. Greeks are absent at expiry or current boundary equality without
a prior touch; stencils crossing either barrier or an observation omit affected measures. The shared
`implied_volatility` solver uses the supplied bracket and rejects
volatility-independent settlements with `UnsupportedOperation`. Double-barrier
prices need not be monotone in volatility, so the bracket must isolate the desired root.
Python uses the shared strict numeric/index protocols, owning results and core
exception mapping, and releases the GIL for pricing, Greeks and solving. Immutable
engines support calls on distinct or shared instances concurrently.

```python
from datetime import date
from kiyosi.instruments import DoubleBarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import FiniteDifferenceDoubleBarrierEngine, implied_volatility

start = date(2025, 1, 1)
option = DoubleBarrierOption(option_type="call", strike=100, effective_date=start,
    expiry_date=date(2026, 1, 1), lower_barrier=80, upper_barrier=120,
    barrier_type="knock_out")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start)
engine = FiniteDifferenceDoubleBarrierEngine()
result = engine.price_with_greeks(option, context, ["delta", "gamma"])
assert abs(result.price - 1.0838814592583166) < 0.001
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-7
```

See `FiniteDifferenceDoubleBarrierEngine::price_with_greeks` for the Rust example.

## Cash double-touch finite differences

Rust `DoubleTouchOption::cash(DoubleTouchOptionTerms::new(start, end, lower,
upper, kind), payout)` owns a cash double one-touch (`DoubleBarrierType::KnockIn`)
or double no-touch (`KnockOut`) contract. Both inclusive boundaries share one
observation schedule and strictly prior touch history. Payout must be finite and
positive, with finite `0 < lower < upper`. Construction validates payout, then
settlement compatibility, then boundaries, life and schedule. No-touch pays only
at expiry; at-hit no-touch returns `InvalidOption`. Defaults come from the core
single-touch terms: expiry payment, continuous monitoring, empty dates and unknown
history. `replace(history)` returns a validated owner without changing the original.
Schedule accessors borrow immutable storage for the owner's lifetime.

`FiniteDifferenceDoubleTouchEngine` supports cash contracts with continuous or
scheduled monitoring and flat or node risk-free/dividend curves. It reuses the
binary-barrier FD solver, double-barrier state validation and the shared
`FiniteDifferenceSettings`: 800 asset steps, 1600 time steps, Crank–Nicolson,
automatic upper boundary. Continuous barriers are exact grid edges, using all
asset steps between them. Scheduled observations project both boundary events
simultaneously on the full grid; rates, curve knots, event times, damping and
explicit stability checks follow the existing FD engines. An explicit spatial
upper bound must exceed spot and both barriers for unresolved monitoring.

A current monitored equality touches before grid construction. Unknown history
is distinct from untouched and is rejected when prior observations require it;
a claimed prior touch without any prior observation returns `InvalidOption`.
Current at-hit touches pay the payout immediately; prior at-hit touches are
already paid and have zero remaining value. Expiry-paid touch cash and no-touch
cash after successful monitoring use exact risk-free discounts. Resolved states
precede grid validation. At-hit scheduled pricing requires both curves only
through the last fixing; expiry payment requires coverage through expiry.
Life, calendar and history validation retain the existing categories and timing.

The engine provides price, spline delta/gamma, fixed-grid speed, other requested
Greeks through shared numerical completion, and shared implied volatility.
The central spatial domain stays fixed for bumps. All Greeks are absent at expiry
or current boundary equality without a prior touch; stencils crossing either
boundary or an observation omit affected measures. Invalid requests/shifts fail
before pricing. Implied volatility preserves the caller's bracket and rejects
volatility-independent remaining payments with `UnsupportedOperation`.
Inputs and settings remain unchanged on success and failure.

Python exports `DoubleTouchOption`, `cash_double_touch` and `cash_double_no_touch`
from `kiyosi.instruments`, and `FiniteDifferenceDoubleTouchEngine` from
`kiyosi.pricing`. Factories use keyword-only terms; no-touch rejects the
inapplicable settlement keyword with native `TypeError`. Numeric/date protocols,
choice strings, native representation errors and domain exception categories
follow the existing touch API. Results and returned schedules own their data.
Price, Greek and solve calls release the GIL around owned core inputs; shared
and distinct instances support concurrent calls under the crate contract.

```python
from datetime import date
from kiyosi.instruments import cash_double_touch, cash_double_no_touch
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import FiniteDifferenceDoubleTouchEngine, implied_volatility

start = date(2025, 1, 1)
terms = dict(effective_date=start, expiry_date=date(2026, 1, 1),
             lower_barrier=80, upper_barrier=120, payout=1,
             touch_state="untouched")
touch = cash_double_touch(**terms)
no_touch = cash_double_no_touch(**terms)
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.03, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start)
engine = FiniteDifferenceDoubleTouchEngine()
result = engine.price_with_greeks(touch, context, ["delta", "vega"])
assert abs(result.price - 0.6031839700294364) < 1e-5
assert abs(result.price + engine.price(no_touch, context) - 0.9704455335485082) < 1e-8
sigma = implied_volatility(engine, touch, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-6
```

See `DoubleTouchOption::cash` and
`FiniteDifferenceDoubleTouchEngine::price_with_greeks` for Rust examples.
Public acceptance tests retain the independent absorbing Brownian series,
conditional lognormal integrals, cash payment parity, near-boundary Greek and
grid convergence, history/settlement cases and concurrent-call checks.

## Double-barrier CPU Monte Carlo

Rust and Python `MonteCarloDoubleBarrierEngine` price `DoubleBarrierOption` with
flat risk-free and dividend rates. They share single-barrier CPU sampling defaults,
validation, exact path counts, Sobol dimensions, antithetic pairing and seeded
thread reproducibility. `step_count` is validated but does not change monitoring.
Continuous monitoring uses one terminal draw and conditional survival between
both boundaries, using the existing image/spectral Brownian-bridge series.
Scheduled monitoring samples the original future fixings and adds an unobserved
expiry leg only when monitoring ends early. Both boundaries share the schedule
and strictly prior touch history; equality touches either boundary.

Core life, calendar, history and resolved-cashflow rules match the analytic and
finite-difference double-barrier engines. Resolved vanilla payoffs use analytic
pricing. Continuous at-hit rebates reuse the existing first-exit integration;
scheduled at-hit rebates discount to the actual hit fixing. Prior at-hit rebates
are already paid. Knock-in at-hit rebates remain invalid even with zero rebate.
Node curves return `UnsupportedOperation` before request, life or solver checks.
Unrepresentable paths/results and nonconvergent survival series return
`InvalidResult`; failed calls preserve contracts, markets and engine settings.

`price_with_greeks` provides requested likelihood-ratio delta/gamma using the
price's samples, including continuous survival derivatives. Other Greeks are
absent. Expiry and spot stencils crossing either current monitored boundary
suppress delta/gamma. Numerical shifts also control the derivatives of continuous
at-hit rebates. Rust `numerical_greeks` provides separate scalar sensitivities.
No barrier `price_with_statistics` method or uncertainty interval is introduced.
Near-expiry likelihood Greeks can have substantial sampling error.

`implied_volatility` pins a request-local seed for seeded and unseeded engines.
It rejects volatility-independent remaining cashflows, including excluded-strike
knock-outs with zero rebate, using `UnsupportedOperation`. Barrier prices can be
nonmonotone in volatility and sampled scheduled prices can jump: the caller's
bracket must isolate a root, and solver tolerances apply to the sampled curve.
Results own their storage. Shared and distinct engines support concurrent calls
without changing global thread settings. Python releases the GIL around owned
core work, uses the shared strict numeric/index conversions and native
representation exceptions, and resumes signal handling on return. Resource
failures, worker panics and allocation exhaustion follow the existing CPU mapping.

```python
from datetime import date
from kiyosi.instruments import DoubleBarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import MonteCarloDoubleBarrierEngine, implied_volatility

start = date(2025, 1, 1)
option = DoubleBarrierOption(option_type="call", strike=100, effective_date=start,
    expiry_date=date(2026, 1, 1), lower_barrier=80, upper_barrier=120,
    barrier_type="knock_out")
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=start)
engine = MonteCarloDoubleBarrierEngine(path_count=32768, seed=42, sampling="sobol",
                                       antithetic=True, thread_count=4)
result = engine.price_with_greeks(option, context, ["delta", "gamma"])
assert abs(result.price - 1.0838814592583166) < 0.003
sigma = implied_volatility(engine, option, context, result.price,
                           lower_bound=0.15, upper_bound=0.25)
assert abs(sigma - 0.2) < 1e-5
```

See `MonteCarloDoubleBarrierEngine::price_with_greeks` for a Rust example.
Acceptance tests include independent QuantLib values, multi-seed Monte Carlo
budgets, series-switch cases, converged exit-series and fixing-date integrals,
finite-difference comparisons, history/event boundaries, fixed-cashflow inversion
rejections, odd path parity, thread reproducibility and strict Python conversions.
The legacy implementation remains available for migration comparison.

## Range-accrual coupons

Rust `RangeAccrualOption::new(RangeAccrualTerms)` owns a single-asset coupon.
`RangeAccrualTerms::new(notional, annual_coupon_rate, lower_bound, upper_bound,
effective_date, expiry_date)` defaults `realized_hit_count` to `None` (unknown).
Notional must be finite and positive, coupon rate finite and nonnegative, and
bounds finite with `0 <= lower_bound < upper_bound`. Negative hit counts return
`InvalidParameter`. Amounts, bounds and negative history are checked before
dates; expiry must be strictly after effective date (`InvalidTimeRange`).
Rust `.replace(Option<i32>)` and Python `.replace(realized_hit_count=...)` return
validated owning copies; `None` restores unknown history. Python omission retains
the count. Originals remain unchanged on success and failure.

`AnalyticRangeAccrualEngine` observes every context trading date in
`(effective_date, expiry_date]`, at midnight UTC, with **inclusive** spot bounds.
History counts hits strictly before valuation; an observation exactly at valuation
uses current spot. Prior observations require explicit history, and hits cannot
exceed the number of prior observations (`InvalidParameter`). Pre-effective
valuation is valid. Valuation after expiry returns `InvalidTimeRange` before
non-trading expiry returns `InvalidDate`, followed by history validation.

The sole payment is `notional * annual_coupon_rate * T * hits / M` at expiry,
where `T` is the full contract's ACT/365 Fixed year fraction and `M` counts all
contract trading observations. Calendar annualization bases do not change `T`.
There is no principal repayment or early termination. Pricing sums exact BSM
marginal range probabilities with deterministic risk-free and dividend curves,
then discounts the entire coupon to valuation from expiry. Log scaling and
compensated summation retain extreme but representable values; non-finite
transitions or prices return `InvalidResult`. Both curves must cover valuation
through expiry when future observations remain (`InvalidTimeRange`); curve knots
must remain distinguishable in valuation-relative pricing time (`InvalidResult`). At expiry
no curve coverage is needed. Zero coupons validate calendar and history first,
then return zero without curve coverage.

All ten Greeks use the existing numerical shifts and scalar prices. Every Greek
is absent at expiry. Spot stencils touching either current bound suppress spot
derivatives and mixed spot Greeks; vega and rho remain available. Time stencils
touching any trading observation suppress theta, charm and color, including
intraday stencils that would change the history regime. Stencils before effective
date and between weekend observations remain valid, subject to curve coverage
and knots. Empty requests and invalid shifts fail before pricing; infeasible
stencils leave measures absent, and feasible bumped failures propagate.

`implied_volatility` retains the shared bracket, validation order and failure
categories. Zero coupons and expiry return `UnsupportedOperation` after pricing
validation. Range coupons can be non-monotone in volatility: callers must supply
a bracket around the desired root. No root scan is added.

Python exports `RangeAccrualOption` from `kiyosi.instruments` and
`AnalyticRangeAccrualEngine` from `kiyosi.pricing`. Amounts use the shared strict
real-number protocol; hit counts use the shared integer protocol (excluding
booleans and floats, with `OverflowError` outside signed 32-bit range). Date
fields require `datetime.date`, excluding `datetime`. Representation/signature
errors remain native Python exceptions and precede domain validation. Properties
are read-only; prices and results are owned. Price, Greek and solve calls release
the GIL around owned Rust inputs and support shared and distinct concurrent
instances. Calls are synchronous without cancellation polling; Python signal
handling resumes when the call returns.

```python
from datetime import date
from kiyosi.instruments import RangeAccrualOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticRangeAccrualEngine, implied_volatility

option = RangeAccrualOption(notional=1000, annual_coupon_rate=0.12,
    lower_bound=95, upper_bound=110, effective_date=date(2025, 1, 1),
    expiry_date=date(2026, 1, 1))
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
    spot_price=100, valuation_time=date(2025, 1, 1))
engine = AnalyticRangeAccrualEngine()
result = engine.price_with_greeks(option, context, ["delta", "vega"])
sigma = implied_volatility(engine, option, context, result.price,
    lower_bound=0.1, upper_bound=0.4)
assert abs(sigma - 0.2) < 1e-8
assert option.replace(realized_hit_count=3).realized_hit_count == 3
assert option.realized_hit_count is None
```

Public Rust/Python tests migrate the existing history, terminal-payoff, curve,
long-contract, tiny-volatility, extreme-tail and numerical-Greek regressions,
using independent marginal/payoff references. The legacy implementation remains
available for migration comparison.

## Basic snowball quadrature

Rust `SnowballOption::new(SnowballTerms)` owns validated contractual terms and a
nonempty, strictly ordered vector of `AutocallableObservation` records. Each record
associates a date, a positive knock-out level, a finite annual knock-out coupon,
and a coupon barrier that must be zero for snowballs. `autocallable_observations`
associates parallel slices, rejecting unequal lengths with `InvalidSchedule`;
record values and schedule bounds are validated by the note constructor.
`SnowballTerms::new(maturity_coupon_rate, initial_spot, knock_in_level,
upper_strike, lower_strike, effective_date, expiry_date, observations)` defaults
to daily monitoring, unknown history and unit principal. Required positive
reference spot, knock-in level and upper strike, nonnegative principal and lower
strike, ordered strikes, and finite signed coupons use `InvalidParameter`.
Reversed life uses `InvalidTimeRange`; invalid schedules use `InvalidSchedule`.

`KnockInObservationMode::EveryTradingDay` observes at midnight on every context
trading date, including the effective date. `AtExpiry` observes only at expiry.
Knock-in requires **spot < level**; equality is intact. Knock-out requires
**spot >= level** on a contractual observation. On simultaneous events,
knock-out settles principal plus that observation's annual coupon multiplied by
Actual/365 Fixed time from the effective date. Otherwise expiry pays principal
plus the annual maturity coupon times the full contract term if intact, or
`principal + (clamp(spot, lower_strike, upper_strike) - upper_strike) / initial_spot`
if knocked in. Principal, coupons and losses are independent normalized
components; signed coupons and nonunit principal can produce negative values.

`barrier_state: None` means unknown history. `Some(AutocallableBarrierState::None)`
means known absence of events; `KnockedIn` and `KnockedOut` describe events
**strictly before valuation**. History becomes mandatory after the first knock-out
observation or daily knock-in observation. Missing history returns
`InvalidParameter`; a claimed event before monitoring began returns
`InvalidOption`, including any claimed prior knock-in for expiry-only monitoring.
`replace(history)` returns an owning validated copy and preserves all terms and
the original. Time consistency is checked during pricing, allowing corrections
and historical revaluations. A past knock-out has no remaining cashflows; its
price is zero. Current events occur at midnight; an intraday spot is not a fixing.

`QuadratureAutocallableEngine::new(QuadratureSettings)` retains the existing
cubic Gaussian integration algorithm, terminal affine-payoff integration and
separate intact/knocked-in layers on daily event dates. Defaults are 512 asset
steps and eight standard deviations. It accepts basic, binary and ternary snowballs and Phoenix notes.
Request-local continuation reuse supports scalar numerical Greeks; no mutable cache is shared between requests. Check spatial and integration-domain
convergence independently. Validation checks flat-rate applicability first
(`UnsupportedOperation` for node curves), then inclusive contract life,
observation trading dates, history, and trading expiry. Non-finite prices or
unrepresentable grids/transitions return `InvalidResult`. Validation also runs
for settled or fixed cashflows. Known fixed cashflows bypass numerical rollback.

All ten Greeks use the shared numerical analysis and Rust bump defaults. Expiry
Greeks are absent. Spot stencils crossing a current barrier are unavailable;
time stencils touching active observations are unavailable. Vega and rho remain
available at current barriers. Completed or economically irrelevant knock-in
monitoring does not suppress otherwise valid sensitivities. Implied volatility
uses the shared solver and caller's bracket unchanged; nonmonotone prices can
have multiple roots. Settled and fixed cashflows return `UnsupportedOperation`
after pricing validation because volatility is unidentifiable. Requests and
solves preserve their inputs on success and failure.

Python exports `AutocallableObservation`, `autocallable_observations`, and
`SnowballOption` from `kiyosi.instruments`, and `QuadratureAutocallableEngine` from
`kiyosi.pricing`. Constructors retain the existing record-based API and required
`knock_in_observation_mode` choice (`"every_trading_day"` or `"at_expiry"`).
`note.replace(barrier_state=...)` requires the history keyword; `None` clears it,
whereas `"none"` records no event. Numeric, integer, date and choice protocols
use the shared strict conversions and native representation exceptions. Defaults
come from Rust. Observation properties return owning lists; values are immutable.
Pricing, Greek and solve calls release the GIL over owned core data and support
concurrent shared or distinct instances. Calls are synchronous; signal handling
resumes on return to Python, with no cancellation polling inside rollback.

```python
from datetime import date
from kiyosi.instruments import SnowballOption, autocallable_observations
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import QuadratureAutocallableEngine, implied_volatility

start, end = date(2025, 1, 1), date(2025, 4, 1)
note = SnowballOption(maturity_coupon_rate=0.06, initial_spot=100,
    knock_in_level=80, upper_strike=100, lower_strike=60,
    knock_in_observation_mode="at_expiry", effective_date=start, expiry_date=end,
    observations=autocallable_observations([end], [110], knock_out_coupon_rates=[0.08]))
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3),
    spot_price=103, valuation_time=start)
engine = QuadratureAutocallableEngine()
result = engine.price_with_greeks(note, context, ["delta", "vega"])
sigma = implied_volatility(engine, note, context, result.price,
                           lower_bound=0.2, upper_bound=0.4)
assert abs(sigma - 0.3) < 1e-6
```

The candidate tests migrate signed and affine terminal portfolios using the
existing independent QuantLib reference, strict event boundaries, history and
calendar precedence, fixed cashflows, dense deterministic ladders, near-expiry
transitions, all ten sensitivities, grid/domain and bump convergence, solver
failures and concurrent reads. A separately refined conditional-payoff integral
checks two-event knock-in/knock-out ordering under a custom trading calendar.
Binary/ternary variants and named presets are described below. Implied coupon
solving is covered by a subsequent ticket.

## Phoenix quadrature

Rust `PhoenixOption::new(PhoenixTerms)` and Python `kiyosi.instruments.PhoenixOption`
use the same common terms, observation records, validation categories, calendar
rules, history replacement and ownership contracts as basic snowballs. Rust's
`SnowballOption` and `PhoenixOption` are aliases of `AutocallableNote<T>` for their
respective terms. `AutocallableTerms` is sealed; settlement rules remain in the
core. `PhoenixTerms::new(coupon_rate, initial_spot, knock_in_level, upper_strike,
lower_strike, effective_date, expiry_date, observations)` supplies daily monitoring,
unknown history and unit principal. Python requires the monitoring choice explicitly.

The finite signed `coupon_rate` is annualized over each **contractual observation
period** using Actual/365 Fixed: effective date to first observation, then previous
observation to current observation. A coupon is paid when **spot >= coupon barrier**,
including at equality, at knock-out and at expiry if expiry is an observation.
A zero barrier pays unconditionally. Each record's `coupon_barrier_level` must be
finite and nonnegative; its snowball `coupon_rate` field must be zero. Violations
return `InvalidParameter` when constructing the note. Phoenix has no coupon
memory and no maturity coupon. Knock-out pays principal plus the current conditional
coupon. Otherwise maturity pays principal plus any current conditional coupon,
less the same capped downside loss as snowballs when knocked in. Coupon barriers
may exceed knock-out levels, so knock-out can occur without a coupon payment.

Coupons strictly before valuation are settled and excluded. A current observation's
coupon is included once; intraday valuation excludes that midnight's coupon. A
seasoned note still uses the complete contractual schedule to determine period
lengths. Unknown history, known no-event history, past knock-out and currently
fixed cashflows retain their distinct validation and pricing behavior.

The same `QuadratureAutocallableEngine` integrates conditional coupons at their
own observation dates, with coupon thresholds included as grid breakpoints and
unconditional coupons retained in both tails and daily monitoring layers. It
supports all ten numerical Greeks and shared implied-volatility solving using
Rust defaults. Spot stencils crossing an active current coupon threshold are
unavailable for nonzero coupons; vega and rho remain available. Zero-rate or
zero-length coupons do not suppress spot Greeks. Time stencils touching observations
remain unavailable. Settled and volatility-independent cashflows return
`UnsupportedOperation` from implied volatility after pricing validation. There
is no root search beyond the caller's supplied bracket.

```python
from datetime import date
from kiyosi.instruments import PhoenixOption, autocallable_observations
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import QuadratureAutocallableEngine, implied_volatility

start, end = date(2025, 1, 1), date(2025, 4, 1)
note = PhoenixOption(coupon_rate=0.08, initial_spot=100, knock_in_level=80,
    upper_strike=100, lower_strike=60, knock_in_observation_mode="at_expiry",
    effective_date=start, expiry_date=end,
    observations=autocallable_observations([date(2025, 2, 3), end], [110, 105],
                                         coupon_barrier_levels=[90, 90]))
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3),
    spot_price=103, valuation_time=start)
engine = QuadratureAutocallableEngine()
result = engine.price_with_greeks(note, context, ["delta", "vega"])
sigma = implied_volatility(engine, note, context, result.price,
                           lower_bound=0.2, upper_bound=0.4)
assert abs(sigma - 0.3) < 1e-6
assert note.coupon_barrier_levels == [90, 90]
```

Python uses the same strict numeric/date/choice conversions, native representation
exceptions, immutable owning results and GIL release around owned Rust work as
snowballs. No Python objects or callbacks are accessed during pricing. Shared and
distinct instances support concurrent reads and pricing. Signal handling resumes
on return to Python. Tests migrate the independent QuantLib terminal portfolios,
conditional two-event payoff integral, signed coupon timing, unconditional tails,
daily/grid/domain and bump convergence, scalar sensitivities, solver failures,
history and calendar precedence, and strict Python representations.

## Snowball variants and named presets

`BinarySnowballOption::new(BinarySnowballTerms)` and
`TernarySnowballOption::new(TernarySnowballTerms)` use the same core observation,
validation, ownership, quadrature, Greeks and implied-volatility contracts as the
basic snowball. Python exports both classes from `kiyosi.instruments`.

Binary terms require a finite signed `maturity_coupon_rate`, life dates and
observations; there is no reference spot, strike or knock-in configuration.
A knock-out pays principal plus that observation's coupon times elapsed
Actual/365 Fixed years. Otherwise maturity pays principal plus the maturity
coupon times the full contract term. History is required only after a knock-out
observation, so a binary note can be valued after inception without history
before its first observation. A `knocked_in` history is rejected at construction
or replacement with `InvalidParameter`; failed replacement preserves the note.

Ternary terms add `minimum_coupon_rate`, positive `knock_in_level` and
`knock_in_observation_mode`. Knock-in is strict; knock-out remains inclusive and
has priority. If not knocked out, expiry pays principal plus either the maturity
coupon (intact) or minimum coupon (knocked in), times the full contract term.
There is no spot-dependent downside loss. Both coupons may be negative but must
be finite, and the minimum must not exceed the maturity coupon; violations use
`InvalidParameter`. Daily and expiry-only monitoring, recovered prior knock-ins,
calendar checks and history timing match basic snowballs. Equal coupons remove
economic knock-in sensitivity while retaining history validation.

Rust term constructors default to unknown history and unit principal, and
ternary terms default to daily monitoring. Python derives history and principal
defaults from Rust and requires an explicit monitoring choice for ternary notes.
All observation accessors return owned Python lists. Prices, all ten numerical
Greeks and implied volatility release the GIL and allow concurrent reads of
shared or distinct instances. Fixed cashflows retain `UnsupportedOperation` for
unidentifiable implied volatility. Native representation errors and stable core
error categories follow the basic snowball contract.

```python
from datetime import date
from kiyosi.instruments import (
    BinarySnowballOption, TernarySnowballOption, autocallable_observations,
    both_down_snowball,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import QuadratureAutocallableEngine

start, end = date(2025, 1, 1), date(2025, 4, 1)
observations = autocallable_observations([end], [110], knock_out_coupon_rates=[0.08])
binary = BinarySnowballOption(maturity_coupon_rate=0.06,
    effective_date=start, expiry_date=end, observations=observations)
ternary = TernarySnowballOption(maturity_coupon_rate=0.06, minimum_coupon_rate=0.02,
    knock_in_level=80, knock_in_observation_mode="at_expiry",
    effective_date=start, expiry_date=end, observations=observations)
preset = both_down_snowball(initial_coupon_rate=0.08, coupon_rate_decrement=0.01,
    initial_spot=100, knock_in_level=80, initial_knock_out_level=110,
    knock_out_level_decrement=2, observation_dates=[date(2025, 2, 3), end],
    effective_date=start, expiry_date=end)
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3),
    spot_price=100, valuation_time=start)
engine = QuadratureAutocallableEngine()
for note in (binary, ternary, preset):
    result = engine.price_with_greeks(note, context, ["delta", "vega"])
    assert result.delta is not None
```

Presets expand only in the Rust core, returning ordinary `SnowballOption` values
through the authoritative constructor. Rust uses
`SnowballPresetTerms::new(coupon_rate, initial_spot, knock_in_level,
knock_out_level, effective_date, expiry_date, observation_dates).expand(preset)`.
Common terms allow history and principal configuration before expansion; defaults
come from `SnowballTerms`. `coupon_rate` supplies the initial coupon for a
both-down ladder and the knock-out coupon for a dual-coupon note.

| Rust `SnowballPreset` | Python constructor | Expansion |
| --- | --- | --- |
| `Standard` | `standard_snowball` | Constant coupon and knock-out level; daily knock-in |
| `European` | `european_snowball` | Constant coupon and level; knock-in only at expiry |
| `StepDown { knock_out_level_decrement }` | `step_down_snowball` | Level at index `i` is initial level minus `i * decrement` |
| `BothDown { coupon_rate_decrement, knock_out_level_decrement }` | `both_down_snowball` | Both ladders decrease from initial values; maturity uses final coupon |
| `DualCoupon { maturity_coupon_rate }` | `dual_coupon_snowball` | Constant knock-out coupon and a separate maturity coupon |
| `Parachute { final_knock_out_level }` | `parachute_snowball` | Replaces only the last knock-out level |
| `Otm { upper_strike }` | `otm_snowball` | Replaces the upper terminal strike |
| `LossCapped { lower_strike }` | `loss_capped_snowball` | Replaces the lower terminal strike |

Python constructors are keyword-only and accept the existing market names:
`coupon_rate` and `knock_out_level`, except step-down uses
`initial_knock_out_level`, both-down uses `initial_coupon_rate` and
`initial_knock_out_level`, and dual-coupon uses `knock_out_coupon_rate`.
Each also takes its variant-specific fields shown above, `initial_spot`,
`knock_in_level`, `observation_dates`, life dates, optional `barrier_state` and
`principal_ratio`. Every preset starts with upper strike equal to initial spot,
lower strike zero and daily knock-in unless its convention overrides them.
Decrements use absolute price or decimal-rate units, starting at index zero;
negative decrements and signed coupons are accepted if all expanded terms pass
core validation. Empty, unordered or out-of-life dates are rejected by the same
constructor as explicit terms. Presets retain ordinary history replacement and
all pricing/analysis operations, with no extra engine or pricing fallback.

Public Rust and isolated Python tests compare every preset with explicit terms
and prices, and exercise binary/ternary terminal regions, signed coupons,
principal, all ten Greeks, volatility recovery, history and failure states,
calendar validation, strict representations and concurrent reads. Numerical
checks reuse the existing independent QuantLib terminal portfolios, signed
cash-digital and one-second regressions, and spatial/domain convergence for
multi-event binary and both ternary monitoring modes. Legacy implementations
remain available for migration comparison.

## Autocallable implied coupons

Rust `implied_snowball_coupon(engine, note, context, observed_price, convention,
settings)` accepts the three snowball types, including all expanded presets.
`CouponQuoteConvention::ShiftMaturityCoupon` shifts maturity coupon along with
observation coupons; `PreserveMaturityCoupon` fixes maturity coupon. The quoted
coupon is the **first contractual observation coupon**, including past records.
All observation coupons move by the difference from that first coupon, preserving
ladders, dates and barriers. The ternary minimum coupon stays fixed. A shifted
maturity coupon below that minimum is invalid, so choose valid endpoint bounds.
Rust `note.with_coupon(coupon, convention)` returns the corresponding owning,
validated snowball; Phoenix `note.with_coupon(coupon)` replaces its single annual
conditional coupon. Original notes and contexts remain unchanged on success and failure.

Rust `implied_coupon(engine, phoenix, context, observed_price, settings)` needs
no convention. Both functions accept `PricingEngine` implementations, including
`QuadratureAutocallableEngine`, `FiniteDifferenceAutocallableEngine` and
`MonteCarloAutocallableEngine`; custom engines retain their own operational and
concurrency contracts. Solves prepare one request through the engine boundary.

`ImpliedCouponSettings::default()` uses bounds `[0, 2]`, price tolerance `1e-8`,
coupon interval tolerance `1e-8`, and 100 bisection iterations. Signed finite
bounds are allowed and must be strictly ordered; tolerances must be finite and
positive and the iteration limit positive. Observed prices must be finite.
Settings and price validation precede endpoint replacement, which validates both
notes before pricing. Engine validation then retains its usual flat-rate, life,
calendar and history checks. Engine errors propagate, including `InvalidResult`.
Non-finite scalar trial prices or residuals return `SolverNonFinite`.

Equal endpoint prices return `UnsupportedOperation` for no quoted coupon exposure,
even when the observed price matches. This includes settled knock-outs and
terminal payoffs independent of the quoted coupon; expiry alone does not prevent
a solve when a current coupon or shifted maturity coupon remains payable.
Endpoint matches return the exact bound; same-sign residuals return
`UnbracketedCoupon` (Python `UNBRACKETED_COUPON`, 16). The solver retains the
quadrature coupon-price relationship: it first interpolates between endpoint
prices and verifies the resulting residual, then bisects if necessary. Either
price or interval tolerance stops bisection; exhausted iteration returns
`SolverNonConvergence`. Endpoint and interpolation evaluations do not consume
the bisection limit. No bracket expansion or root search is performed.

Python exports `implied_coupon` and the `CouponQuoteConvention` Literal alias
from `kiyosi.pricing`. Snowballs require the keyword-only `quote_convention`;
Phoenix rejects that keyword. Numeric controls use the strict real protocol;
`max_iterations` uses the integer index protocol, excluding booleans and floats,
with native `OverflowError` outside signed 32-bit range. Protocol/signature
errors remain native Python exceptions and precede core validation. Unknown
choice strings raise `ValueError`. Defaults derive from Rust. Calls release
the GIL around owned Rust inputs and have the synchronous signal-handling and
concurrency contract described above.

```python
from kiyosi.pricing import QuadratureAutocallableEngine, implied_coupon

engine = QuadratureAutocallableEngine()
quote = engine.price(snowball, context)
coupon = implied_coupon(engine, snowball, context, quote,
                        quote_convention="preserve_maturity_coupon")
assert abs(coupon - snowball.knock_out_coupon_rates[0]) < 1e-7
phoenix_quote = engine.price(phoenix, context)
phoenix_coupon = implied_coupon(engine, phoenix, context, phoenix_quote)
assert abs(phoenix_coupon - phoenix.coupon_rate) < 1e-7
```

## Snowball and Phoenix finite differences

`FiniteDifferenceAutocallableEngine` prices `SnowballOption`, `BinarySnowballOption`,
`TernarySnowballOption`, `PhoenixOption`, and all snowball presets, which expand to
ordinary core notes. It accepts flat and node risk-free/dividend curves. No other
engine is selected for unsupported configurations. Constructor defaults are **800
asset steps, 2,000 time steps, damped Crank–Nicolson, and an automatic upper domain**.
Rust `new(FiniteDifferenceSettings::default())` explicitly selects the shared FD
settings (200 time steps); Python omitted fields derive from the core engine defaults.

The rollback retains intact and knocked-in layers when daily monitoring affects
settlement. Exact future trading dates, contractual observations, expiry and curve
knots enter the time grid. Explicit Euler retains daily subdivisions even when
knock-in cannot affect settlement, and rejects unstable grids. Implicit Euler and
Crank–Nicolson concentrate nodes around contractual levels, suppress sub-resolution
anchors without changing event thresholds, and project discontinuous future events.
Crank–Nicolson restarts four implicit half-steps after each event. Short initial
intervals receive extra time resolution. Automatic domains use the existing
volatility/carry expansion; expanded explicit domains use sinh coordinates.
Explicit upper domains must exceed spot and all product levels. Check convergence
when changing the grid, domain, volatility or distance to an event.

Payments, normalized units, daily/expiry knock-in, inclusive knock-out and Phoenix
coupon thresholds, signed coupons, principal/downside, history and immutable
replacement follow the same core note semantics as quadrature. Current events
resolve against actual spot before allocating a grid. Known fixed cashflows bypass
the grid after contract/history validation; node-curve discounts are supported.
Validation checks inclusive life, trading observations, prior history, and trading
expiry before curve coverage and grid feasibility. Existing structured error
categories and failure-state guarantees apply; originals remain unchanged.

All ten Greeks are available when their stencils are feasible. Delta/gamma use the
solved layer's cubic spline and speed differences spline gamma; other measures use
the shared numerical completion and core bump defaults. Native delta/gamma do not
require feasible scalar spot bumps; speed does. Bumped pricing fixes the central
upper domain. Expiry Greeks and stencils crossing active spot/time events
are absent; vega and rho remain available at current thresholds. Feasible bumped
failures fail the request.

`implied_volatility`, `implied_snowball_coupon` (Rust), and `implied_coupon` use the
existing solver controls, explicit snowball quote conventions, residual-checked
coupon interpolation and bisection fallback. Fixed settlements and quotes without
remaining parameter exposure return `UnsupportedOperation`; bracket, convergence
and non-finite failures retain their structured categories. Solvers do not mutate
notes, contexts or engine settings. Engines own immutable settings and support
concurrent shared/distinct calls; Python price, Greek and solver calls release the
GIL only around owned Rust inputs. Python native representation errors, strict
numeric/index conversion, exception mapping and cancellation behavior follow the
candidate-wide contracts above.

```python
from kiyosi.pricing import FiniteDifferenceAutocallableEngine, implied_coupon

engine = FiniteDifferenceAutocallableEngine()
result = engine.price_with_greeks(note, context, ["delta", "gamma", "vega"])
coupon = implied_coupon(engine, note, context, result.price,
                        quote_convention="preserve_maturity_coupon")
```

For Phoenix omit `quote_convention`. Rust construction and pricing are illustrated
in `FiniteDifferenceAutocallableEngine::price_with_greeks` rustdoc. Candidate tests
retain the migrated accuracy budgets for nearby barriers, one-second-to-expiry
prices/Greeks, payoff bounds, smooth extrema, domain/space/time refinement, event
restarts, history, and all four note variants. Independent analytic portfolios,
QuantLib references and quadrature comparisons check numerical accuracy; solver
cases check recovery, residuals, state-layer changes and concurrent isolation.

## Snowball and Phoenix CPU Monte Carlo

Rust and Python `MonteCarloAutocallableEngine` price `SnowballOption`,
`BinarySnowballOption`, `TernarySnowballOption`, `PhoenixOption` and all eight
expanded snowball presets. `price`, `price_with_statistics` and
`price_with_greeks` use the existing shared settlement and history rules:
strict knock-in, inclusive knock-out and Phoenix coupon barriers, normalized
principal and capped downside payments, signed coupons, current events once,
and zero remaining value after a historical knock-out. Phoenix coupons accrue
from the previous contractual observation, even when that observation is past,
and discount separately at their payment dates.

Settings and defaults are `MonteCarloSettings` in Rust and keyword arguments in
Python: 100,000 paths, `seed=None`, automatic threads, pseudorandom sampling,
and no antithetic pairing. `step_count` retains shared validation and its default
50 but does not change the event grid. Exact lognormal BSM transitions use
Actual/365 Fixed intervals from valuation through each remaining trading day
when daily knock-in can affect settlement. Already knocked-in notes, expiry-only
knock-in and notes without knock-in exposure use only remaining observations
plus expiry. Valuation on a non-trading day does not trigger knock-in; intraday
valuation skips that day's already-past midnight events. Observation and expiry
dates must be trading days in the context's calendar.

The engine accepts flat rates, including dated flat curves; node curves return
`UnsupportedOperation` before life, history or Greek-request validation. Other
validation timing and error categories match autocallable quadrature. Complete
normal vectors are generated before evaluating each path, so early knock-out
cannot shift later paths' random draws. Pseudorandom, scrambled Sobol,
antithetic pairing, exact actual path counts and thread caps reuse the shared
CPU sampler. Sobol needs power-of-two paths and at most 21,201 remaining path
dimensions. A fixed seed reproduces prices and statistics across thread counts
within a build; it need not reproduce the legacy C++ random sequence.

Statistics use the shared `MonteCarloEstimate`: unbiased sample variance and an
asymptotic 95% normal interval for pseudorandom samples, independent pair means
for even antithetic counts, and absent uncertainty for Sobol, insufficient
samples or odd antithetic counts. Known pseudorandom settlements and fixed
cashflows have zero uncertainty, including before expiry with one path.

All ten Greeks use scalar finite differences with shared normal draws and the
core bump defaults. Expiry Greeks and infeasible stencils are absent. Spot/time
stencils crossing active current events are unavailable; irrelevant or already
completed knock-in does not suppress them. Discontinuous payoffs can make small
bumps noisy. `implied_volatility`, `implied_snowball_coupon` and `implied_coupon`
reuse the core solvers. Each analysis or solve pins one request-local seed when
none is configured. Coupon endpoints replay the same paths when their grids
agree; if a coupon removes knock-in exposure or fixes a payment, each endpoint
retains its own event-grid shortcut, matching the existing semantics.

Solver tolerances constrain the **sampled price curve**, not sampling error.
Volatility need not be monotone; brackets are retained without root search, and
a sampled digital-like curve may have flat intervals or multiple roots. Fixed
cashflows reject implied volatility with `UnsupportedOperation`; equal coupon
endpoint prices reject coupon solving with that category even for a matching
quote. Inputs and engine settings remain unchanged on success and failure.

Python prices, statistics, Greeks and solves release the GIL around owned Rust
inputs with no Python callbacks. Shared and distinct engines support concurrent
calls with request-owned workers and buffers. Calls are synchronous without
cancellation polling; signal handling resumes when Python regains control.
Entropy and worker-creation failures retain native I/O errors (`OSError` in
Python), worker panics retain native behavior, and allocation exhaustion follows
the allocator. Representation errors retain native Python exceptions, including
rejection of booleans, fractional integer controls and integer overflow.

```python
from kiyosi.pricing import MonteCarloAutocallableEngine, implied_coupon

engine = MonteCarloAutocallableEngine(
    path_count=65536, seed=42, antithetic=True, thread_count=4)
estimate = engine.price_with_statistics(phoenix, context)
analysis = engine.price_with_greeks(phoenix, context, ["delta", "rho"], spot_shift=0.5)
coupon = implied_coupon(engine, phoenix, context, estimate.price)
assert abs(coupon - phoenix.coupon_rate) < 1e-7
sobol = MonteCarloAutocallableEngine(path_count=32768, sampling="sobol", seed=42)
assert sobol.price_with_statistics(phoenix, context).standard_error is None
```

Rust uses `MonteCarloAutocallableEngine::new(MonteCarloSettings { seed: Some(42),
..Default::default() })`; its rustdoc includes a complete usage example. Tests
cover independent cash/digital and QuantLib terminal portfolios, all four note
families against quadrature under both monitoring modes, migrated cashflow and
shared-path regressions, coupon endpoint exposure changes, native Python
protocol boundaries, and exact thread consistency. No runtime threshold is used.

## Accumulator quadrature

`Accumulator` owns a purchase strike, inclusive knock-out level, daily quantity,
below-strike acceleration factor, life dates and accrued quantity. Strike and
knock-out must be finite and positive; quantities and the multiplier must be
finite and nonnegative. Reversed life dates are rejected; equal dates are valid.
Invalid strike returns `InvalidStrike`, other invalid amounts `InvalidParameter`,
then reversed dates `InvalidTimeRange`. Accrued quantity defaults to **zero in
Rust**, also supplying Python's default. Zero is known history, and fractions
are valid: there is no unknown-history sentinel. History is a caller assertion;
corrections and historical revaluations remain allowed without inferred bounds
on quantity or a history record date. Python rejects explicit `None`.

Rust constructs `Accumulator::new(AccumulatorTerms::new(strike, knock_out_level,
daily_quantity, acceleration_factor, effective_date, expiry_date))`. Set
`AccumulatorTerms::accumulated_quantity` or use `.replace(quantity)` for history.
Python `.replace(accumulated_quantity=quantity)` requires that keyword and accepts
only history replacement. Replacement validates before returning an owning value;
the original and inputs remain valid on both success and failure.

`QuadratureAccumulatorEngine` observes the context calendar's trading dates at
midnight UTC, including the effective date and expiry. Each observation first
checks `spot >= knock_out_level`: it settles all accrued units at
`quantity * (spot - strike)` without a new purchase. Otherwise it adds
`daily_quantity * acceleration_factor` strictly below strike, or `daily_quantity`
at and above strike. Expiry settles after its observation. At an exact trading
midnight valuation the current event is processed once; intraday and non-trading
valuations introduce no current event. Prices exclude previously settled
cashflows; callers must not value an already terminated contract.

The engine checks flat rates (`UnsupportedOperation` for node curves, including
at expiry), inclusive contract life (`InvalidTimeRange`), and trading expiry
(`InvalidDate`), in order, before resolving current cashflows. It reuses the
existing cubic Gaussian quadrature with separate slope/intercept layers in
accrued quantity, exact terminal cash/asset moments, barrier-aligned cells and
affine low-price tails. Shared `QuadratureSettings` defaults are 512 spatial steps
and eight standard deviations. A single remaining observation uses exact moments,
independent of the spatial budget. Non-finite transitions, grids and prices return
`InvalidResult`. Signed prices, negative rates, zero purchases, acceleration zero
or one, and knock-out at or below strike remain supported.

All ten Greeks use the shared numerical defaults and error handling. Expiry
Greeks are absent; spot stencils crossing current knock-out or active acceleration
thresholds and time stencils crossing any trading observation are unavailable.
Vega and rho remain available at current thresholds. Curve reuse is confined to
a request and agrees with independent repricing. `implied_volatility` retains
the caller's bracket and shared solver errors; prices can be non-monotone.
It rejects unidentifiable volatility at expiry, current knock-out, zero exposure,
and fixed holdings with no purchases and zero rates. Inputs remain unchanged.
Core values are `Send + Sync`; shared reads and pricing are safe. Python values
are immutable, conversions use the strict protocols above, and pricing, Greeks
and solves release the GIL around owned core inputs only.

```python
from datetime import date
from kiyosi.instruments import Accumulator
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import QuadratureAccumulatorEngine, implied_volatility

option = Accumulator(strike=100, knock_out_level=110, daily_quantity=1,
    acceleration_factor=2, effective_date=date(2025, 1, 1),
    expiry_date=date(2025, 1, 6)).replace(accumulated_quantity=3)
context = PricingContext(model_parameters=BlackScholesMertonParameters(
    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.4),
    spot_price=100, valuation_time=date(2025, 1, 4))
engine = QuadratureAccumulatorEngine()
result = engine.price_with_greeks(option, context, ["delta", "vega"])
assert result.delta is not None and result.vega is not None
sigma = implied_volatility(engine, option, context, result.price,
    lower_bound=0.2, upper_bound=0.6)
assert abs(sigma - 0.4) < 1e-6
assert engine.price(option, context.replace(spot_price=90,
    valuation_time=option.expiry_date)) == -50
```

Rust and Python tests cover shared contract/history/error scenarios, exact
terminal portfolios, an independently integrated two-event reference, the
existing representative spatial/domain and Greek/bump convergence tolerances,
quantity affinity, tiny-volatility tails, concurrent reads and request isolation.
Accumulator Monte Carlo arrives in a later ticket; its legacy cross-engine tests
remain with the legacy implementation.

## Accumulator finite differences

Rust and Python expose `FiniteDifferenceAccumulatorEngine` for the same
`Accumulator` contracts, accrued quantity, trading calendar and current-event
semantics described above. It accepts flat and node rate curves. It checks
inclusive contract life and trading expiry before resolving immediate settlement;
only future cashflows require curve coverage and a spatial grid. Immediate
settlement therefore bypasses clipped domains and incomplete curve horizons.
History replacement and every pricing, Greek and solve operation preserve inputs
on success and failure.

The engine reuses `FiniteDifferenceSettings`: **800 asset steps, 200 time steps,
damped Crank–Nicolson and an automatic upper boundary**, with Python defaults
derived from Rust. Explicit and implicit Euler are also available. Invalid
settings return `InvalidParameter` at construction. Pricing rejects an upper
boundary that does not exceed spot, strike and knock-out, or an unstable explicit
grid, with `InvalidParameter`. Non-finite computed prices or derivatives return
`InvalidResult`. Life/curve violations return `InvalidTimeRange`; non-trading
expiry returns `InvalidDate`. Python representations retain native `TypeError`,
`ValueError` and `OverflowError` through the existing strict protocols.

The migrated algorithm evolves slope and intercept in accrued quantity on a
uniform asset grid. Exact trading observations and curve knots split the time
grid. Terminal payoffs are cell averaged across strike and knock-out; later
events project only the jump correction, preserving smooth continuation.
Crank–Nicolson restarts two intervals of implicit half-step damping after each
event. At the upper boundary the affine asset slope discounts at the dividend
yield and its cash intercept at the risk-free rate.

Delta and gamma use spline derivatives; speed differences spline gamma. Other
Greeks use shared numerical completion, including native delta/gamma for mixed
derivatives. Greek requests hold the central upper domain fixed for bumped
valuations. All ten measures have the existing units, bump validation, selective
requests and event availability rules; expiry returns price with no Greeks.
Vega and rho remain available at current thresholds. Scalar `numerical_greeks`
is also available through Rust's `PricingEngine<Accumulator>` implementation.
`implied_volatility` keeps the supplied bracket, solver settings, error categories
and unidentifiable-payoff rules shared with accumulator quadrature. Prices can
be non-monotone in volatility; a bracket selects a root or returns
`UnbracketedVolatility`.

```python
from kiyosi.pricing import FiniteDifferenceAccumulatorEngine, implied_volatility

# Reuses the accumulator and market from the quadrature example above.
engine = FiniteDifferenceAccumulatorEngine(asset_step_count=1600,
    time_step_count=800, asset_upper_boundary=440)
result = engine.price_with_greeks(option, context, ["delta", "gamma", "vega"])
assert result.delta is not None and result.gamma is not None
sigma = implied_volatility(engine, option, context, result.price,
    lower_bound=0.2, upper_bound=0.6)
assert abs(sigma - 0.4) < 1e-6
```

Core and Python values support concurrent reads and shared-engine pricing.
Python pricing, Greeks and solves release the GIL around owned Rust inputs;
signal handling resumes on return to Python. Public API tests cover the same
history, timing, validation, failure-state, curve and identifiability cases in
both languages. Migrated numerical regressions check terminal price/Greek
portfolios, all schemes, daily-event damping, spline/scalar consistency, mixed
Greeks, and live QuantLib terminal portfolios with independent space, time and
domain refinement. Explicit domains and coarse grids still require convergence
checks appropriate to the contract.

## Checks

From the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
cargo doc --no-deps
cargo package -p kiyosi-core
PYO3_PYTHON=/nonexistent/python cargo run --manifest-path tests/rust-consumer/Cargo.toml
ruff check rust/python
ruff format --check rust/python
ty check rust/python/python
maturin build --manifest-path rust/python/Cargo.toml --interpreter .venv/candidate/bin/python
maturin sdist --manifest-path rust/python/Cargo.toml
```

Install the built wheel into the candidate environment before running its tests.
Also rebuild a wheel from the sdist in a temporary directory to check that the
workspace and core path dependency are included. `cargo package --allow-dirty`
is useful while developing; it still verifies the packaged core independently.
For core-only build/test/doc/package checks, setting `PYO3_PYTHON` to a nonexistent
path demonstrates that Python discovery is not involved. The separate consumer
has its own workspace and uses only the public crate API.

Install the unchanged root `.[test]` into `.venv/legacy` separately, then run:

```sh
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_european.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_market.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_calendars.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_analytics.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_digital.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_american.py'
.venv/legacy/bin/python -m unittest discover -s rust/python/tests -p 'test_binomial.py'
.venv/legacy/bin/python -m unittest discover -s tests/python -p 'test_*.py'
```

The shared market suite migrates the applicable legacy curve and replacement
contracts, adds independent live QuantLib curve pricing and parity, and checks
coverage timing, representation errors, ownership and concurrent pricing. The
Rust public API tests cover equivalent market scenarios. Other product/engine
cases remain in the legacy suite.

The shared CRR suite checks explicit rollback across small and large grids,
odd/even and off-strike convergence, negative carry, immediate exercise,
strict digital boundaries, deterministic endpoint probabilities, tail weights,
configuration and representation errors, volatility bracket intersection and
concurrent pricing. It reuses the existing European and American CRR QuantLib
profiles and their error budgets; near-expiry European scenarios excluded from
the numerical profile retain price and node-Greek checks only.

The shared analytics suite checks all 20 European scenarios against live QuantLib
Greeks and scalar price differences at two spot bump sizes, using existing error
budgets and independently checked oracle uncertainty. It covers volatility
recovery, mixed requests, availability, validation order, strict representations,
expiry, curve knots, ownership and concurrent solves. Rust public API tests cover
equivalent scenarios plus reusable numerical analysis, collapsed and infeasible
stencils, feasible bump failures, non-finite solver evaluations and
high-precision extreme-scale derivative regressions.

The shared digital suite covers all 40 existing digital QuantLib scenarios with
the existing native and numerical error budgets, and scalar-price differences at
two spot bumps away from expiry. It migrates strict terminal strike boundaries,
valuation timing, construction, request/shift validation, solver errors, curve
coverage and knots, concurrent reads, large-scale speed, tiny-volatility gamma,
normal-tail delta and discount/settlement overflow and underflow regressions.
Rust public API tests exercise equivalent contracts and reusable scalar numerical
analysis. Candidate-only digital tests verify owning replacement and failure state;
the legacy implementation has no digital contract replacement method.

The shared calendar suite reuses existing custom-calendar contracts and checks
counting, adjustment, schedule endpoints, month clamping, large offsets,
conversion errors, ownership, iteration and concurrent reads. It checks every
date in the SSE data range against the generated Rust table in both isolated
implementations. Generator tests cover the upstream forecast boundary and Rust
rendering. Calendar data and its attribution ship in the standalone Cargo crate
and candidate Python artifacts; Python is needed only to regenerate the data.

The shared European suite covers the same domain and representation cases in
both processes. It reuses all 20 existing European scenarios and the independent
live QuantLib 1.43 oracle, restoring QuantLib settings after each scenario.
Additional Rust and Python cases retain high-precision tiny-volatility/tail
regressions, extreme scales, validation timing, expiry boundaries, owning values,
and shared/distinct-instance concurrency. Candidate-only tests check installed
metadata, exports, category values and stub members. Tests require QuantLib;
missing reference dependencies fail discovery.

Windows and all CPython versions require separate build/test runs; a local Linux
run does not establish that delivery matrix. No release or package publication
is part of these tickets.
