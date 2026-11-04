"""Price contracts, compute sensitivities, and solve implied parameters.

All engine ``price_with_greeks`` methods accept the same keyword-only controls:
``spot_shift=None``, ``volatility_shift``, ``rate_shift`` and ``time_shift_days``.
All controls are validated before pricing, including controls a native estimator
does not use; its supported Greek set is unchanged. At expiry the spot bump
controls terminal-payoff stencils. Use ``numerical_greeks`` to select scalar
differences explicitly.

Finite-difference engines support local volatility for European and American
vanilla options, digitals, single and double barriers, binary barriers, touch
and no-touch options, autocallable notes and accumulators. They provide price,
delta and gamma, holding the absolute surface fixed. Other requested Greeks remain None, including in "all" requests.
Other engine families and implied-parameter solvers raise UnsupportedOperationError
for this model. Digital finite-difference pricing requires constant risk-free
rates and dividend yields. Delta and gamma are absent when their
finite-difference stencils cross product events, including terminal payoff kinks.
Surface coverage follows remaining stochastic evolution; rates must cover
required discounting. Automatic domains use the maximum interpolated local
volatility over the remaining stochastic horizon. Check time, space and boundary
convergence for local-volatility domains, including automatically selected domains.

``FiniteDifferenceScheme`` selects ``"explicit_euler"``, ``"implicit_euler"``,
``"crank_nicolson"`` (without time-step damping), or ``"rannacher"`` (the default).
Rannacher replaces each of the first two time intervals with two implicit Euler
half-steps and restarts after scheduled barrier, accumulator and autocallable
observations. Automatic European vanilla and local-volatility digital domains
concentrate spatial nodes around the strike for non-explicit schemes; BSM
digitals and explicit upper boundaries retain uniform spot grids.
Concentrated vanilla grids average the payoff kink;
digital terminal payoffs are cell-averaged on either grid. American grids retain
their sinh coordinates. One-dimensional finite-difference engines share defaults
of 800 spatial and 400 time steps; observations and damping can add substeps. Check convergence before
reducing grid counts further.
Continuous barrier grids use all ``asset_step_count`` intervals on the actual
domain between barrier edges. Continuous up and double barriers fix the upper
grid edge, so ``asset_upper_boundary`` is inactive for that barrier grid;
scheduled monitoring and local-volatility knock-in vanilla components still use
the full domain.
Rust Monte Carlo ``sampling_settings()`` returns sampling controls only; preserve
``step_count()`` separately when reconstructing vanilla or Asian engines with
``with_step_count``. Python constructors expose both sampling and grid controls.

Monte Carlo ``step_count`` counts time intervals for both American exercise
and arithmetic Asian integration; American pricing needs at least two intervals.
``MonteCarloSampling`` selects
``"pseudo_random"`` or scrambled ``"sobol"``; a fixed seed reproduces draws
across thread counts in the same build. Sobol estimates omit sampling error.
Every Monte Carlo engine exposes ``price_with_statistics`` for its supported
contracts; vanilla statistics require European options. It returns the same
seeded price as ``price`` and also computes and validates sampling uncertainty.
Sobol, fewer than two independent observations and odd antithetic counts omit
uncertainty; known pseudorandom analytic branches have zero error. Sampling error
does not measure arithmetic Asian integration bias or other approximation error.
Future Monte Carlo barrier, knock-in, knock-out, coupon and acceleration tests
retain log-price movements even when the simulated spot rounds to a threshold.
Single- and double-barrier Monte Carlo payoffs also retain movements when
terminal spot rounds to the strike.
Current observations continue to use the supplied spot.

Autocallable quadrature extends its spatial domain for forward drift and the
backward movement of event boundaries. Check spatial resolution and integration
extent convergence separately, especially for low volatility and daily monitoring.

``QuadratureVanillaEngine`` prices European and American calls/puts with flat-rate
BSM parameters. Controls are ``asset_step_count=512`` (at least 4),
``time_step_count=200`` (at least 1) and ``integration_standard_deviations=8``
(finite and positive). European pricing uses composite Simpson integration;
American pricing uses Gaussian backward induction with exercise at valuation,
uniform time points and expiry. The time count specifies intervals and is ignored
for European pricing. Check time, space and extent convergence separately;
fixed-grid completion has no error estimate. Numerical Greeks use shared bump
defaults; higher derivatives near exercise boundaries also require bump checks.

``QuadratureDigitalEngine`` prices European cash-or-nothing and asset-or-nothing
calls/puts under flat-rate BSM. It uses Simpson integration with
``asset_step_count=512`` and ``integration_standard_deviations=8``; the extent
is centered on the payment-weighted normal density and omitted tails are zero.
Check step-count, extent and numerical Greek bump convergence separately;
completion provides no error estimate. Equality at expiry pays zero.

Pricing raises ``ComputationFailureError`` for materially negative option
prices. Settled barrier cash flows do not require curve coverage through expiry;
remaining payments and stochastic evolution retain their coverage checks. Only
rounding-sized negative results may be clipped to zero.
Coarse numerical grids may require refinement before pricing succeeds.
American Monte Carlo also rejects non-finite continuation regressions;
finite fits with insufficient independent samples use a lower polynomial degree.
American exercise-grid pricing limits retained path storage to 1 GiB per call.
Storage is ``8 * path_count * step_count`` bytes, with odd antithetic
path counts rounded up to the next even count for reserved capacity; other
buffers are additional. Larger grids raise ``InvalidInputError`` before sampling.
Reduce either count to fit. Expiry and terminal-only American calls (nonpositive
dividends and nonnegative rates) do not allocate this grid or apply this limit.

``AnalyticEuropeanEngine``, ``CoxRossRubinsteinDigitalEngine``,
``CoxRossRubinsteinBarrierEngine``, ``MonteCarloDigitalEngine`` and
``MonteCarloAsianEngine`` compute Greeks directly without numerical bumps
before expiry. Their shared shift controls are validated but do not affect
these pre-expiry estimates. At expiry, ``spot_shift`` controls terminal-payoff
stencils. Engines that use numerical differences apply the relevant bumps:
``volatility_shift`` controls the volatility bump for vega and volga;
``rate_shift`` controls parallel bumps of both the risk-free and dividend-yield
curves for rho and dividend_rho. Numerical volga requires a strictly positive
lower volatility leg. Monte Carlo volga can amplify sampling and exercise noise;
check convergence across bump sizes and simulation settings.

``MonteCarloBarrierEngine`` and ``MonteCarloDoubleBarrierEngine`` return only
delta and gamma. They use ``spot_shift`` for barrier
availability checks and numerical derivatives of continuous at-hit rebates.
The other shared shift controls are accepted and validated but do not affect
these estimates.
For these rebates, an unavailable numerical stencil leaves delta and gamma
as None for the whole contract. Choose a spot bump that gives distinct,
positive spots on both sides of the valuation spot.

Pass ``greeks="all"`` to request every measure, a Greek name for one measure,
or an iterable of names for a selection. The ``greeks`` argument is required;
unavailable measures remain None. Inspect ``result.status(name)`` to distinguish
unrequested, unsupported and undefined measures; ``result.requested`` and
``result.supported`` expose the request and engine/model coverage.

At expiry, supported spot sensitivities use terminal-payoff stencils. Supported
vega, volga, rho and dividend rho are zero because no stochastic evolution or
discounting remains. Vanna and zomma are zero where spot derivatives exist.
Theta, charm and color remain None at expiry. Spot stencils crossing a payoff
kink or product event remain unavailable, and engine/model coverage still applies.
Automatic terminal spot bumps use 0.01 spot units, including American Monte Carlo;
explicit bump controls retain their usual meaning.

Every engine shares ``price(instrument, context)`` and
``price_with_greeks(instrument, context, greeks)`` with the same keyword-only
shift controls. For a uniform BSM numerical calculation, call
``numerical_greeks(engine, instrument, context, greeks, spot_shift=...)``.
It uses scalar prices and accepts the same bump controls for every supported
engine/product pair. It can compute measures beyond a native engine's coverage;
local-volatility and Heston models remain unsupported. Monte Carlo sampling is
pinned within each numerical request. All numerical bumps are validated.

Pricing controls are explicit keyword-only parameters, available through
``inspect.signature``, with defaults derived from Rust. The displayed
defaults can be passed back explicitly. Instrument conversion and supported-family checks
precede Greek-name parsing. Monte Carlo vanilla spot bumps default by product:
0.01 for European options and 8% of spot for American options before expiry. Explicit
``None`` selects the automatic bump for either product. Other numerical engines
use ``None`` to select the shared 0.01 spot-unit bump. Rust
``NumericalShiftSettings.spot_shift=None`` uses the same engine-specific defaults.

``AnalyticHestonEuropeanEngine`` prices European calls/puts with ``HestonParameters``
using adaptive Fourier integration and a deterministic-variance control variate.
It supports deterministic rate/dividend curves, zero parameters and correlation
endpoints without imposing the Feller condition. Requested Greeks remain None; implied-parameter solvers
raise ``UnsupportedOperationError``. Integration controls are
``absolute_tolerance=1e-8`` (price units), ``relative_tolerance=1e-8`` and
``max_evaluations=10000`` (a total characteristic-function budget, including the
control variate and tail). Tolerances are finite and nonnegative, at least one
positive; the budget is a positive integer. Error estimates are not universal
accuracy guarantees; check convergence for tail prices. Budget exhaustion or
insufficient numerical resolution raises ``NonConvergenceError``. Expiry and
deterministic-variance branches consume no integration budget.

``FiniteDifferenceHestonVanillaEngine`` prices European and American calls/puts
with the same Heston parameter domain. It uses the modified Craig-Sneyd (MCS)
alternating direction implicit (ADI) scheme, with Ikonen-Toivanen splitting for
American exercise.
Controls are ``asset_step_count=400``, ``variance_step_count=100``,
``time_step_count=400``, ``asset_upper_boundary=None`` and
``variance_upper_boundary=None``. Counts specify intervals; spatial counts are
at least 3 and the base time count is at least 1. Curve knots are added.
Optional finite positive bounds must cover spot/strike or initial variance
when the corresponding grid is required. With zero volatility of volatility,
the engine uses the deterministic variance path; absorbing zero variance retains optimal American
exercise, including between curve nodes. Requested Greeks remain absent; implied-parameter solvers are unsupported.
Fixed-grid completion carries no error estimate: check spatial/time refinement
and domain enlargement separately. Numerical failures raise
``ComputationFailureError``.

For convergence checks across numerical engines, see the engine comparison guide:
https://lilkui.github.io/kiyosi/guide/engines.html#compare-methods-carefully.

``implied_snowball_coupon`` and ``implied_phoenix_coupon`` mirror the Rust
solvers. Snowball solving requires ``quote_convention``; Phoenix solving has no
such argument.

``CouponQuoteConvention`` controls whether a snowball's maturity coupon
shifts with observation coupons (``"shift_maturity_coupon"``) or stays fixed
(``"preserve_maturity_coupon"``). Shifted ternary solves intersect the coupon
bracket with the domain that keeps the maturity coupon at or above the fixed
minimum. A bracket with no
nondegenerate valid interval raises ``RootNotBracketedError``.
Phoenix coupon solves omit this choice.
Unsupported coupon-solver instruments are rejected before checking this choice.
Phoenix solves preserve the contract's coupon units: annual rate for annual
coupons, fixed per-period proportion for FCN/DCN. Bounds, parameter tolerance
and the returned value use the same units.
Apply the result with ``note.with_coupon(coupon, quote_convention=convention)``
for snowballs or ``note.with_coupon(coupon)`` for Phoenix, then price the new note.

Examples
--------
Price a European call and request its delta and vega:

>>> from datetime import datetime
>>> from kiyosi.instruments import EuropeanOption
>>> from kiyosi.market import BlackScholesMertonParameters, PricingContext
>>> from kiyosi.pricing import AnalyticEuropeanEngine
>>> option = EuropeanOption(
...     option_type="call", strike=100,
...     effective_time=datetime(2025, 1, 1), expiry_time=datetime(2026, 1, 1),
... )
>>> context = PricingContext(
...     model_parameters=BlackScholesMertonParameters(
...         risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2,
...     ),
...     spot_price=100, valuation_time=datetime(2025, 1, 1),
... )
>>> result = AnalyticEuropeanEngine().price_with_greeks(
...     option, context, greeks=["delta", "vega"],
... )
>>> round(result.price, 6)
9.227006
>>> result.require("delta") > 0
True
>>> sensitivities = AnalyticEuropeanEngine().price_with_greeks(
...     option, context, greeks="all",
... )
>>> sensitivities.require("gamma") > 0
True
>>> round(sensitivities.require("volga"), 8)
0.00023688
>>> round(sensitivities.require("dividend_rho"), 6)
-0.586851

Recover the model volatility from its price using a chosen bracket:

>>> sigma = implied_volatility(
...     AnalyticEuropeanEngine(), option, context, result.price,
...     lower_bound=0.1, upper_bound=0.3,
... )
>>> round(sigma, 6)
0.2

Solve a snowball coupon with an explicit maturity convention:

>>> from kiyosi.instruments import AutocallableObservation, BinarySnowballOption
>>> observation = AutocallableObservation(option.expiry_time, 110)
>>> note = BinarySnowballOption(maturity_coupon_rate=0.06,
...     effective_time=option.effective_time, expiry_time=option.expiry_time,
...     observations=[observation])
>>> coupon_engine = QuadratureAutocallableEngine(asset_step_count=64)
>>> convention = "shift_maturity_coupon"
>>> target = coupon_engine.price(note.with_coupon(0.12,
...     quote_convention=convention), context)
>>> round(implied_snowball_coupon(coupon_engine, note, context, target,
...     quote_convention=convention), 6)
0.12

Solve a Phoenix coupon in its existing per-period units:

>>> from kiyosi.instruments import PhoenixOption
>>> phoenix = PhoenixOption(coupon_per_period=0.01, initial_spot=100,
...     knock_in_level=80, upper_strike=100, lower_strike=100,
...     effective_time=option.effective_time, expiry_time=option.expiry_time,
...     observations=[observation])
>>> target = coupon_engine.price(phoenix.with_coupon(0.02), context)
>>> round(implied_phoenix_coupon(coupon_engine, phoenix, context, target), 6)
0.02

Inspect sampling uncertainty for a cash digital using the same market snapshot:

>>> from kiyosi.instruments import CashOrNothingOption
>>> digital = CashOrNothingOption(
...     option_type=option.option_type, strike=option.strike, payout=10,
...     effective_time=option.effective_time, expiry_time=option.expiry_time,
... )
>>> sampler = MonteCarloDigitalEngine(path_count=4096, seed=42, thread_count=1)
>>> estimate = sampler.price_with_statistics(digital, context)
>>> estimate.price == sampler.price(digital, context)
True
>>> estimate.standard_error > 0
True
>>> lower, upper = estimate.confidence_interval
>>> lower < estimate.price < upper
True
"""

from typing import Literal

from ._native import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    AnalyticDoubleBarrierEngine,
    AnalyticDoubleTouchEngine,
    AnalyticEuropeanEngine,
    AnalyticGeometricAveragePriceEngine,
    AnalyticHestonEuropeanEngine,
    AnalyticRangeAccrualEngine,
    BjerksundStenslandVanillaEngine,
    CoxRossRubinsteinBarrierEngine,
    CoxRossRubinsteinDigitalEngine,
    CoxRossRubinsteinVanillaEngine,
    FiniteDifferenceAccumulatorEngine,
    FiniteDifferenceAutocallableEngine,
    FiniteDifferenceBarrierEngine,
    FiniteDifferenceBinaryBarrierEngine,
    FiniteDifferenceDigitalEngine,
    FiniteDifferenceDoubleBarrierEngine,
    FiniteDifferenceDoubleTouchEngine,
    FiniteDifferenceHestonVanillaEngine,
    FiniteDifferenceVanillaEngine,
    MonteCarloAccumulatorEngine,
    MonteCarloAsianEngine,
    MonteCarloAutocallableEngine,
    MonteCarloBarrierEngine,
    MonteCarloDigitalEngine,
    MonteCarloDoubleBarrierEngine,
    MonteCarloEstimate,
    MonteCarloVanillaEngine,
    PricingResult,
    QuadratureAccumulatorEngine,
    QuadratureAutocallableEngine,
    QuadratureBarrierEngine,
    QuadratureDigitalEngine,
    QuadratureVanillaEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
    implied_phoenix_coupon,
    implied_snowball_coupon,
    implied_volatility,
    numerical_greeks,
)

CouponQuoteConvention = Literal["shift_maturity_coupon", "preserve_maturity_coupon"]
FiniteDifferenceScheme = Literal[
    "explicit_euler", "implicit_euler", "crank_nicolson", "rannacher"
]
MonteCarloSampling = Literal["pseudo_random", "sobol"]

__all__ = [
    "AnalyticBarrierEngine",
    "AnalyticBinaryBarrierEngine",
    "AnalyticDigitalEngine",
    "AnalyticDoubleBarrierEngine",
    "AnalyticDoubleTouchEngine",
    "AnalyticEuropeanEngine",
    "AnalyticGeometricAveragePriceEngine",
    "AnalyticHestonEuropeanEngine",
    "AnalyticRangeAccrualEngine",
    "BjerksundStenslandVanillaEngine",
    "CouponQuoteConvention",
    "CoxRossRubinsteinBarrierEngine",
    "CoxRossRubinsteinDigitalEngine",
    "CoxRossRubinsteinVanillaEngine",
    "FiniteDifferenceAccumulatorEngine",
    "FiniteDifferenceAutocallableEngine",
    "FiniteDifferenceBarrierEngine",
    "FiniteDifferenceBinaryBarrierEngine",
    "FiniteDifferenceDigitalEngine",
    "FiniteDifferenceDoubleBarrierEngine",
    "FiniteDifferenceDoubleTouchEngine",
    "FiniteDifferenceHestonVanillaEngine",
    "FiniteDifferenceScheme",
    "FiniteDifferenceVanillaEngine",
    "MonteCarloAccumulatorEngine",
    "MonteCarloAsianEngine",
    "MonteCarloAutocallableEngine",
    "MonteCarloBarrierEngine",
    "MonteCarloDigitalEngine",
    "MonteCarloDoubleBarrierEngine",
    "MonteCarloEstimate",
    "MonteCarloSampling",
    "MonteCarloVanillaEngine",
    "PricingResult",
    "QuadratureAccumulatorEngine",
    "QuadratureAutocallableEngine",
    "QuadratureBarrierEngine",
    "QuadratureDigitalEngine",
    "QuadratureVanillaEngine",
    "TurnbullWakemanArithmeticAveragePriceEngine",
    "implied_phoenix_coupon",
    "implied_snowball_coupon",
    "implied_volatility",
    "numerical_greeks",
]
