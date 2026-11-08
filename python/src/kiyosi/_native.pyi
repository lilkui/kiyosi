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

def _repr_previews(value: object) -> dict[str, str]: ...

class KiyosiError(Exception):
    """Base exception for structured domain failures."""

class InvalidInputError(KiyosiError, ValueError):
    """Invalid parameters, dates, schedules or contract history."""

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
        and quote_convention. Apply with instrument.with_quoted_coupon(**quote)
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
    """Solve a Phoenix coupon without changing its annual or per-period units.

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
        Dictionary with exactly one key: coupon_rate for an annual rate or
        coupon_per_period for a fixed proportion per observation. Apply with
        instrument.replace(**quote) to retain the coupon convention.

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
    terminal payoff. Model data must cover valuation through expiry; valuation
    after expiry remains invalid. Other contract families retain their own
    valuation bounds.
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
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike : float, optional
            Positive finite strike price.
        effective_time : datetime.datetime, optional
            Inclusive contract start.
        expiry_time : datetime.datetime, optional
            Inclusive expiry time.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
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
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike : float, optional
            Positive finite strike price.
        effective_time : datetime.datetime, optional
            Inclusive contract start.
        expiry_time : datetime.datetime, optional
            Inclusive expiry time.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
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
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike : float, optional
            Positive finite strike price.

        payout : float, optional
            Positive finite cash payment.
        effective_time : datetime.datetime, optional
            Inclusive contract start.
        expiry_time : datetime.datetime, optional
            Inclusive expiry time.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
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
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Call or put payoff direction.
        strike : float, optional
            Positive finite strike price.

        effective_time : datetime.datetime, optional
            Inclusive contract start.
        expiry_time : datetime.datetime, optional
            Inclusive expiry time.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
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
        """Return a validated replacement.

        Parameters
        ----------
        risk_free_rate : float or RateTermStructure, optional
            Continuously compounded risk-free rate or owned rate curve.
        dividend_yield : float or RateTermStructure, optional
            Continuously compounded dividend yield or owned rate curve.
        volatility : float, optional
            Nonnegative finite annualized log-return volatility in decimal units.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
        """

    @property
    def flat_risk_free_rate(self) -> float:
        """Return the scalar rate for dated or undated constant curves.

        Raises UnsupportedOperationError for nonconstant curves; use
        risk_free_curve to preserve coverage when passing rates to constructors or
        replacements.
        """

    @property
    def flat_dividend_yield(self) -> float:
        """Return the scalar yield for dated or undated constant curves.

        Raises UnsupportedOperationError for nonconstant curves; use dividend_curve
        to preserve coverage when passing yields to constructors or replacements.
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
    options. Requested Greeks are absent; implied-parameter solvers are
    unsupported. Numeric conversion rejects booleans. Replacement validates in
    Rust before returning a new owned model; the original is unchanged on
    failure.
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
        """Return a validated model, retaining omitted fields or Ellipsis.

        Parameters
        ----------
        risk_free_rate, dividend_yield : float or RateTermStructure, optional
            Replacement continuously compounded rates or owned curves.
        initial_variance, long_run_variance, mean_reversion_speed, vol_of_vol : float, optional
            Replacement finite nonnegative variance-process parameters.
        correlation : float, optional
            Replacement finite correlation in [-1, 1].

        Returns
        -------
        HestonParameters
            New owned value; the original is unchanged on success and failure.

        Raises
        ------
        InvalidInputError
            If replacement parameters fail model validation.
        TypeError
            If a supplied value is None, bool or another unsupported
            representation.
        """

    @property
    def flat_risk_free_rate(self) -> float:
        """Return the scalar rate for dated or undated constant curves.

        Raises UnsupportedOperationError for nonconstant curves; use
        risk_free_curve to preserve coverage when passing rates to constructors or
        replacements.
        """

    @property
    def flat_dividend_yield(self) -> float:
        """Return the scalar yield for dated or undated constant curves.

        Raises UnsupportedOperationError for nonconstant curves; use dividend_curve
        to preserve coverage when passing yields to constructors or replacements.
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
        """Return a validated model, retaining omitted fields or Ellipsis.

        Parameters
        ----------
        risk_free_rate, dividend_yield : float or RateTermStructure, optional
            Replacement continuously compounded rates or curves.
        surface : LocalVolatilitySurface, optional
            Replacement owned surface.

        Returns
        -------
        LocalVolatilityParameters
            Independent replacement; the original is unchanged on failure.

        Raises
        ------
        InvalidInputError
            If a replacement scalar rate is non-finite.
        """

    @property
    def surface(self) -> LocalVolatilitySurface:
        """Return an owned copy of the fixed surface."""

    @property
    def flat_risk_free_rate(self) -> float:
        """Return the scalar rate for dated or undated constant curves.

        Raises UnsupportedOperationError for nonconstant curves; use
        risk_free_curve to preserve coverage when passing rates to constructors or
        replacements.
        """

    @property
    def flat_dividend_yield(self) -> float:
        """Return the scalar yield for dated or undated constant curves.

        Raises UnsupportedOperationError for nonconstant curves; use dividend_curve
        to preserve coverage when passing yields to constructors or replacements.
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
        Calendar used to validate each observation date; defaults to weekdays.

    Raises
    ------
    InvalidInputError
        If life bounds are reversed, times are unordered or outside life, or an
        observation date is closed in the calendar.

    Notes
    -----
    Construct from explicit times, or use fixed_interval_schedule or
    monthly_schedule. Bounds and calendar validate the input; the schedule owns
    only its times. Empty schedules are valid. The times property returns a
    copy; iteration retains the schedule owner. Integer indexing accepts
    negative indices, rejects bool, and raises IndexError outside the sequence.
    Slices return owned lists of timestamps, including reversed slices. Slice
    bounds and steps accept integers excluding bool; a zero step raises
    ValueError. len(schedule) counts observations.
    """

    def __init__(
        self,
        *,
        times: Iterable[datetime],
        start_time: datetime,
        end_time: datetime,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    @property
    def times(self) -> tuple[datetime, ...]:
        """Return a copy of the strictly ordered observation times."""

    def __len__(self) -> int: ...
    @overload
    def __getitem__(self, index: SupportsIndex) -> datetime: ...
    @overload
    def __getitem__(self, index: slice) -> list[datetime]: ...
    def __iter__(self) -> Iterator[datetime]: ...

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
    adjustment merges collisions and stops before crossing `end_time`;
    `end_time` need not appear.
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
    stops before crossing `end_time`; `end_time` need not appear.
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
        """Return a validated replacement.

        Parameters
        ----------
        model_parameters : BlackScholesMertonParameters, LocalVolatilityParameters or HestonParameters, optional
            Validated rates and the selected volatility or variance model.
        spot_price : float, optional
            Positive finite underlying spot price.
        valuation_time : datetime.datetime, optional
            Valuation time.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
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

class AnalyticEuropeanEngine:
    """Analytic European vanilla pricing with deterministic rate curves.

    See Also
    --------
    EuropeanOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticEuropeanEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: EuropeanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: EuropeanOption, context: PricingContext
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
        """

    def price(self, instrument: EuropeanOption, context: PricingContext) -> float:
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
        instrument: EuropeanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Uses analytic Greeks at positive volatility before expiry, scalar
        differences at zero volatility, and default terminal stencils at expiry.
        Use numerical_greeks for configurable scalar differences.
        """

class AnalyticHestonEuropeanEngine:
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
    need no evaluations. Requested Greeks are None; implied-parameter solvers
    raise UnsupportedOperationError. Pricing releases the GIL for core
    computation, with independent state for concurrent calls.
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        absolute_tolerance : float, optional
            Finite nonnegative tolerance in price units; defaults to 1e-8.
        relative_tolerance : float, optional
            Finite nonnegative relative tolerance; defaults to 1e-8.
        max_evaluations : int, optional
            Positive request-wide characteristic-function budget; defaults to
            10000.

        Returns
        -------
        AnalyticHestonEuropeanEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self, instrument: EuropeanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: EuropeanOption, context: PricingContext
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
        """

    def price(self, instrument: EuropeanOption, context: PricingContext) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : EuropeanOption
            European call or put within its inclusive contract life.
        context : PricingContext
            Heston market snapshot; initial variance is at valuation.

        Returns
        -------
        float
            Price, or intrinsic payoff at the expiry instant.

        Raises
        ------
        UnsupportedOperationError
            For unsupported instruments or models, including at expiry.
        InvalidInputError
            Outside contract life or curve coverage.
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        All requested Greeks are unsupported and are None. Bump controls are not
        accepted; numerical_greeks also rejects Heston.
        """

class BjerksundStenslandVanillaEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        BjerksundStenslandVanillaEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: AmericanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: AmericanOption, context: PricingContext
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
        """

    def price(self, instrument: AmericanOption, context: PricingContext) -> float:
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
        instrument: AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

class AnalyticDigitalEngine:
    """Analytic European digital pricing with deterministic rate curves.

    See Also
    --------
    CashOrNothingOption : Supported contract.
    AssetOrNothingOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticDigitalEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class CoxRossRubinsteinBarrierEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        step_count : int, optional
            Number of tree time steps, from 1 through 1,000,000; defaults to 256.

        Returns
        -------
        CoxRossRubinsteinBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    @property
    def step_count(self) -> int:
        """Return the configured number of binomial time steps."""

    def greek_method(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class QuadratureBarrierEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Base number of spatial intervals, at least 4; defaults to 512. Event
            boundaries may add intervals; Simpson panels round up to even.

        integration_standard_deviations : float, optional
            Positive finite integration extent in standard deviations; defaults to
            8.

        Returns
        -------
        QuadratureBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

    def greek_method(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

class QuadratureDigitalEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Base number of spatial intervals, at least 4; defaults to 512. Event
            boundaries may add intervals; Simpson panels round up to even.

        integration_standard_deviations : float, optional
            Positive finite integration extent in standard deviations; defaults to
            8.

        Returns
        -------
        QuadratureDigitalEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

    def greek_method(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

class QuadratureVanillaEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        QuadratureVanillaEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

class CoxRossRubinsteinVanillaEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        step_count : int, optional
            Number of tree time steps, from 1 through 1,000,000; defaults to 256.

        Returns
        -------
        CoxRossRubinsteinVanillaEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    @property
    def step_count(self) -> int:
        """Return the configured number of binomial time steps."""

    def greek_method(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class CoxRossRubinsteinDigitalEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        step_count : int, optional
            Number of tree time steps, from 1 through 1,000,000; defaults to 256.

        Returns
        -------
        CoxRossRubinsteinDigitalEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    @property
    def step_count(self) -> int:
        """Return the configured number of binomial time steps."""

    def greek_method(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceAccumulatorEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceAccumulatorEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceAccumulatorEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceAccumulatorEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self, instrument: Accumulator, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: Accumulator, context: PricingContext
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

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
        """

    def price_with_greeks(
        self,
        instrument: Accumulator,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceBarrierEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceBarrierEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceBarrierEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

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
        """

    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceDoubleBarrierEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceDoubleBarrierEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceDoubleBarrierEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceDoubleBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self, instrument: DoubleBarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: DoubleBarrierOption, context: PricingContext
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

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
        """

    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceAutocallableEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceAutocallableEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceAutocallableEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceAutocallableEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceDoubleTouchEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceDoubleTouchEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceDoubleTouchEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceDoubleTouchEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self, instrument: DoubleTouchOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: DoubleTouchOption, context: PricingContext
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price(self, instrument: DoubleTouchOption, context: PricingContext) -> float:
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
        instrument: DoubleTouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceBinaryBarrierEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceBinaryBarrierEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceBinaryBarrierEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceBinaryBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

    def price(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: BinaryBarrierOption | TouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceHestonVanillaEngine:
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
    ComputationFailureError. Requested Greeks are None; requiring an absent
    Greek or using an implied-parameter solver raises
    UnsupportedOperationError. Inputs and settings are immutable; core pricing
    releases the GIL and permits concurrent calls with independent state.
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        FiniteDifferenceHestonVanillaEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float:
        """Return the price.

        Parameters
        ----------
        instrument : EuropeanOption or AmericanOption
            Call or put within its inclusive contract life.
        context : PricingContext
            Heston market snapshot, with initial variance at valuation.

        Returns
        -------
        float
            Price, or intrinsic value at the expiry instant.

        Raises
        ------
        UnsupportedOperationError
            For other models or instruments, including at expiry.
        InvalidInputError
            Outside contract life/curve coverage or for insufficient explicit
            bounds.
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        All requested Greeks are unsupported and are None. Bump controls are not
        accepted; numerical_greeks also rejects Heston.
        """

class FiniteDifferenceVanillaEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceVanillaEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceVanillaEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceVanillaEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

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
        """

    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class FiniteDifferenceDigitalEngine:
    """Finite-difference pricing.

    Parameters
    ----------
    asset_step_count : int, optional
        Number of spatial steps, at least 3; defaults to 800. Continuous
        barrier grids use all steps between their actual domain edges.
    time_step_count : int, optional
        Positive base number of time intervals; event dates and curve knots are
        added.

        Read `FiniteDifferenceDigitalEngine().time_step_count` for the engine's default.
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
    volatility_shift, rate_shift : float, optional
        Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
        spatial delta and gamma retain their estimator.
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
        volatility_shift: _Real = ...,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Number of spatial steps, at least 3; defaults to 800. Continuous
            barrier grids use all steps between their actual domain edges.
        time_step_count : int, optional
            Positive base number of time intervals; event dates and curve knots are
            added.

            Read `FiniteDifferenceDigitalEngine().time_step_count` for the engine's default.
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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical Greeks; defaults to 0.0001. Native
            spatial delta and gamma retain their estimator.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond time bump, or None for automatic selection.

        Returns
        -------
        FiniteDifferenceDigitalEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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
    def volatility_shift(self) -> float:
        """Return the numerical volatility bump in decimal units."""

    @property
    def rate_shift(self) -> float:
        """Return the numerical parallel rate bump in decimal units."""

    @property
    def time_shift(self) -> timedelta | None:
        """Return the explicit time bump, or None for automatic selection."""

    def greek_method(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    @property
    def spot_shift(self) -> float | None:
        """Return the native Greek spot bump, or None for automatic selection."""

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
        """

    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class MonteCarloVanillaEngine:
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
        If paths are outside 1 through 10,000,000, steps outside 1 through
        10,000, max_active_workers is negative, or Sobol paths are not a power
        of two.

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
    measure sampling error. European pricing samples the exact terminal
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        MonteCarloVanillaEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
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
        """

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

        Notes
        -----
        System entropy and worker-creation failures raise OSError.
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class MonteCarloDigitalEngine:
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
    measure sampling error. Only likelihood-ratio delta and gamma are returned;
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        MonteCarloDigitalEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
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
        """

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

        Notes
        -----
        System entropy and worker-creation failures raise OSError.
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class MonteCarloBarrierEngine:
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
    measure sampling error. Monitoring uses continuous survival or the original
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        MonteCarloBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: BarrierOption, context: PricingContext
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
        """

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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class MonteCarloDoubleBarrierEngine:
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
    measure sampling error. Monitoring uses continuous survival or the original
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        MonteCarloDoubleBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self, instrument: DoubleBarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: DoubleBarrierOption, context: PricingContext
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
        """

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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class MonteCarloAutocallableEngine:
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
    measure sampling error. Simulation uses trading days when knock-in matters,
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        MonteCarloAutocallableEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
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
        """

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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class MonteCarloAccumulatorEngine:
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
    measure sampling error. Simulation uses the future trading dates. A current
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

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

        Returns
        -------
        MonteCarloAccumulatorEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self, instrument: Accumulator, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: Accumulator, context: PricingContext
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
        """

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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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

class MonteCarloAsianEngine:
    """Flat-rate CPU Monte Carlo pricing for continuous average-price options.

    Parameters
    ----------
    path_count : int, optional
        Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
        sampling requires a power of two.
    step_count : int, optional
        Grid size, 2 through 10,000; defaults to 50. Validated even when
        unused; see Notes for product-specific use.
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
        If paths are outside 1 through 10,000,000, steps outside 2 through
        10,000, max_active_workers is negative, or Sobol paths are not a power
        of two.

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
    measure sampling error. Geometric averages and single fixings sample exact
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        path_count : int, optional
            Exact path count, 1 through 10,000,000; defaults to 131,072. Sobol
            sampling requires a power of two.
        step_count : int, optional
            Grid size, 2 through 10,000; defaults to 50. Validated even when
            unused; see Notes for product-specific use.
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

        Returns
        -------
        MonteCarloAsianEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
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

    def greek_method(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
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
        """

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

        Notes
        -----
        System entropy and worker-creation failures raise OSError.
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: AnalyticRangeAccrualEngine,
    instrument: RangeAccrualOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: AnalyticDoubleBarrierEngine
    | FiniteDifferenceDoubleBarrierEngine
    | MonteCarloDoubleBarrierEngine,
    instrument: DoubleBarrierOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: AnalyticDoubleTouchEngine | FiniteDifferenceDoubleTouchEngine,
    instrument: DoubleTouchOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: AnalyticGeometricAveragePriceEngine | MonteCarloAsianEngine,
    instrument: GeometricAveragePriceOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: TurnbullWakemanArithmeticAveragePriceEngine | MonteCarloAsianEngine,
    instrument: ArithmeticAveragePriceOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: AnalyticEuropeanEngine | MonteCarloVanillaEngine,
    instrument: EuropeanOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: BjerksundStenslandVanillaEngine | MonteCarloVanillaEngine,
    instrument: AmericanOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: CoxRossRubinsteinVanillaEngine
    | FiniteDifferenceVanillaEngine
    | QuadratureVanillaEngine,
    instrument: EuropeanOption | AmericanOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def implied_volatility(
    engine: AnalyticBinaryBarrierEngine | FiniteDifferenceBinaryBarrierEngine,
    instrument: BinaryBarrierOption | TouchOption,
    context: PricingContext,
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound : float, optional
        Nonnegative finite lower annualized volatility in decimal units;
        defaults to zero.
    upper_bound : float, optional
        Finite upper annualized volatility, above lower_bound; defaults to 4.
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
    curve, not its sampling error. Digital prices can have several volatility
    roots. The supplied bracket selects a root; CRR engines restrict it to
    their admissible tree domain.
    """

@overload
def numerical_greeks(
    engine: QuadratureAccumulatorEngine
    | FiniteDifferenceAccumulatorEngine
    | MonteCarloAccumulatorEngine,
    instrument: Accumulator,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticRangeAccrualEngine,
    instrument: RangeAccrualOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticDoubleBarrierEngine
    | FiniteDifferenceDoubleBarrierEngine
    | MonteCarloDoubleBarrierEngine,
    instrument: DoubleBarrierOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticDoubleTouchEngine | FiniteDifferenceDoubleTouchEngine,
    instrument: DoubleTouchOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticBarrierEngine
    | CoxRossRubinsteinBarrierEngine
    | MonteCarloBarrierEngine
    | QuadratureBarrierEngine
    | FiniteDifferenceBarrierEngine,
    instrument: BarrierOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticGeometricAveragePriceEngine | MonteCarloAsianEngine,
    instrument: GeometricAveragePriceOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: TurnbullWakemanArithmeticAveragePriceEngine | MonteCarloAsianEngine,
    instrument: ArithmeticAveragePriceOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticEuropeanEngine | MonteCarloVanillaEngine,
    instrument: EuropeanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticDigitalEngine
    | QuadratureDigitalEngine
    | MonteCarloDigitalEngine
    | CoxRossRubinsteinDigitalEngine
    | FiniteDifferenceDigitalEngine,
    instrument: CashOrNothingOption | AssetOrNothingOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: BjerksundStenslandVanillaEngine | MonteCarloVanillaEngine,
    instrument: AmericanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: CoxRossRubinsteinVanillaEngine
    | FiniteDifferenceVanillaEngine
    | QuadratureVanillaEngine,
    instrument: EuropeanOption | AmericanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
    engine: AnalyticBinaryBarrierEngine | FiniteDifferenceBinaryBarrierEngine,
    instrument: BinaryBarrierOption | TouchOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift: timedelta | None = None,
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
        Positive absolute spot bump, or None for the engine/product default.
    volatility_shift, rate_shift : float, optional
        Positive decimal-unit bumps. Defaults come from the Rust core.
    time_shift : datetime.timedelta or None, optional
        Positive explicit bump, or None for an automatic bump up to one day
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
        consistency is checked at pricing.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If strike, realized_average, life times or the averaging window are
        invalid.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: _Real | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, averaging_start_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        realized_average : float or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
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
        averaging_start_time: datetime,
        expiry_time: datetime,
        realized_average: _Real | None = None,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        realized_average: _Real | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        realized_average : float or None, optional
            Prior average; None restores unknown history. Pricing checks time
            consistency.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
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
    def averaging_start_time(self) -> datetime:
        """Return first averaging timestamp, within the contract life."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def realized_average(self) -> float | None:
        """Return the supplied historical average, or None if unknown."""

class AnalyticGeometricAveragePriceEngine:
    """Prices continuously monitored geometric average-price Asian options.

    Supports deterministic risk-free-rate and dividend-yield curves.

    See Also
    --------
    GeometricAveragePriceOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticGeometricAveragePriceEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: GeometricAveragePriceOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: GeometricAveragePriceOption, context: PricingContext
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
        """

    def price(
        self, instrument: GeometricAveragePriceOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: GeometricAveragePriceOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
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
        consistency is checked at pricing.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If strike, realized_average, life times or the averaging window are
        invalid.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: _Real | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, averaging_start_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        realized_average : float or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
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
        averaging_start_time: datetime,
        expiry_time: datetime,
        realized_average: _Real | None = None,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        realized_average: _Real | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        realized_average : float or None, optional
            Prior average; None restores unknown history. Pricing checks time
            consistency.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
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
    def averaging_start_time(self) -> datetime:
        """Return first averaging timestamp, within the contract life."""

    @property
    def expiry_time(self) -> datetime:
        """Return inclusive expiry time."""

    @property
    def realized_average(self) -> float | None:
        """Return the supplied historical average, or None if unknown."""

class TurnbullWakemanArithmeticAveragePriceEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        TurnbullWakemanArithmeticAveragePriceEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: ArithmeticAveragePriceOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: ArithmeticAveragePriceOption, context: PricingContext
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
        """

    def price(
        self, instrument: ArithmeticAveragePriceOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: ArithmeticAveragePriceOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
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
        Owned observation calendar; defaults to weekdays on construction and is
        retained on replacement.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If levels, rebate, life times, monitoring schedule, calendar or rebate
        timing are invalid. Trading-day compatibility is checked on
        construction; valuation-history checks occur at pricing.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

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
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_level : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        rebate : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        rebate_timing : {'at_hit', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_times : Iterable[datetime.datetime] or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned observation calendar; defaults to weekdays on construction and is
            retained on replacement.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior history. None means unknown; untouched means known no
            hit.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        InvalidInputError
            If supplied history has no timestamp or history_time falls outside the
            contract life.
        TypeError
            If a field is unknown or touch_state is not a string, None or Ellipsis.
        ValueError
            If touch_state is an unknown choice.
        """

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
    def observation_schedule(self) -> ObservationSchedule:
        """Return an owned copy of the monitoring schedule."""

    @property
    def mean_observation_year_fraction(self) -> float:
        """Return mean fixing spacing in Actual/365 Fixed years."""

    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None:
        """Return strictly prior touch history, or None if unknown."""

class AnalyticBarrierEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: BarrierOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
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
        Owned observation calendar; defaults to weekdays on construction and is
        retained on replacement.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If boundaries, strike, rebate, life times, monitoring schedule or
        rebate timing are invalid. Calendar compatibility is checked on
        construction; history is checked during pricing.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

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
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        lower_barrier, upper_barrier : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_type : {'knock_in', 'knock_out'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        rebate : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        rebate_timing : {'at_hit', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_times : Iterable[datetime.datetime] or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned observation calendar; defaults to weekdays on construction and is
            retained on replacement.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior history. None means unknown; untouched means known no
            hit.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        InvalidInputError
            If supplied history has no timestamp or history_time falls outside the
            contract life.
        TypeError
            If a field is unknown or touch_state is not a string, None or Ellipsis.
        ValueError
            If touch_state is an unknown choice.
        """

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
    def observation_schedule(self) -> ObservationSchedule:
        """Return an owned copy of the monitoring schedule."""

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
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

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
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        notional, annual_coupon_rate, lower_bound, upper_bound : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_clock_time : datetime.time, optional
            Daily observation clock, without timezone information.
        realized_hit_count : int | None, optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
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
        notional: _Real,
        annual_coupon_rate: _Real,
        lower_bound: _Real,
        upper_bound: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        observation_clock_time: time,
        realized_hit_count: SupportsIndex | None = ...,
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        realized_hit_count: SupportsIndex | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        realized_hit_count : int or None, optional
            Hits strictly before valuation, or None for unknown history.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
        """

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

class AnalyticRangeAccrualEngine:
    """Exact BSM range-coupon marginals with deterministic rate curves.

    See Also
    --------
    RangeAccrualOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticRangeAccrualEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: RangeAccrualOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: RangeAccrualOption, context: PricingContext
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
        """

    def price(self, instrument: RangeAccrualOption, context: PricingContext) -> float:
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
        instrument: RangeAccrualOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

class AnalyticDoubleBarrierEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticDoubleBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: DoubleBarrierOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: DoubleBarrierOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

class AnalyticDoubleTouchEngine:
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
    cash_double_touch : Double one-touch constructor.
    cash_double_no_touch : Double no-touch constructor.
    FiniteDifferenceDoubleTouchEngine : Supports schedules and nonconstant rate curves.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticDoubleTouchEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: DoubleTouchOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: DoubleTouchOption, context: PricingContext
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
        """

    def price(self, instrument: DoubleTouchOption, context: PricingContext) -> float:
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
        instrument: DoubleTouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
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
        Owned observation calendar; defaults to weekdays.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If numeric terms, life, schedule or calendar compatibility are invalid.

    Notes
    -----
    The cash_binary_barrier_option and asset_binary_barrier_option functions
    are conveniences. Barrier equality touches; terminal strike equality pays
    zero.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
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
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_level : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_times : Iterable[datetime.datetime] or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        payoff_type : {'cash', 'asset'}, optional
            Explicit settlement choice; omission preserves the current kind.
        payout : float or None, optional
            Cash amount; None is invalid for cash and never changes the payoff
            kind.
        calendar : TradingCalendar, optional
            Owned observation calendar; defaults to weekdays on construction and is
            retained on replacement.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior history. None means unknown; untouched means known no
            hit.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        InvalidInputError
            If supplied history has no timestamp or history_time falls outside the
            contract life.
        TypeError
            If a field is unknown or touch_state is not a string, None or Ellipsis.
        ValueError
            If touch_state is an unknown choice.
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
    def observation_schedule(self) -> ObservationSchedule:
        """Return an owned copy of the monitoring schedule."""

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
        Owned observation calendar; defaults to weekdays.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If numeric terms, life, monitoring or calendar terms are invalid, or
        at-hit settlement is requested for a no-touch.

    Notes
    -----
    The cash_touch_option and asset_touch_option functions are conveniences.
    Prior at-hit payments are settled.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
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
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_level : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        settlement_timing : {'at_hit', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_times : Iterable[datetime.datetime] or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        payoff_type : {'cash', 'asset'}, optional
            Explicit settlement choice; omission preserves the current kind.
        payout : float or None, optional
            Cash amount; None is invalid for cash and never changes the payoff
            kind.
        calendar : TradingCalendar, optional
            Owned observation calendar; defaults to weekdays on construction and is
            retained on replacement.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior history. None means unknown; untouched means known no
            hit.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        InvalidInputError
            If supplied history has no timestamp or history_time falls outside the
            contract life.
        TypeError
            If a field is unknown or touch_state is not a string, None or Ellipsis.
        ValueError
            If touch_state is an unknown choice.
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
    def observation_schedule(self) -> ObservationSchedule:
        """Return an owned copy of the monitoring schedule."""

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

def cash_binary_barrier_option(
    *,
    option_type: Literal["call", "put"],
    strike: _Real,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    payout: _Real,
    observation_times: Iterable[datetime] | None = None,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
) -> BinaryBarrierOption:
    """Create a barrier cash digital with strict terminal exercise.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite exercise or purchase price.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_level : float
        Positive finite trigger level; equality counts as a touch.
    barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        Trigger direction and knock-in or knock-out behavior.
    payout : float
        Positive finite cash payment.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None means unknown; untouched means known no
        hit.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history; required when history is known.

    Returns
    -------
    BinaryBarrierOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Barrier equality counts as a touch. Trading-calendar compatibility is
    checked on construction; strictly prior history is checked during pricing.
    Terminal strike equality pays zero.
    """

def asset_binary_barrier_option(
    *,
    option_type: Literal["call", "put"],
    strike: _Real,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    observation_times: Iterable[datetime] | None = None,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
) -> BinaryBarrierOption:
    """Create a barrier asset digital with strict terminal exercise.

    Parameters
    ----------
    option_type : {'call', 'put'}
        Call or put payoff direction.
    strike : float
        Positive finite exercise or purchase price.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_level : float
        Positive finite trigger level; equality counts as a touch.
    barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        Trigger direction and knock-in or knock-out behavior.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None means unknown; untouched means known no
        hit.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history; required when history is known.

    Returns
    -------
    BinaryBarrierOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Barrier equality counts as a touch. Trading-calendar compatibility is
    checked on construction; strictly prior history is checked during pricing.
    Terminal strike equality pays zero.
    """

def cash_touch_option(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    payout: _Real,
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_times: Iterable[datetime] | None = None,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
) -> TouchOption:
    """Create a cash or asset one-touch or no-touch contract.

    Parameters
    ----------
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_level : float
        Positive finite trigger level; equality counts as a touch.
    barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        In selects one-touch; out selects no-touch. Up/down selects direction.

    payout : float
        Positive finite cash payment.
    settlement_timing : {'at_hit', 'at_expiry'}, optional
        Defaults to expiry. At-hit settlement requires a one-touch contract.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None is unknown; untouched means known no hit.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays on construction and is
        retained on replacement.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history; required when history is known.

    Returns
    -------
    TouchOption
        Validated owned contract. Asset settlement has no cash payout.

    Raises
    ------
    InvalidInputError
        If amounts, life times or observation terms are invalid, or at-hit
        settlement is requested for a no-touch contract.

    Notes
    -----
    Trading-calendar consistency is checked on construction; history is checked
    during pricing. Prior at-hit payments have no remaining value. A current
    asset hit transfers spot; a future continuous asset hit transfers the
    barrier amount. See the kiyosi.instruments module examples for
    construction.
    """

def asset_touch_option(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_times: Iterable[datetime] | None = None,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
) -> TouchOption:
    """Create a cash or asset one-touch or no-touch contract.

    Parameters
    ----------
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_level : float
        Positive finite trigger level; equality counts as a touch.
    barrier_type : {'up_and_in', 'up_and_out', 'down_and_in', 'down_and_out'}
        In selects one-touch; out selects no-touch. Up/down selects direction.

    settlement_timing : {'at_hit', 'at_expiry'}, optional
        Defaults to expiry. At-hit settlement requires a one-touch contract.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None is unknown; untouched means known no hit.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays on construction and is
        retained on replacement.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history; required when history is known.

    Returns
    -------
    TouchOption
        Validated owned contract. Asset settlement has no cash payout.

    Raises
    ------
    InvalidInputError
        If amounts, life times or observation terms are invalid, or at-hit
        settlement is requested for a no-touch contract.

    Notes
    -----
    Trading-calendar consistency is checked on construction; history is checked
    during pricing. Prior at-hit payments have no remaining value. A current
    asset hit transfers spot; a future continuous asset hit transfers the
    barrier amount. See the kiyosi.instruments module examples for
    construction.
    """

class AnalyticBinaryBarrierEngine:
    """Flat-rate binary-barrier and single-touch analytics.

    See Also
    --------
    BinaryBarrierOption : Supported contract.
    TouchOption : Supported contract.
    """

    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def replace(self) -> Self:
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Returns
        -------
        AnalyticBinaryBarrierEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    def greek_method(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
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
        """

    def price(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: BinaryBarrierOption | TouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
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
        Owned observation calendar; defaults to weekdays.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If numeric terms, life, monitoring or calendar terms are invalid, or
        at-hit settlement is requested for double no-touch.

    Notes
    -----
    Double one-touch pays when either barrier is touched; double no-touch pays
    only if neither barrier is touched during monitoring. The cash_double_touch
    and cash_double_no_touch functions are conveniences. Prior at-hit payments
    have no remaining value.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
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
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        lower_barrier, upper_barrier : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_type : {'knock_in', 'knock_out'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        settlement_timing : {'at_hit', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_times : Iterable[datetime.datetime] or None, optional
            Omission retains monitoring; None selects continuous monitoring.
            A nonempty iterable selects scheduled monitoring.
        touch_state : {'untouched', 'touched'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        payout : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned observation calendar; defaults to weekdays on construction and is
            retained on replacement.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        touch_state : {'untouched', 'touched'} or None, optional
            Strictly prior history. None means unknown; untouched means known no
            hit.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Validated copy; omitted fields or Ellipsis retain their values.

        Raises
        ------
        InvalidInputError
            If supplied history has no timestamp or history_time falls outside the
            contract life.
        TypeError
            If a field is unknown or touch_state is not a string, None or Ellipsis.
        ValueError
            If touch_state is an unknown choice.
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
    def observation_schedule(self) -> ObservationSchedule:
        """Return an owned copy of the monitoring schedule."""

    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None:
        """Return strictly prior touch history, or None if unknown."""

def cash_double_touch(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    lower_barrier: _Real,
    upper_barrier: _Real,
    payout: _Real,
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_times: Iterable[datetime] | None = None,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
) -> DoubleTouchOption:
    """Create a cash payment activated by either inclusive boundary.

    Parameters
    ----------
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    lower_barrier : float
        Positive finite lower trigger; equality counts as a touch.
    upper_barrier : float
        Finite upper trigger, strictly above the lower trigger.
    payout : float
        Positive finite cash payment.
    settlement_timing : {'at_hit', 'at_expiry'}, optional
        At-hit or expiry payment; no-touch contracts settle only at expiry.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None means unknown; untouched means known no
        hit.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history; required when history is known.

    Returns
    -------
    DoubleTouchOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Barrier equality counts as a touch. Trading-calendar compatibility is
    checked on construction; strictly prior history is checked during pricing.
    Prior at-hit payments have no remaining value. No-touch payments occur only
    at expiry.
    """

def cash_double_no_touch(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    lower_barrier: _Real,
    upper_barrier: _Real,
    payout: _Real,
    observation_times: Iterable[datetime] | None = None,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
) -> DoubleTouchOption:
    """Create an expiry cash payment requiring neither boundary to be touched.

    Parameters
    ----------
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    lower_barrier : float
        Positive finite lower trigger; equality counts as a touch.
    upper_barrier : float
        Finite upper trigger, strictly above the lower trigger.
    payout : float
        Positive finite cash payment.
    observation_times : iterable of datetime.datetime or None, optional
        Nonempty, strictly increasing times within life; None selects continuous monitoring.
    touch_state : {'untouched', 'touched'} or None, optional
        Strictly prior history. None means unknown; untouched means known no
        hit.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history; required when history is known.

    Returns
    -------
    DoubleTouchOption
        Validated owned contract.

    Raises
    ------
    InvalidInputError
        If contractual amounts, life times or observation terms are invalid.

    Notes
    -----
    Barrier equality counts as a touch. Trading-calendar compatibility is
    checked on construction; strictly prior history is checked during pricing.
    Prior at-hit payments have no remaining value. No-touch payments occur only
    at expiry.
    """

class QuadratureAutocallableEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Base number of spatial intervals, at least 4; defaults to 512. Event
            boundaries may add intervals; Simpson panels round up to even.

        integration_standard_deviations : float, optional
            Positive finite integration extent in standard deviations; defaults to
            8.

        Returns
        -------
        QuadratureAutocallableEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

    def greek_method(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
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
        """

    def active_greek_controls(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
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
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

class AutocallableObservation:
    """Observation with validated numeric terms for an autocallable note.

    Notes
    -----
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
    observation_times: Iterable[datetime],
    knock_out_levels: Iterable[_Real],
    *,
    knock_out_coupon_rates: Iterable[_Real],
    coupon_barrier_levels: None = None,
) -> list[AutocallableObservation]:
    """Associate parallel sequences into input observation records.

    Parameters
    ----------
    observation_times : iterable of datetime.datetime
        Observation times; the note constructor checks ordering and life
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
    """

@overload
def autocallable_observations(
    observation_times: Iterable[datetime],
    knock_out_levels: Iterable[_Real],
    *,
    knock_out_coupon_rates: None = None,
    coupon_barrier_levels: Iterable[_Real],
) -> list[AutocallableObservation]:
    """Associate parallel sequences into input observation records.

    Parameters
    ----------
    observation_times : iterable of datetime.datetime
        Observation times; the note constructor checks ordering and life
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
    """

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
    knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
        Observe knock-in on every trading day or only at expiry.
    observation_clock_time : datetime.time or None
        Required keyword: supply a time for daily monitoring or None for
        expiry-only monitoring.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observations : iterable of AutocallableObservation
        Nonempty, strictly ordered contractual observations within life.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If amounts, strikes, life times or observation terms are invalid.
        Calendar compatibility is checked on construction; prior barrier
        history is checked during pricing.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

    @property
    def knock_in_observation_schedule(self) -> ObservationSchedule:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through observations.
        """

    @property
    def observation_clock_time(self) -> time | None:
        """Return the daily clock, or None for expiry-only monitoring."""

    def replace(
        self,
        *,
        maturity_coupon_rate: _Real | EllipsisType = ...,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        observation_clock_time: time | None | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        maturity_coupon_rate, initial_spot, knock_in_level : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        upper_strike, lower_strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_clock_time : datetime.time or None, optional
            Required for daily monitoring; absent for expiry-only monitoring.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        principal_redemption_ratio : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observations : Iterable[AutocallableObservation], optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def with_quoted_coupon(
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
        object
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
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        observation_clock_time: time | None,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> SnowballOption:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior events. None is unknown; the string no_event means no
            event.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            New instance of the same class; omission or Ellipsis retains history.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
        """

    @property
    def observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual observations."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

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
    def knock_in_observation_mode(self) -> Literal["every_trading_day", "at_expiry"]:
        """Return the knock-in monitoring choice string."""

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
    coupon_rate : float, optional
        Finite signed annual coupon rate. Supply exactly one of coupon_rate and
        coupon_per_period; omission, None or Ellipsis leaves a choice
        unspecified.
    coupon_per_period : float, optional
        Finite signed fixed coupon proportion per qualifying observation,
        without day accrual. The unused coupon property returns None.
    initial_spot : float
        Positive reference spot used to normalize downside losses.
    knock_in_level : float
        Positive strict downside threshold.
    upper_strike : float
        Positive upper terminal settlement strike.
    lower_strike : float
        Nonnegative lower terminal strike, at most the upper strike.
    knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
        Observe knock-in on every trading day or only at expiry.
    observation_clock_time : datetime.time or None
        Required keyword: supply a time for daily monitoring or None for
        expiry-only monitoring.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observations : iterable of AutocallableObservation
        Nonempty, strictly ordered contractual observations within life.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    TypeError
        If both coupon choices or neither are supplied, or a supplied value is
        not real.
    InvalidInputError
        If amounts, strikes, life times or observations are invalid.
        Observation coupon_rate must be zero; coupon_barrier_level is
        nonnegative.

    Notes
    -----
    Annual coupons accrue over contractual observation periods on Actual/365
    Fixed. Fixed per-period coupons pay once per qualifying observation, even
    for an observation on the effective timestamp. Explicit None is not a
    coupon value. Coupon and knock-out barriers are inclusive; knock-in is
    strict. Past coupons are settled. There is no coupon memory or maturity
    coupon.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

    @overload
    def with_quoted_coupon(
        self, *, coupon_rate: _Real, coupon_per_period: None = None
    ) -> Self:
        """Return a note with an annual or per-period coupon.

        Supply exactly one coupon choice. History and other terms are preserved.

        Parameters
        ----------
        coupon_rate : float, optional
            Finite signed annual coupon in decimal units.
        coupon_per_period : float, optional
            Finite signed payment per observation, as a notional proportion.

        Returns
        -------
        PhoenixOption
            Note using the selected coupon units.

        Raises
        ------
        InvalidInputError
            If the coupon is invalid.
        TypeError
            If neither or both choices are supplied, or representations are
            invalid.
        """

    @overload
    def with_quoted_coupon(
        self, *, coupon_rate: None = None, coupon_per_period: _Real
    ) -> Self:
        """Return a note with an annual or per-period coupon.

        Supply exactly one coupon choice. History and other terms are preserved.

        Parameters
        ----------
        coupon_rate : float, optional
            Finite signed annual coupon in decimal units.
        coupon_per_period : float, optional
            Finite signed payment per observation, as a notional proportion.

        Returns
        -------
        PhoenixOption
            Note using the selected coupon units.

        Raises
        ------
        InvalidInputError
            If the coupon is invalid.
        TypeError
            If neither or both choices are supplied, or representations are
            invalid.
        """

    @property
    def knock_in_observation_schedule(self) -> ObservationSchedule:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through observations.
        """

    @property
    def observation_clock_time(self) -> time | None:
        """Return the daily clock, or None for expiry-only monitoring."""

    @overload
    def replace(
        self,
        *,
        coupon_rate: _Real,
        coupon_per_period: None | EllipsisType = ...,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        observation_clock_time: time | None | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        coupon_rate : float, optional
            Absolute annual rate; omission or Ellipsis retains the coupon. None
            requires a concrete coupon_per_period value.
        coupon_per_period : float, optional
            Fixed per-period proportion; omission or Ellipsis retains the coupon.
            None requires a concrete coupon_rate value.
        initial_spot, knock_in_level, upper_strike, lower_strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_clock_time : datetime.time or None, optional
            Required for daily monitoring; absent for expiry-only monitoring.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        principal_redemption_ratio : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observations : Iterable[AutocallableObservation], optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid, both coupon choices have
            values, or an explicit None coupon has no concrete alternative.
        """

    @overload
    def replace(
        self,
        *,
        coupon_rate: None | EllipsisType = ...,
        coupon_per_period: _Real,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        observation_clock_time: time | None | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        coupon_rate : float, optional
            Absolute annual rate; omission or Ellipsis retains the coupon. None
            requires a concrete coupon_per_period value.
        coupon_per_period : float, optional
            Fixed per-period proportion; omission or Ellipsis retains the coupon.
            None requires a concrete coupon_rate value.
        initial_spot, knock_in_level, upper_strike, lower_strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_clock_time : datetime.time or None, optional
            Required for daily monitoring; absent for expiry-only monitoring.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        principal_redemption_ratio : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observations : Iterable[AutocallableObservation], optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid, both coupon choices have
            values, or an explicit None coupon has no concrete alternative.
        """

    @overload
    def replace(
        self,
        *,
        coupon_rate: EllipsisType = ...,
        coupon_per_period: EllipsisType = ...,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        observation_clock_time: time | None | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        coupon_rate : float, optional
            Absolute annual rate; omission or Ellipsis retains the coupon. None
            requires a concrete coupon_per_period value.
        coupon_per_period : float, optional
            Fixed per-period proportion; omission or Ellipsis retains the coupon.
            None requires a concrete coupon_rate value.
        initial_spot, knock_in_level, upper_strike, lower_strike : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_clock_time : datetime.time or None, optional
            Required for daily monitoring; absent for expiry-only monitoring.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        principal_redemption_ratio : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observations : Iterable[AutocallableObservation], optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid, both coupon choices have
            values, or an explicit None coupon has no concrete alternative.
        """

    @overload
    def __init__(
        self,
        *,
        coupon_rate: _Real,
        coupon_per_period: None | EllipsisType = ...,
        initial_spot: _Real,
        knock_in_level: _Real,
        upper_strike: _Real,
        lower_strike: _Real,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        observation_clock_time: time | None,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    @overload
    def __init__(
        self,
        *,
        coupon_rate: None | EllipsisType = ...,
        coupon_per_period: _Real,
        initial_spot: _Real,
        knock_in_level: _Real,
        upper_strike: _Real,
        lower_strike: _Real,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        observation_clock_time: time | None,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> PhoenixOption:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior events. None is unknown; the string no_event means no
            event.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            New instance of the same class; omission or Ellipsis retains history.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
        """

    @property
    def observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual observations."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

    @property
    def knock_out_levels(self) -> tuple[float, ...]:
        """Return copied knock-out thresholds in observation order."""

    @property
    def coupon_barrier_levels(self) -> tuple[float, ...]:
        """Return copied Phoenix coupon thresholds in observation order."""

    @property
    def coupon_rate(self) -> float | None:
        """Return the annual coupon rate, or None for a fixed per-period coupon."""

    @property
    def coupon_per_period(self) -> float | None:
        """Return the fixed per-period coupon, or None for an annual coupon."""

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
    def knock_in_observation_mode(self) -> Literal["every_trading_day", "at_expiry"]:
        """Return the knock-in monitoring choice string."""

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
    observations : iterable of AutocallableObservation
        Nonempty, strictly ordered contractual observations within life.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If coupons, principal, times or observations are invalid, or
        barrier_state is knocked_in; this contract has no knock-in barrier.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

    @property
    def knock_in_observation_schedule(self) -> None:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through observations.
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
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        maturity_coupon_rate : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        principal_redemption_ratio : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observations : Iterable[AutocallableObservation], optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def with_quoted_coupon(
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
        object
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
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> BinarySnowballOption:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior events. None is unknown; the string no_event means no
            event.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            New instance of the same class; omission or Ellipsis retains history.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
        """

    @property
    def observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual observations."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

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
    knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
        Observe knock-in on every trading day or only at expiry.
    observation_clock_time : datetime.time or None
        Required keyword: supply a time for daily monitoring or None for
        expiry-only monitoring.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observations : iterable of AutocallableObservation
        Nonempty, strictly ordered contractual observations within life.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Raises
    ------
    InvalidInputError
        If coupons, principal, barriers, times or observations are invalid.
        minimum_coupon_rate must not exceed maturity_coupon_rate.
    """

    @property
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

    @property
    def knock_in_observation_schedule(self) -> ObservationSchedule:
        """Return an owned whole-life knock-in schedule, or None without knock-in.

        Daily monitoring includes the effective instant at the contractual clock on
        open dates. Expiry-only monitoring returns just expiry. Observations for
        knock-out and coupons remain available through observations.
        """

    @property
    def observation_clock_time(self) -> time | None:
        """Return the daily clock, or None for expiry-only monitoring."""

    def replace(
        self,
        *,
        maturity_coupon_rate: _Real | EllipsisType = ...,
        minimum_coupon_rate: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        observation_clock_time: time | None | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_redemption_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        maturity_coupon_rate, minimum_coupon_rate, knock_in_level : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        knock_in_observation_mode : {'every_trading_day', 'at_expiry'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_clock_time : datetime.time or None, optional
            Required for daily monitoring; absent for expiry-only monitoring.
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        principal_redemption_ratio : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observations : Iterable[AutocallableObservation], optional
            Replacement value; omission or Ellipsis retains the current field.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            Independent contract of the same class, preserving omitted history and
            terms.

        Raises
        ------
        KiyosiError
            If the resulting contract fails core validation.
        TypeError
            If a representation or field name is invalid.
        """

    def with_quoted_coupon(
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
        object
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
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        observation_clock_time: time | None,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_redemption_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> TernarySnowballOption:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
            Strictly prior events. None is unknown; the string no_event means no
            event.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            New instance of the same class; omission or Ellipsis retains history.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
        """

    @property
    def observations(self) -> tuple[AutocallableObservation, ...]:
        """Return owned copies of the contractual observations."""

    @property
    def observation_times(self) -> tuple[datetime, ...] | None:
        """Return a copy of the contractual observation times."""

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
    def knock_in_observation_mode(self) -> Literal["every_trading_day", "at_expiry"]:
        """Return the knock-in monitoring choice string."""

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
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    """

def european_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time | None = None,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    """

def step_down_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    initial_knock_out_level: _Real,
    knock_out_level_decrement: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    """

def both_down_snowball(
    *,
    initial_coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    initial_knock_out_level: _Real,
    coupon_rate_decrement: _Real,
    knock_out_level_decrement: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    """

def dual_coupon_snowball(
    *,
    knock_out_coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    maturity_coupon_rate: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    """

def parachute_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    final_knock_out_level: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    """

def otm_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    upper_strike: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    """

def loss_capped_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    lower_strike: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
        Owned observation calendar; defaults to weekdays, independently of
        PricingContext.
    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    def history_time(self) -> datetime | None:
        """Return the timestamp through which supplied history is known, or None."""

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
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Parameters
        ----------
        option_type : {'call', 'put'}, optional
            Replacement value; omission or Ellipsis retains the current field.
        strike, knock_out_level, daily_quantity : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        acceleration_factor : float, optional
            Replacement value; omission or Ellipsis retains the current field.
        accumulated_quantity : float or None, optional
            Replacement value; omission or Ellipsis retains the current field.
        effective_time, expiry_time : datetime.datetime, optional
            Replacement value; omission or Ellipsis retains the current field.
        observation_clock_time : datetime.time, optional
            Daily observation clock, without timezone information.
        knock_out_observation_times : Iterable[datetime.datetime] | None, optional
            Replacement value; omission or Ellipsis retains the current field.
        knock_out_settled : bool or None, optional
            Records a settled knock-out strictly before valuation.
        calendar : TradingCalendar, optional
            Owned contractual observation calendar; omission retains it.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
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
        calendar: TradingCalendar = ...,
        history_time: datetime | None = None,
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

    def replace_history(
        self,
        *,
        accumulated_quantity: _Real | None | EllipsisType = ...,
        knock_out_settled: bool | None | EllipsisType = ...,
        history_time: datetime | None | EllipsisType = ...,
    ) -> Self:
        """Return a validated replacement.

        Clearing the final history payload clears an omitted history_time. An
        explicitly supplied history_time is validated and retained.

        Parameters
        ----------
        accumulated_quantity : float or None, optional
            Outstanding quantity strictly before valuation. None means unknown;
            zero means no units. Required after a prior observation unless
            knock_out_settled is True.
        knock_out_settled : bool or None, optional
            None restores unknown history; omission retains the current state.
            Pricing rejects knock-out settlement before or at the first knock-out
            observation.
        history_time : datetime.datetime or None, optional
            Timestamp of supplied history, covering events strictly before it.
            Required with history; refresh payload and timestamp together.

        Returns
        -------
        object
            New instance of the same class; omission or Ellipsis retains history.

        Raises
        ------
        KiyosiError
            If replacement terms fail domain validation.
        """

    def __eq__(self, other: object) -> bool: ...
    @property
    def calendar(self) -> TradingCalendar:
        """Return an owned copy of the contract's observation calendar."""

class QuadratureAccumulatorEngine:
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
        """Return a validated copy with replacement controls.

        Omission or Ellipsis retains a control; None clears nullable controls.
        Parameter defaults below describe construction, not replacement.

        Parameters
        ----------
        asset_step_count : int, optional
            Base number of spatial intervals, at least 4; defaults to 512. Event
            boundaries may add intervals; Simpson panels round up to even.

        integration_standard_deviations : float, optional
            Positive finite integration extent in standard deviations; defaults to
            8.

        Returns
        -------
        QuadratureAccumulatorEngine
            Validated engine.

        Raises
        ------
        InvalidInputError
            If controls fail constructor validation.
        """

    @property
    def asset_step_count(self) -> int:
        """Return the configured number of spatial intervals."""

    @property
    def integration_standard_deviations(self) -> float:
        """Return the integration extent in standard deviations."""

    def greek_method(
        self, instrument: Accumulator, context: PricingContext
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
        """

    def active_greek_controls(
        self, instrument: Accumulator, context: PricingContext
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
        """

    def price_with_greeks(
        self,
        instrument: Accumulator,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None | EllipsisType = ...,
        volatility_shift: _Real | EllipsisType = ...,
        rate_shift: _Real | EllipsisType = ...,
        time_shift: timedelta | None | EllipsisType = ...,
    ) -> PricingResult:
        """Return the price and selected sensitivities with native estimators.

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
        volatility_shift, rate_shift : float, optional
            Positive decimal bumps for numerical fallback Greeks.
        time_shift : datetime.timedelta or None, optional
            Positive whole-microsecond bump; None selects an automatic time step.
            Omitted controls or Ellipsis retain engine settings, whose shared
            defaults are automatic spot/time bumps and 0.0001 volatility/rate
            bumps. Controls affect numerical fallbacks, event checks and terminal
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
        Before expiry requested Greeks use scalar numerical differences and the
        engine defaults. Use numerical_greeks for configurable scalar differences.
        """

def fixed_coupon_note(
    *,
    coupon_per_period: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
        Positive strict expiry knock-in threshold and upper settlement strike.
    knock_out_level : float
        Positive inclusive knock-out threshold.

    observation_times : iterable of datetime.datetime
        Strictly increasing observation and payment times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events; None means unknown history.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupon and loss
        components.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Returns
    -------
    PhoenixOption
        Validated owned contract, with expiry-only knock-in and zero lower
        strike.

    Raises
    ------
    InvalidInputError
        If numeric terms, times or observations are invalid.

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
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
        Positive strict expiry knock-in threshold and upper settlement strike.
    knock_out_level : float
        Positive inclusive knock-out threshold.

    coupon_barrier_level : float
        Finite nonnegative inclusive coupon threshold, also tested on knock-out times.
    observation_times : iterable of datetime.datetime
        Strictly increasing observation and payment times within life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events; None means unknown history.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupon and loss
        components.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

    Returns
    -------
    PhoenixOption
        Validated owned contract, with expiry-only knock-in and zero lower
        strike.

    Raises
    ------
    InvalidInputError
        If numeric terms, times or observations are invalid.

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
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    times, stages may have no observations, and rates need not decrease.
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
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    observation_clock_time: time,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_redemption_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
    history_time: datetime | None = None,
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
    observation_times : iterable of datetime.datetime
        Strictly increasing observation times within the contract life.
    effective_time : datetime.datetime
        Inclusive contract start.
    expiry_time : datetime.datetime
        Inclusive expiry time.
    observation_clock_time : datetime.time or None
        Required for daily monitoring; absent for expiry-only monitoring.
    barrier_state : {'no_event', 'knocked_in', 'knocked_out'} or None, optional
        Strictly prior events. None is unknown; the string no_event means no
        event.
    principal_redemption_ratio : float, optional
        Nonnegative normalized principal, independent of coupons and losses.
    calendar : TradingCalendar, optional
        Owned contractual calendar; defaults to weekdays.

    history_time : datetime.datetime or None, optional
        Timestamp of supplied history, covering events strictly before it.
        Required with history; refresh payload and timestamp together.

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
    times, stages may have no observations, and rates need not decrease.
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
