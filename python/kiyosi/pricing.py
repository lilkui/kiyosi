"""Pricing engines, analytics, and implied-value solvers."""

from typing import Literal

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
    calculate_numerical_risk_measures,
    implied_coupon,
    implied_volatility,
)

CouponQuoteConvention = Literal["shift_maturity_coupon", "preserve_maturity_coupon"]
FiniteDifferenceScheme = Literal["explicit_euler", "implicit_euler", "crank_nicolson"]
MonteCarloBackend = Literal["cpu", "cuda"]


__all__ = [
    "AnalyticBarrierEngine",
    "AnalyticBinaryBarrierEngine",
    "AnalyticDigitalEngine",
    "AnalyticGeometricAveragePriceEngine",
    "AnalyticVanillaEngine",
    "BjerksundStenslandVanillaEngine",
    "CouponQuoteConvention",
    "CoxRossRubinsteinVanillaEngine",
    "FiniteDifferenceAccumulatorEngine",
    "FiniteDifferenceBarrierEngine",
    "FiniteDifferenceBinarySnowballEngine",
    "FiniteDifferenceDigitalEngine",
    "FiniteDifferencePhoenixEngine",
    "FiniteDifferenceScheme",
    "FiniteDifferenceSnowballEngine",
    "FiniteDifferenceTernarySnowballEngine",
    "FiniteDifferenceVanillaEngine",
    "MonteCarloAccumulatorEngine",
    "MonteCarloBackend",
    "MonteCarloBinarySnowballEngine",
    "MonteCarloPhoenixEngine",
    "MonteCarloSnowballEngine",
    "MonteCarloTernarySnowballEngine",
    "MonteCarloVanillaEngine",
    "QuadratureDigitalEngine",
    "QuadratureVanillaEngine",
    "TurnbullWakemanArithmeticAveragePriceEngine",
    "calculate_numerical_risk_measures",
    "implied_coupon",
    "implied_volatility",
]
