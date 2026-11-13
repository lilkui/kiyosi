"""Typed Python interfaces for the native contracts, market data, and pricing API."""

from collections.abc import Iterable, Iterator
from datetime import date, datetime, time, timedelta
from decimal import Decimal as _Decimal
from numbers import Real as _RealNumber
from types import EllipsisType
from typing import Literal, Never, Self, SupportsIndex, overload

from . import pricing as _pricing

type _Real = float | _RealNumber | _Decimal
__version__: str

class KiyosiError(Exception):
    """Base exception for structured domain failures."""

class InvalidInputError(KiyosiError, ValueError):
    """Invalid parameters, dates, schedules or contract history."""

class MissingHistoryError(InvalidInputError):
    """Required history payload is missing."""

class UnsupportedOperationError(KiyosiError):
    """Unsupported pricing combination or unavailable result measure."""

class GreekUnavailableError(UnsupportedOperationError):
    """Unavailable Greek; greek and status attributes identify the measure and reason."""

    greek: Greek
    status: Literal["not_requested", "unsupported", "undefined", "stencil_unavailable"]

class ComputationFailureError(KiyosiError):
    """Non-finite numerical result or unavailable computation state."""

class RootNotBracketedError(KiyosiError):
    """Supplied interval does not bracket a root in the engine's domain."""

class NonConvergenceError(KiyosiError):
    """Iteration or integration limit reached without convergence."""

def configure_monte_carlo_pool(*, max_workers: SupportsIndex) -> None:
    """Cap the process-wide Monte Carlo pool before its first parallel request.

    Parameters
    ----------
    max_workers : int
        Positive maximum worker count, limited further by available CPUs.

    Raises
    ------
    InvalidInputError
        If max_workers is nonpositive.
    UnsupportedOperationError
        If a different cap is supplied after pool initialization.
    TypeError
        If max_workers is a bool or does not support integer indexing.
    OverflowError
        If max_workers is outside the signed 32-bit integer range.

    Notes
    -----
    Creates no threads. The default pool size is available CPU parallelism.
    Configure each process before submitting parallel pricing work. Repeating
    the configured cap is valid; failed configuration leaves it unchanged.
    Engine max_active_workers caps active work per request within this pool.
    """

def implied_snowball_coupon(
    engine: QuadratureAutocallableEngine
    | FiniteDifferenceAutocallableEngine
    | MonteCarloAutocallableEngine,
    instrument: SnowballOption | BinarySnowballOption | TernarySnowballOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    quote_convention: _pricing.CouponQuoteConvention,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> _pricing.SnowballCouponQuote:
    """Solve the annual snowball coupon that reproduces an observed price.

    Parameters
    ----------
    engine : object
        QuadratureAutocallableEngine, FiniteDifferenceAutocallableEngine or
        MonteCarloAutocallableEngine.
    instrument : object
        SnowballOption, BinarySnowballOption or TernarySnowballOption.
    context : PricingContext
        Market snapshot.
    observed_price : float
        Finite target price in the engine's payment units.
    quote_convention : CouponQuoteConvention
        Required: shift_maturity_coupon shifts the maturity coupon with
        observation coupons; preserve_maturity_coupon keeps it fixed.
    lower_bound : float, optional
        Finite lower coupon in the contract's units; defaults to 0. Signed
        bounds are accepted when the resulting contract terms are valid. For
        shifted ternary maturity coupons, the bracket is intersected with the
        valid domain that keeps maturity at or above the fixed minimum.
    upper_bound : float, optional
        Finite upper coupon in the same units, above lower_bound; defaults to
        2.
    price_tolerance : float, optional
        Positive finite absolute price residual tolerance; defaults to 1e-8.
    parameter_tolerance : float, optional
        Positive finite bracket-width tolerance in the contract's coupon units;
        defaults to 1e-8. Bracket convergence without a price match raises
        NonConvergenceError.
    max_iterations : int, optional
        Positive iteration budget; defaults to 100. Endpoint and
        affine-interpolation evaluations do not use this budget.

    Returns
    -------
    SnowballCouponQuote
        Dictionary containing first_observation_coupon, an annual decimal rate,
        and quote_convention. Apply with instrument.with_coupon(**quote)
        to preserve observation spreads and the maturity convention.

    Raises
    ------
    InvalidInputError
        If the quote, settings or repriced terms are invalid.
    UnsupportedOperationError
        If the combination is unsupported or the parameter is not identifiable.
    RootNotBracketedError
        If the supplied endpoints do not bracket the target. Also raised when
        the bracket has no nondegenerate interval in the valid domain.
    NonConvergenceError
        If the iteration budget is exhausted or the bracket converges without a
        price match.
    ComputationFailureError
        If a price, residual or numerical evaluation is non-finite.

    Notes
    -----
    Pricing failures propagate. Monte Carlo solves require an explicit seed;
    call with_resolved_seed() first; tolerances constrain the sampled price
    curve, not its sampling error. Snowballs shift all observation coupons
    relative to the first one, including past observations. The maturity
    convention controls the final coupon; the ternary minimum remains fixed.
    The quoted rate, bounds and parameter tolerance use annual decimal units.
    Trials use affine interpolation, then bisection.
    Model support follows the engine, including local volatility with finite differences.
    """

def implied_phoenix_coupon(
    engine: QuadratureAutocallableEngine
    | FiniteDifferenceAutocallableEngine
    | MonteCarloAutocallableEngine,
    instrument: PhoenixOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> _pricing.PhoenixCouponQuote:
    """Solve a Phoenix coupon in absolute periodic rate units.

    Parameters
    ----------
    engine : object
        QuadratureAutocallableEngine, FiniteDifferenceAutocallableEngine or
        MonteCarloAutocallableEngine.
    instrument : PhoenixOption
        Contract to reprice.
    context : PricingContext
        Market snapshot.
    observed_price : float
        Finite target price in the engine's payment units.
    lower_bound, upper_bound : float, optional
        Finite coupon bracket in the contract's units, defaulting to [0, 2].
    price_tolerance : float, optional
        Positive finite price residual tolerance; defaults to 1e-8.
    parameter_tolerance : float, optional
        Positive finite bracket-width tolerance in coupon units; defaults to
        1e-8. Bracket convergence without a price match raises
        NonConvergenceError.
    max_iterations : int, optional
        Positive iteration budget, default 100; endpoint and affine evaluations
        are extra.

    Returns
    -------
    PhoenixCouponQuote
        Dictionary with one key, coupon_per_period: the signed coupon rate per
        qualifying observation. Apply with instrument.with_coupon(**quote).

    Raises
    ------
    InvalidInputError
        If the quote, settings or repriced terms are invalid.
    UnsupportedOperationError
        If the combination is unsupported or the coupon is not identifiable.
    RootNotBracketedError
        If endpoints do not bracket the target.
    NonConvergenceError
        If the iteration budget is exhausted or the bracket converges without a
        price match.
    ComputationFailureError
        If a price, residual or numerical evaluation is non-finite.

    Notes
    -----
    Replaces the conditional coupon, preserving its units. Monte Carlo solves
    require an explicit seed; call with_resolved_seed() first; tolerances
    constrain the sampled curve, not sampling error. Pricing failures
    propagate. Trials use affine interpolation, then bisection.
    Model support follows the engine, including local volatility with finite differences.
    """

class EuropeanOption:
    """European vanilla option.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.

    Raises
    ------
    InvalidInputError
        If strike is nonpositive or non-finite, or expiry precedes
        effective_time.

    Notes
    -----
    Pricing permits valuation before effective_time for this fixed-strike
    terminal payoff. Model data must cover valuation through expiry. Valuation
    after expiry is invalid.
    """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> EuropeanOption:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

class AmericanOption:
    """Vanilla option exercisable throughout its inclusive contract life.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.

    Raises
    ------
    InvalidInputError
        If strike is nonpositive or non-finite, or expiry precedes
        effective_time.
    """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> AmericanOption:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

class CashOrNothingOption:
    """European cash-or-nothing option paying only strictly in the money.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    payout : float
        Positive finite cash payment.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.

    Raises
    ------
    InvalidInputError
        If payout or strike is nonpositive or non-finite, or expiry precedes
        effective_time.
    """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        payout: _Real,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        payout: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> CashOrNothingOption:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def payout(self) -> float:
        """Return the fixed cash payout."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

class AssetOrNothingOption:
    """European asset-or-nothing option paying only strictly in the money.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.

    Raises
    ------
    InvalidInputError
        If strike is nonpositive or non-finite, or expiry precedes
        effective_time.
    """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> AssetOrNothingOption:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

class RateTermStructure:
    """Rate term structure with log-linear discount factors.

    Parameters
    ----------
    reference_time : datetime.datetime
        Inclusive curve reference; factors equal one at this instant.
    times : iterable of datetime.datetime
        Strictly increasing nodes after the reference.
    factors : iterable of float
        Positive finite factors at the nodes; log factors interpolate linearly.

    Raises
    ------
    InvalidInputError
        If nodes or factors are empty or mismatched, times do not increase
        after the reference, factors are nonpositive or non-finite, or implied
        forward rates are non-finite.

    Notes
    -----
    Uses the Actual/365 Fixed day-count convention. Log-linear interpolation of
    discount factors gives piecewise constant instantaneous forward rates.
    Dated curves reject times before their reference; node-based rate curves
    also reject times after the last node. Negative rates are allowed.
    """

    def __init__(
        self,
        *,
        reference_time: datetime,
        times: Iterable[datetime],
        factors: Iterable[_Real],
    ) -> None: ...
    @staticmethod
    def flat(
        rate: _Real, *, reference_time: datetime | None = None
    ) -> RateTermStructure:
        """Create a continuously compounded flat rate curve.

        Parameters
        ----------
        rate : float
            Finite annual rate in decimal units; negative rates are allowed.
        reference_time : datetime.datetime, optional
            Inclusive lower coverage bound, or None for an undated curve.

        Returns
        -------
        RateTermStructure
            Owned flat curve with no upper coverage bound.

        Raises
        ------
        InvalidInputError
            If the rate is non-finite.
        """

    @staticmethod
    def bounded_flat(
        rate: _Real, *, reference_time: datetime, max_time: datetime
    ) -> RateTermStructure:
        """Create an exactly constant rate with inclusive coverage bounds.

        Parameters
        ----------
        rate : float
            Finite continuously compounded annual rate; negative rates are allowed.
        reference_time, max_time : datetime.datetime
            Inclusive coverage bounds. Equal bounds select a single instant.

        Returns
        -------
        RateTermStructure
            Owned curve retaining the exact scalar rate and both bounds, including
            through shifts and pickle. Nodes and factors are empty, as for flat().

        Raises
        ------
        InvalidInputError
            If the rate is non-finite or max_time precedes reference_time.
        """

    @property
    def exact_flat_rate(self) -> float | None:
        """Return the scalar rate when all implied forwards are exactly constant.

        Nonconstant curves return None. Node-based coverage is preserved. This
        query does not change the curve or its coverage bounds.
        """

    @property
    def reference_time(self) -> datetime | None:
        """Return the inclusive lower coverage bound, or None if undated."""

    @property
    def max_time(self) -> datetime | None:
        """Return the inclusive upper bound, or None for an unbounded flat curve."""

    @property
    def nodes(self) -> tuple[datetime, ...]:
        """Return a copy of the supplied curve node times."""

    @property
    def factors(self) -> tuple[float, ...]:
        """Return owned node factors including shifts, allowing underflow to zero.

        Raises
        ------
        ComputationFailureError
            If a log factor is non-finite or exponentiation overflows.

        Notes
        -----
        Zero factors cannot reconstruct a curve through its constructor.
        Reconstruct node curves from base_factors, then apply
        shifted(parallel_shift). Pickle preserves this representation, including
        extreme shifted curves.
        """

    @property
    def base_factors(self) -> tuple[float, ...]:
        """Return original node factors without parallel shifts.

        Flat curves have no node factors. For node curves, pass these factors with
        reference_time and nodes to RateTermStructure, then call
        shifted(parallel_shift).
        """

    @property
    def parallel_shift(self) -> float:
        """Return the accumulated node-curve parallel shift in decimal units.

        Flat curves include shifts in their rate and return zero here.
        """

    def factor(self, start_time: datetime, end_time: datetime) -> float:
        """Return the factor over an ordered, covered interval.

        Parameters
        ----------
        start_time, end_time : datetime.datetime
            Ordered interval endpoints.

        Returns
        -------
        float
            Interval factor; underflow to zero is allowed.

        Raises
        ------
        InvalidInputError
            If endpoints are reversed or outside curve coverage.
        ComputationFailureError
            If the log factor is non-finite or exponentiation overflows.
        """

    def log_factor(self, start_time: datetime, end_time: datetime) -> float:
        """Return the log factor over an ordered, covered interval.

        Parameters
        ----------
        start_time, end_time : datetime.datetime
            Ordered interval endpoints.

        Returns
        -------
        float
            Log of the interval factor, using Actual/365 Fixed time.

        Raises
        ------
        InvalidInputError
            If endpoints are reversed or outside curve coverage.
        ComputationFailureError
            If the result is non-finite.
        """

    def forward_rate(self, time: datetime) -> float:
        """Return the right-hand instantaneous forward rate.

        Parameters
        ----------
        time : datetime.datetime
            Query instant. The final node uses its left interval.

        Returns
        -------
        float
            Annual continuously compounded forward rate in decimal units.

        Raises
        ------
        InvalidInputError
            If the instant is outside curve coverage.
        """

    def shifted(self, shift: _Real) -> RateTermStructure:
        """Create a parallel rate shift without changing this curve.

        Parameters
        ----------
        shift : float
            Finite additive rate shift in decimal units.

        Returns
        -------
        RateTermStructure
            Independent curve with unchanged coverage.

        Raises
        ------
        InvalidInputError
            If the shift or resulting rates are non-finite.
        """

class BlackScholesMertonParameters:
    """Black-Scholes-Merton (BSM) model parameters.

    Parameters
    ----------
    risk_free_rate : float or RateTermStructure
        Continuously compounded risk-free rate or owned rate curve.
    dividend_yield : float or RateTermStructure
        Continuously compounded dividend yield or owned rate curve.
    volatility : float
        Nonnegative finite annualized log-return volatility in decimal units.
        Zero selects the exact deterministic path implied by the rate curves.

    Raises
    ------
    InvalidInputError
        If scalar rates are non-finite or volatility is negative or non-finite.
    """

    def __init__(
        self,
        *,
        risk_free_rate: _Real | RateTermStructure,
        dividend_yield: _Real | RateTermStructure,
        volatility: _Real,
    ) -> None: ...
    def replace(
        self,
        *,
        risk_free_rate: _Real | RateTermStructure | EllipsisType = ...,
        dividend_yield: _Real | RateTermStructure | EllipsisType = ...,
        volatility: _Real | EllipsisType = ...,
    ) -> BlackScholesMertonParameters:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def flat_risk_free_rate(self) -> float:
        """Return the scalar rate for dated or undated constant curves.

        Use ``risk_free_curve`` to preserve coverage when passing rates to constructors
        or replacements.

        Raises
        ------
        UnsupportedOperationError
            If the curve is nonconstant.
        """

    @property
    def flat_dividend_yield(self) -> float:
        """Return the scalar yield for dated or undated constant curves.

        Use ``dividend_curve`` to preserve coverage when passing yields to constructors
        or replacements.

        Raises
        ------
        UnsupportedOperationError
            If the curve is nonconstant.
        """

    @property
    def risk_free_curve(self) -> RateTermStructure:
        """Return an owned copy of the risk-free curve."""

    @property
    def dividend_curve(self) -> RateTermStructure:
        """Return an owned copy of the dividend curve."""

    @property
    def volatility(self) -> float:
        """Return nonnegative finite annualized volatility in decimal units."""

class HestonParameters:
    """Heston stochastic-volatility parameters under the risk-neutral measure.

    Parameters
    ----------
    risk_free_rate, dividend_yield : float or RateTermStructure
        Continuously compounded annual rates or owned deterministic curves.
    initial_variance : float
        Variance at valuation; 20% initial volatility means 0.04 variance.
    long_run_variance : float
        Long-run variance level theta.
    mean_reversion_speed : float
        Mean-reversion coefficient kappa, with time measured in years.
    vol_of_vol : float
        Volatility-of-volatility parameter xi in dv = kappa * (theta - v) dt +
        xi * sqrt(v) dW.
    correlation : float
        Correlation between the asset and variance Brownian motions in [-1, 1],
        including endpoints.

    Raises
    ------
    InvalidInputError
        For non-finite inputs, negative variance parameters or invalid
        correlation.

    Notes
    -----
    Zero parameters and violations of the Feller condition (2 * kappa * theta
    >= xi**2) are accepted. AnalyticHestonEuropeanEngine prices European
    options; FiniteDifferenceHestonVanillaEngine prices European and American
    options. Requested Greeks have status "unsupported"; implied-parameter solvers
    raise UnsupportedOperationError. Numeric conversion rejects booleans.
    Replacement validates in Rust before returning a new owned model; the original
    is unchanged on failure.
    """

    def __init__(
        self,
        *,
        risk_free_rate: _Real | RateTermStructure,
        dividend_yield: _Real | RateTermStructure,
        initial_variance: _Real,
        long_run_variance: _Real,
        mean_reversion_speed: _Real,
        vol_of_vol: _Real,
        correlation: _Real,
    ) -> None: ...
    def replace(
        self,
        *,
        risk_free_rate: _Real | RateTermStructure | EllipsisType = ...,
        dividend_yield: _Real | RateTermStructure | EllipsisType = ...,
        initial_variance: _Real | EllipsisType = ...,
        long_run_variance: _Real | EllipsisType = ...,
        mean_reversion_speed: _Real | EllipsisType = ...,
        vol_of_vol: _Real | EllipsisType = ...,
        correlation: _Real | EllipsisType = ...,
    ) -> HestonParameters:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def flat_risk_free_rate(self) -> float:
        """Return the scalar rate for dated or undated constant curves.

        Use ``risk_free_curve`` to preserve coverage when passing rates to constructors
        or replacements.

        Raises
        ------
        UnsupportedOperationError
            If the curve is nonconstant.
        """

    @property
    def flat_dividend_yield(self) -> float:
        """Return the scalar yield for dated or undated constant curves.

        Use ``dividend_curve`` to preserve coverage when passing yields to constructors
        or replacements.

        Raises
        ------
        UnsupportedOperationError
            If the curve is nonconstant.
        """

    @property
    def risk_free_curve(self) -> RateTermStructure:
        """Return an owned copy of the risk-free curve."""

    @property
    def dividend_curve(self) -> RateTermStructure:
        """Return an owned copy of the dividend curve."""

    @property
    def initial_variance(self) -> float:
        """Return instantaneous variance at valuation."""

    @property
    def long_run_variance(self) -> float:
        """Return the long-run variance level."""

    @property
    def mean_reversion_speed(self) -> float:
        """Return mean-reversion speed per year."""

    @property
    def vol_of_vol(self) -> float:
        """Return the volatility-of-volatility parameter xi."""

    @property
    def correlation(self) -> float:
        """Return the correlation between the asset and variance Brownian motions."""

class LocalVolatilitySurface:
    """Local-volatility table indexed by time (rows) and absolute spot (columns).

    Parameters
    ----------
    times : iterable of datetime.datetime
        At least two strictly increasing instants; the first is the reference.
    spots : iterable of float
        At least two strictly increasing positive finite prices.
    values : iterable of iterable of float
        Rectangular positive finite annualized volatilities (20% is 0.2).

    Raises
    ------
    InvalidInputError
        If the table shape, nodes or values are invalid.

    Notes
    -----
    Local volatility interpolates bilinearly in time and absolute spot price,
    using Actual/365 Fixed year fractions. Time extrapolation is rejected;
    extrapolation in the spot dimension holds the nearest endpoint volatility
    constant. Inputs are copied. Properties return owned copies; spot changes
    and time rolls never translate the surface. Native conversion errors reject
    booleans, dates, timezone-aware datetimes and invalid iterable elements.
    """

    def __init__(
        self,
        *,
        times: Iterable[datetime],
        spots: Iterable[_Real],
        values: Iterable[Iterable[_Real]],
    ) -> None: ...
    @property
    def times(self) -> tuple[datetime, ...]:
        """Return copied absolute time nodes as datetimes."""

    @property
    def spots(self) -> tuple[float, ...]:
        """Return copied absolute price nodes."""

    @property
    def values(self) -> tuple[tuple[float, ...], ...]:
        """Return copied volatility rows, indexed by time then price."""

    @property
    def maximum_volatility(self) -> float:
        """Return the largest supplied annualized volatility."""

    def volatility(self, time: datetime, spot: _Real) -> float:
        """Interpolate sigma at an absolute time and positive spot.

        Parameters
        ----------
        time : datetime.datetime
            Covered query instant.
        spot : float
            Positive finite underlying price; extrapolation holds endpoint
            volatility constant.

        Returns
        -------
        float
            Annualized local volatility in decimal units.

        Raises
        ------
        InvalidInputError
            If time is outside coverage or spot is nonpositive or non-finite.
        """

class LocalVolatilityParameters:
    """Local-volatility model parameters.

    Parameters
    ----------
    risk_free_rate, dividend_yield : float or RateTermStructure
        Finite continuously compounded rates or validated rate curves.
    surface : LocalVolatilitySurface
        Validated surface in absolute time and price coordinates.

    Raises
    ------
    InvalidInputError
        If a scalar rate is non-finite.

    Notes
    -----
    Surface and curve coverage are checked by the context and during pricing
    over the required remaining horizons. The one-dimensional finite-difference
    engines support price, delta and gamma; digital pricing requires constant
    rates.
    """

    def __init__(
        self,
        *,
        risk_free_rate: _Real | RateTermStructure,
        dividend_yield: _Real | RateTermStructure,
        surface: LocalVolatilitySurface,
    ) -> None: ...
    def replace(
        self,
        *,
        risk_free_rate: _Real | RateTermStructure | EllipsisType = ...,
        dividend_yield: _Real | RateTermStructure | EllipsisType = ...,
        surface: LocalVolatilitySurface | EllipsisType = ...,
    ) -> LocalVolatilityParameters:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def surface(self) -> LocalVolatilitySurface:
        """Return an owned copy of the fixed surface."""

    @property
    def flat_risk_free_rate(self) -> float:
        """Return the scalar rate for dated or undated constant curves.

        Use ``risk_free_curve`` to preserve coverage when passing rates to constructors
        or replacements.

        Raises
        ------
        UnsupportedOperationError
            If the curve is nonconstant.
        """

    @property
    def flat_dividend_yield(self) -> float:
        """Return the scalar yield for dated or undated constant curves.

        Use ``dividend_curve`` to preserve coverage when passing yields to constructors
        or replacements.

        Raises
        ------
        UnsupportedOperationError
            If the curve is nonconstant.
        """

    @property
    def risk_free_curve(self) -> RateTermStructure:
        """Return an owned copy of the risk-free curve."""

    @property
    def dividend_curve(self) -> RateTermStructure:
        """Return an owned copy of the dividend curve."""

class TradingCalendar:
    """Trading calendar with a fixed annualization basis.

    Notes
    -----
    Use the calendar factory functions to construct instances. Calendars with
    equal bases, normalized overrides and annualization bases compare equal.
    Empty overrides retain the base calendar's kind.
    """

    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def trading_days_per_year(self) -> int:
        """Return the positive calendar annualization basis."""

    def is_trading_day(self, value: date) -> bool:
        """Return whether a date is open in this calendar.

        Parameters
        ----------
        value : datetime.date
            Date to test; datetime values are not accepted.

        Returns
        -------
        bool
            Whether the date is a trading day, including explicit overrides.
        """

    def adjust(
        self, nominal: date, convention: Literal["following", "preceding"]
    ) -> date:
        """Find a trading date in the selected direction, including the input date.

        Parameters
        ----------
        nominal : datetime.date
            Unadjusted date.
        convention : {'following', 'preceding'}
            Direction to search.

        Returns
        -------
        datetime.date
            First trading date encountered.

        Raises
        ------
        InvalidInputError
            If the Rust core's supported date range is exhausted.
        ValueError
            If the resulting date is outside Python's years 1 through 9999. For
            example, preceding adjustment of a closed datetime.date.min cannot be
            converted back to a Python date.
        """

    def trading_days_between(self, start: date, end: date) -> int:
        """Count trading dates in the half-open interval [start, end).

        Parameters
        ----------
        start, end : datetime.date
            Ordered interval endpoints.

        Returns
        -------
        int
            Number of trading dates.

        Raises
        ------
        InvalidInputError
            If end precedes start.
        """

    def trading_year_fraction(self, start: date, end: date) -> float:
        """Return trading-day count divided by the calendar annualization basis.

        Parameters
        ----------
        start, end : datetime.date
            Ordered endpoints of the half-open interval [start, end).

        Returns
        -------
        float
            Trading-year fraction.

        Raises
        ------
        InvalidInputError
            If end precedes start.
        """

def weekdays_calendar() -> TradingCalendar:
    """Create a Monday-through-Friday calendar with a 252-day basis.

    Returns
    -------
    TradingCalendar
        Calendar with the stated annualization basis.
    """

def all_days_calendar() -> TradingCalendar:
    """Create a calendar with every date open and a 365-day basis.

    Returns
    -------
    TradingCalendar
        Calendar with the stated annualization basis.
    """

def sse_calendar() -> TradingCalendar:
    """Create a Shanghai Stock Exchange (SSE) calendar with a 243-day basis.

    Returns
    -------
    TradingCalendar
        Calendar with the stated annualization basis.

    Notes
    -----
    Uses recorded and forecast weekday closures for 1991-2099, and weekdays
    outside that range. Forecasts omit ad-hoc closures.
    """

def custom_calendar(
    *,
    base: TradingCalendar,
    closed_dates: Iterable[date] = (),
    open_dates: Iterable[date] = (),
    trading_days_per_year: SupportsIndex | None = None,
) -> TradingCalendar:
    """Create open and closed overrides without changing the base calendar.

    Parameters
    ----------
    base : TradingCalendar
        Base calendar.
    closed_dates : iterable of date, optional
        Dates forced closed; duplicates and unsorted input are accepted.
    open_dates : iterable of date, optional
        Dates forced open, including weekends; must not also be closed.
    trading_days_per_year : int or None, optional
        Positive annualization basis, or None to retain the base basis.

    Returns
    -------
    TradingCalendar
        Observation records.

    Raises
    ------
    InvalidInputError
        If a date is both open and closed, or the annualization basis is
        nonpositive.
    """

class ObservationSchedule:
    """Ordered observation times with sequence access.

    Parameters
    ----------
    times : iterable of datetime.datetime
        Strictly increasing observation timestamps; empty input is valid.
    start_time, end_time : datetime.datetime
        Inclusive validation bounds, with start_time at or before end_time.
    calendar : TradingCalendar, optional
        Calendar used to validate each observation date. Omission or Ellipsis
        inherits times.calendar for an ObservationSchedule, otherwise weekdays.

    Raises
    ------
    InvalidInputError
        If life bounds are reversed, times are unordered or outside life, or an
        observation date is closed in the calendar.

    Notes
    -----
    Construct from explicit times, or use fixed_interval_schedule or
    monthly_schedule. Bounds and calendar validate the input; the schedule owns
    its times and calendar. Empty schedules are valid. The times property returns a
    copy; iteration retains the schedule owner. Integer indexing accepts
    negative indices, rejects bool, and raises IndexError outside the sequence.
    reversed(schedule) iterates over an owned copy of the timestamps.
    Slices return independent schedules with the same calendar. Decreasing
    selections raise InvalidInputError. Slice bounds and steps accept integers
    excluding bool; a zero step raises ValueError. len(schedule) counts observations.
    """

    def __init__(
        self,
        *,
        times: Iterable[datetime],
        start_time: datetime,
        end_time: datetime,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the calendar used to validate the schedule."""

    @property
    def times(self) -> tuple[datetime, ...]:
        """Return a copy of the strictly ordered observation times."""

    def __len__(self) -> int: ...
    @overload
    def __getitem__(self, index: SupportsIndex) -> datetime: ...
    @overload
    def __getitem__(self, index: slice) -> ObservationSchedule: ...
    def __iter__(self) -> Iterator[datetime]: ...
    def __reversed__(self) -> Iterator[datetime]: ...

def fixed_interval_schedule(
    *,
    start_time: datetime,
    end_time: datetime,
    interval_days: SupportsIndex,
    observation_clock_time: time,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule:
    """Create adjusted observations at positive fixed calendar-day intervals.

    Parameters
    ----------
    start_time : datetime.datetime
        First timestamp of the interval.
    end_time : datetime.datetime
        Last timestamp of the interval.
    interval_days : int
        Positive calendar-day spacing of unadjusted observations, up to 2**63 -
        1.
    observation_clock_time : datetime.time
        Required observation time of day, without timezone information.
    calendar : TradingCalendar, optional
        Trading calendar. Defaults to weekdays with a 252-day basis.

    Returns
    -------
    ObservationSchedule
        Observation records.

    Raises
    ------
    InvalidInputError
        If end_time precedes start_time or interval_days is nonpositive.
    TypeError
        If interval_days is not an integer or is bool.
    OverflowError
        If interval_days is outside the signed 64-bit range.

    Notes
    -----
    Exclude start_time and include candidates through end_time. Following
    adjustment merges collisions and stops before crossing ``end_time``;
    ``end_time`` need not appear.
    """

def monthly_schedule(
    *,
    start_time: datetime,
    end_time: datetime,
    first_observation_month: SupportsIndex,
    observation_clock_time: time,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule:
    """Create monthly observations anchored to the start_time day.

    Parameters
    ----------
    start_time : datetime.datetime
        First timestamp of the interval.
    end_time : datetime.datetime
        Last timestamp of the interval.
    first_observation_month : int
        Positive month offset from start_time to the first observation.
    observation_clock_time : datetime.time
        Required observation time of day, without timezone information.
    calendar : TradingCalendar, optional
        Trading calendar. Defaults to weekdays with a 252-day basis.

    Returns
    -------
    ObservationSchedule
        Observation records.

    Raises
    ------
    InvalidInputError
        If end_time precedes start_time or first_observation_month is
        nonpositive.

    Notes
    -----
    Clamp each candidate to its month end while retaining the original
    starting-day anchor for later months. Exclude start_time and include
    candidates through end_time. Following adjustment merges collisions and
    stops before crossing ``end_time``; ``end_time`` need not appear.
    """

class PricingContext:
    """Market snapshot for one valuation instant.

    Parameters
    ----------
    model_parameters : BlackScholesMertonParameters, LocalVolatilityParameters or HestonParameters
        Validated rates and the selected volatility or variance model.
    spot_price : float
        Positive finite underlying spot price.
    valuation_time : datetime.datetime
        Valuation time.

    Raises
    ------
    InvalidInputError
        If spot_price is nonpositive or non-finite or valuation_time is outside
        either rate curve or the local-volatility surface. Contract-life and
        expiry coverage checks occur at pricing.
    """

    def __init__(
        self,
        *,
        model_parameters: BlackScholesMertonParameters
        | LocalVolatilityParameters
        | HestonParameters,
        spot_price: _Real,
        valuation_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        model_parameters: BlackScholesMertonParameters
        | LocalVolatilityParameters
        | HestonParameters
        | EllipsisType = ...,
        spot_price: _Real | EllipsisType = ...,
        valuation_time: datetime | EllipsisType = ...,
    ) -> PricingContext:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def model_parameters(
        self,
    ) -> BlackScholesMertonParameters | LocalVolatilityParameters | HestonParameters:
        """Return an owned copy of the model parameters."""

    @property
    def spot_price(self) -> float:
        """Return positive finite underlying spot price."""

    @property
    def valuation_date(self) -> date:
        """Return the date containing the valuation instant."""

    @property
    def valuation_time(self) -> datetime:
        """Return valuation time."""

class _PricingMethods[Instrument]:
    """Shared pricing signatures; concrete engines specify supported contracts."""

    def greek_method(
        self, instrument: Instrument, context: PricingContext
    ) -> _pricing.GreekMethod:
        """Return the Greek estimation method.

        Checks model applicability; pricing validates life and history.

        Parameters
        ----------
        instrument : object
            Contract supported by this engine.
        context : PricingContext
            Market snapshot.

        Returns
        -------
        str
            "native", "numerical", "mixed", "native_with_spot_control", "terminal",
            or "unsupported".

        Raises
        ------
        KiyosiError
            If the model is unsupported by this engine.
        """

    def active_greek_controls(
        self, instrument: Instrument, context: PricingContext
    ) -> list[_pricing.GreekControl]:
        """Return shift controls used by the Greek estimation method.

        Parameters
        ----------
        instrument : object
            Contract supported by this engine.
        context : PricingContext
            Market snapshot.

        Returns
        -------
        list of str
            Potentially active controls: spot_shift, volatility_shift, rate_shift,
            and time_shift. Individual requests may use fewer controls.

        Raises
        ------
        KiyosiError
            If the model is unsupported by this engine.
        """

    def price(self, instrument: Instrument, context: PricingContext) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.
        """

    def price_with_greeks(
        self,
        instrument: Instrument,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry, requested Greeks use scalar numerical differences with
        the request's shift controls. Omitted controls use engine defaults.
        """

class AnalyticEuropeanEngine(_PricingMethods[EuropeanOption]):
    """Analytic European vanilla pricing with deterministic rate curves.

    See Also
    --------
    EuropeanOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    def price_with_greeks(
        self,
        instrument: EuropeanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Uses analytic Greeks at positive volatility before expiry and scalar
        differences at zero volatility. At expiry, supported Greeks use exact
        payoff derivatives; spot derivatives are undefined at the strike and
        time Greeks are absent. Use numerical_greeks for scalar differences.
        """

class AnalyticHestonEuropeanEngine(_PricingMethods[EuropeanOption]):
    """Price European calls and puts under Heston by adaptive Fourier integration.

    Parameters
    ----------
    absolute_tolerance : float, optional
        Finite nonnegative tolerance in price units; defaults to 1e-8.
    relative_tolerance : float, optional
        Finite nonnegative relative tolerance; defaults to 1e-8.
    max_evaluations : int, optional
        Positive request-wide characteristic-function budget; defaults to
        10000.

    Raises
    ------
    InvalidInputError
        If tolerances are invalid or both zero, or the budget is zero.

    Notes
    -----
    Supports HestonParameters and EuropeanOption only, including zero variance
    parameters and correlation endpoints. Deterministic rates may be curves.
    The target price error is absolute_tolerance + relative_tolerance *
    abs(price). Error estimates are not a universal accuracy guarantee. The
    budget covers Heston and control-variate characteristic functions across
    all integrals and tail checks. Expiry and deterministic-variance branches
    need no evaluations. Requested Greeks have status "unsupported";
    implied-parameter solvers raise UnsupportedOperationError. Pricing releases
    the GIL for core computation, with independent state for concurrent calls.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        absolute_tolerance: _Real = ...,
        relative_tolerance: _Real = ...,
        max_evaluations: SupportsIndex = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        absolute_tolerance: _Real | EllipsisType = ...,
        relative_tolerance: _Real | EllipsisType = ...,
        max_evaluations: SupportsIndex | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def absolute_tolerance(self) -> float:
        """Return the absolute tolerance in price units."""

    @property
    def relative_tolerance(self) -> float:
        """Return the dimensionless relative tolerance."""

    @property
    def max_evaluations(self) -> int:
        """Return the total characteristic-function budget per price request."""

    def price(self, instrument: EuropeanOption, context: PricingContext) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : EuropeanOption
            European call or put; valuation may precede effective_time.
        context : PricingContext
            Heston market snapshot; initial variance is at valuation. Rate/dividend
            curves must cover valuation through expiry.

        Returns
        -------
        float
            Price, or intrinsic payoff at the expiry instant.

        Raises
        ------
        UnsupportedOperationError
            For unsupported instruments or models, including at expiry.
        InvalidInputError
            If valuation follows expiry or lies outside curve coverage.
        NonConvergenceError
            If the budget or numerical resolution cannot meet the estimated target.
        ComputationFailureError
            For invalid numerical intermediates or prices.
        """

    def price_with_greeks(
        self,
        instrument: EuropeanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Requested Greeks have status "unsupported". Bump controls are
        validated but unused; numerical_greeks rejects Heston.
        """

class BjerksundStenslandVanillaEngine(_PricingMethods[AmericanOption]):
    """Bjerksund-Stensland 2002 American approximation with flat rates.

    See Also
    --------
    AmericanOption : Supported contract.

    Notes
    -----
    Requires BSM with constant rates. At positive volatility before expiry,
    negative transformed interest rates (risk-free rate for calls, dividend
    yield for puts) and exercise boundaries at or below strike raise
    UnsupportedOperationError. Zero volatility uses exact deterministic
    exercise; expiry returns intrinsic value.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class AnalyticDigitalEngine(
    _PricingMethods[CashOrNothingOption | AssetOrNothingOption]
):
    """Analytic European digital pricing with deterministic rate curves.

    See Also
    --------
    CashOrNothingOption : Supported contract.
    AssetOrNothingOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Delta and gamma are analytic at positive volatility before expiry. At zero
        volatility they use scalar differences. Other Greeks use numerical
        differences with the supplied shifts. At expiry spot Greeks use the
        supplied terminal-payoff stencils. Use numerical_greeks for scalar-only
        differences.
        """

class CoxRossRubinsteinBarrierEngine(_PricingMethods[BarrierOption]):
    """Flat-rate Cox-Ross-Rubinstein tree pricing.

    Parameters
    ----------
    step_count : int, optional
        Number of tree time steps, from 1 through 1,000,000; defaults to 256.

    Raises
    ------
    InvalidInputError
        If step_count is outside 1 through 1,000,000.

    See Also
    --------
    BarrierOption : Supported contract.

    Notes
    -----
    Provides node delta/gamma for its product family; other Greeks are absent.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self, *, step_count: SupportsIndex = ...) -> None: ...
    def replace(self, *, step_count: SupportsIndex | EllipsisType = ...) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def step_count(self) -> int:
        """Return the configured number of binomial time steps."""

    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        At positive volatility before expiry delta and gamma use tree nodes. Zero
        volatility uses scalar differences. Other Greeks are unsupported. At expiry
        spot Greeks use default terminal-payoff stencils. Use numerical_greeks for
        configurable scalar differences.
        """

class QuadratureBarrierEngine(_PricingMethods[BarrierOption]):
    """Flat-rate quadrature pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Base number of spatial intervals, at least 4; defaults to 512. Event
        boundaries may add intervals; Simpson panels round up to even.
    integration_standard_deviations : float, optional
        Positive finite integration extent in standard deviations; defaults to
        8.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 4 or integration_standard_deviations is
        nonpositive or non-finite.

    See Also
    --------
    BarrierOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        integration_standard_deviations: _Real | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

class QuadratureDigitalEngine(
    _PricingMethods[CashOrNothingOption | AssetOrNothingOption]
):
    """Flat-rate quadrature pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Base number of spatial intervals, at least 4; defaults to 512. Event
        boundaries may add intervals; Simpson panels round up to even.
    integration_standard_deviations : float, optional
        Positive finite integration extent in standard deviations; defaults to
        8.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 4 or integration_standard_deviations is
        nonpositive or non-finite.

    See Also
    --------
    CashOrNothingOption : Supported European cash payment.
    AssetOrNothingOption : Supported European asset payment.

    Notes
    -----
    Uses Simpson integration on the paying side of the strike under flat-rate
    BSM. The extent is centered on the payment-weighted normal density; omitted
    tails are zero. Equality at expiry pays zero. Check step-count and extent
    convergence separately; completion provides no error estimate. Numerical
    Greeks also require bump convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        integration_standard_deviations: _Real | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

class QuadratureVanillaEngine(_PricingMethods[EuropeanOption | AmericanOption]):
    """Flat-rate quadrature pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Base number of spatial intervals, at least 4; defaults to 512. Event
        boundaries may add intervals; Simpson panels round up to even.
    time_step_count : int, optional
        American exercise intervals, at least 1; defaults to 200. Ignored for European pricing.
    integration_standard_deviations : float, optional
        Positive finite integration extent in standard deviations; defaults to
        8.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 4 or integration_standard_deviations is
        nonpositive or non-finite.
        Also raised if time_step_count is below 1.

    See Also
    --------
    EuropeanOption : Supported contract.
    AmericanOption : Supported contract.

    Notes
    -----
    Uses Simpson integration for European options and Gaussian backward
    induction with piecewise cubic continuation interpolation for American
    options. Requires flat-rate BSM parameters. Check spatial, time and
    integration-extent convergence separately; fixed-grid completion does not
    estimate error. Higher Greeks near exercise boundaries also require bump
    convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        integration_standard_deviations: _Real | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured American exercise interval count."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

class CoxRossRubinsteinVanillaEngine(_PricingMethods[EuropeanOption | AmericanOption]):
    """Flat-rate Cox-Ross-Rubinstein tree pricing.

    Parameters
    ----------
    step_count : int, optional
        Number of tree time steps, from 1 through 1,000,000; defaults to 256.

    Raises
    ------
    InvalidInputError
        If step_count is outside 1 through 1,000,000.

    Notes
    -----
    Accepts EuropeanOption and AmericanOption.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self, *, step_count: SupportsIndex = ...) -> None: ...
    def replace(self, *, step_count: SupportsIndex | EllipsisType = ...) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def step_count(self) -> int:
        """Return the configured number of binomial time steps."""

    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        At positive volatility before expiry delta and gamma use tree nodes;
        one-step gamma and other Greeks use numerical differences with engine
        defaults. At expiry spot Greeks use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences. Zero volatility uses
        scalar differences.
        """

class CoxRossRubinsteinDigitalEngine(
    _PricingMethods[CashOrNothingOption | AssetOrNothingOption]
):
    """Flat-rate Cox-Ross-Rubinstein tree pricing.

    Parameters
    ----------
    step_count : int, optional
        Number of tree time steps, from 1 through 1,000,000; defaults to 256.

    Raises
    ------
    InvalidInputError
        If step_count is outside 1 through 1,000,000.

    See Also
    --------
    CashOrNothingOption : Supported contract.
    AssetOrNothingOption : Supported contract.

    Notes
    -----
    Provides node delta/gamma for its product family; other Greeks are absent.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self, *, step_count: SupportsIndex = ...) -> None: ...
    def replace(self, *, step_count: SupportsIndex | EllipsisType = ...) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def step_count(self) -> int:
        """Return the configured number of binomial time steps."""

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        At positive volatility before expiry delta and gamma use tree nodes. Zero
        volatility uses scalar differences. Other Greeks are unsupported. At expiry
        spot Greeks use default terminal-payoff stencils. Use numerical_greeks for
        configurable scalar differences.
        """

class FiniteDifferenceAccumulatorEngine(_PricingMethods[Accumulator]):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceAccumulatorEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: Accumulator,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class FiniteDifferenceBarrierEngine(_PricingMethods[BarrierOption]):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceBarrierEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class FiniteDifferenceDoubleBarrierEngine(_PricingMethods[DoubleBarrierOption]):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceDoubleBarrierEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class FiniteDifferenceAutocallableEngine(
    _PricingMethods[
        SnowballOption | BinarySnowballOption | TernarySnowballOption | PhoenixOption
    ]
):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceAutocallableEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class FiniteDifferenceDoubleTouchEngine(_PricingMethods[DoubleTouchOption]):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceDoubleTouchEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: DoubleTouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class FiniteDifferenceBinaryBarrierEngine(
    _PricingMethods[BinaryBarrierOption | TouchOption]
):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceBinaryBarrierEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: BinaryBarrierOption | TouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class FiniteDifferenceHestonVanillaEngine(
    _PricingMethods[EuropeanOption | AmericanOption]
):
    """Finite-difference prices for European and American options under Heston.

    Parameters
    ----------
    asset_step_count : int, optional
        Asset intervals, at least 3; read the property for the Rust-defined
        default.
    variance_step_count : int, optional
        Variance intervals, at least 3; read the property for the default.
    time_step_count : int, optional
        Base time intervals, at least 1; rate/dividend knots are added.
    asset_upper_boundary : float or None, optional
        Finite positive bound, or automatic selection. Must exceed spot and
        strike when a numerical grid is needed.
    variance_upper_boundary : float or None, optional
        Finite positive bound, or automatic selection. Must exceed initial
        variance when a variance grid is needed.

    Raises
    ------
    InvalidInputError
        For invalid counts or bounds. Context-dependent coverage is checked at
        pricing.

    Notes
    -----
    Uses the modified Craig-Sneyd (MCS) alternating direction implicit (ADI)
    scheme on nonuniform grids, with Ikonen-Toivanen splitting for American
    exercise. Supports HestonParameters and EuropeanOption/AmericanOption,
    including violations of the Feller condition, zero parameters, correlation
    endpoints and deterministic rate curves. Fixed-grid completion does not
    establish accuracy. Check spatial/time refinement and domain enlargement
    separately. Non-finite or materially negative results raise
    ComputationFailureError. Requested Greeks have status "unsupported";
    implied-parameter solvers raise UnsupportedOperationError. Inputs and settings
    are immutable; core pricing releases the GIL and permits concurrent calls
    with independent state.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        variance_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        asset_upper_boundary: _Real | None = None,
        variance_upper_boundary: _Real | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        variance_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        variance_upper_boundary: _Real | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of asset intervals."""

    @property
    def variance_step_count(self) -> int:
        """Return the configured number of variance intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the base time intervals before inserting curve knots."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit asset upper bound, or None."""

    @property
    def variance_upper_boundary(self) -> float | None:
        """Return the explicit variance upper bound, or None."""

    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : EuropeanOption or AmericanOption
            European or American call or put. European valuation may precede
            effective_time; American valuation must be within contract life.
        context : PricingContext
            Heston market snapshot, with initial variance at valuation. Rate/dividend
            curves must cover valuation through expiry.

        Returns
        -------
        float
            Price, or intrinsic value at the expiry instant.

        Raises
        ------
        UnsupportedOperationError
            For other models or instruments, including at expiry.
        InvalidInputError
            Outside permitted valuation bounds or curve coverage, or for
            insufficient explicit bounds.
        ComputationFailureError
            For unrepresentable grids or invalid numerical results.
        """

    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Requested Greeks have status "unsupported". Bump controls are
        validated but unused; numerical_greeks rejects Heston.
        """

class FiniteDifferenceVanillaEngine(_PricingMethods[EuropeanOption | AmericanOption]):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceVanillaEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class FiniteDifferenceDigitalEngine(
    _PricingMethods[CashOrNothingOption | AssetOrNothingOption]
):
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.
        Read ``FiniteDifferenceDigitalEngine().time_step_count`` for the engine's default.
    scheme : FiniteDifferenceScheme, optional
        Time-marching scheme; defaults to "rannacher" (damped Crank-Nicolson).
        "crank_nicolson" uses standard Crank-Nicolson without time-step
        damping.
    asset_upper_boundary : float or None, optional
        Finite positive upper spot boundary, or None (default) for the
        automatic domain. Must exceed spot, strike and barriers at pricing.
        Continuous up and double barriers fix their own upper grid edge; this
        bound is inactive for that grid. Local-volatility knock-in vanilla
        components and scheduled monitoring use the full domain.
    spot_shift : float or None, optional
        Positive absolute spot bump for native Greek availability and terminal
        stencils. None selects the automatic bump; prices and grids are
        unchanged.
    volatility_shift : float or None, optional
        Positive decimal volatility bump, or None (default) for the smaller of
        0.0001 and half the current volatility. Native spatial delta and gamma
        retain their estimator.
    rate_shift : float, optional
        Positive decimal rate bump; defaults to 0.0001.
    time_shift : datetime.timedelta or None, optional
        Positive whole-microsecond time bump, or None for automatic selection.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 3, time_step_count is below 1, or an
        explicit upper boundary or Greek bump is invalid. Boundary coverage and
        explicit-Euler stability are checked during pricing.

    Notes
    -----
    The engine name selects its product family. The vanilla engine accepts
    EuropeanOption and AmericanOption. The digital engine accepts
    CashOrNothingOption and AssetOrNothingOption. The binary-barrier engine
    also accepts TouchOption. The autocallable engine accepts all snowball
    variants and PhoenixOption. Other engines accept their named product.
    Digital pricing requires constant rates; other families accept
    deterministic rate curves. All FD product families support local-volatility
    price, delta and gamma, including American exercise. Other requested Greeks
    are None. Autocallable coupon solvers support local volatility; implied
    volatility raises UnsupportedOperationError. Sigma
    uses fixed absolute surface coordinates; expiry Greeks follow terminal
    payoffs and are absent where their stencils cross product events. Vanilla
    expiry Greeks use exact supported payoff derivatives, undefined at the
    strike. Surface coverage follows remaining stochastic evolution; rate
    coverage follows required future discounting. Automatic local-volatility
    domains require convergence checks.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
        spot_shift: _Real | None = None,
        volatility_shift: _Real | None = ...,
        rate_shift: _Real = ...,
        time_shift: timedelta | None = None,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        time_step_count: SupportsIndex | EllipsisType = ...,
        scheme: _pricing.FiniteDifferenceScheme | EllipsisType = ...,
        asset_upper_boundary: _Real | None | EllipsisType = ...,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def time_step_count(self) -> int:
        """Return the configured base number of time intervals before event insertion."""

    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme:
        """Return the configured time-marching choice string."""

    @property
    def asset_upper_boundary(self) -> float | None:
        """Return the explicit upper spot boundary, or None for automatic selection."""

    @property
    def volatility_shift(self) -> float | None:
        """Return the numerical volatility bump, or None for automatic selection."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.

        Notes
        -----
        Before expiry delta and gamma come from the spatial solution, except zero
        BSM volatility uses scalar differences. Other BSM Greeks use numerical
        differences with the configured shifts. Local volatility provides only
        delta and gamma. At expiry vanilla Greeks use exact supported payoff
        derivatives; other products use default terminal-payoff stencils. Use
        numerical_greeks for configurable scalar differences under BSM.
        """

class MonteCarloVanillaEngine(_PricingMethods[EuropeanOption | AmericanOption]):
    """Flat-rate Monte Carlo pricing for European and American vanilla options.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    step_count : int, optional
        Exercise intervals, 1 through 10,000; defaults to 50. Validated even
        when unused; see Notes for product-specific use.
    seed : int or None, optional
        Unsigned 64-bit seed, or None (default) to use system entropy.
    max_active_workers : int, optional
        Nonnegative active worker cap per call: zero (default) is automatic,
        one is serial. Does not bound thread creation; parallel calls share a
        worker pool, bounded by configure_monte_carlo_pool().
    sampling : MonteCarloSampling, optional
        Sampling method; defaults to pseudo_random.
    antithetic : bool, optional
        Pair opposite normal draws; defaults to False. An odd path count leaves
        one unpaired path.

    Raises
    ------
    InvalidInputError
        If paths are outside 1 through 10,000,000, max_active_workers is
        negative, or Sobol paths are not a power of two.
        Also raised if step_count is outside 1 through 10,000.

    See Also
    --------
    EuropeanOption : Supported contract.
    AmericanOption : Supported contract.

    Notes
    -----
    Fixed seeds reproduce results across thread counts in the same build. Calls
    own their samples and share a library-owned Rayon pool. A worker cap does
    not reserve workers. Greeks pin a seed per request. Solves require an
    explicit seed; call with_resolved_seed() first. Solver tolerances do not
    measure sampling error.
    European pricing samples the exact terminal
    distribution and ignores step_count. Near-strike payoffs preserve small
    movements even when the terminal spot rounds to the strike. American
    pricing uses least-squares Monte Carlo (Longstaff-Schwartz) with step_count
    exercise intervals and requires at least two before expiry.
    """

    def with_resolved_seed(self) -> Self:
        """Return an engine with a concrete seed, preserving all other controls.

        An existing seed is retained. Save or pickle the returned engine before
        pricing to replay its draws within the same build. The original is
        unchanged. Entropy failures raise OSError.
        """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        step_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        max_active_workers: SupportsIndex = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        path_count: SupportsIndex | EllipsisType = ...,
        step_count: SupportsIndex | EllipsisType = ...,
        seed: SupportsIndex | None | EllipsisType = ...,
        max_active_workers: SupportsIndex | EllipsisType = ...,
        sampling: _pricing.MonteCarloSampling | EllipsisType = ...,
        antithetic: bool | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def path_count(self) -> int:
        """Return the exact number of sampled paths."""

    @property
    def step_count(self) -> int:
        """Return the configured grid size."""

    @property
    def seed(self) -> int | None:
        """Return the unsigned 64-bit seed, or None for system entropy."""

    @property
    def max_active_workers(self) -> int:
        """Return the per-call active worker cap; zero selects automatic sizing.

        Parallel calls share a CPU-sized pool; this cap does not bound thread
        creation.
        """

    @property
    def sampling(self) -> _pricing.MonteCarloSampling:
        """Return the sampling choice string: pseudo_random or sobol."""

    @property
    def antithetic(self) -> bool:
        """Return whether opposite normal draws are paired."""

    def price_with_seed(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> tuple[float, int | None]:
        """Return the scalar price and the seed used by this request.

        Parameters
        ----------
        instrument : object
            Any contract supported by price(), including American vanilla options.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        tuple[float, int or None]
            Price and actual sampling seed; None when no sampling is needed.
            Reprice with replace(seed=seed) and the same inputs and controls to
            replay the result within the same build. The engine is unchanged.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or scalar pricing fails.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Does not compute sampling uncertainty. Signal exceptions cancel the request
        and propagate without a partial result, as for price().
        """

    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.

        OSError
            If system entropy or worker creation fails.
        """

    def price_with_statistics(
        self, instrument: EuropeanOption, context: PricingContext
    ) -> MonteCarloEstimate:
        """Return a price with sampling uncertainty.

        Parameters
        ----------
        instrument : object
            A supported contract. Vanilla statistics require EuropeanOption; other
            engines accept the same contracts as price().
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        MonteCarloEstimate
            Mean, optional standard error and asymptotic 95% confidence interval.
            Sobol, insufficient independent samples and odd antithetic counts omit
            uncertainty. Known pseudorandom analytic values have zero error.
            Sampling uncertainty does not measure numerical integration bias.

        Raises
        ------
        KiyosiError
            If pricing or uncertainty validation fails, or the contract is
            unsupported.
        OSError
            If system entropy or worker creation fails.
        """

    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.
        """

class MonteCarloEstimate:
    """Monte Carlo price with optional sampling uncertainty.

    Notes
    -----
    Standard error uses independent pseudorandom observations or even
    antithetic pair means. The interval is an asymptotic 95% normal confidence
    interval. Uncertainty is absent for Sobol, insufficient independent samples
    and odd antithetic counts; known pseudorandom settlements have zero
    uncertainty. The seed property identifies the actual draws, including for
    unseeded engines; None means no sampling occurred. Replay requires the same
    inputs, engine controls and build.
    """

    def __eq__(self, other: object) -> bool: ...
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def price(self) -> float:
        """Return the finite price in contract payment units."""

    @property
    def seed(self) -> int | None:
        """Return the sampling seed, or None when no sampling occurred."""

    @property
    def standard_error(self) -> float | None:
        """Return sampling standard error, or None when unavailable."""

    @property
    def uncertainty_status(self) -> _pricing.MonteCarloUncertaintyStatus:
        """Return availability or the reason sampling uncertainty is absent."""

    @property
    def confidence_interval(self) -> tuple[float, float] | None:
        """Return the asymptotic 95% confidence interval, or None when unavailable."""

class MonteCarloDigitalEngine(
    _PricingMethods[CashOrNothingOption | AssetOrNothingOption]
):
    """Flat-rate CPU Monte Carlo pricing for European digitals.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    seed : int or None, optional
        Unsigned 64-bit seed, or None (default) to use system entropy.
    max_active_workers : int, optional
        Nonnegative active worker cap per call: zero (default) is automatic,
        one is serial. Does not bound thread creation; parallel calls share a
        worker pool, bounded by configure_monte_carlo_pool().
    sampling : MonteCarloSampling, optional
        Sampling method; defaults to pseudo_random.
    antithetic : bool, optional
        Pair opposite normal draws; defaults to False. An odd path count leaves
        one unpaired path.

    Raises
    ------
    InvalidInputError
        If paths are outside 1 through 10,000,000, max_active_workers is
        negative, or Sobol paths are not a power of two.

    See Also
    --------
    CashOrNothingOption : Supported contract.
    AssetOrNothingOption : Supported contract.

    Notes
    -----
    Fixed seeds reproduce results across thread counts in the same build. Calls
    own their samples and share a library-owned Rayon pool. A worker cap does
    not reserve workers. Greeks pin a seed per request. Solves require an
    explicit seed; call with_resolved_seed() first. Solver tolerances do not
    measure sampling error.
    Only likelihood-ratio delta and gamma are returned;
    other requested Greeks are absent.
    """

    def with_resolved_seed(self) -> Self:
        """Return an engine with a concrete seed, preserving all other controls.

        An existing seed is retained. Save or pickle the returned engine before
        pricing to replay its draws within the same build. The original is
        unchanged. Entropy failures raise OSError.
        """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        max_active_workers: SupportsIndex = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        path_count: SupportsIndex | EllipsisType = ...,
        seed: SupportsIndex | None | EllipsisType = ...,
        max_active_workers: SupportsIndex | EllipsisType = ...,
        sampling: _pricing.MonteCarloSampling | EllipsisType = ...,
        antithetic: bool | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def path_count(self) -> int:
        """Return the exact number of sampled paths."""

    @property
    def seed(self) -> int | None:
        """Return the unsigned 64-bit seed, or None for system entropy."""

    @property
    def max_active_workers(self) -> int:
        """Return the per-call active worker cap; zero selects automatic sizing.

        Parallel calls share a CPU-sized pool; this cap does not bound thread
        creation.
        """

    @property
    def sampling(self) -> _pricing.MonteCarloSampling:
        """Return the sampling choice string: pseudo_random or sobol."""

    @property
    def antithetic(self) -> bool:
        """Return whether opposite normal draws are paired."""

    def price_with_seed(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> tuple[float, int | None]:
        """Return the scalar price and the seed used by this request.

        Parameters
        ----------
        instrument : object
            Any contract supported by price(), including American vanilla options.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        tuple[float, int or None]
            Price and actual sampling seed; None when no sampling is needed.
            Reprice with replace(seed=seed) and the same inputs and controls to
            replay the result within the same build. The engine is unchanged.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or scalar pricing fails.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Does not compute sampling uncertainty. Signal exceptions cancel the request
        and propagate without a partial result, as for price().
        """

    def price(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.

        OSError
            If system entropy or worker creation fails.
        """

    def price_with_statistics(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> MonteCarloEstimate:
        """Return a price with sampling uncertainty.

        Parameters
        ----------
        instrument : object
            A supported contract. Vanilla statistics require EuropeanOption; other
            engines accept the same contracts as price().
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        MonteCarloEstimate
            Mean, optional standard error and asymptotic 95% confidence interval.
            Sobol, insufficient independent samples and odd antithetic counts omit
            uncertainty. Known pseudorandom analytic values have zero error.
            Sampling uncertainty does not measure numerical integration bias.

        Raises
        ------
        KiyosiError
            If pricing or uncertainty validation fails, or the contract is
            unsupported.
        OSError
            If system entropy or worker creation fails.
        """

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        At positive volatility before expiry delta and gamma use native
        likelihood-ratio estimators. Zero volatility uses scalar differences. At
        expiry they use default terminal-payoff stencils. Use numerical_greeks for
        configurable scalar differences.
        """

class MonteCarloBarrierEngine(_PricingMethods[BarrierOption]):
    """Flat-rate CPU Monte Carlo pricing for single-barrier vanilla options.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    seed : int or None, optional
        Unsigned 64-bit seed, or None (default) to use system entropy.
    max_active_workers : int, optional
        Nonnegative active worker cap per call: zero (default) is automatic,
        one is serial. Does not bound thread creation; parallel calls share a
        worker pool, bounded by configure_monte_carlo_pool().
    sampling : MonteCarloSampling, optional
        Sampling method; defaults to pseudo_random.
    antithetic : bool, optional
        Pair opposite normal draws; defaults to False. An odd path count leaves
        one unpaired path.

    Raises
    ------
    InvalidInputError
        If paths are outside 1 through 10,000,000, max_active_workers is
        negative, or Sobol paths are not a power of two.

    See Also
    --------
    BarrierOption : Supported contract.

    Notes
    -----
    Fixed seeds reproduce results across thread counts in the same build. Calls
    own their samples and share a library-owned Rayon pool. A worker cap does
    not reserve workers. Greeks pin a seed per request. Solves require an
    explicit seed; call with_resolved_seed() first. Solver tolerances do not
    measure sampling error.
    Monitoring uses continuous survival or the original
    fixing dates. Near-strike payoffs retain movements even when terminal spot
    rounds to strike. Only delta and gamma are returned.
    """

    def with_resolved_seed(self) -> Self:
        """Return an engine with a concrete seed, preserving all other controls.

        An existing seed is retained. Save or pickle the returned engine before
        pricing to replay its draws within the same build. The original is
        unchanged. Entropy failures raise OSError.
        """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        max_active_workers: SupportsIndex = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        path_count: SupportsIndex | EllipsisType = ...,
        seed: SupportsIndex | None | EllipsisType = ...,
        max_active_workers: SupportsIndex | EllipsisType = ...,
        sampling: _pricing.MonteCarloSampling | EllipsisType = ...,
        antithetic: bool | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def path_count(self) -> int:
        """Return the exact number of sampled paths."""

    @property
    def seed(self) -> int | None:
        """Return the unsigned 64-bit seed, or None for system entropy."""

    @property
    def max_active_workers(self) -> int:
        """Return the per-call active worker cap; zero selects automatic sizing.

        Parallel calls share a CPU-sized pool; this cap does not bound thread
        creation.
        """

    @property
    def sampling(self) -> _pricing.MonteCarloSampling:
        """Return the sampling choice string: pseudo_random or sobol."""

    @property
    def antithetic(self) -> bool:
        """Return whether opposite normal draws are paired."""

    def price_with_seed(
        self, instrument: BarrierOption, context: PricingContext
    ) -> tuple[float, int | None]:
        """Return the scalar price and the seed used by this request.

        Parameters
        ----------
        instrument : object
            Any contract supported by price(), including American vanilla options.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        tuple[float, int or None]
            Price and actual sampling seed; None when no sampling is needed.
            Reprice with replace(seed=seed) and the same inputs and controls to
            replay the result within the same build. The engine is unchanged.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or scalar pricing fails.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Does not compute sampling uncertainty. Signal exceptions cancel the request
        and propagate without a partial result, as for price().
        """

    def price(self, instrument: BarrierOption, context: PricingContext) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.

        OSError
            If system entropy or worker creation fails.
        """

    def price_with_statistics(
        self, instrument: BarrierOption, context: PricingContext
    ) -> MonteCarloEstimate:
        """Return a price with sampling uncertainty.

        Parameters
        ----------
        instrument : object
            A supported contract. Vanilla statistics require EuropeanOption; other
            engines accept the same contracts as price().
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        MonteCarloEstimate
            Mean, optional standard error and asymptotic 95% confidence interval.
            Sobol, insufficient independent samples and odd antithetic counts omit
            uncertainty. Known pseudorandom analytic values have zero error.
            Sampling uncertainty does not measure numerical integration bias.

        Raises
        ------
        KiyosiError
            If pricing or uncertainty validation fails, or the contract is
            unsupported.
        OSError
            If system entropy or worker creation fails.
        """

    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Barrier engines use spot_shift for event-availability checks and continuous
        at-hit rebate derivatives. Autocallable and accumulator engines use
        numerical differences with engine defaults. At expiry spot Greeks use
        default terminal-payoff stencils. Use numerical_greeks for explicit scalar
        bump calculations on supported BSM pairs.
        """

class MonteCarloDoubleBarrierEngine(_PricingMethods[DoubleBarrierOption]):
    """Flat-rate CPU Monte Carlo pricing for double-barrier vanilla options.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    seed : int or None, optional
        Unsigned 64-bit seed, or None (default) to use system entropy.
    max_active_workers : int, optional
        Nonnegative active worker cap per call: zero (default) is automatic,
        one is serial. Does not bound thread creation; parallel calls share a
        worker pool, bounded by configure_monte_carlo_pool().
    sampling : MonteCarloSampling, optional
        Sampling method; defaults to pseudo_random.
    antithetic : bool, optional
        Pair opposite normal draws; defaults to False. An odd path count leaves
        one unpaired path.

    Raises
    ------
    InvalidInputError
        If paths are outside 1 through 10,000,000, max_active_workers is
        negative, or Sobol paths are not a power of two.

    See Also
    --------
    DoubleBarrierOption : Supported contract.

    Notes
    -----
    Fixed seeds reproduce results across thread counts in the same build. Calls
    own their samples and share a library-owned Rayon pool. A worker cap does
    not reserve workers. Greeks pin a seed per request. Solves require an
    explicit seed; call with_resolved_seed() first. Solver tolerances do not
    measure sampling error.
    Monitoring uses continuous survival or the original
    fixing dates. Near-strike payoffs retain movements even when terminal spot
    rounds to strike. Only delta and gamma are returned.
    """

    def with_resolved_seed(self) -> Self:
        """Return an engine with a concrete seed, preserving all other controls.

        An existing seed is retained. Save or pickle the returned engine before
        pricing to replay its draws within the same build. The original is
        unchanged. Entropy failures raise OSError.
        """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        max_active_workers: SupportsIndex = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        path_count: SupportsIndex | EllipsisType = ...,
        seed: SupportsIndex | None | EllipsisType = ...,
        max_active_workers: SupportsIndex | EllipsisType = ...,
        sampling: _pricing.MonteCarloSampling | EllipsisType = ...,
        antithetic: bool | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def path_count(self) -> int:
        """Return the exact number of sampled paths."""

    @property
    def seed(self) -> int | None:
        """Return the unsigned 64-bit seed, or None for system entropy."""

    @property
    def max_active_workers(self) -> int:
        """Return the per-call active worker cap; zero selects automatic sizing.

        Parallel calls share a CPU-sized pool; this cap does not bound thread
        creation.
        """

    @property
    def sampling(self) -> _pricing.MonteCarloSampling:
        """Return the sampling choice string: pseudo_random or sobol."""

    @property
    def antithetic(self) -> bool:
        """Return whether opposite normal draws are paired."""

    def price_with_seed(
        self, instrument: DoubleBarrierOption, context: PricingContext
    ) -> tuple[float, int | None]:
        """Return the scalar price and the seed used by this request.

        Parameters
        ----------
        instrument : object
            Any contract supported by price(), including American vanilla options.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        tuple[float, int or None]
            Price and actual sampling seed; None when no sampling is needed.
            Reprice with replace(seed=seed) and the same inputs and controls to
            replay the result within the same build. The engine is unchanged.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or scalar pricing fails.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Does not compute sampling uncertainty. Signal exceptions cancel the request
        and propagate without a partial result, as for price().
        """

    def price(self, instrument: DoubleBarrierOption, context: PricingContext) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.

        OSError
            If system entropy or worker creation fails.
        """

    def price_with_statistics(
        self, instrument: DoubleBarrierOption, context: PricingContext
    ) -> MonteCarloEstimate:
        """Return a price with sampling uncertainty.

        Parameters
        ----------
        instrument : object
            A supported contract. Vanilla statistics require EuropeanOption; other
            engines accept the same contracts as price().
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        MonteCarloEstimate
            Mean, optional standard error and asymptotic 95% confidence interval.
            Sobol, insufficient independent samples and odd antithetic counts omit
            uncertainty. Known pseudorandom analytic values have zero error.
            Sampling uncertainty does not measure numerical integration bias.

        Raises
        ------
        KiyosiError
            If pricing or uncertainty validation fails, or the contract is
            unsupported.
        OSError
            If system entropy or worker creation fails.
        """

    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Barrier engines use spot_shift for event-availability checks and continuous
        at-hit rebate derivatives. Autocallable and accumulator engines use
        numerical differences with engine defaults. At expiry spot Greeks use
        default terminal-payoff stencils. Use numerical_greeks for explicit scalar
        bump calculations on supported BSM pairs.
        """

class MonteCarloAutocallableEngine(
    _PricingMethods[
        SnowballOption | BinarySnowballOption | TernarySnowballOption | PhoenixOption
    ]
):
    """Flat-rate CPU Monte Carlo pricing for snowball and Phoenix notes.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    seed : int or None, optional
        Unsigned 64-bit seed, or None (default) to use system entropy.
    max_active_workers : int, optional
        Nonnegative active worker cap per call: zero (default) is automatic,
        one is serial. Does not bound thread creation; parallel calls share a
        worker pool, bounded by configure_monte_carlo_pool().
    sampling : MonteCarloSampling, optional
        Sampling method; defaults to pseudo_random.
    antithetic : bool, optional
        Pair opposite normal draws; defaults to False. An odd path count leaves
        one unpaired path.

    Raises
    ------
    InvalidInputError
        If paths are outside 1 through 10,000,000, max_active_workers is
        negative, or Sobol paths are not a power of two.

    See Also
    --------
    SnowballOption : Supported contract.
    BinarySnowballOption : Supported contract.
    TernarySnowballOption : Supported contract.
    PhoenixOption : Supported contract.

    Notes
    -----
    Fixed seeds reproduce results across thread counts in the same build. Calls
    own their samples and share a library-owned Rayon pool. A worker cap does
    not reserve workers. Greeks pin a seed per request. Solves require an
    explicit seed; call with_resolved_seed() first. Solver tolerances do not
    measure sampling error.
    Simulation uses trading days when knock-in matters,
    or observation dates and expiry otherwise.
    """

    def with_resolved_seed(self) -> Self:
        """Return an engine with a concrete seed, preserving all other controls.

        An existing seed is retained. Save or pickle the returned engine before
        pricing to replay its draws within the same build. The original is
        unchanged. Entropy failures raise OSError.
        """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        max_active_workers: SupportsIndex = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        path_count: SupportsIndex | EllipsisType = ...,
        seed: SupportsIndex | None | EllipsisType = ...,
        max_active_workers: SupportsIndex | EllipsisType = ...,
        sampling: _pricing.MonteCarloSampling | EllipsisType = ...,
        antithetic: bool | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def path_count(self) -> int:
        """Return the exact number of sampled paths."""

    @property
    def seed(self) -> int | None:
        """Return the unsigned 64-bit seed, or None for system entropy."""

    @property
    def max_active_workers(self) -> int:
        """Return the per-call active worker cap; zero selects automatic sizing.

        Parallel calls share a CPU-sized pool; this cap does not bound thread
        creation.
        """

    @property
    def sampling(self) -> _pricing.MonteCarloSampling:
        """Return the sampling choice string: pseudo_random or sobol."""

    @property
    def antithetic(self) -> bool:
        """Return whether opposite normal draws are paired."""

    def price_with_seed(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
    ) -> tuple[float, int | None]:
        """Return the scalar price and the seed used by this request.

        Parameters
        ----------
        instrument : object
            Any contract supported by price(), including American vanilla options.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        tuple[float, int or None]
            Price and actual sampling seed; None when no sampling is needed.
            Reprice with replace(seed=seed) and the same inputs and controls to
            replay the result within the same build. The engine is unchanged.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or scalar pricing fails.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Does not compute sampling uncertainty. Signal exceptions cancel the request
        and propagate without a partial result, as for price().
        """

    def price(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
    ) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.

        OSError
            If system entropy or worker creation fails.
        """

    def price_with_statistics(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
    ) -> MonteCarloEstimate:
        """Return a price with sampling uncertainty.

        Parameters
        ----------
        instrument : object
            A supported contract. Vanilla statistics require EuropeanOption; other
            engines accept the same contracts as price().
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        MonteCarloEstimate
            Mean, optional standard error and asymptotic 95% confidence interval.
            Sobol, insufficient independent samples and odd antithetic counts omit
            uncertainty. Known pseudorandom analytic values have zero error.
            Sampling uncertainty does not measure numerical integration bias.

        Raises
        ------
        KiyosiError
            If pricing or uncertainty validation fails, or the contract is
            unsupported.
        OSError
            If system entropy or worker creation fails.
        """

    def price_with_greeks(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Barrier engines use spot_shift for event-availability checks and continuous
        at-hit rebate derivatives. Autocallable and accumulator engines use
        numerical differences with engine defaults. At expiry spot Greeks use
        default terminal-payoff stencils. Use numerical_greeks for explicit scalar
        bump calculations on supported BSM pairs.
        """

class MonteCarloAccumulatorEngine(_PricingMethods[Accumulator]):
    """Flat-rate CPU Monte Carlo pricing for accumulators.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    seed : int or None, optional
        Unsigned 64-bit seed, or None (default) to use system entropy.
    max_active_workers : int, optional
        Nonnegative active worker cap per call: zero (default) is automatic,
        one is serial. Does not bound thread creation; parallel calls share a
        worker pool, bounded by configure_monte_carlo_pool().
    sampling : MonteCarloSampling, optional
        Sampling method; defaults to pseudo_random.
    antithetic : bool, optional
        Pair opposite normal draws; defaults to False. An odd path count leaves
        one unpaired path.

    Raises
    ------
    InvalidInputError
        If paths are outside 1 through 10,000,000, max_active_workers is
        negative, or Sobol paths are not a power of two.

    See Also
    --------
    Accumulator : Supported contract.

    Notes
    -----
    Fixed seeds reproduce results across thread counts in the same build. Calls
    own their samples and share a library-owned Rayon pool. A worker cap does
    not reserve workers. Greeks pin a seed per request. Solves require an
    explicit seed; call with_resolved_seed() first. Solver tolerances do not
    measure sampling error.
    Simulation uses the future trading dates. A current
    knock-out settles before any purchase.
    """

    def with_resolved_seed(self) -> Self:
        """Return an engine with a concrete seed, preserving all other controls.

        An existing seed is retained. Save or pickle the returned engine before
        pricing to replay its draws within the same build. The original is
        unchanged. Entropy failures raise OSError.
        """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        max_active_workers: SupportsIndex = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        path_count: SupportsIndex | EllipsisType = ...,
        seed: SupportsIndex | None | EllipsisType = ...,
        max_active_workers: SupportsIndex | EllipsisType = ...,
        sampling: _pricing.MonteCarloSampling | EllipsisType = ...,
        antithetic: bool | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def path_count(self) -> int:
        """Return the exact number of sampled paths."""

    @property
    def seed(self) -> int | None:
        """Return the unsigned 64-bit seed, or None for system entropy."""

    @property
    def max_active_workers(self) -> int:
        """Return the per-call active worker cap; zero selects automatic sizing.

        Parallel calls share a CPU-sized pool; this cap does not bound thread
        creation.
        """

    @property
    def sampling(self) -> _pricing.MonteCarloSampling:
        """Return the sampling choice string: pseudo_random or sobol."""

    @property
    def antithetic(self) -> bool:
        """Return whether opposite normal draws are paired."""

    def price_with_seed(
        self, instrument: Accumulator, context: PricingContext
    ) -> tuple[float, int | None]:
        """Return the scalar price and the seed used by this request.

        Parameters
        ----------
        instrument : object
            Any contract supported by price(), including American vanilla options.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        tuple[float, int or None]
            Price and actual sampling seed; None when no sampling is needed.
            Reprice with replace(seed=seed) and the same inputs and controls to
            replay the result within the same build. The engine is unchanged.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or scalar pricing fails.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Does not compute sampling uncertainty. Signal exceptions cancel the request
        and propagate without a partial result, as for price().
        """

    def price(self, instrument: Accumulator, context: PricingContext) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.

        OSError
            If system entropy or worker creation fails.
        """

    def price_with_statistics(
        self, instrument: Accumulator, context: PricingContext
    ) -> MonteCarloEstimate:
        """Return a price with sampling uncertainty.

        Parameters
        ----------
        instrument : object
            A supported contract. Vanilla statistics require EuropeanOption; other
            engines accept the same contracts as price().
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        MonteCarloEstimate
            Mean, optional standard error and asymptotic 95% confidence interval.
            Sobol, insufficient independent samples and odd antithetic counts omit
            uncertainty. Known pseudorandom analytic values have zero error.
            Sampling uncertainty does not measure numerical integration bias.

        Raises
        ------
        KiyosiError
            If pricing or uncertainty validation fails, or the contract is
            unsupported.
        OSError
            If system entropy or worker creation fails.
        """

    def price_with_greeks(
        self,
        instrument: Accumulator,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Barrier engines use spot_shift for event-availability checks and continuous
        at-hit rebate derivatives. Autocallable and accumulator engines use
        numerical differences with engine defaults. At expiry spot Greeks use
        default terminal-payoff stencils. Use numerical_greeks for explicit scalar
        bump calculations on supported BSM pairs.
        """

class MonteCarloAsianEngine(
    _PricingMethods[GeometricAveragePriceOption | ArithmeticAveragePriceOption]
):
    """Flat-rate CPU Monte Carlo pricing for continuous average-price options.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    step_count : int, optional
        Grid size, 2 through 10,000; defaults to 50. Validated even
        when unused; see Notes for product-specific use.
    seed : int or None, optional
        Unsigned 64-bit seed, or None (default) to use system entropy.
    max_active_workers : int, optional
        Nonnegative active worker cap per call: zero (default) is automatic,
        one is serial. Does not bound thread creation; parallel calls share a
        worker pool, bounded by configure_monte_carlo_pool().
    sampling : MonteCarloSampling, optional
        Sampling method; defaults to pseudo_random.
    antithetic : bool, optional
        Pair opposite normal draws; defaults to False. An odd path count leaves
        one unpaired path.

    Raises
    ------
    InvalidInputError
        If paths are outside 1 through 10,000,000, max_active_workers is
        negative, or Sobol paths are not a power of two.
        Also raised if step_count is outside 2 through 10,000.

    See Also
    --------
    GeometricAveragePriceOption : Supported contract.
    ArithmeticAveragePriceOption : Supported contract.

    Notes
    -----
    Fixed seeds reproduce results across thread counts in the same build. Calls
    own their samples and share a library-owned Rayon pool. A worker cap does
    not reserve workers. Greeks pin a seed per request. Solves require an
    explicit seed; call with_resolved_seed() first. Solver tolerances do not
    measure sampling error.
    Geometric averages and single fixings sample exact
    distributions. Arithmetic averages use step_count trapezoidal intervals
    with the first interval integrated conditionally.
    """

    def with_resolved_seed(self) -> Self:
        """Return an engine with a concrete seed, preserving all other controls.

        An existing seed is retained. Save or pickle the returned engine before
        pricing to replay its draws within the same build. The original is
        unchanged. Entropy failures raise OSError.
        """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        step_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        max_active_workers: SupportsIndex = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        path_count: SupportsIndex | EllipsisType = ...,
        step_count: SupportsIndex | EllipsisType = ...,
        seed: SupportsIndex | None | EllipsisType = ...,
        max_active_workers: SupportsIndex | EllipsisType = ...,
        sampling: _pricing.MonteCarloSampling | EllipsisType = ...,
        antithetic: bool | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def path_count(self) -> int:
        """Return the exact number of sampled paths."""

    @property
    def step_count(self) -> int:
        """Return the configured grid size."""

    @property
    def seed(self) -> int | None:
        """Return the unsigned 64-bit seed, or None for system entropy."""

    @property
    def max_active_workers(self) -> int:
        """Return the per-call active worker cap; zero selects automatic sizing.

        Parallel calls share a CPU-sized pool; this cap does not bound thread
        creation.
        """

    @property
    def sampling(self) -> _pricing.MonteCarloSampling:
        """Return the sampling choice string: pseudo_random or sobol."""

    @property
    def antithetic(self) -> bool:
        """Return whether opposite normal draws are paired."""

    def price_with_seed(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
    ) -> tuple[float, int | None]:
        """Return the scalar price and the seed used by this request.

        Parameters
        ----------
        instrument : object
            Any contract supported by price(), including American vanilla options.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        tuple[float, int or None]
            Price and actual sampling seed; None when no sampling is needed.
            Reprice with replace(seed=seed) and the same inputs and controls to
            replay the result within the same build. The engine is unchanged.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or scalar pricing fails.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        Does not compute sampling uncertainty. Signal exceptions cancel the request
        and propagate without a partial result, as for price().
        """

    def price(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
    ) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        float
            Present value of remaining contractual payments.

        Raises
        ------
        KiyosiError
            If domain validation, engine applicability or numerical pricing fails.

        OSError
            If system entropy or worker creation fails.
        """

    def price_with_statistics(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
    ) -> MonteCarloEstimate:
        """Return a price with sampling uncertainty.

        Parameters
        ----------
        instrument : object
            A supported contract. Vanilla statistics require EuropeanOption; other
            engines accept the same contracts as price().
        context : PricingContext
            Market snapshot and valuation instant.

        Returns
        -------
        MonteCarloEstimate
            Mean, optional standard error and asymptotic 95% confidence interval.
            Sobol, insufficient independent samples and odd antithetic counts omit
            uncertainty. Known pseudorandom analytic values have zero error.
            Sampling uncertainty does not measure numerical integration bias.

        Raises
        ------
        KiyosiError
            If pricing or uncertainty validation fails, or the contract is
            unsupported.
        OSError
            If system entropy or worker creation fails.
        """

    def price_with_greeks(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | None | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities using engine estimators.

        Parameters
        ----------
        instrument : object
            Supported contract; see the engine class for accepted types.
        context : PricingContext
            Market snapshot and valuation instant.
        greeks : str or iterable of str
            Greek names or "all" to request every measure; duplicates are ignored.
            An empty iterable requests price only.
        spot_shift : float or None, optional
            Positive absolute spot bump; None selects the product's automatic bump.
        volatility_shift : float or None, optional
            Positive decimal volatility bump. None selects the smaller of 0.0001
            and half the current volatility.
        rate_shift : float, optional
            Positive decimal rate bump; defaults to 0.0001.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot, volatility and time bumps and a 0.0001 rate
            bump. Controls affect numerical fallbacks, event checks and terminal
            stencils; native estimators and engine/model Greek coverage are
            preserved.

        Returns
        -------
        PricingResult
            Price and requested Greeks. Unsupported or undefined Greeks are None,
            with time Greeks absent at expiry; other terminal Greeks follow the
            payoff.

        Raises
        ------
        KiyosiError
            If controls are invalid, pricing fails or a feasible bumped valuation
            fails.
        ValueError
            If a Greek name is unknown.
        TypeError
            If representations, Greek names or keyword arguments are invalid.
        OSError
            If system entropy or worker creation fails.

        Notes
        -----
        At positive volatility before expiry delta and gamma use native estimators.
        Zero volatility uses scalar differences. At expiry delta and gamma use
        default terminal-payoff stencils. Use numerical_greeks for configurable
        scalar differences.
        """

type Greek = Literal[
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "volga",
    "vanna",
    "zomma",
    "rho",
    "dividend_rho",
]

class PricingResult:
    """Finite price and optional finite price sensitivities.

    Notes
    -----
    Unavailable or unrequested Greeks are None. All derivatives are unscaled:
    spot uses spot units, volatility and rates use decimal values, and forward
    valuation time uses years (Actual/365 Fixed). Other inputs, including
    expiry, remain fixed. With price V, spot S, volatility σ and valuation time
    t: delta = ∂V/∂S, gamma = ∂²V/∂S², speed = ∂³V/∂S³, theta = ∂V/∂t, charm =
    ∂²V/(∂S∂t), color = ∂³V/(∂S²∂t), vega = ∂V/∂σ, volga = ∂²V/∂σ², vanna =
    ∂²V/(∂S∂σ), and zomma = ∂³V/(∂S²∂σ). Rho shifts the risk-free curve in
    parallel; dividend rho shifts the dividend-yield curve in parallel: rho =
    ∂V/∂r and dividend_rho = ∂V/∂q, with decimal continuously compounded shifts
    r and q. Time derivatives hold market curves fixed. Positive time
    increments advance valuation.
    """

    def __eq__(self, other: object) -> bool: ...
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def seed(self) -> int | None:
        """Return the simulation seed used by this request, or None without sampling."""

    @property
    def price(self) -> float:
        """Return the finite price in contract payment units."""

    @property
    def delta(self) -> float | None:
        """Return delta, or None if unrequested or unavailable."""

    @property
    def gamma(self) -> float | None:
        """Return gamma, or None if unrequested or unavailable."""

    @property
    def speed(self) -> float | None:
        """Return speed, or None if unrequested or unavailable."""

    @property
    def theta(self) -> float | None:
        """Return theta, or None if unrequested or unavailable."""

    @property
    def charm(self) -> float | None:
        """Return charm, or None if unrequested or unavailable."""

    @property
    def color(self) -> float | None:
        """Return color, or None if unrequested or unavailable."""

    @property
    def vega(self) -> float | None:
        """Return vega, or None if unrequested or unavailable."""

    @property
    def volga(self) -> float | None:
        """Return volga, or None if unrequested or unavailable."""

    @property
    def vanna(self) -> float | None:
        """Return vanna, or None if unrequested or unavailable."""

    @property
    def zomma(self) -> float | None:
        """Return zomma, or None if unrequested or unavailable."""

    @property
    def rho(self) -> float | None:
        """Return rho, or None if unrequested or unavailable."""

    @property
    def dividend_rho(self) -> float | None:
        """Return dividend rho, or None if unrequested or unavailable."""

    def require(self, measure: Greek) -> float:
        """Return a requested Greek or raise when it is unavailable.

        Parameters
        ----------
        measure : str
            Greek name, such as delta or vega.

        Returns
        -------
        float
            Finite sensitivity in PricingResult reporting units.

        Raises
        ------
        GreekUnavailableError
            If the measure has no value. Its greek and status attributes identify
            the measure and reason. Also inherits UnsupportedOperationError.
        ValueError
            If the Greek name is unknown.
        TypeError
            If the measure is not a string.
        """

    def status(
        self, measure: Greek
    ) -> Literal[
        "available", "not_requested", "unsupported", "undefined", "stencil_unavailable"
    ]:
        """Return the availability status of a Greek.

        Parameters
        ----------
        measure : str
            Greek name.

        Returns
        -------
        str
            "available", "not_requested", "unsupported" (engine/model coverage),
            "undefined" (no derivative at this valuation), or "stencil_unavailable"
            (try different numerical controls).

        Raises
        ------
        ValueError
            If the Greek name is unknown.
        TypeError
            If the measure is not a string.
        """

    @property
    def requested(self) -> tuple[Greek, ...]:
        """Return requested Greek names in the library's stable measure order."""

    @property
    def supported(self) -> tuple[Greek, ...]:
        """Return engine/model-supported names, regardless of availability."""

@overload
def implied_volatility(
    engine: QuadratureAccumulatorEngine
    | FiniteDifferenceAccumulatorEngine
    | MonteCarloAccumulatorEngine,
    instrument: Accumulator,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float:
    """Solve the annualized volatility that reproduces an observed price.

    Parameters
    ----------
    engine : object
        Supported pricing engine for the instrument.
    instrument : object
        Contract to reprice.
    context : PricingContext
        Market snapshot.
    observed_price : float
        Finite target price in the engine's payment units.
    lower_bound : float
        Required nonnegative finite lower annualized volatility in decimal units.
    upper_bound : float
        Required finite upper annualized volatility, above lower_bound.
    price_tolerance : float, optional
        Positive finite absolute price residual tolerance; defaults to 1e-8.
    parameter_tolerance : float, optional
        Positive finite bracket-width tolerance in decimal units; defaults to
        1e-8. Bracket convergence without a price match raises
        NonConvergenceError.
    max_iterations : int, optional
        Positive iteration budget; defaults to 100.

    Returns
    -------
    float
        Annual parameter in decimal units.

    Raises
    ------
    InvalidInputError
        If the quote, settings or repriced terms are invalid.
    UnsupportedOperationError
        If the combination is unsupported or the parameter is not identifiable.
    RootNotBracketedError
        If the supplied endpoints do not bracket the target.
    NonConvergenceError
        If the iteration budget is exhausted or the bracket converges without a
        price match.
    ComputationFailureError
        If a price, residual or numerical evaluation is non-finite.

    Notes
    -----
    Pricing failures propagate. Monte Carlo solves require an explicit seed;
    call with_resolved_seed() first; tolerances constrain the sampled price
    curve, not its sampling error. Digital and barrier prices can have several
    volatility roots. The required bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: QuadratureAutocallableEngine
    | FiniteDifferenceAutocallableEngine
    | MonteCarloAutocallableEngine,
    instrument: SnowballOption
    | BinarySnowballOption
    | TernarySnowballOption
    | PhoenixOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticRangeAccrualEngine,
    instrument: RangeAccrualOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticDoubleBarrierEngine
    | FiniteDifferenceDoubleBarrierEngine
    | MonteCarloDoubleBarrierEngine,
    instrument: DoubleBarrierOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticDoubleTouchEngine | FiniteDifferenceDoubleTouchEngine,
    instrument: DoubleTouchOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticBarrierEngine
    | CoxRossRubinsteinBarrierEngine
    | MonteCarloBarrierEngine
    | QuadratureBarrierEngine
    | FiniteDifferenceBarrierEngine,
    instrument: BarrierOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticGeometricAveragePriceEngine | MonteCarloAsianEngine,
    instrument: GeometricAveragePriceOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: TurnbullWakemanArithmeticAveragePriceEngine | MonteCarloAsianEngine,
    instrument: ArithmeticAveragePriceOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticEuropeanEngine | MonteCarloVanillaEngine,
    instrument: EuropeanOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticDigitalEngine
    | QuadratureDigitalEngine
    | MonteCarloDigitalEngine
    | CoxRossRubinsteinDigitalEngine
    | FiniteDifferenceDigitalEngine,
    instrument: CashOrNothingOption | AssetOrNothingOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: BjerksundStenslandVanillaEngine | MonteCarloVanillaEngine,
    instrument: AmericanOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: CoxRossRubinsteinVanillaEngine
    | FiniteDifferenceVanillaEngine
    | QuadratureVanillaEngine,
    instrument: EuropeanOption | AmericanOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticBinaryBarrierEngine | FiniteDifferenceBinaryBarrierEngine,
    instrument: BinaryBarrierOption | TouchOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real,
    upper_bound: _Real,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
) -> float: ...
@overload
def numerical_greeks(
    engine: QuadratureAccumulatorEngine
    | FiniteDifferenceAccumulatorEngine
    | MonteCarloAccumulatorEngine,
    instrument: Accumulator,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult:
    """Compute numerical BSM Greeks through any supported built-in scalar engine.

    Parameters
    ----------
    engine, instrument : object
        Built-in engine and one of its supported contracts.
    context : PricingContext
        BSM market snapshot; local-volatility and Heston models are rejected.
    greeks : str or iterable of str
        Required measure names or "all". Unsupported instrument/engine pairs
        are checked before Greek names.
    spot_shift : float or None, optional
        Positive absolute spot bump. Omission or Ellipsis retains the engine's
        setting; None selects the automatic engine/product bump.
    volatility_shift : float or None, optional
        Positive decimal volatility bump. None selects the smaller of 0.0001 and
        half the current volatility. Omission or Ellipsis retains engine settings.
    rate_shift : float, optional
        Positive decimal rate bump. Omission or Ellipsis retains engine settings.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump. Omission or Ellipsis retains the engine's setting;
        None selects an automatic bump up to one day
        shortened to avoid observations. Use smaller durations to check
        convergence for intraday valuations and between observations.

    Returns
    -------
    PricingResult
        Numerical sensitivities with request and availability metadata. An
        unavailable numerical stencil has status "stencil_unavailable";
        undefined derivatives retain "undefined". Units match native
        price_with_greeks.

    Raises
    ------
    KiyosiError
        If settings are invalid, or model, contract or bumped pricing fails.
    ValueError
        If a Greek name is unknown.
    TypeError
        If greeks is not a string or iterable of strings.
    OSError
        If system entropy or worker creation fails. Monte Carlo draws are
        pinned within the request, matching the Rust numerical_greeks
        operation.
    """

@overload
def numerical_greeks(
    engine: QuadratureAutocallableEngine
    | FiniteDifferenceAutocallableEngine
    | MonteCarloAutocallableEngine,
    instrument: SnowballOption
    | BinarySnowballOption
    | TernarySnowballOption
    | PhoenixOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticRangeAccrualEngine,
    instrument: RangeAccrualOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticDoubleBarrierEngine
    | FiniteDifferenceDoubleBarrierEngine
    | MonteCarloDoubleBarrierEngine,
    instrument: DoubleBarrierOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticDoubleTouchEngine | FiniteDifferenceDoubleTouchEngine,
    instrument: DoubleTouchOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticBarrierEngine
    | CoxRossRubinsteinBarrierEngine
    | MonteCarloBarrierEngine
    | QuadratureBarrierEngine
    | FiniteDifferenceBarrierEngine,
    instrument: BarrierOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticGeometricAveragePriceEngine | MonteCarloAsianEngine,
    instrument: GeometricAveragePriceOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: TurnbullWakemanArithmeticAveragePriceEngine | MonteCarloAsianEngine,
    instrument: ArithmeticAveragePriceOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticEuropeanEngine | MonteCarloVanillaEngine,
    instrument: EuropeanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticDigitalEngine
    | QuadratureDigitalEngine
    | MonteCarloDigitalEngine
    | CoxRossRubinsteinDigitalEngine
    | FiniteDifferenceDigitalEngine,
    instrument: CashOrNothingOption | AssetOrNothingOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: BjerksundStenslandVanillaEngine | MonteCarloVanillaEngine,
    instrument: AmericanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: CoxRossRubinsteinVanillaEngine
    | FiniteDifferenceVanillaEngine
    | QuadratureVanillaEngine,
    instrument: EuropeanOption | AmericanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticBinaryBarrierEngine | FiniteDifferenceBinaryBarrierEngine,
    instrument: BinaryBarrierOption | TouchOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None | EllipsisType = ...,
    volatility_shift: _Real | None | EllipsisType = ...,
    rate_shift: _Real | EllipsisType = ...,
    time_shift: timedelta | None | EllipsisType = ...,
) -> PricingResult: ...

class GeometricAveragePriceOption:
    """Continuously monitored average-price Asian option.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    effective_time : datetime.datetime
        Inclusive contract start.
    averaging_start_time : datetime.datetime
        First averaging timestamp, within the contract life.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    realized_average : float or None, optional
        Positive finite prior average; defaults to None (unknown). Must be None
        at or before averaging starts, and supplied strictly after it; time
        consistency is checked at pricing. History covers averaging strictly
        before valuation; callers supply the applicable average.

    Raises
    ------
    InvalidInputError
        If strike, realized_average, life times or the averaging window are
        invalid.
    """

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: _Real | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        averaging_start_time: datetime,
        expiry_time: datetime,
        realized_average: _Real | None = None,
    ) -> None: ...
    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def averaging_start_time(self) -> datetime:
        """Return first averaging timestamp, within the contract life."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def realized_average(self) -> float | None:
        """Return the supplied historical average, or None if unknown."""

class AnalyticGeometricAveragePriceEngine(_PricingMethods[GeometricAveragePriceOption]):
    """Prices continuously monitored geometric average-price Asian options.

    Supports deterministic risk-free-rate and dividend-yield curves.

    See Also
    --------
    GeometricAveragePriceOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class ArithmeticAveragePriceOption:
    """Continuously monitored average-price Asian option.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    effective_time : datetime.datetime
        Inclusive contract start.
    averaging_start_time : datetime.datetime
        First averaging timestamp, within the contract life.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    realized_average : float or None, optional
        Positive finite prior average; defaults to None (unknown). Must be None
        at or before averaging starts, and supplied strictly after it; time
        consistency is checked at pricing. History covers averaging strictly
        before valuation; callers supply the applicable average.

    Raises
    ------
    InvalidInputError
        If strike, realized_average, life times or the averaging window are
        invalid.
    """

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: _Real | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        averaging_start_time: datetime,
        expiry_time: datetime,
        realized_average: _Real | None = None,
    ) -> None: ...
    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def averaging_start_time(self) -> datetime:
        """Return first averaging timestamp, within the contract life."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def realized_average(self) -> float | None:
        """Return the supplied historical average, or None if unknown."""

class TurnbullWakemanArithmeticAveragePriceEngine(
    _PricingMethods[ArithmeticAveragePriceOption]
):
    """Continuous arithmetic average-price Asian approximation.

    Requires flat rates before expiry. Uses the Turnbull-Wakeman
    (Levy-compatible) lognormal moment-matching approximation.

    See Also
    --------
    ArithmeticAveragePriceOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class BarrierOption:
    """Single-barrier vanilla option with inclusive barrier touches.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_level : float
        Positive finite trigger level; equality counts as a touch.
    barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        Trigger direction and knock-in or knock-out behavior.
    rebate : float, optional
        Nonnegative finite cash rebate; defaults to zero.
    rebate_timing : {'at_hit', 'at_expiry'}, optional
        Payment timing; at-hit rebates are valid only for knock-outs.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None means unknown; untouched means known no
        hit.
    calendar : TradingCalendar, optional
        Explicit override. Omission or Ellipsis inherits a supplied
        ObservationSchedule calendar; otherwise construction uses weekdays
        and replacement retains the current calendar.

    Raises
    ------
    InvalidInputError
        If levels, rebate, life times, monitoring schedule, calendar or rebate
        timing are invalid. Trading-day compatibility is checked on
        construction; valuation-history checks occur at pricing.
    """

    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        barrier_level: _Real | EllipsisType = ...,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
        | EllipsisType = ...,
        rebate: _Real | EllipsisType = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | None | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike : float, optional
            Positive finite strike price.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        barrier_level : float, optional
            Positive finite trigger level; equality counts as a touch.
        barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
            Barrier direction and knock-in or knock-out behavior.
        rebate : float, optional
            Nonnegative finite cash rebate.
        rebate_timing : {'at_hit', 'at_expiry'}, optional
            Rebate payment timing; at-hit rebates require knock-out.
        observation_times : iterable of datetime.datetime or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior touch history; None clears it to unknown.
        calendar : TradingCalendar, optional
            Explicit override. Omission or Ellipsis inherits a supplied
            ObservationSchedule calendar; otherwise construction uses weekdays
            and replacement retains the current calendar.

        Returns
        -------
        BarrierOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: _Real,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        rebate: _Real = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_times: Iterable[datetime] | None = None,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def effective_time(self) -> datetime:
        """Return the inclusive contract start."""

    @property
    def expiry_time(self) -> datetime:
        """Return the inclusive expiry time."""

    @property
    def barrier_level(self) -> float:
        """Return positive finite trigger level; equality counts as a touch."""

    @property
    def barrier_type(
        self,
    ) -> Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]:
        """Return trigger direction and knock-in or knock-out behavior."""

    @property
    def rebate(self) -> float:
        """Return nonnegative finite cash rebate; defaults to zero."""

    @property
    def rebate_timing(self) -> Literal["at_hit", "at_expiry"]:
        """Return payment timing; at-hit rebates are valid only for knock-outs."""

    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]:
        """Return continuous monitoring or observations on the supplied times."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

    @property
    def observation_schedule(self) -> ObservationSchedule | None:
        """Returns an owned schedule, or None for continuous monitoring."""

    @property
    def mean_observation_year_fraction(self) -> float:
        """Return mean fixing spacing in Actual/365 Fixed years."""

    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None:
        """Return strictly prior touch history, or None if unknown."""

class AnalyticBarrierEngine(_PricingMethods[BarrierOption]):
    """Analytic single-barrier pricing with flat rates.

    Scheduled monitoring uses the Broadie-Glasserman-Kou (BGK) continuity
    correction.

    See Also
    --------
    BarrierOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class DoubleBarrierOption:
    """Vanilla option activated or terminated by either inclusive boundary.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite strike price.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    lower_barrier : float
        Positive finite lower trigger; equality counts as a touch.
    upper_barrier : float
        Finite upper trigger, strictly above the lower trigger.
    barrier_type : {'knock_in', 'knock_out'}
        Knock-in or knock-out activation by either barrier.
    rebate : float, optional
        Nonnegative finite cash rebate; defaults to zero.
    rebate_timing : {'at_hit', 'at_expiry'}, optional
        Payment timing; at-hit rebates are valid only for knock-outs.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None means unknown; untouched means known no
        hit.
    calendar : TradingCalendar, optional
        Explicit override. Omission or Ellipsis inherits a supplied
        ObservationSchedule calendar; otherwise construction uses weekdays
        and replacement retains the current calendar.

    Raises
    ------
    InvalidInputError
        If boundaries, strike, rebate, life times, monitoring schedule or
        rebate timing are invalid. Calendar compatibility is checked on
        construction; history is checked during pricing.
    """

    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        lower_barrier: _Real | EllipsisType = ...,
        upper_barrier: _Real | EllipsisType = ...,
        barrier_type: Literal["knock_in", "knock_out"] | EllipsisType = ...,
        rebate: _Real | EllipsisType = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | None | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike : float, optional
            Positive finite strike price.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        lower_barrier, upper_barrier : float, optional
            Positive finite lower and upper triggers, with the lower strictly smaller.
        barrier_type : {'knock_in', 'knock_out'}, optional
            Knock-in or knock-out activation at either boundary.
        rebate : float, optional
            Nonnegative finite cash rebate.
        rebate_timing : {'at_hit', 'at_expiry'}, optional
            Rebate payment timing; at-hit rebates require knock-out.
        observation_times : iterable of datetime.datetime or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior touch history; None clears it to unknown.
        calendar : TradingCalendar, optional
            Explicit override. Omission or Ellipsis inherits a supplied
            ObservationSchedule calendar; otherwise construction uses weekdays
            and replacement retains the current calendar.

        Returns
        -------
        DoubleBarrierOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        lower_barrier: _Real,
        upper_barrier: _Real,
        barrier_type: Literal["knock_in", "knock_out"],
        rebate: _Real = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_times: Iterable[datetime] | None = None,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

    @property
    def effective_time(self) -> datetime:
        """Return the inclusive contract start."""

    @property
    def expiry_time(self) -> datetime:
        """Return the inclusive expiry time."""

    @property
    def lower_barrier(self) -> float:
        """Return positive finite lower trigger; equality counts as a touch."""

    @property
    def upper_barrier(self) -> float:
        """Return finite upper trigger, strictly above the lower trigger."""

    @property
    def barrier_type(self) -> Literal["knock_in", "knock_out"]:
        """Return knock-in or knock-out activation by either barrier."""

    @property
    def rebate(self) -> float:
        """Return nonnegative finite cash rebate; defaults to zero."""

    @property
    def rebate_timing(self) -> Literal["at_hit", "at_expiry"]:
        """Return payment timing; at-hit rebates are valid only for knock-outs."""

    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]:
        """Return continuous monitoring or observations on the supplied times."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

    @property
    def observation_schedule(self) -> ObservationSchedule | None:
        """Returns an owned schedule, or None for continuous monitoring."""

    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None:
        """Return strictly prior touch history, or None if unknown."""

class RangeAccrualOption:
    """Range coupon paid at expiry without principal repayment.

    Parameters
    ----------
    notional : float
        Positive finite coupon notional, without principal repayment.
    annual_coupon_rate : float
        Nonnegative finite annual coupon rate in decimal units.
    lower_bound : float
        Inclusive nonnegative lower spot boundary.
    upper_bound : float
        Inclusive finite upper spot boundary, above the lower boundary.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time
        Required daily observation time, without timezone information.
    realized_hit_count : int or None, optional
        Hits strictly before valuation, or None for unknown history.
        Cannot exceed the number of observations strictly before expiry.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.

    Raises
    ------
    InvalidInputError
        If amounts, bounds or history are invalid, or expiry is not after
        effective_time, or the observation schedule is empty. Pricing checks
        the inclusive contract life and history against elapsed observations.

    Notes
    -----
    Observe trading times in (effective_time, expiry_time]. Bounds are
    inclusive. Settlement may fall on a closed date without adding an
    observation. Payment is notional times annual coupon times Actual/365 Fixed
    life times the fraction of observations inside the range. Payment is at
    expiry; a completed observation schedule still has a discounted future
    payment.
    """

    @property
    def observation_schedule(self) -> ObservationSchedule:
        """Return the daily range observation schedule.

        Observations exclude the effective instant.

        Applies the contract calendar and clock through expiry, independently of
        supplied history. Payment adds no observation.
        """

    @property
    def observation_clock_time(self) -> time:
        """Return the explicitly specified daily observation clock time."""

    def replace(
        self,
        *,
        notional: _Real | EllipsisType = ...,
        annual_coupon_rate: _Real | EllipsisType = ...,
        lower_bound: _Real | EllipsisType = ...,
        upper_bound: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observation_clock_time: time | EllipsisType = ...,
        realized_hit_count: SupportsIndex | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    def __init__(
        self,
        *,
        notional: _Real,
        annual_coupon_rate: _Real,
        lower_bound: _Real,
        upper_bound: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        observation_clock_time: time,
        realized_hit_count: SupportsIndex | None = ...,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    @property
    def notional(self) -> float:
        """Return positive finite coupon notional, without principal repayment."""

    @property
    def annual_coupon_rate(self) -> float:
        """Return nonnegative finite annual coupon rate in decimal units."""

    @property
    def lower_bound(self) -> float:
        """Return inclusive nonnegative lower spot boundary."""

    @property
    def upper_bound(self) -> float:
        """Return inclusive finite upper spot boundary, above the lower boundary."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def realized_hit_count(self) -> int | None:
        """Return hits strictly before valuation, or None for unknown history."""

    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

class AnalyticRangeAccrualEngine(_PricingMethods[RangeAccrualOption]):
    """Exact BSM range-coupon marginals with deterministic rate curves.

    See Also
    --------
    RangeAccrualOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class AnalyticDoubleBarrierEngine(_PricingMethods[DoubleBarrierOption]):
    """Flat-rate continuous double-barrier analytics.

    Zero-volatility BSM paths are evaluated exactly, including scheduled
    monitoring.

    See Also
    --------
    DoubleBarrierOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class AnalyticDoubleTouchEngine(_PricingMethods[DoubleTouchOption]):
    """Flat-rate continuous cash double-touch and double-no-touch pricing.

    Either inclusive barrier triggers a touch. Supports expiry payments and
    double-touch at-hit payments; past at-hit payments have zero remaining
    value. Uses image/spectral series and adaptive first-exit integration for
    at-hit payments. Nonconstant rate curves are rejected. At positive
    volatility, unresolved scheduled monitoring is unsupported. Resolved cash
    flows and zero-volatility paths support scheduled monitoring after calendar
    and history validation. Greeks use numerical differences; expiry and
    current barrier equality without a prior touch omit Greeks. Stencils
    crossing a barrier omit affected measures.

    See Also
    --------
    DoubleTouchOption : Supported cash contract.
    FiniteDifferenceDoubleTouchEngine : Supports schedules and nonconstant rate curves.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class BinaryBarrierOption:
    """Cash-or-nothing or asset-or-nothing option with barrier monitoring.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Terminal payoff direction; strike equality pays zero.
    strike : float
        Positive finite terminal exercise price.
    effective_time, expiry_time : datetime.datetime
        Inclusive contract life.
    barrier_level : float
        Positive finite trigger level.
    barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        Trigger direction and knock-in or knock-out behavior.
    payoff_type : {'cash', 'asset'}
        Explicit settlement choice.
    payout : float or None, optional
        Positive cash amount required for cash settlement; absent for asset
        settlement.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior touch history; defaults to unknown.
    calendar : TradingCalendar, optional
        Explicit override. Omission or Ellipsis inherits a supplied
        ObservationSchedule calendar; otherwise construction uses weekdays
        and replacement retains the current calendar.

    Raises
    ------
    InvalidInputError
        If numeric terms, life, schedule or calendar compatibility are invalid.

    Notes
    -----
    Barrier equality touches; terminal strike equality pays zero.
    """

    @overload
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: _Real,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        payoff_type: Literal["cash"],
        payout: _Real,
        observation_times: Iterable[datetime] | None = None,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @overload
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: _Real,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        payoff_type: Literal["asset"],
        payout: None = None,
        observation_times: Iterable[datetime] | None = None,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        barrier_level: _Real | EllipsisType = ...,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
        | EllipsisType = ...,
        observation_times: Iterable[datetime] | None | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payoff_type: Literal["cash", "asset"] | EllipsisType = ...,
        payout: _Real | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike : float, optional
            Positive finite strike price.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        barrier_level : float, optional
            Positive finite trigger level; equality counts as a touch.
        barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
            Barrier direction and knock-in or knock-out behavior.
        observation_times : iterable of datetime.datetime or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior touch history; None clears it to unknown.
        payoff_type : {'cash', 'asset'}, optional
            Explicit settlement choice; omission preserves the current kind.
        payout : float or None, optional
            Cash amount; None is invalid for cash and never changes the payoff
            kind.
        calendar : TradingCalendar, optional
            Explicit override. Omission or Ellipsis inherits a supplied
            ObservationSchedule calendar; otherwise construction uses weekdays
            and replacement retains the current calendar.

        Returns
        -------
        BinaryBarrierOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    @property
    def effective_time(self) -> datetime:
        """Return the inclusive contract start."""

    @property
    def expiry_time(self) -> datetime:
        """Return the inclusive expiry time."""

    @property
    def barrier_level(self) -> float:
        """Return positive finite trigger level; equality counts as a touch."""

    @property
    def barrier_type(
        self,
    ) -> Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]:
        """Return trigger direction and knock-in or knock-out behavior."""

    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]:
        """Return continuous monitoring or observations on the supplied times."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

    @property
    def observation_schedule(self) -> ObservationSchedule | None:
        """Returns an owned schedule, or None for continuous monitoring."""

    @property
    def mean_observation_year_fraction(self) -> float:
        """Return mean fixing spacing in Actual/365 Fixed years."""

    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None:
        """Return strictly prior touch history, or None if unknown."""

    @property
    def payoff_type(self) -> Literal["cash", "asset"]:
        """Return cash or asset settlement as a choice string."""

    @property
    def payout(self) -> float | None:
        """Return the cash payout, or None for asset settlement."""

    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call or put payoff direction."""

    @property
    def strike(self) -> float:
        """Return the strike price in underlying price units."""

class TouchOption:
    """Strike-free cash or asset one-touch or no-touch contract.

    Parameters
    ----------
    effective_time, expiry_time : datetime.datetime
        Inclusive contract life.
    barrier_level : float
        Positive finite trigger level; equality counts as a touch.
    barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        In selects one-touch; out selects no-touch. Up/down selects direction.
    payoff_type : {'cash', 'asset'}
        Explicit settlement choice.
    payout : float or None, optional
        Positive cash amount required for cash settlement; absent for asset
        settlement.
    settlement_timing : {'at_hit', 'at_expiry'}, optional
        Defaults to expiry; at-hit settlement requires a one-touch.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior touch history; defaults to unknown.
    calendar : TradingCalendar, optional
        Explicit override. Omission or Ellipsis inherits a supplied
        ObservationSchedule calendar; otherwise construction uses weekdays
        and replacement retains the current calendar.

    Raises
    ------
    InvalidInputError
        If numeric terms, life, monitoring or calendar terms are invalid, or
        at-hit settlement is requested for a no-touch.

    Notes
    -----
    Prior at-hit payments are settled. A current asset hit transfers spot;
    a future continuous asset hit transfers the barrier amount.
    """

    @overload
    def __init__(
        self,
        *,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: _Real,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        payoff_type: Literal["cash"],
        payout: _Real,
        settlement_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_times: Iterable[datetime] | None = None,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @overload
    def __init__(
        self,
        *,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: _Real,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        payoff_type: Literal["asset"],
        payout: None = None,
        settlement_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_times: Iterable[datetime] | None = None,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

    def replace(
        self,
        *,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        barrier_level: _Real | EllipsisType = ...,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
        | EllipsisType = ...,
        settlement_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | None | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payoff_type: Literal["cash", "asset"] | EllipsisType = ...,
        payout: _Real | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        barrier_level : float, optional
            Positive finite trigger level; equality counts as a touch.
        barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
            Barrier direction and knock-in or knock-out behavior.
        settlement_timing : {'at_hit', 'at_expiry'}, optional
            Payment timing; at-hit settlement requires a one-touch contract.
        observation_times : iterable of datetime.datetime or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior touch history; None clears it to unknown.
        payoff_type : {'cash', 'asset'}, optional
            Explicit settlement choice; omission preserves the current kind.
        payout : float or None, optional
            Cash amount; None is invalid for cash and never changes the payoff
            kind.
        calendar : TradingCalendar, optional
            Explicit override. Omission or Ellipsis inherits a supplied
            ObservationSchedule calendar; otherwise construction uses weekdays
            and replacement retains the current calendar.

        Returns
        -------
        TouchOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    @property
    def effective_time(self) -> datetime:
        """Return the inclusive contract start."""

    @property
    def expiry_time(self) -> datetime:
        """Return the inclusive expiry time."""

    @property
    def barrier_level(self) -> float:
        """Return positive finite trigger level; equality counts as a touch."""

    @property
    def barrier_type(
        self,
    ) -> Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]:
        """Return trigger direction and knock-in or knock-out behavior."""

    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]:
        """Return continuous monitoring or observations on the supplied times."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

    @property
    def observation_schedule(self) -> ObservationSchedule | None:
        """Returns an owned schedule, or None for continuous monitoring."""

    @property
    def mean_observation_year_fraction(self) -> float:
        """Return mean fixing spacing in Actual/365 Fixed years."""

    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None:
        """Return strictly prior touch history, or None if unknown."""

    @property
    def payoff_type(self) -> Literal["cash", "asset"]:
        """Return cash or asset settlement as a choice string."""

    @property
    def payout(self) -> float | None:
        """Return the cash payout, or None for asset settlement."""

    @property
    def is_one_touch(self) -> bool:
        """Return whether touching activates payment."""

    @property
    def is_up(self) -> bool:
        """Return whether the barrier triggers upward."""

    @property
    def settlement_timing(self) -> Literal["at_hit", "at_expiry"]:
        """Return at-hit or expiry payment; no-touch contracts settle only at expiry."""

class AnalyticBinaryBarrierEngine(_PricingMethods[BinaryBarrierOption | TouchOption]):
    """Flat-rate binary-barrier and single-touch analytics.

    See Also
    --------
    BinaryBarrierOption : Supported contract.
    TouchOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

class DoubleTouchOption:
    """Cash double one-touch or double no-touch option with inclusive barriers.

    Parameters
    ----------
    effective_time, expiry_time : datetime.datetime
        Inclusive contract life.
    lower_barrier, upper_barrier : float
        Positive finite boundaries, with lower strictly below upper.
    barrier_type : {'knock_in', 'knock_out'}
        Knock-in selects double one-touch; knock-out selects double no-touch.
    payout : float
        Positive finite cash payment.
    settlement_timing : {'at_hit', 'at_expiry'}, optional
        Defaults to expiry; at-hit settlement requires double one-touch.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior touch history; defaults to unknown.
    calendar : TradingCalendar, optional
        Explicit override. Omission or Ellipsis inherits a supplied
        ObservationSchedule calendar; otherwise construction uses weekdays
        and replacement retains the current calendar.

    Raises
    ------
    InvalidInputError
        If numeric terms, life, monitoring or calendar terms are invalid, or
        at-hit settlement is requested for double no-touch.

    Notes
    -----
    Double one-touch pays when either barrier is touched; double no-touch pays
    only if neither barrier is touched during monitoring. Prior at-hit payments
    have no remaining value.
    """

    def __init__(
        self,
        *,
        effective_time: datetime,
        expiry_time: datetime,
        lower_barrier: _Real,
        upper_barrier: _Real,
        barrier_type: Literal["knock_in", "knock_out"],
        payout: _Real,
        settlement_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_times: Iterable[datetime] | None = None,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

    def replace(
        self,
        *,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        lower_barrier: _Real | EllipsisType = ...,
        upper_barrier: _Real | EllipsisType = ...,
        barrier_type: Literal["knock_in", "knock_out"] | EllipsisType = ...,
        settlement_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | None | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payout: _Real | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        lower_barrier, upper_barrier : float, optional
            Positive finite lower and upper triggers, with the lower strictly smaller.
        barrier_type : {'knock_in', 'knock_out'}, optional
            Knock-in or knock-out activation at either boundary.
        settlement_timing : {'at_hit', 'at_expiry'}, optional
            Payment timing; at-hit settlement requires a one-touch contract.
        observation_times : iterable of datetime.datetime or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior touch history; None clears it to unknown.
        payout : float, optional
            Positive finite cash payment.
        calendar : TradingCalendar, optional
            Explicit override. Omission or Ellipsis inherits a supplied
            ObservationSchedule calendar; otherwise construction uses weekdays
            and replacement retains the current calendar.

        Returns
        -------
        DoubleTouchOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    @property
    def effective_time(self) -> datetime:
        """Return the inclusive contract start."""

    @property
    def expiry_time(self) -> datetime:
        """Return the inclusive expiry time."""

    @property
    def lower_barrier(self) -> float:
        """Return positive finite lower trigger; equality counts as a touch."""

    @property
    def upper_barrier(self) -> float:
        """Return finite upper trigger, strictly above the lower trigger."""

    @property
    def payout(self) -> float:
        """Return the positive finite cash payout."""

    @property
    def is_one_touch(self) -> bool:
        """Return whether touching activates payment."""

    @property
    def barrier_type(self) -> Literal["knock_in", "knock_out"]:
        """Return knock_in for one-touch or knock_out for no-touch payment."""

    @property
    def settlement_timing(self) -> Literal["at_hit", "at_expiry"]:
        """Return at-hit or expiry payment; no-touch contracts settle only at expiry."""

    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]:
        """Return continuous monitoring or observations on the supplied times."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

    @property
    def observation_schedule(self) -> ObservationSchedule | None:
        """Returns an owned schedule, or None for continuous monitoring."""

    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None:
        """Return strictly prior touch history, or None if unknown."""

class QuadratureAutocallableEngine(
    _PricingMethods[
        SnowballOption | BinarySnowballOption | TernarySnowballOption | PhoenixOption
    ]
):
    """Flat-rate quadrature pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Base number of spatial intervals, at least 4; defaults to 512. Event
        boundaries may add intervals; Simpson panels round up to even.
    integration_standard_deviations : float, optional
        Positive finite integration extent in standard deviations; defaults to
        8.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 4 or integration_standard_deviations is
        nonpositive or non-finite.

    See Also
    --------
    SnowballOption : Supported contract.
    BinarySnowballOption : Supported contract.
    TernarySnowballOption : Supported contract.
    PhoenixOption : Supported contract.

    Notes
    -----
    Preserves cash and asset payments below the spatial grid, including
    zero-lower-strike losses after large spot declines.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        integration_standard_deviations: _Real | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

class KnockInObservation:
    """Knock-in observation rule with an owned daily clock when applicable.

    Notes
    -----
    Construct with every_trading_day(observation_clock_time) or at_expiry().
    Knock-out and coupon timestamps are supplied independently through
    AutocallableObservation.
    """

    def __eq__(self, other: object) -> bool: ...
    @staticmethod
    def every_trading_day(observation_clock_time: time) -> KnockInObservation:
        """Create daily knock-in observation on trading days.

        Parameters
        ----------
        observation_clock_time : datetime.time
            Timezone-free daily knock-in clock with microsecond precision.

        Returns
        -------
        KnockInObservation
            Immutable daily observation rule.

        Raises
        ------
        TypeError
            If the clock representation is invalid or timezone-aware.
        """

    @staticmethod
    def at_expiry() -> KnockInObservation:
        """Create knock-in observation only at the contract's expiry timestamp.

        Returns
        -------
        KnockInObservation
            Immutable expiry-only observation rule without a daily clock.
        """

    @property
    def kind(self) -> Literal["every_trading_day", "at_expiry"]:
        """Return every_trading_day or at_expiry."""

    @property
    def observation_clock_time(self) -> time | None:
        """Return the owned daily clock, or None for expiry-only observation."""

class AutocallableObservation:
    """Knock-out and coupon observation with validated numeric terms.

    Notes
    -----
    Knock-in monitoring is defined independently by KnockInObservation. Phoenix
    coupon observations remain active during knock-out lockout.

    Construct with snowball(..., coupon_rate=...) or phoenix(...,
    coupon_barrier_level=...). A zero knock-out coupon or unconditional Phoenix
    coupon requires an explicit zero. Observations retain their snowball or
    Phoenix kind, including at zero. Notes reject observations of the other
    kind; replacements preserve the kind. Numeric terms are validated in Rust
    on construction. Note construction also validates observation ordering,
    life bounds and product-specific constraints. Invalid numeric terms raise
    InvalidInputError.
    """

    def replace(
        self,
        *,
        time: datetime | EllipsisType = ...,
        knock_out_level: _Real | EllipsisType = ...,
        coupon_rate: _Real | None | EllipsisType = ...,
        coupon_barrier_level: _Real | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated observation with the supplied fields replaced.

        Omitted fields or Ellipsis retain their values and observation kind.

        Parameters
        ----------
        time : datetime.datetime, optional
            Observation timestamp.
        knock_out_level : float, optional
            Positive finite knock-out threshold.
        coupon_rate : float or None, optional
            Finite annual snowball coupon. None only for Phoenix observations.
        coupon_barrier_level : float or None, optional
            Nonnegative Phoenix threshold. None only for snowball observations.

        Returns
        -------
        AutocallableObservation
            Validated copy; note construction checks schedule constraints.

        Raises
        ------
        InvalidInputError
            If numeric terms are invalid or belong to the other observation kind,
            including zero values.
        TypeError
            If the active coupon field is None or representations are invalid.
        """

    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def kind(self) -> Literal["snowball", "phoenix"]:
        """Return snowball or phoenix, independently of coupon values."""

    @staticmethod
    def snowball(
        time: datetime, knock_out_level: _Real, *, coupon_rate: _Real
    ) -> AutocallableObservation:
        """Create a snowball observation with an annual knock-out coupon.

        Parameters
        ----------
        time : datetime.datetime
            Exact observation timestamp.
        knock_out_level : float
            Positive finite inclusive knock-out threshold.
        coupon_rate : float
            Finite signed annual knock-out coupon; supply zero explicitly for no
            coupon.

        Raises
        ------
        InvalidInputError
            If the level is nonpositive or non-finite, or the coupon is non-finite.
        """

    @staticmethod
    def phoenix(
        time: datetime, knock_out_level: _Real, *, coupon_barrier_level: _Real
    ) -> AutocallableObservation:
        """Create a Phoenix observation with a coupon threshold.

        The note supplies the coupon amount.

        Parameters
        ----------
        time : datetime.datetime
            Exact observation timestamp.
        knock_out_level : float
            Positive finite inclusive knock-out threshold.
        coupon_barrier_level : float
            Nonnegative finite coupon threshold; supply zero explicitly for
            unconditional coupons.

        Raises
        ------
        InvalidInputError
            If the knock-out level is nonpositive or non-finite, or the coupon
            threshold is negative or non-finite.
        """

    @property
    def time(self) -> datetime:
        """Return exact observation time."""

    @property
    def knock_out_level(self) -> float:
        """Return the positive finite knock-out threshold."""

    @property
    def coupon_rate(self) -> float | None:
        """Return the annual snowball coupon rate, or None for Phoenix observations."""

    @property
    def coupon_barrier_level(self) -> float | None:
        """Return the Phoenix coupon threshold, or None for snowball observations."""

@overload
def autocallable_observations(
    autocall_times: Iterable[datetime],
    knock_out_levels: Iterable[_Real],
    *,
    knock_out_coupon_rates: Iterable[_Real],
    coupon_barrier_levels: None = None,
) -> list[AutocallableObservation]:
    """Associates parallel sequences into input observation records.

    Parameters
    ----------
    autocall_times : iterable of datetime.datetime
        Knock-out and coupon observation times; the note checks ordering and life
        bounds.
    knock_out_levels : iterable of float
        Positive inclusive knock-out levels, one per observation timestamp.
    knock_out_coupon_rates : iterable of float or None, optional
        Explicit annual snowball coupons, including zero for no coupon.
    coupon_barrier_levels : iterable of float or None, optional
        Explicit Phoenix coupon thresholds, including zero for unconditional
        coupons.

    Returns
    -------
    list of AutocallableObservation
        Observation records.

    Raises
    ------
    InvalidInputError
        If neither or both coupon choices are supplied, sequence lengths
        differ, or numeric levels or coupons are invalid.

    Notes
    -----
    The note constructor validates ordering, life bounds and product-specific
    constraints. Supply exactly one of knock_out_coupon_rates and
    coupon_barrier_levels.

    Passing an ObservationSchedule copies its timestamps into the records;
    the records carry no calendar. Pass calendar=schedule.calendar to the note
    constructor to preserve its observation calendar. Raw note constructors require
    an explicit calendar, including for daily knock-in monitoring.

    Examples
    --------
    Forward a schedule's custom calendar when constructing a snowball:

    >>> from datetime import datetime, time
    >>> from kiyosi import instruments as ins, market
    >>> start, end = datetime(2025, 1, 2, 15), datetime(2025, 1, 10, 15)
    >>> calendar = market.custom_calendar(
    ...     base=market.weekdays_calendar(),
    ...     closed_dates=[datetime(2025, 1, 6).date()],
    ... )
    >>> schedule = market.ObservationSchedule(
    ...     times=[end], start_time=start, end_time=end, calendar=calendar,
    ... )
    >>> records = ins.autocallable_observations(
    ...     schedule, [110], knock_out_coupon_rates=[0.1],
    ... )
    >>> note = ins.SnowballOption(
    ...     maturity_coupon_rate=0.1, initial_spot=100, knock_in_level=80,
    ...     upper_strike=100, lower_strike=0,
    ...     knock_in_observation=ins.KnockInObservation.every_trading_day(time(15)),
    ...     effective_time=start, expiry_time=end, autocall_observations=records,
    ...     calendar=schedule.calendar,
    ... )
    >>> note.calendar == schedule.calendar
    True
    >>> len(note.knock_in_observation_schedule)
    6
    """

@overload
def autocallable_observations(
    autocall_times: Iterable[datetime],
    knock_out_levels: Iterable[_Real],
    *,
    knock_out_coupon_rates: None = None,
    coupon_barrier_levels: Iterable[_Real],
) -> list[AutocallableObservation]: ...

class SnowballOption:
    """Snowball with knock-out coupons and capped knock-in downside loss.

    Parameters
    ----------
    maturity_coupon_rate : float
        Finite annual maturity coupon rate in decimal units.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    upper_strike : float
        Positive upper terminal settlement strike.
    lower_strike : float
        Nonnegative lower terminal strike, at most the upper strike.
    knock_in_observation : KnockInObservation, optional
        Observe knock-in on every trading day or only at expiry.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    autocall_observations : iterable of AutocallableObservation
        Nonempty, strictly ordered knock-out and coupon observations within life.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar
        Required owned observation calendar. Records carry no calendar; pass
        schedule.calendar when constructing from a schedule. Controls contractual
        observations independently of PricingContext.

    Raises
    ------
    InvalidInputError
        If amounts, strikes, life times or observation terms are invalid.
        Calendar compatibility is checked on construction; prior barrier
        history is checked during pricing.
    """

    @property
    def knock_in_observation_schedule(self) -> ObservationSchedule:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through autocall_observations.
        """

    def replace(
        self,
        *,
        maturity_coupon_rate: _Real | EllipsisType = ...,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation: KnockInObservation | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        autocall_observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        maturity_coupon_rate, initial_spot, knock_in_level : float, optional
            Annual maturity coupon, reference spot, and strict downside barrier.
        upper_strike, lower_strike : float, optional
            Upper and lower strikes defining the downside loss interval.
        knock_in_observation : KnockInObservation, optional
            Daily or expiry-only knock-in monitoring rule.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior event history; None clears it to unknown.
        principal_redemption_ratio : float, optional
            Finite nonnegative principal redemption per unit of original notional.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        autocall_observations : iterable of AutocallableObservation, optional
            Product-specific knock-out and coupon observations, in time order.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.

        Returns
        -------
        SnowballOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def with_coupon(
        self,
        first_observation_coupon: _Real,
        *,
        quote_convention: _pricing.CouponQuoteConvention,
    ) -> Self:
        """Return a note with the quoted first coupon and preserved coupon spreads.

        Parameters
        ----------
        first_observation_coupon : float
            Absolute annual first observation coupon, not an additive shift. All
            observation coupons shift equally to preserve their spreads.
        quote_convention : CouponQuoteConvention
            Whether to shift the maturity coupon or preserve it.

        Returns
        -------
        SnowballOption
            New note of the same type, preserving history and other terms.

        Raises
        ------
        InvalidInputError
            If shifted coupons are non-finite or violate contractual constraints.
        """

    def __init__(
        self,
        *,
        maturity_coupon_rate: _Real,
        initial_spot: _Real,
        knock_in_level: _Real,
        upper_strike: _Real,
        lower_strike: _Real,
        knock_in_observation: KnockInObservation,
        effective_time: datetime,
        expiry_time: datetime,
        autocall_observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar,
    ) -> None: ...
    @property
    def autocall_observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual knock-out and coupon observations.

        Phoenix records include coupon observations during knock-out lockout.
        Knock-in monitoring is available through knock_in_observation_schedule.
        """

    @property
    def autocall_times(self) -> tuple[datetime, ...]:
        """Return a copy of the contractual knock-out and coupon observation times.

        Includes Phoenix coupon times during knock-out lockout; excludes
        independently scheduled knock-in monitoring.
        """

    @property
    def knock_out_levels(self) -> tuple[float, ...]:
        """Return copied knock-out thresholds in observation order."""

    @property
    def knock_out_coupon_rates(self) -> tuple[float, ...]:
        """Return copied annual knock-out coupons in observation order."""

    @property
    def maturity_coupon_rate(self) -> float:
        """Return finite annual maturity coupon rate in decimal units."""

    @property
    def initial_spot(self) -> float:
        """Return positive reference spot used to normalize downside losses."""

    @property
    def knock_in_level(self) -> float:
        """Return positive strict downside threshold."""

    @property
    def upper_strike(self) -> float:
        """Return positive upper terminal settlement strike."""

    @property
    def lower_strike(self) -> float:
        """Return nonnegative lower terminal strike, at most the upper strike."""

    @property
    def principal_redemption_ratio(self) -> float:
        """Return nonnegative normalized principal, independent of coupons and losses."""

    @property
    def knock_in_observation(self) -> KnockInObservation:
        """Return an owned knock-in observation rule."""

    @property
    def barrier_state(self) -> Literal["no_event", "knocked_in", "knocked_out"] | None:
        """Return strictly prior autocallable history, or None if unknown."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's monitoring calendar."""

class PhoenixOption:
    """Autocallable with conditional period coupons and capped downside loss.

    Parameters
    ----------
    coupon_per_period : float
        Finite signed coupon proportion per qualifying observation, independent
        of period length.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    upper_strike : float
        Positive upper terminal settlement strike.
    lower_strike : float
        Nonnegative lower terminal strike, at most the upper strike.
    knock_in_observation : KnockInObservation, optional
        Observe knock-in on every trading day or only at expiry.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    autocall_observations : iterable of AutocallableObservation
        Nonempty, strictly ordered knock-out and coupon observations within life.
    lockout_periods : int, optional
        Initial observations excluded from knock-out; defaults to zero and must
        leave at least one eligible observation. Coupons and knock-in remain active.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar
        Required owned observation calendar. Records carry no calendar; pass
        schedule.calendar when constructing from a schedule. Controls contractual
        observations independently of PricingContext.

    Raises
    ------
    TypeError
        If a required value is missing or a supplied representation is invalid.
    InvalidInputError
        If amounts, strikes, life times or observations are invalid.
        Observation coupon_rate must be zero; coupon_barrier_level is
        nonnegative.

    Notes
    -----
    Periodic coupons pay once per qualifying observation, including at the
    effective timestamp, without time accrual. None is not a coupon value.
    Coupon and knock-out barriers are inclusive; knock-in is strict.
    Past coupons are settled. There is no coupon memory or maturity coupon.
    The initial lockout disables only knock-out; coupons and knock-in remain active.
    """

    @property
    def lockout_periods(self) -> int:
        """Return the number of initial observations excluded from knock-out."""

    def with_coupon(self, *, coupon_per_period: _Real) -> Self:
        """Return a note with the supplied periodic coupon rate.

        History and other terms are preserved.

        Parameters
        ----------
        coupon_per_period : float
            Finite signed payment per observation, as a proportion of initial notional.

        Returns
        -------
        PhoenixOption
            Note using the supplied periodic coupon rate.

        Raises
        ------
        InvalidInputError
            If the coupon is non-finite.
        TypeError
            If the supplied representation is invalid.
        """

    @property
    def knock_in_observation_schedule(self) -> ObservationSchedule:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through autocall_observations.
        """

    def replace(
        self,
        *,
        coupon_per_period: _Real | EllipsisType = ...,
        lockout_periods: SupportsIndex | EllipsisType = ...,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation: KnockInObservation | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        autocall_observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        coupon_per_period : float, optional
            Finite signed periodic coupon rate; omission or Ellipsis retains it.
        lockout_periods : int, optional
            Initial observations excluded from knock-out. Does not regenerate barriers.
        initial_spot, knock_in_level, upper_strike, lower_strike : float, optional
            Reference spot, strict downside barrier, and loss interval strikes.
        knock_in_observation : KnockInObservation, optional
            Daily or expiry-only knock-in monitoring rule.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior event history; None clears it to unknown.
        principal_redemption_ratio : float, optional
            Finite nonnegative principal redemption per unit of original notional.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        autocall_observations : iterable of AutocallableObservation, optional
            Product-specific knock-out and coupon observations, in time order.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.

        Returns
        -------
        PhoenixOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid, including a None coupon.
        """

    def __init__(
        self,
        *,
        coupon_per_period: _Real,
        initial_spot: _Real,
        knock_in_level: _Real,
        upper_strike: _Real,
        lower_strike: _Real,
        knock_in_observation: KnockInObservation,
        effective_time: datetime,
        expiry_time: datetime,
        autocall_observations: Iterable[AutocallableObservation],
        lockout_periods: SupportsIndex = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar,
    ) -> None: ...
    @property
    def autocall_observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual knock-out and coupon observations.

        Phoenix records include coupon observations during knock-out lockout.
        Knock-in monitoring is available through knock_in_observation_schedule.
        """

    @property
    def autocall_times(self) -> tuple[datetime, ...]:
        """Return a copy of the contractual knock-out and coupon observation times.

        Includes Phoenix coupon times during knock-out lockout; excludes
        independently scheduled knock-in monitoring.
        """

    @property
    def knock_out_levels(self) -> tuple[float, ...]:
        """Return copied knock-out thresholds in observation order."""

    @property
    def coupon_barrier_levels(self) -> tuple[float, ...]:
        """Return copied Phoenix coupon thresholds in observation order."""

    @property
    def coupon_per_period(self) -> float:
        """Return the finite signed coupon proportion per qualifying observation."""

    @property
    def initial_spot(self) -> float:
        """Return positive reference spot used to normalize downside losses."""

    @property
    def knock_in_level(self) -> float:
        """Return positive strict downside threshold."""

    @property
    def upper_strike(self) -> float:
        """Return positive upper terminal settlement strike."""

    @property
    def lower_strike(self) -> float:
        """Return nonnegative lower terminal strike, at most the upper strike."""

    @property
    def principal_redemption_ratio(self) -> float:
        """Return nonnegative normalized principal, independent of coupons and losses."""

    @property
    def knock_in_observation(self) -> KnockInObservation:
        """Return an owned knock-in observation rule."""

    @property
    def barrier_state(self) -> Literal["no_event", "knocked_in", "knocked_out"] | None:
        """Return strictly prior autocallable history, or None if unknown."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's monitoring calendar."""

class BinarySnowballOption:
    """Snowball with knock-out coupons and no knock-in or downside loss.

    Parameters
    ----------
    maturity_coupon_rate : float
        Finite annual maturity coupon rate in decimal units.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    autocall_observations : iterable of AutocallableObservation
        Nonempty, strictly ordered knock-out and coupon observations within life.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar
        Required owned observation calendar. Records carry no calendar; pass
        schedule.calendar when constructing from a schedule. Controls contractual
        observations independently of PricingContext.

    Raises
    ------
    InvalidInputError
        If coupons, principal, times or observations are invalid, or
        barrier_state is knocked_in; this contract has no knock-in barrier.
    """

    @property
    def knock_in_observation_schedule(self) -> None:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through autocall_observations.
        """

    def replace(
        self,
        *,
        maturity_coupon_rate: _Real | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        autocall_observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        maturity_coupon_rate : float, optional
            Finite annual maturity coupon rate in decimal units.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior event history; None clears it to unknown.
        principal_redemption_ratio : float, optional
            Finite nonnegative principal redemption per unit of original notional.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        autocall_observations : iterable of AutocallableObservation, optional
            Product-specific knock-out and coupon observations, in time order.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.

        Returns
        -------
        BinarySnowballOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def with_coupon(
        self,
        first_observation_coupon: _Real,
        *,
        quote_convention: _pricing.CouponQuoteConvention,
    ) -> Self:
        """Return a note with the quoted first coupon and preserved coupon spreads.

        Parameters
        ----------
        first_observation_coupon : float
            Absolute annual first observation coupon, not an additive shift. All
            observation coupons shift equally to preserve their spreads.
        quote_convention : CouponQuoteConvention
            Whether to shift the maturity coupon or preserve it.

        Returns
        -------
        BinarySnowballOption
            New note of the same type, preserving history and other terms.

        Raises
        ------
        InvalidInputError
            If shifted coupons are non-finite or violate contractual constraints.
        """

    def __init__(
        self,
        *,
        maturity_coupon_rate: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        autocall_observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar,
    ) -> None: ...
    @property
    def autocall_observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual knock-out and coupon observations.

        Phoenix records include coupon observations during knock-out lockout.
        Knock-in monitoring is available through knock_in_observation_schedule.
        """

    @property
    def autocall_times(self) -> tuple[datetime, ...]:
        """Return a copy of the contractual knock-out and coupon observation times.

        Includes Phoenix coupon times during knock-out lockout; excludes
        independently scheduled knock-in monitoring.
        """

    @property
    def knock_out_levels(self) -> tuple[float, ...]:
        """Return copied knock-out thresholds in observation order."""

    @property
    def knock_out_coupon_rates(self) -> tuple[float, ...]:
        """Return copied annual knock-out coupons in observation order."""

    @property
    def maturity_coupon_rate(self) -> float:
        """Return finite annual maturity coupon rate in decimal units."""

    @property
    def principal_redemption_ratio(self) -> float:
        """Return nonnegative normalized principal, independent of coupons and losses."""

    @property
    def barrier_state(self) -> Literal["no_event", "knocked_in", "knocked_out"] | None:
        """Return strictly prior autocallable history, or None if unknown."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's monitoring calendar."""

class TernarySnowballOption:
    """Snowball with distinct intact and knocked-in maturity coupons.

    Parameters
    ----------
    maturity_coupon_rate : float
        Finite annual maturity coupon rate in decimal units.
    minimum_coupon_rate : float
        Knocked-in maturity coupon, at most the intact maturity coupon.
    knock_in_level : float
        Positive strict downside threshold.
    knock_in_observation : KnockInObservation, optional
        Observe knock-in on every trading day or only at expiry.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    autocall_observations : iterable of AutocallableObservation
        Nonempty, strictly ordered knock-out and coupon observations within life.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar
        Required owned observation calendar. Records carry no calendar; pass
        schedule.calendar when constructing from a schedule. Controls contractual
        observations independently of PricingContext.

    Raises
    ------
    InvalidInputError
        If coupons, principal, barriers, times or observations are invalid.
        minimum_coupon_rate must not exceed maturity_coupon_rate.
    """

    @property
    def knock_in_observation_schedule(self) -> ObservationSchedule:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through autocall_observations.
        """

    def replace(
        self,
        *,
        maturity_coupon_rate: _Real | EllipsisType = ...,
        minimum_coupon_rate: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        knock_in_observation: KnockInObservation | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        autocall_observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        maturity_coupon_rate, minimum_coupon_rate, knock_in_level : float, optional
            Annual maturity and minimum coupons, and the strict downside barrier.
        knock_in_observation : KnockInObservation, optional
            Daily or expiry-only knock-in monitoring rule.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior event history; None clears it to unknown.
        principal_redemption_ratio : float, optional
            Finite nonnegative principal redemption per unit of original notional.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        autocall_observations : iterable of AutocallableObservation, optional
            Product-specific knock-out and coupon observations, in time order.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.

        Returns
        -------
        TernarySnowballOption
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def with_coupon(
        self,
        first_observation_coupon: _Real,
        *,
        quote_convention: _pricing.CouponQuoteConvention,
    ) -> Self:
        """Return a note with the quoted first coupon and preserved coupon spreads.

        Parameters
        ----------
        first_observation_coupon : float
            Absolute annual first observation coupon, not an additive shift. All
            observation coupons shift equally to preserve their spreads.
        quote_convention : CouponQuoteConvention
            Whether to shift the maturity coupon or preserve it.

        Returns
        -------
        TernarySnowballOption
            New note of the same type, preserving history and other terms.

        Raises
        ------
        InvalidInputError
            If shifted coupons are non-finite or violate contractual constraints.
        """

    def __init__(
        self,
        *,
        maturity_coupon_rate: _Real,
        minimum_coupon_rate: _Real,
        knock_in_level: _Real,
        knock_in_observation: KnockInObservation,
        effective_time: datetime,
        expiry_time: datetime,
        autocall_observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar,
    ) -> None: ...
    @property
    def autocall_observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual knock-out and coupon observations.

        Phoenix records include coupon observations during knock-out lockout.
        Knock-in monitoring is available through knock_in_observation_schedule.
        """

    @property
    def autocall_times(self) -> tuple[datetime, ...]:
        """Return a copy of the contractual knock-out and coupon observation times.

        Includes Phoenix coupon times during knock-out lockout; excludes
        independently scheduled knock-in monitoring.
        """

    @property
    def knock_out_levels(self) -> tuple[float, ...]:
        """Return copied knock-out thresholds in observation order."""

    @property
    def knock_out_coupon_rates(self) -> tuple[float, ...]:
        """Return copied annual knock-out coupons in observation order."""

    @property
    def maturity_coupon_rate(self) -> float:
        """Return finite annual maturity coupon rate in decimal units."""

    @property
    def knock_in_level(self) -> float:
        """Return positive strict downside threshold."""

    @property
    def minimum_coupon_rate(self) -> float:
        """Return the annual knocked-in maturity coupon."""

    @property
    def principal_redemption_ratio(self) -> float:
        """Return nonnegative normalized principal, independent of coupons and losses."""

    @property
    def knock_in_observation(self) -> KnockInObservation:
        """Return an owned knock-in observation rule."""

    @property
    def barrier_state(self) -> Literal["no_event", "knocked_in", "knocked_out"] | None:
        """Return strictly prior autocallable history, or None if unknown."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's monitoring calendar."""

def standard_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create constant knock-out coupons and levels with daily knock-in monitoring.

    Parameters
    ----------
    coupon_rate : float
        Finite annual coupon rate in decimal units.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def european_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a snowball with knock-in observed only at expiry.

    Parameters
    ----------
    coupon_rate : float
        Finite annual coupon rate in decimal units.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed only at expiry.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def step_down_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    initial_knock_out_level: _Real,
    knock_out_level_decrement: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a snowball with decreasing knock-out levels.

    Parameters
    ----------
    coupon_rate : float
        Finite annual coupon rate in decimal units.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    initial_knock_out_level : float
        Positive knock-out threshold at the first observation.
    knock_out_level_decrement : float
        Absolute price decrement after each observation; may be negative.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day. Decrements apply after each observation; the first uses the initial
    value. Signed coupons and decrements are accepted if all expanded terms
    remain valid.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def both_down_snowball(
    *,
    initial_coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    initial_knock_out_level: _Real,
    coupon_rate_decrement: _Real,
    knock_out_level_decrement: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a snowball with decreasing coupons and knock-out levels.

    Parameters
    ----------
    initial_coupon_rate : float
        Annual coupon rate at the first observation.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    initial_knock_out_level : float
        Positive knock-out threshold at the first observation.
    coupon_rate_decrement : float
        Absolute decimal-rate decrement after each observation.
    knock_out_level_decrement : float
        Absolute price decrement after each observation; may be negative.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day. Decrements apply after each observation; the first uses the initial
    value. Signed coupons and decrements are accepted if all expanded terms
    remain valid. Maturity uses the final observation coupon.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def dual_coupon_snowball(
    *,
    knock_out_coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    maturity_coupon_rate: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a snowball with separate knock-out and maturity coupons.

    Parameters
    ----------
    knock_out_coupon_rate : float
        Annual coupon rate for knock-out settlement.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    maturity_coupon_rate : float
        Finite annual maturity coupon rate in decimal units.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def parachute_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    final_knock_out_level: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a snowball with a distinct final knock-out level.

    Parameters
    ----------
    coupon_rate : float
        Finite annual coupon rate in decimal units.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    final_knock_out_level : float
        Positive threshold replacing only the final knock-out level.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def otm_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    upper_strike: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a snowball with a replaced upper terminal strike.

    Parameters
    ----------
    coupon_rate : float
        Finite annual coupon rate in decimal units.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    upper_strike : float
        Positive upper terminal settlement strike.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def loss_capped_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    lower_strike: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a snowball whose lower terminal strike caps downside loss.

    Parameters
    ----------
    coupon_rate : float
        Finite annual coupon rate in decimal units.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    lower_strike : float
        Nonnegative lower terminal strike, at most the upper strike.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

class Accumulator:
    """Daily purchases or sales settled at scheduled knock-out or expiry.

    Parameters
    ----------
    strike : float
        Positive finite purchase or sale strike.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    daily_quantity : float
        Nonnegative finite quantity bought or sold on each trading timestamp.
    acceleration_factor : float
        Nonnegative multiplier below strike for calls or above strike for puts.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time
        Required daily observation time, without timezone information.
    accumulated_quantity : float or None, optional
        Outstanding quantity strictly before valuation. None means unknown;
        zero means no units. Explicit history is required after any prior
        accumulation or knock-out observation. Pricing rejects positive
        quantity before a strictly prior accumulation unless already settled.
    option_type : {'call', 'put'}, optional
        Call accumulates purchases (the default); put accumulates sales.
    knock_out_observation_times : iterable of datetime.datetime or None, optional
        Strictly increasing, unique times within the inclusive contract life.
        None observes knock-out at the daily accumulation time; an empty
        sequence disables early knock-out. Times need not include either
        contract endpoint.
    knock_out_settled : bool or None, optional
        Prior knock-out settlement history: None (default) is unknown, False
        means active, and True means settled. Required after a strictly prior
        knock-out observation. True requires a strictly prior knock-out; stored
        quantity is ignored and remaining value is zero.
    calendar : TradingCalendar, optional
        Explicit override. Omission or Ellipsis inherits a supplied
        ObservationSchedule calendar; otherwise construction uses weekdays
        and replacement retains the current calendar.

    Raises
    ------
    InvalidInputError
        If strike or knock-out level is invalid, quantities are negative or
        non-finite, expiry precedes effective_time, or explicit times are
        invalid. Trading-timestamp compatibility is checked against the
        calendar on construction.

    Notes
    -----
    Daily accumulations occur at observation_clock_time on trading dates.
    Explicit knock-out observations occur independently at their supplied
    times. On a knock-out observation timestamp, calls knock out at or above
    the barrier and puts at or below it. Knock-out precedes accumulation and
    adds no units. Other trading times accumulate even beyond the barrier. At
    strike, the base quantity applies. Expiry settles after its event, whether
    or not it is a knock-out timestamp. Calls pay quantity * (settlement spot -
    strike); puts pay its negative. Intraday history includes that day's
    earlier accumulation. Record settled contracts with knock_out_settled=True.
    """

    @property
    def accumulation_schedule(self) -> ObservationSchedule:
        """Return an owned schedule of daily accumulation opportunities.

        Includes the effective instant when it matches the contract clock and
        calendar; expiry adds no observation. Coincident knock-out can prevent
        accumulation.
        """

    @property
    def knock_out_observation_schedule(self) -> ObservationSchedule:
        """Return an owned schedule resolving daily or explicit knock-out monitoring.

        An empty explicit list disables knock-out. Supplied history does not filter
        this whole-life schedule.
        """

    @property
    def observation_clock_time(self) -> time:
        """Return the explicitly specified daily observation clock time."""

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        knock_out_level: _Real | EllipsisType = ...,
        daily_quantity: _Real | EllipsisType = ...,
        acceleration_factor: _Real | EllipsisType = ...,
        accumulated_quantity: _Real | None | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observation_clock_time: time | EllipsisType = ...,
        knock_out_observation_times: Iterable[datetime] | None | EllipsisType = ...,
        knock_out_settled: bool | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Omission or Ellipsis retains a field. The original value is unchanged.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike, knock_out_level, daily_quantity : float, optional
            Strike, inclusive knock-out threshold, and daily purchase or sale quantity.
        acceleration_factor : float, optional
            Nonnegative quantity multiplier below strike for calls or above strike for puts.
        accumulated_quantity : float or None, optional
            Quantity strictly before valuation; None clears to unknown.
            Omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Inclusive contract start and expiry timestamps.
        observation_clock_time : datetime.time, optional
            Daily observation clock, without timezone information.
        knock_out_observation_times : iterable of datetime.datetime or None, optional
            Knock-out schedule; None observes at each daily accumulation time.
        knock_out_settled : bool or None, optional
            Records a settled knock-out strictly before valuation; None clears to
            unknown. Omission or Ellipsis retains the current state.
        calendar : TradingCalendar, optional
            Explicit override. Omission or Ellipsis inherits a supplied
            ObservationSchedule calendar; otherwise construction uses weekdays
            and replacement retains the current calendar.

        Returns
        -------
        Accumulator
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def __init__(
        self,
        *,
        strike: _Real,
        knock_out_level: _Real,
        daily_quantity: _Real,
        acceleration_factor: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        observation_clock_time: time,
        accumulated_quantity: _Real | None = None,
        option_type: Literal["call", "put"] = ...,
        knock_out_observation_times: Iterable[datetime] | None = None,
        knock_out_settled: bool | None = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> None: ...
    @property
    def option_type(self) -> Literal["call", "put"]:
        """Return call for purchases or put for sales."""

    @property
    def knock_out_observation_times(self) -> tuple[datetime, ...] | None:
        """Return owned knock-out times, or None for at the daily accumulation time."""

    @property
    def strike(self) -> float:
        """Return the purchase or sale strike in underlying price units."""

    @property
    def knock_out_level(self) -> float:
        """Return positive inclusive knock-out threshold."""

    @property
    def daily_quantity(self) -> float:
        """Return the nonnegative base quantity per trading timestamp."""

    @property
    def acceleration_factor(self) -> float:
        """Return the quantity multiplier below strike for calls or above it for puts."""

    @property
    def accumulated_quantity(self) -> float | None:
        """Return caller-asserted outstanding quantity strictly before valuation."""

    @property
    def knock_out_settled(self) -> bool | None:
        """Return prior knock-out settlement history, or None if unknown."""

    @property
    def effective_time(self) -> datetime:
        """Return inclusive first contract timestamp."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    def __eq__(self, other: object) -> bool: ...
    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

class QuadratureAccumulatorEngine(_PricingMethods[Accumulator]):
    """Flat-rate quadrature pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Base number of spatial intervals, at least 4; defaults to 512. Event
        boundaries may add intervals; Simpson panels round up to even.
    integration_standard_deviations : float, optional
        Positive finite integration extent in standard deviations; defaults to
        8.

    Raises
    ------
    InvalidInputError
        If asset_step_count is below 4 or integration_standard_deviations is
        nonpositive or non-finite.

    See Also
    --------
    Accumulator : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        asset_step_count: SupportsIndex | EllipsisType = ...,
        integration_standard_deviations: _Real | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

def standard_phoenix(
    *,
    coupon_per_period: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    coupon_barrier_level: _Real,
    knock_in_observation_clock_time: time,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    lockout_periods: SupportsIndex = ...,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> PhoenixOption:
    """Create a standard Phoenix with a constant knock-out barrier and optional initial lockout.

    Parameters
    ----------
    coupon_per_period : float
        Finite signed coupon proportion per qualifying observation; not
        annualized.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict knock-in threshold.
    knock_out_level : float
        Positive inclusive threshold at the first eligible knock-out observation.
    lockout_periods : int, optional
        Initial observations excluded from knock-out; defaults to zero and must
        leave at least one eligible observation. Coupons and knock-in remain active.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.

    coupon_barrier_level : float
        Finite nonnegative inclusive coupon threshold, also tested on knock-out times.
    autocall_times : iterable of datetime.datetime
        Strictly increasing observation and payment times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        State strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out, knocked_in means outstanding after knock-in, and
        knocked_out means settled. Current observations use current spot.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupon and loss
        components.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    PhoenixOption
        Validated owned contract with daily knock-in, initial spot as upper strike and zero lower strike.

    Raises
    ------
    InvalidInputError
        If numeric terms, lockout, times or observations are invalid.
    TypeError
        If a supplied representation is invalid, including Boolean or fractional counts.
    OverflowError
        If a count is negative or outside the native unsigned integer range.

    Notes
    -----
    Coupons pay once per qualifying observation, including a qualifying
    knock-out, with no day accrual or memory. Knock-out and coupon barriers are
    inclusive; knock-in is strict. Observation times are payment times; no
    observation is implicitly added at expiry. Calendar compatibility is
    checked on construction; history consistency is checked during pricing.
    Past coupons are settled.
    """

def step_down_phoenix(
    *,
    coupon_per_period: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    initial_knock_out_level: _Real,
    coupon_barrier_level: _Real,
    knock_out_level_decrement: _Real,
    knock_in_observation_clock_time: time,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    lockout_periods: SupportsIndex = ...,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> PhoenixOption:
    """Create a step-down Phoenix whose first eligible knock-out uses the initial level.

    Parameters
    ----------
    coupon_per_period : float
        Finite signed coupon proportion per qualifying observation; not
        annualized.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict knock-in threshold.
    initial_knock_out_level : float
        Positive inclusive threshold at the first eligible knock-out observation.
    lockout_periods : int, optional
        Initial observations excluded from knock-out; defaults to zero and must
        leave at least one eligible observation. Coupons and knock-in remain active.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.

    coupon_barrier_level : float
        Finite nonnegative inclusive coupon threshold, also tested on knock-out times.
    knock_out_level_decrement : float
        Finite nonnegative absolute decrement after each eligible observation; lockout does not consume ladder steps.
    autocall_times : iterable of datetime.datetime
        Strictly increasing observation and payment times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        State strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out, knocked_in means outstanding after knock-in, and
        knocked_out means settled. Current observations use current spot.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupon and loss
        components.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    PhoenixOption
        Validated owned contract with daily knock-in, initial spot as upper strike and zero lower strike.

    Raises
    ------
    InvalidInputError
        If numeric terms, lockout, times or observations are invalid.
    TypeError
        If a supplied representation is invalid, including Boolean or fractional counts.
    OverflowError
        If a count is negative or outside the native unsigned integer range.

    Notes
    -----
    Coupons pay once per qualifying observation, including a qualifying
    knock-out, with no day accrual or memory. Knock-out and coupon barriers are
    inclusive; knock-in is strict. Observation times are payment times; no
    observation is implicitly added at expiry. Calendar compatibility is
    checked on construction; history consistency is checked during pricing.
    Past coupons are settled.
    """

def fixed_coupon_note(
    *,
    coupon_per_period: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    lockout_periods: SupportsIndex = ...,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> PhoenixOption:
    """Create a fixed coupon note with unconditional surviving coupons.

    Parameters
    ----------
    coupon_per_period : float
        Finite signed coupon proportion per qualifying observation; not
        annualized.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict knock-in threshold.
    knock_out_level : float
        Positive inclusive threshold at the first eligible knock-out observation.
    lockout_periods : int, optional
        Initial observations excluded from knock-out; defaults to zero and must
        leave at least one eligible observation. Coupons and knock-in remain active.

    autocall_times : iterable of datetime.datetime
        Strictly increasing observation and payment times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        State strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out, knocked_in means outstanding after knock-in, and
        knocked_out means settled. Current observations use current spot.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupon and loss
        components.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    PhoenixOption
        Validated owned contract with expiry-only knock-in, knock-in level as upper strike and zero lower strike.

    Raises
    ------
    InvalidInputError
        If numeric terms, lockout, times or observations are invalid.
    TypeError
        If a supplied representation is invalid, including Boolean or fractional counts.
    OverflowError
        If a count is negative or outside the native unsigned integer range.

    Notes
    -----
    Coupons pay once per qualifying observation, including a qualifying
    knock-out, with no day accrual or memory. Knock-out and coupon barriers are
    inclusive; knock-in is strict. Observation times are payment times; no
    observation is implicitly added at expiry. Calendar compatibility is
    checked on construction; history consistency is checked during pricing.
    Past coupons are settled.
    """

def digital_coupon_note(
    *,
    coupon_per_period: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    coupon_barrier_level: _Real,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    lockout_periods: SupportsIndex = ...,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> PhoenixOption:
    """Create a digital coupon note with barrier-conditional fixed coupons.

    Parameters
    ----------
    coupon_per_period : float
        Finite signed coupon proportion per qualifying observation; not
        annualized.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict knock-in threshold.
    knock_out_level : float
        Positive inclusive threshold at the first eligible knock-out observation.
    lockout_periods : int, optional
        Initial observations excluded from knock-out; defaults to zero and must
        leave at least one eligible observation. Coupons and knock-in remain active.

    coupon_barrier_level : float
        Finite nonnegative inclusive coupon threshold, also tested on knock-out times.
    autocall_times : iterable of datetime.datetime
        Strictly increasing observation and payment times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        State strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out, knocked_in means outstanding after knock-in, and
        knocked_out means settled. Current observations use current spot.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupon and loss
        components.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    PhoenixOption
        Validated owned contract with expiry-only knock-in, knock-in level as upper strike and zero lower strike.

    Raises
    ------
    InvalidInputError
        If numeric terms, lockout, times or observations are invalid.
    TypeError
        If a supplied representation is invalid, including Boolean or fractional counts.
    OverflowError
        If a count is negative or outside the native unsigned integer range.

    Notes
    -----
    Coupons pay once per qualifying observation, including a qualifying
    knock-out, with no day accrual or memory. Knock-out and coupon barriers are
    inclusive; knock-in is strict. Observation times are payment times; no
    observation is implicitly added at expiry. Calendar compatibility is
    checked on construction; history consistency is checked during pricing.
    Past coupons are settled.
    """

def early_profit_snowball(
    *,
    first_coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    second_coupon_rate: _Real,
    first_stage_end_time: datetime,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a two-stage early-profit snowball.

    Parameters
    ----------
    first_coupon_rate : float
        Finite signed annual coupon rate through first_stage_end_time.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    second_coupon_rate : float
        Finite signed annual coupon rate after the cutoff and at maturity.
    first_stage_end_time : datetime.datetime
        Inclusive first-stage end within life and strictly before expiry.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day. Knock-out uses the observation's stage rate over the whole holding period.
    Maturity uses the final stage rate. Cutoffs need not be observation or trading
    times, stages may have no autocall_observations, and rates need not decrease.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def butterfly_snowball(
    *,
    first_coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    second_coupon_rate: _Real,
    third_coupon_rate: _Real,
    first_stage_end_time: datetime,
    second_stage_end_time: datetime,
    autocall_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    knock_in_observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar | EllipsisType = ...,
) -> SnowballOption:
    """Create a three-stage butterfly snowball.

    Parameters
    ----------
    first_coupon_rate : float
        Finite signed annual coupon rate through first_stage_end_time.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    knock_out_level : float
        Positive inclusive knock-out threshold.
    second_coupon_rate : float
        Finite signed annual coupon in the second stage.
    third_coupon_rate : float
        Finite signed annual coupon in the third stage and at maturity.
    first_stage_end_time : datetime.datetime
        Inclusive first-stage end within life and before second_stage_end_time.
    second_stage_end_time : datetime.datetime
        Inclusive second-stage end strictly before expiry.
    autocall_times : iterable of datetime.datetime
        Strictly increasing knock-out and coupon observation times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    knock_in_observation_clock_time : datetime.time
        Daily knock-in observation clock on trading days.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Events strictly before valuation. None is unknown; no_event means no prior
        knock-in or knock-out; knocked_in means an outstanding knocked-in note;
        knocked_out means settled with no remaining value. The engine handles
        observations at valuation using current spot. Callers supply applicable state.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Explicit calendar override; omission or Ellipsis inherits an
        ObservationSchedule calendar, otherwise weekdays.

    Returns
    -------
    SnowballOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Knock-in is observed on every trading day. Knock-out uses the observation's stage rate over the whole holding period.
    Maturity uses the final stage rate. Cutoffs need not be observation or trading
    times, stages may have no autocall_observations, and rates need not decrease.

    The returned SnowballOption stores observation and maturity coupons separately.
    To edit its first annual observation coupon while preserving coupon spreads,
    use the returned note's quoting operation:

    .. code-block:: python

        edited = note.with_coupon(
            first_observation_coupon=0.15,
            quote_convention="shift_maturity_coupon",
        )

    This convention shifts the maturity coupon by the same amount. Use
    preserve_maturity_coupon to keep maturity fixed. Replacing only
    maturity_coupon_rate edits only the maturity coupon.
    """

def _calendar_parts(
    calendar: TradingCalendar,
) -> tuple[str, TradingCalendar | None, list[datetime], list[datetime], int]: ...
def _pricing_result(
    price: _Real,
    greeks: dict[Greek, _Real],
    *,
    requested: Iterable[Greek] | None = None,
    supported: Iterable[Greek] | None = None,
    unavailable_stencils: Iterable[Greek] | None = None,
    seed: SupportsIndex | None = None,
) -> PricingResult: ...
def _monte_carlo_estimate(
    *,
    price: _Real,
    standard_error: _Real | None,
    uncertainty_status: _pricing.MonteCarloUncertaintyStatus,
    seed: SupportsIndex | None = None,
) -> MonteCarloEstimate: ...
