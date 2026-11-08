"""Price contracts, compute sensitivities, and solve implied parameters.

European vanilla and digital contracts support valuation before effective_time, including
Greeks and implied volatility, when the model data covers valuation through expiry.
Other contracts require valuation within their inclusive life and validate prior history.

Engine ``price_with_greeks(instrument, context, greeks)`` methods use the engine's
estimator and configured controls. Finite-difference engines accept a positive
``spot_shift`` at construction to configure native Greek availability checks
and terminal stencils, including under local volatility. Their ``volatility_shift``,
``rate_shift`` and ``time_shift`` constructor controls tune numerical BSM Greeks
while retaining native spatial delta and gamma. AnalyticDigitalEngine accepts all four
shift controls on price_with_greeks, preserving analytic delta and gamma at positive
volatility before expiry while tuning numerical fallback Greeks.
Monte Carlo single- and double-barrier engines accept
an optional keyword-only ``spot_shift`` for event checks, at-hit rebate derivatives
and expiry stencils, retaining their native estimator. Other engines accept no bump arguments. Configure scalar
finite differences explicitly through ``numerical_greeks(engine, instrument,
context, greeks, spot_shift=..., volatility_shift=..., rate_shift=...,
time_shift=...)``. All numerical controls are validated there.

BSM ``volatility=0`` uses exact deterministic valuation in each supported engine,
including American exercise, continuous or scheduled touches, and structured-product
events. Model and rate-curve applicability is checked before deterministic pricing. Monte Carlo uses
no sampling for these prices; pseudorandom statistics have zero standard error
and Sobol uncertainty is absent. Spot and rate Greeks use scalar differences;
before expiry, volatility Greeks are undefined at zero because symmetric volatility
bumps leave the model domain. Deterministic payoff and event boundaries also leave
sensitivities undefined. At expiry the usual terminal Greek rules apply.
The implied-volatility solver defaults to ``lower_bound=0`` and can return an exact
zero-volatility endpoint before restricting a CRR bracket to positive tree widths.

Finite-difference engines support local volatility for European and American
vanilla options, digitals, single and double barriers, binary barriers, touch
and no-touch options, autocallable notes and accumulators. They provide price,
delta and gamma, holding the absolute surface fixed. Other requested Greeks are None, including in "all" requests.
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
scheduled monitoring and local-volatility knock-in vanilla components use
the full domain.
Monte Carlo ``step_count`` counts time intervals for both American exercise
and arithmetic Asian integration; American pricing needs at least two intervals.
``MonteCarloSampling`` selects
``"pseudo_random"`` or scrambled ``"sobol"``; a fixed seed reproduces draws
across thread counts in the same build. Sobol estimates omit sampling error.
Call ``engine.with_resolved_seed()`` before pricing to obtain an independent
engine with a concrete seed. Existing seeds and all other controls are preserved.
Save or pickle that returned engine to replay draws within the same build;
``seed=None`` selects fresh entropy per pricing or Greek request.
Implied-parameter solvers require a concrete seed and raise InvalidInputError
for unseeded Monte Carlo engines. Call ``with_resolved_seed()`` before solving
and retain that engine to reprice the returned quote.

Engine ``replace()`` methods return validated independent engines and preserve
omitted controls. Explicit None clears nullable controls; Ellipsis retains them.

Every Monte Carlo engine exposes ``price_with_statistics`` for its supported
contracts; vanilla statistics require European options. It returns the same
seeded price as ``price`` and also computes and validates sampling uncertainty.
``estimate.seed`` records the actual sampling seed, even when ``engine.seed`` is None.
Rebuild an engine with that seed and the same controls to replay the estimate
within the same build. No-sampling valuations report ``estimate.seed=None``.
``estimate.uncertainty_status`` explains its availability: ``"available"``,
``"unsupported_sampling"`` for Sobol, ``"insufficient_observations"``, or
``"incomplete_antithetic_pair"`` for an odd antithetic path count.
Sobol, fewer than two independent observations and odd antithetic counts omit
uncertainty; known pseudorandom analytic branches have zero error. Sampling error
does not measure arithmetic Asian integration bias or other approximation error.
Future Monte Carlo barrier, knock-in, knock-out, coupon and acceleration tests
retain log-price movements even when the simulated spot rounds to a threshold.
Single- and double-barrier Monte Carlo payoffs also retain movements when
terminal spot rounds to the strike.
Observations at valuation use the supplied spot.

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
prices. Settled barrier, accumulator and autocallable cash flows do not require curve coverage through expiry;
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

Analytic estimators and native Monte Carlo/binomial estimators retain their
supported measures and engine defaults. Monte Carlo single- and double-barrier
engines provide delta and gamma, using the optional spot stencil where needed
for availability checks and continuous at-hit rebates. Use ``numerical_greeks``
to select scalar price differences and control their bumps. Its rate shift applies
separately to the risk-free and dividend curves; its volatility shift controls
vega and volga. Numerical volga requires a positive lower volatility leg.

Pass ``greeks="all"`` to request every measure, a Greek name for one measure,
or an iterable of names for a selection. The ``greeks`` argument is required;
unavailable measures are None. Inspect ``result.status(name)`` to distinguish
unrequested, unsupported, undefined and ``"stencil_unavailable"`` measures.
The last status identifies a numerical stencil that may be recovered with other
controls, such as a smaller volatility bump; ``result.requested`` and
``result.supported`` expose the request and engine/model coverage.

At expiry, ``AnalyticEuropeanEngine`` and ``FiniteDifferenceVanillaEngine`` compute
exact supported payoff derivatives away from the strike, with delta, gamma,
speed, vanna and zomma absent exactly at the strike.
Other native engines and ``numerical_greeks`` use terminal-payoff stencils. Supported
vega, volga, rho and dividend rho are zero because no stochastic evolution or
discounting remains. Vanna and zomma are zero where spot derivatives exist.
Theta, charm and color are None at expiry. Spot stencils crossing a payoff
kink or product event are unavailable. Terminal rules apply within engine/model coverage.
Automatic terminal spot bumps use the smaller of 0.01 spot units and 1% of spot,
including American Monte Carlo; custom terminal bumps belong to ``numerical_greeks``.

Inspect ``engine.greek_method(instrument, context)`` before a parameter sweep:
``"native"`` uses no numerical shifts, ``"numerical"`` uses scalar differences,
``"mixed"`` completes native estimates numerically, and ``"native_with_spot_control"``
uses spot controls for numerical derivatives or availability checks. ``"terminal"``
uses expiry payoff stencils; ``"unsupported"`` supplies no Greeks.
``engine.active_greek_controls(instrument, context)`` lists shift controls
potentially used across supported requests. Monte Carlo barrier methods expose
``spot_shift``; finite-difference constructors expose all four shift controls.
Other engines use internal defaults. Queries check model
applicability without pricing or sampling; pricing validates life and history.
Use ``numerical_greeks`` for configurable scalar differences. This can provide
measures beyond native engine coverage; Heston and local volatility are
unsupported by the scalar helper. Monte Carlo draws are pinned within each
numerical request. ``PricingResult.seed`` records the simulation seed used by native
and numerical Greek requests, or None without sampling. Retain the seed and the
same engine controls and inputs to replay within the same build.

Numerical bump controls are keyword-only and discoverable through
``inspect.signature(numerical_greeks)``. ``spot_shift=None`` selects the smaller
of 0.01 spot units and 1% of spot, except American Monte Carlo before expiry uses
8% of spot. Volatility and rate shifts default to 0.0001 decimal units; time
shifts default to None: an automatic bump of at most one day, shortened to stay
between observations. Supply a positive
``timedelta`` to check intraday convergence or keep the stencil between observations.
These are sampling steps: Greek outputs are unscaled partial derivatives,
with time measured in Actual/365 Fixed years and volatility and rates in decimals.
Rust ``NumericalShiftSettings`` supplies the same defaults. Its
``fallback_spot_shift()`` accessor returns the uncapped general fallback; keep
``None`` when forwarding settings to preserve engine-specific automatic resolution.

``AnalyticHestonEuropeanEngine`` prices European calls/puts with ``HestonParameters``
using adaptive Fourier integration and a deterministic-variance control variate.
It supports deterministic rate/dividend curves, zero parameters and correlation
endpoints without imposing the Feller condition. Requested Greeks are None; implied-parameter solvers
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
exercise, including between curve nodes. Requested Greeks are absent; implied-parameter solvers are unsupported.
Fixed-grid completion carries no error estimate: check spatial/time refinement
and domain enlargement separately. Numerical failures raise
``ComputationFailureError``.

Compare engines with identical contract terms, calendars, history and model inputs.
Refine numerical grids and integration domains separately, and assess Monte Carlo
sampling uncertainty alongside any time-discretization bias. Solver tolerances
control the residual of the chosen engine's price curve, not its pricing accuracy.

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
and the quoted value use the same units. Phoenix results are dictionaries with
exactly one key: ``coupon_rate`` or ``coupon_per_period``.
Apply the result with ``note.with_quoted_coupon(coupon, quote_convention=convention)``
for snowballs. For Phoenix, use ``note.replace(**quote)`` to preserve
the solved contract's convention, then price the new note.

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
2.36882234
>>> round(sensitivities.require("dividend_rho"), 6)
-58.685115

Recover the model volatility from its price using a chosen bracket:

>>> sigma = implied_volatility(
...     AnalyticEuropeanEngine(), option, context, result.price,
...     lower_bound=0.1, upper_bound=0.3,
... )
>>> round(sigma, 6)
0.2

Solve a snowball coupon with an explicit maturity convention:

>>> from kiyosi.instruments import AutocallableObservation, BinarySnowballOption
>>> observation = AutocallableObservation.snowball(option.expiry_time, 110, coupon_rate=0)
>>> note = BinarySnowballOption(maturity_coupon_rate=0.06,
...     effective_time=option.effective_time, expiry_time=option.expiry_time,
...     observations=[observation])
>>> coupon_engine = QuadratureAutocallableEngine(asset_step_count=64)
>>> convention = "shift_maturity_coupon"
>>> target = coupon_engine.price(note.with_quoted_coupon(0.12,
...     quote_convention=convention), context)
>>> round(implied_snowball_coupon(coupon_engine, note, context, target,
...     quote_convention=convention), 6)
0.12

Solve a Phoenix coupon in its existing per-period units:

>>> from kiyosi.instruments import PhoenixOption
>>> phoenix = PhoenixOption(coupon_per_period=0.01, initial_spot=100,
...     knock_in_level=80, upper_strike=100, lower_strike=100,
...     observation_clock_time=option.expiry_time.time(),
...     effective_time=option.effective_time, expiry_time=option.expiry_time,
...     observations=[AutocallableObservation.phoenix(
...         option.expiry_time, 110, coupon_barrier_level=0)])
>>> target = coupon_engine.price(phoenix.replace(coupon_per_period=0.02), context)
>>> quote = implied_phoenix_coupon(coupon_engine, phoenix, context, target)
>>> round(quote["coupon_per_period"], 6)
0.02

Inspect sampling uncertainty for a cash digital using the same market snapshot:

>>> from kiyosi.instruments import CashOrNothingOption
>>> digital = CashOrNothingOption(
...     option_type=option.option_type, strike=option.strike, payout=10,
...     effective_time=option.effective_time, expiry_time=option.expiry_time,
... )
>>> sampler = MonteCarloDigitalEngine(path_count=4096, seed=42, max_active_workers=1)
>>> estimate = sampler.price_with_statistics(digital, context)
>>> estimate.price == sampler.price(digital, context)
True
>>> estimate.standard_error > 0
True
>>> lower, upper = estimate.confidence_interval
>>> lower < estimate.price < upper
True
"""

from typing import Literal, NotRequired, TypedDict

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

GreekMethod = Literal[
    "native",
    "numerical",
    "mixed",
    "native_with_spot_control",
    "terminal",
    "unsupported",
]
GreekControl = Literal["spot_shift", "volatility_shift", "rate_shift", "time_shift"]

CouponQuoteConvention = Literal["shift_maturity_coupon", "preserve_maturity_coupon"]
FiniteDifferenceScheme = Literal[
    "explicit_euler", "implicit_euler", "crank_nicolson", "rannacher"
]
MonteCarloSampling = Literal["pseudo_random", "sobol"]
MonteCarloUncertaintyStatus = Literal[
    "available",
    "unsupported_sampling",
    "insufficient_observations",
    "incomplete_antithetic_pair",
]


class _AnnualPhoenixCouponQuote(TypedDict):
    coupon_rate: float
    coupon_per_period: NotRequired[None]


class _PerPeriodPhoenixCouponQuote(TypedDict):
    coupon_rate: NotRequired[None]
    coupon_per_period: float


type PhoenixCouponQuote = _AnnualPhoenixCouponQuote | _PerPeriodPhoenixCouponQuote


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
    "GreekControl",
    "GreekMethod",
    "MonteCarloAccumulatorEngine",
    "MonteCarloAsianEngine",
    "MonteCarloAutocallableEngine",
    "MonteCarloBarrierEngine",
    "MonteCarloDigitalEngine",
    "MonteCarloDoubleBarrierEngine",
    "MonteCarloEstimate",
    "MonteCarloSampling",
    "MonteCarloUncertaintyStatus",
    "MonteCarloVanillaEngine",
    "PhoenixCouponQuote",
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
