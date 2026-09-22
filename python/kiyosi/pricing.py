"""Pricing engines, analytics, and implied-value solvers."""

from ._native import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    AnalyticVanillaEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
    BjerksundStenslandVanillaEngine,
    CoxRossRubinsteinVanillaEngine,
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
    AnalyticGeometricAveragePriceEngine,
    QuadratureDigitalEngine,
    QuadratureVanillaEngine,
    MonteCarloAccumulatorEngine,
    MonteCarloBackend,
    MonteCarloBinarySnowballEngine,
    MonteCarloPhoenixEngine,
    MonteCarloSnowballEngine,
    MonteCarloTernarySnowballEngine,
    MonteCarloVanillaEngine,
    implied_coupon,
    implied_volatility,
    calculate_numerical_risk_measures,
)


def _specified(**values):
    return {name: value for name, value in values.items() if value is not None}


class NumericalAnalyticsEngine:
    """Add numerical risk analytics and implied-value solvers to an engine.

    Omitted settings use the defaults owned by the C++ core. Invalid settings
    are rejected by the core when an operation is performed. Spot, volatility,
    and rate shifts are absolute; time shifts are calendar days. Measures with
    no supported stencil inside a model boundary are ``None`` without
    discarding a valid price. Failures from feasible bumped valuations are
    still raised.

    Parameters
    ----------
    engine : pricing engine
        Engine used for every base and bumped valuation.
    spot_shift : float, optional
        Absolute spot change used by spot-based finite differences.
    volatility_shift : float, optional
        Absolute volatility change used by volatility-based finite differences.
    rate_shift : float, optional
        Absolute risk-free-rate change used by rate-based finite differences.
    time_shift_days : int, optional
        Calendar-day step used by time-based finite differences.

    Raises
    ------
    TypeError
        If a supplied setting cannot be converted to its required type.
    KiyosiError
        When a supplied setting is rejected by the core during an operation.

    Notes
    -----
    The wrapper is immutable only with respect to ``engine`` access: the
    property has no setter. It does not copy the wrapped engine.
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
        """Return the wrapped pricing engine.

        Returns
        -------
        pricing engine
            The same engine object passed to the constructor.
        """
        return self._engine

    def price(self, instrument, context):
        """Price an instrument and calculate feasible numerical risk measures.

        Parameters
        ----------
        instrument : instrument
            Instrument supported by the wrapped engine.
        context : PricingContext
            Market state and valuation instant.

        Returns
        -------
        PricingResult
            Price and each feasible numerical risk measure. An infeasible or
            unsupported measure is ``None``.

        Raises
        ------
        TypeError
            If the engine and instrument combination is not supported.
        KiyosiError
            If the core rejects the inputs or a feasible valuation fails.
        """
        return calculate_numerical_risk_measures(
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
        """Solve for the volatility that matches an observed price.

        Parameters
        ----------
        instrument : instrument
            Instrument supported by the wrapped engine.
        context : PricingContext
            Market state whose volatility is varied by the solver.
        observed_price : float
            Target instrument price.
        lower_bound : float, optional
            Lower volatility bound. Uses the core default when omitted.
        upper_bound : float, optional
            Upper volatility bound. Uses the core default when omitted.
        tolerance : float, optional
            Solver convergence tolerance. Uses the core default when omitted.
        max_iterations : int, optional
            Maximum solver iterations. Uses the core default when omitted.

        Returns
        -------
        float
            Implied volatility as a decimal rate.

        Raises
        ------
        TypeError
            If an argument has an incompatible representation.
        KiyosiError
            If the inputs are invalid, the target is not bracketed, or the
            solver does not converge.
        """
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
        """Solve for the coupon rate that matches an observed price.

        Snowball instruments require an explicit quote_convention. Phoenix
        instruments have one unambiguous coupon and require none.

        Parameters
        ----------
        instrument : SnowballOption, BinarySnowballOption, TernarySnowballOption, or PhoenixOption
            Coupon-bearing instrument supported by the wrapped engine.
        context : PricingContext
            Market state used by the solver.
        observed_price : float
            Target instrument price.
        quote_convention : CouponQuoteConvention, optional
            Snowball coupon component to shift. Required for snowballs and
            omitted for Phoenix options.
        lower_bound : float, optional
            Lower coupon-rate bound. Uses the core default when omitted.
        upper_bound : float, optional
            Upper coupon-rate bound. Uses the core default when omitted.
        tolerance : float, optional
            Solver convergence tolerance. Uses the core default when omitted.
        max_iterations : int, optional
            Maximum solver iterations. Uses the core default when omitted.

        Returns
        -------
        float
            Implied coupon rate as a decimal rate.

        Raises
        ------
        TypeError
            If the engine/instrument combination or quote convention is not
            supported, or an argument has an incompatible representation.
        KiyosiError
            If the inputs are invalid, the target is not bracketed, or the
            solver does not converge.
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
    "TurnbullWakemanArithmeticAveragePriceEngine",
    "BjerksundStenslandVanillaEngine",
    "CoxRossRubinsteinVanillaEngine",
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
    "AnalyticGeometricAveragePriceEngine",
    "QuadratureDigitalEngine",
    "QuadratureVanillaEngine",
    "MonteCarloAccumulatorEngine",
    "MonteCarloBackend",
    "MonteCarloBinarySnowballEngine",
    "MonteCarloPhoenixEngine",
    "MonteCarloSnowballEngine",
    "MonteCarloTernarySnowballEngine",
    "MonteCarloVanillaEngine",
    "NumericalAnalyticsEngine",
    "calculate_numerical_risk_measures",
    "implied_coupon",
    "implied_volatility",
]
