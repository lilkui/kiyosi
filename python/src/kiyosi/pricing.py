"""Price contracts, compute sensitivities, and solve implied parameters.

Notes
-----
Choose an engine for the contract and model. Check grid and integration-domain
convergence separately; Monte Carlo sampling uncertainty excludes discretization
bias. Solver tolerances constrain the engine's price curve, not pricing accuracy.

``price_with_greeks`` requires a Greek name, an iterable of names, or ``"all"``.
An empty iterable requests price only. Unsupported measures remain absent with
status ``"unsupported"``, including requests with no supported measures.
`PricingResult` documents units;
``result.status`` explains missing measures, and ``result.require`` rejects them.
Use ``engine.greek_method`` and ``engine.active_greek_controls`` to inspect
estimation before pricing. `numerical_greeks` selects scalar BSM differences.

Request-level shifts tune numerical fallbacks and availability checks without
changing engine settings. Both Greek entry points inherit the engine's settings
when controls are omitted or Ellipsis;
``None`` selects automatic spot, volatility and time bumps. Automatic volatility
bumps use the smaller of 0.0001 and half the current volatility. Every supplied
control is validated.
Supported expiry Greeks follow the payoff; time Greeks are absent at expiry.

Monte Carlo engines support pseudo-random and scrambled Sobol draws. A fixed seed
replays draws across thread counts within the same build. ``price_with_seed``
returns the actual seed; ``price_with_statistics`` adds sampling uncertainty.
Sobol estimates omit uncertainty. Implied-parameter solvers require an explicit
seed: retain the engine returned by ``with_resolved_seed`` for solving and repricing.

Coupon solvers return dictionaries for ``note.with_coupon(**quote)``,
preserving the snowball maturity convention and quoting Phoenix rates per observation.

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

Recover the model volatility from its price using a chosen bracket:

>>> sigma = implied_volatility(
...     AnalyticEuropeanEngine(), option, context, result.price,
...     lower_bound=0.1, upper_bound=0.3,
... )
>>> round(sigma, 6)
0.2
"""

from typing import Literal, TypedDict

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
    configure_monte_carlo_pool,
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


class SnowballCouponQuote(TypedDict):
    """Annual first-observation coupon and its maturity linkage."""

    first_observation_coupon: float
    quote_convention: CouponQuoteConvention


class PhoenixCouponQuote(TypedDict):
    """Absolute coupon rate per qualifying observation."""

    coupon_per_period: float


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
    "SnowballCouponQuote",
    "TurnbullWakemanArithmeticAveragePriceEngine",
    "configure_monte_carlo_pool",
    "implied_phoenix_coupon",
    "implied_snowball_coupon",
    "implied_volatility",
    "numerical_greeks",
]
