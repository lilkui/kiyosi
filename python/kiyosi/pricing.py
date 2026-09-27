"""Pricing engines, analytics, and implied-value solvers."""

from typing import Literal as _Literal

from ._native import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    AnalyticGeometricAveragePriceEngine,
    AnalyticVanillaEngine,
    BjerksundStenslandVanillaEngine,
    CoxRossRubinsteinVanillaEngine,
    FiniteDifferenceAccumulatorEngine,
    FiniteDifferenceBarrierEngine,
    FiniteDifferenceBinarySnowballEngine,
    FiniteDifferenceDigitalEngine,
    FiniteDifferencePhoenixEngine,
    FiniteDifferenceSnowballEngine,
    FiniteDifferenceTernarySnowballEngine,
    FiniteDifferenceVanillaEngine,
    MonteCarloAccumulatorEngine,
    MonteCarloBinarySnowballEngine,
    MonteCarloPhoenixEngine,
    MonteCarloSnowballEngine,
    MonteCarloTernarySnowballEngine,
    MonteCarloVanillaEngine,
    QuadratureDigitalEngine,
    QuadratureVanillaEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
    calculate_numerical_greeks,
    implied_coupon,
    implied_volatility,
)

CouponQuoteConvention = _Literal["shift_maturity_coupon", "preserve_maturity_coupon"]
FiniteDifferenceScheme = _Literal["explicit_euler", "implicit_euler", "crank_nicolson"]
MonteCarloBackend = _Literal["cpu", "cuda"]
