"""Price contracts, compute sensitivities, and solve implied parameters.

Finite-difference engines support local volatility for European and American
vanilla options, digitals, single and double barriers, binary barriers, touch
and no-touch options, autocallable notes and accumulators. They provide price,
delta and gamma, holding the absolute surface fixed. Other Greeks (including
"all"), non-FD engines and inverse solves raise UnsupportedOperationError for
this model. Digital FD requires explicitly flat risk-free and dividend rates.
Delta/gamma are absent at expiry and unavailable event-crossing stencils.
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
their sinh coordinates. Autocallable engines default to 800 spatial and 400
time steps; observations and damping can add substeps. Check convergence before
reducing grid counts further.
``MonteCarloSampling`` selects
``"pseudo_random"`` or scrambled ``"sobol"``; a fixed seed reproduces draws
across thread counts in the same build. Sobol estimates omit sampling error.
Autocallable and accumulator ``price`` calls return the sample mean;
``price_with_statistics`` also computes and validates sampling uncertainty.
Future Monte Carlo barrier, knock-in, knock-out, coupon and acceleration tests
retain log-price movements even when the simulated spot rounds to a threshold.
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
prices; only rounding-sized negative results may be clipped to zero.
Coarse numerical grids may require refinement before pricing succeeds.
American Monte Carlo also rejects non-finite continuation regressions;
finite fits with insufficient independent samples use a lower polynomial degree.
American exercise-grid pricing limits retained path storage to 1 GiB per call.
Storage is ``8 * path_count * (step_count - 1)`` bytes, with odd antithetic
path counts rounded up to the next even count for reserved capacity; other
buffers are additional. Larger grids raise ``InvalidInputError`` before sampling.
Reduce either count to fit. Expiry and terminal-only American calls (nonpositive
dividends and nonnegative rates) do not allocate this grid or apply this limit.

``AnalyticEuropeanEngine``, ``CoxRossRubinsteinDigitalEngine``,
``CoxRossRubinsteinBarrierEngine``, ``MonteCarloDigitalEngine`` and
``MonteCarloAsianEngine`` compute native Greeks without numerical bumps;
their ``price_with_greeks`` methods accept only ``greeks`` as a request
control. Engines that use numerical differences also accept bump
settings.
``volatility_shift`` controls the volatility bump for vega and volga;
``rate_shift`` controls parallel bumps of both the risk-free and dividend-yield
curves for rho and dividend_rho. Numerical volga requires a strictly positive
lower volatility leg. Monte Carlo volga can amplify sampling and exercise noise;
check convergence across bump sizes and simulation settings.

``MonteCarloBarrierEngine`` and ``MonteCarloDoubleBarrierEngine`` return only
delta and gamma. Their only bump control is ``spot_shift``, used for barrier
availability checks and numerical derivatives of continuous at-hit rebates.

Pass ``greeks="all"`` to request every measure, a Greek name for one measure,
or an iterable of names for a selection. The ``greeks`` argument is required;
unavailable measures remain None.

Pricing controls are explicit keyword-only parameters, available through
``inspect.signature``. Instrument conversion and supported-family checks
precede Greek-name parsing. Monte Carlo vanilla spot bumps default by product:
0.01 for European options and 8% of spot for American options. Explicit
``None`` selects the automatic bump for either product.

``AnalyticHestonEuropeanEngine`` prices European calls/puts with ``HestonParameters``
using adaptive Fourier integration and a deterministic-variance control variate.
It supports deterministic rate/dividend curves, zero parameters and correlation
endpoints without imposing the Feller condition. Greeks and inverse solves
raise ``UnsupportedOperationError``. Integration controls are
``absolute_tolerance=1e-8`` (price units), ``relative_tolerance=1e-8`` and
``max_evaluations=10000`` (a total characteristic-function budget, including the
control variate and tail). Tolerances are finite and nonnegative, at least one
positive; the budget is a positive integer. Error estimates are not universal
accuracy guarantees; check convergence for tail prices. Budget exhaustion or
insufficient numerical resolution raises ``NonConvergenceError``. Expiry and
deterministic-variance branches consume no integration budget.

``FiniteDifferenceHestonVanillaEngine`` prices European and American calls/puts
with the same Heston model domain using MCS ADI and IT American splitting.
Controls are ``asset_step_count=400``, ``variance_step_count=100``,
``time_step_count=400``, ``asset_upper_boundary=None`` and
``variance_upper_boundary=None``. Counts specify intervals; spatial counts are
at least 3 and the base time count is at least 1. Curve knots are added.
Optional finite positive bounds must cover spot/strike or initial variance
when the corresponding grid is required. Zero vol-of-vol uses its actual
deterministic variance path; absorbing zero variance retains optimal American
exercise, including between curve nodes. Greeks and inverse solves are unsupported.
Fixed-grid completion carries no error estimate: check spatial/time refinement
and domain enlargement separately. Numerical failures raise
``ComputationFailureError``.

For convergence checks across numerical engines, see the engine comparison guide:
https://lilkui.github.io/kiyosi/guide/engines.html#compare-methods-carefully.

``CouponQuoteConvention`` controls whether a snowball's maturity coupon
shifts with observation coupons (``"shift_maturity_coupon"``) or stays fixed
(``"preserve_maturity_coupon"``). Phoenix coupon solves omit this choice;
the displayed default ``...`` (``Ellipsis``) also means omitted.
Unsupported coupon-solver instruments are rejected before checking this choice.
Phoenix solves preserve the contract's coupon units: annual rate for annual
coupons, fixed per-period proportion for FCN/DCN. Bounds, parameter tolerance
and the returned value use the same units.

Examples
--------
Price a European call and request its delta and vega:

>>> from datetime import date
>>> from kiyosi.instruments import EuropeanOption
>>> from kiyosi.market import BlackScholesMertonParameters, PricingContext
>>> from kiyosi.pricing import AnalyticEuropeanEngine
>>> option = EuropeanOption(
...     option_type="call", strike=100,
...     effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
... )
>>> context = PricingContext(
...     model_parameters=BlackScholesMertonParameters(
...         risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2,
...     ),
...     spot_price=100, valuation_time=date(2025, 1, 1),
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
    implied_coupon,
    implied_volatility,
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
    "implied_coupon",
    "implied_volatility",
]
