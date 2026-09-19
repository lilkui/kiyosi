"""Pricing engines, analytics, and implied-value solvers."""

from ._native import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    AnalyticVanillaEngine,
    ArithmeticAverageAsianEngine,
    BjerksundStenslandVanillaEngine,
    CrrVanillaEngine,
    CouponQuoteConvention,
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
    MonteCarloBackend,
    MonteCarloBinarySnowballEngine,
    MonteCarloPhoenixEngine,
    MonteCarloSnowballEngine,
    MonteCarloTernarySnowballEngine,
    MonteCarloVanillaEngine,
    implied_coupon,
    implied_volatility,
    numerical_analytics as _numerical_analytics,
)


def _specified(**values):
    return {name: value for name, value in values.items() if value is not None}


class NumericalAnalyticsEngine:
    """Add numerical risk analytics to a pricing engine.

    Omitted settings use the defaults owned by the C++ core. Invalid settings
    are rejected by the core when an operation is performed. Spot, volatility,
    and rate shifts are absolute; time shifts are calendar days. Measures with
    no supported stencil inside a model boundary are ``None`` without
    discarding a valid price. Failures from feasible bumped valuations are
    still raised.
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
        """Price an instrument and calculate every feasible numerical risk measure."""
        return _numerical_analytics(
            self._engine, instrument, context, **self._shift_settings
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
        return implied_volatility(
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
        quote_convention=None,
        lower_bound=None,
        upper_bound=None,
        tolerance=None,
        max_iterations=None,
    ):
        """Solve for the coupon rate matching the observed price.

        Snowball instruments require an explicit quote_convention. Phoenix
        instruments have one unambiguous coupon and require none.
        """
        return implied_coupon(
            self._engine,
            instrument,
            context,
            observed_price,
            **_specified(
                quote_convention=quote_convention,
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
    "CouponQuoteConvention",
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
    "MonteCarloBackend",
    "MonteCarloBinarySnowballEngine",
    "MonteCarloPhoenixEngine",
    "MonteCarloSnowballEngine",
    "MonteCarloTernarySnowballEngine",
    "MonteCarloVanillaEngine",
    "NumericalAnalyticsEngine",
    "implied_coupon",
    "implied_volatility",
]
