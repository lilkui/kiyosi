"""Pricing engines and cross-engine analytics."""

from ._native import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    AnalyticVanillaEngine,
    ArithmeticAverageAsianEngine,
    BjerksundStenslandVanillaEngine,
    CrrVanillaEngine,
    FiniteDifferenceAccumulatorEngine,
    FiniteDifferenceBarrierEngine,
    FiniteDifferenceBinarySnowballEngine,
    FiniteDifferenceDigitalEngine,
    FiniteDifferencePhoenixEngine,
    FiniteDifferenceScheme,
    FiniteDifferenceSnowballEngine,
    FiniteDifferenceTernarySnowballEngine,
    FiniteDifferenceVanillaEngine,
    GeometricAverageAsianEngine,
    IntegralDigitalEngine,
    IntegralVanillaEngine,
    MonteCarloAccumulatorEngine,
    MonteCarloBinarySnowballEngine,
    MonteCarloPhoenixEngine,
    MonteCarloSnowballEngine,
    MonteCarloTernarySnowballEngine,
    MonteCarloVanillaEngine,
    ScenarioGridResult,
    implied_coupon as _implied_coupon,
    implied_volatility as _implied_volatility,
    numerical_analytics as _numerical_analytics,
    scenario_grid as _scenario_grid,
)


def _specified(**values):
    return {name: value for name, value in values.items() if value is not None}


class NumericalAnalyticsEngine:
    """Add numerical risk analytics to a pricing engine.

    Omitted settings use the defaults owned by the C++ core. Invalid settings
    are rejected by the core when an operation is performed.
    """

    def __init__(
        self,
        engine,
        *,
        spot_shift=None,
        volatility_shift=None,
        rate_shift=None,
        time_shift_days=None,
    ):
        self._engine = engine
        self._shift_settings = _specified(
            spot_shift=spot_shift,
            volatility_shift=volatility_shift,
            rate_shift=rate_shift,
            time_shift_days=time_shift_days,
        )

    @property
    def engine(self):
        """Return the wrapped pricing engine."""
        return self._engine

    def price(self, instrument, context):
        """Price an instrument and calculate numerical risk measures."""
        return _numerical_analytics(
            self._engine, instrument, context, **self._shift_settings
        )

    def scenario_grid(self, instrument, context, spots):
        """Calculate price and spot risks at each supplied spot value."""
        return _scenario_grid(
            self._engine, instrument, context, spots, **self._shift_settings
        )

    def implied_volatility(
        self,
        instrument,
        context,
        observed_price,
        *,
        lower_bound=None,
        upper_bound=None,
        tolerance=None,
        max_iterations=None,
    ):
        """Solve for volatility matching the observed price."""
        return _implied_volatility(
            self._engine,
            instrument,
            context,
            observed_price,
            **_specified(
                lower_bound=lower_bound,
                upper_bound=upper_bound,
                tolerance=tolerance,
                max_iterations=max_iterations,
            ),
        )

    def implied_coupon(
        self,
        instrument,
        context,
        observed_price,
        *,
        lower_bound=None,
        upper_bound=None,
        tolerance=None,
        max_iterations=None,
    ):
        """Solve for the coupon rate matching the observed price."""
        return _implied_coupon(
            self._engine,
            instrument,
            context,
            observed_price,
            **_specified(
                lower_bound=lower_bound,
                upper_bound=upper_bound,
                tolerance=tolerance,
                max_iterations=max_iterations,
            ),
        )

__all__ = [
    "AnalyticBarrierEngine",
    "AnalyticBinaryBarrierEngine",
    "AnalyticDigitalEngine",
    "AnalyticVanillaEngine",
    "ArithmeticAverageAsianEngine",
    "BjerksundStenslandVanillaEngine",
    "CrrVanillaEngine",
    "FiniteDifferenceAccumulatorEngine",
    "FiniteDifferenceBarrierEngine",
    "FiniteDifferenceBinarySnowballEngine",
    "FiniteDifferenceDigitalEngine",
    "FiniteDifferencePhoenixEngine",
    "FiniteDifferenceScheme",
    "FiniteDifferenceSnowballEngine",
    "FiniteDifferenceTernarySnowballEngine",
    "FiniteDifferenceVanillaEngine",
    "GeometricAverageAsianEngine",
    "IntegralDigitalEngine",
    "IntegralVanillaEngine",
    "MonteCarloAccumulatorEngine",
    "MonteCarloBinarySnowballEngine",
    "MonteCarloPhoenixEngine",
    "MonteCarloSnowballEngine",
    "MonteCarloTernarySnowballEngine",
    "MonteCarloVanillaEngine",
    "NumericalAnalyticsEngine",
    "ScenarioGridResult",
]
