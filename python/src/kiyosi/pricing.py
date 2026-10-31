"""Price contracts, compute sensitivities, and solve implied parameters.

``FiniteDifferenceScheme`` selects ``"explicit_euler"``, ``"implicit_euler"``
or ``"crank_nicolson"`` (with initial damping). ``MonteCarloSampling`` selects
``"pseudo_random"`` or scrambled ``"sobol"``; a fixed seed reproduces draws
across thread counts in the same build. Sobol estimates omit sampling error.
Autocallable and accumulator ``price`` calls return the sample mean;
``price_with_statistics`` also computes and validates sampling uncertainty.

Pricing raises ``ComputationFailureError`` for materially negative option
prices; only rounding-sized negative results may be clipped to zero.
Coarse numerical grids may require refinement before pricing succeeds.
American Monte Carlo also rejects non-finite continuation regressions;
finite fits with insufficient independent samples use a lower polynomial degree.

``AnalyticEuropeanEngine``, ``CoxRossRubinsteinDigitalEngine``,
``CoxRossRubinsteinBarrierEngine``, ``MonteCarloDigitalEngine`` and
``MonteCarloAsianEngine`` compute native Greeks without numerical bumps;
their ``price_with_greeks`` methods accept only ``greeks`` and ``all_greeks``
as request controls. Engines that use numerical differences also accept bump
settings.

Pricing controls are explicit keyword-only parameters, available through
``inspect.signature``. Monte Carlo vanilla spot bumps default by product:
0.01 for European options and 8% of spot for American options. Explicit
``None`` selects the automatic bump only for American options.

``CouponQuoteConvention`` controls whether a snowball's maturity coupon
shifts with observation coupons (``"shift_maturity_coupon"``) or stays fixed
(``"preserve_maturity_coupon"``). Phoenix coupon solves omit this choice.

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

Recover the model volatility from its price using a chosen bracket:

>>> sigma = implied_volatility(
...     AnalyticEuropeanEngine(), option, context, result.price,
...     lower_bound=0.1, upper_bound=0.3,
... )
>>> round(sigma, 6)
0.2
"""

from ._native import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    AnalyticDoubleBarrierEngine,
    AnalyticEuropeanEngine,
    AnalyticGeometricAveragePriceEngine,
    AnalyticRangeAccrualEngine,
    BjerksundStenslandVanillaEngine,
    CouponQuoteConvention,
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
    FiniteDifferenceScheme,
    FiniteDifferenceVanillaEngine,
    MonteCarloAccumulatorEngine,
    MonteCarloAsianEngine,
    MonteCarloAutocallableEngine,
    MonteCarloBarrierEngine,
    MonteCarloDigitalEngine,
    MonteCarloDoubleBarrierEngine,
    MonteCarloEstimate,
    MonteCarloSampling,
    MonteCarloVanillaEngine,
    PricingResult,
    QuadratureAccumulatorEngine,
    QuadratureAutocallableEngine,
    QuadratureBarrierEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
    implied_coupon,
    implied_volatility,
)

__all__ = [
    "AnalyticBarrierEngine",
    "AnalyticBinaryBarrierEngine",
    "AnalyticDigitalEngine",
    "AnalyticDoubleBarrierEngine",
    "AnalyticEuropeanEngine",
    "AnalyticGeometricAveragePriceEngine",
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
    "TurnbullWakemanArithmeticAveragePriceEngine",
    "implied_coupon",
    "implied_volatility",
]
