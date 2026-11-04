"""Typed Python interfaces for the native contracts, market data, and pricing API."""

from collections.abc import Iterable, Iterator
from datetime import date, datetime
from types import EllipsisType
from typing import Literal, Never, Self, overload

from . import pricing as _pricing

__version__: str

class KiyosiError(Exception): ...
class InvalidInputError(KiyosiError): ...
class UnsupportedOperationError(KiyosiError): ...
class ComputationFailureError(KiyosiError): ...
class RootNotBracketedError(KiyosiError): ...
class NonConvergenceError(KiyosiError): ...

@overload
def implied_coupon(
    engine: QuadratureAutocallableEngine
    | FiniteDifferenceAutocallableEngine
    | MonteCarloAutocallableEngine,
    instrument: SnowballOption | BinarySnowballOption | TernarySnowballOption,
    context: PricingContext,
    observed_price: float,
    *,
    quote_convention: _pricing.CouponQuoteConvention,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_coupon(
    engine: QuadratureAutocallableEngine
    | FiniteDifferenceAutocallableEngine
    | MonteCarloAutocallableEngine,
    instrument: PhoenixOption,
    context: PricingContext,
    observed_price: float,
    *,
    quote_convention: EllipsisType = ...,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...

class EuropeanOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> EuropeanOption: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

class AmericanOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> AmericanOption: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

class CashOrNothingOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        payout: float,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        payout: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> CashOrNothingOption: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def payout(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

class AssetOrNothingOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_time: datetime,
        expiry_time: datetime,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
    ) -> AssetOrNothingOption: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

class RateTermStructure:
    def __init__(
        self,
        *,
        reference_time: datetime,
        times: Iterable[datetime],
        factors: Iterable[float],
    ) -> None: ...
    @staticmethod
    def flat(
        rate: float, *, reference_time: datetime | None = None
    ) -> RateTermStructure: ...
    @property
    def is_explicitly_flat(self) -> bool: ...
    @property
    def flat_rate(self) -> float | None: ...
    @property
    def reference_time(self) -> datetime | None: ...
    @property
    def max_time(self) -> datetime | None: ...
    @property
    def nodes(self) -> list[datetime]: ...
    @property
    def factors(self) -> list[float]: ...
    def factor(self, start_time: datetime, end_time: datetime) -> float: ...
    def log_factor(self, start_time: datetime, end_time: datetime) -> float: ...
    def forward_rate(self, time: datetime) -> float: ...
    def shifted(self, shift: float) -> RateTermStructure: ...

class BlackScholesMertonParameters:
    def __init__(
        self,
        *,
        risk_free_rate: float | RateTermStructure,
        dividend_yield: float | RateTermStructure,
        volatility: float,
    ) -> None: ...
    def replace(
        self,
        *,
        risk_free_rate: float | RateTermStructure | EllipsisType = ...,
        dividend_yield: float | RateTermStructure | EllipsisType = ...,
        volatility: float | EllipsisType = ...,
    ) -> BlackScholesMertonParameters: ...
    @property
    def risk_free_rate(self) -> float | RateTermStructure: ...
    @property
    def dividend_yield(self) -> float | RateTermStructure: ...
    @property
    def risk_free_curve(self) -> RateTermStructure: ...
    @property
    def dividend_curve(self) -> RateTermStructure: ...
    @property
    def volatility(self) -> float: ...

class HestonParameters:
    def __init__(
        self,
        *,
        risk_free_rate: float | RateTermStructure,
        dividend_yield: float | RateTermStructure,
        initial_variance: float,
        long_run_variance: float,
        mean_reversion_speed: float,
        vol_of_vol: float,
        correlation: float,
    ) -> None: ...
    def replace(
        self,
        *,
        risk_free_rate: float | RateTermStructure | EllipsisType = ...,
        dividend_yield: float | RateTermStructure | EllipsisType = ...,
        initial_variance: float | EllipsisType = ...,
        long_run_variance: float | EllipsisType = ...,
        mean_reversion_speed: float | EllipsisType = ...,
        vol_of_vol: float | EllipsisType = ...,
        correlation: float | EllipsisType = ...,
    ) -> HestonParameters: ...
    @property
    def risk_free_rate(self) -> float | RateTermStructure: ...
    @property
    def dividend_yield(self) -> float | RateTermStructure: ...
    @property
    def risk_free_curve(self) -> RateTermStructure: ...
    @property
    def dividend_curve(self) -> RateTermStructure: ...
    @property
    def initial_variance(self) -> float: ...
    @property
    def long_run_variance(self) -> float: ...
    @property
    def mean_reversion_speed(self) -> float: ...
    @property
    def vol_of_vol(self) -> float: ...
    @property
    def correlation(self) -> float: ...

class LocalVolatilitySurface:
    def __init__(
        self,
        *,
        times: Iterable[datetime],
        spots: Iterable[float],
        values: Iterable[Iterable[float]],
    ) -> None: ...
    @property
    def times(self) -> list[datetime]: ...
    @property
    def spots(self) -> list[float]: ...
    @property
    def values(self) -> list[list[float]]: ...
    @property
    def maximum_volatility(self) -> float: ...
    def volatility(self, time: datetime, spot: float) -> float: ...

class LocalVolatilityParameters:
    def __init__(
        self,
        *,
        risk_free_rate: float | RateTermStructure,
        dividend_yield: float | RateTermStructure,
        surface: LocalVolatilitySurface,
    ) -> None: ...
    def replace(
        self,
        *,
        risk_free_rate: float | RateTermStructure | EllipsisType = ...,
        dividend_yield: float | RateTermStructure | EllipsisType = ...,
        surface: LocalVolatilitySurface | EllipsisType = ...,
    ) -> LocalVolatilityParameters: ...
    @property
    def surface(self) -> LocalVolatilitySurface: ...
    @property
    def risk_free_rate(self) -> float | RateTermStructure: ...
    @property
    def dividend_yield(self) -> float | RateTermStructure: ...
    @property
    def risk_free_curve(self) -> RateTermStructure: ...
    @property
    def dividend_curve(self) -> RateTermStructure: ...

class TradingCalendar:
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def trading_days_per_year(self) -> int: ...
    def is_trading_day(self, value: date) -> bool: ...
    def adjust(
        self, nominal: date, convention: Literal["following", "preceding"]
    ) -> date: ...
    def trading_days_between(self, start: date, end: date) -> int: ...
    def trading_year_fraction(self, start: date, end: date) -> float: ...

def weekdays_calendar() -> TradingCalendar: ...
def all_days_calendar() -> TradingCalendar: ...
def sse_calendar() -> TradingCalendar: ...
def custom_calendar(
    *,
    base: TradingCalendar,
    closed_dates: Iterable[date] = (),
    open_dates: Iterable[date] = (),
    trading_days_per_year: int | None = None,
) -> TradingCalendar: ...

class ObservationSchedule:
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def times(self) -> list[datetime]: ...
    def __len__(self) -> int: ...
    def __getitem__(self, index: int) -> datetime: ...
    def __iter__(self) -> Iterator[datetime]: ...

def fixed_interval_schedule(
    *,
    start_time: datetime,
    end_time: datetime,
    interval_days: int,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule: ...
def monthly_schedule(
    *,
    start_time: datetime,
    end_time: datetime,
    first_observation_month: int,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule: ...

class PricingContext:
    def __init__(
        self,
        *,
        model_parameters: BlackScholesMertonParameters
        | LocalVolatilityParameters
        | HestonParameters,
        spot_price: float,
        valuation_time: datetime,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        model_parameters: BlackScholesMertonParameters
        | LocalVolatilityParameters
        | HestonParameters
        | EllipsisType = ...,
        spot_price: float | EllipsisType = ...,
        valuation_time: datetime | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> PricingContext: ...
    @property
    def model_parameters(
        self,
    ) -> (
        BlackScholesMertonParameters | LocalVolatilityParameters | HestonParameters
    ): ...
    @property
    def spot_price(self) -> float: ...
    @property
    def valuation_date(self) -> date: ...
    @property
    def valuation_time(self) -> datetime: ...
    @property
    def calendar(self) -> TradingCalendar: ...

class AnalyticEuropeanEngine:
    def __init__(self) -> None: ...
    def price(self, instrument: EuropeanOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
    ) -> PricingResult: ...

class AnalyticHestonEuropeanEngine:
    def __init__(
        self,
        *,
        absolute_tolerance: float = ...,
        relative_tolerance: float = ...,
        max_evaluations: int = ...,
    ) -> None: ...
    @property
    def absolute_tolerance(self) -> float: ...
    @property
    def relative_tolerance(self) -> float: ...
    @property
    def max_evaluations(self) -> int: ...
    def price(self, instrument: EuropeanOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
    ) -> PricingResult: ...

class BjerksundStenslandVanillaEngine:
    def __init__(self) -> None: ...
    def price(self, instrument: AmericanOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class AnalyticDigitalEngine:
    def __init__(self) -> None: ...
    def price(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class CoxRossRubinsteinBarrierEngine:
    def __init__(self, *, step_count: int = ...) -> None: ...
    @property
    def step_count(self) -> int: ...
    def price(self, instrument: BarrierOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
    ) -> PricingResult: ...

class QuadratureBarrierEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        integration_standard_deviations: float = ...,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def integration_standard_deviations(self) -> float: ...
    def price(self, instrument: BarrierOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class QuadratureDigitalEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        integration_standard_deviations: float = ...,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def integration_standard_deviations(self) -> float: ...
    def price(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class QuadratureVanillaEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        integration_standard_deviations: float = ...,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def integration_standard_deviations(self) -> float: ...
    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class CoxRossRubinsteinVanillaEngine:
    def __init__(self, *, step_count: int = ...) -> None: ...
    @property
    def step_count(self) -> int: ...
    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class CoxRossRubinsteinDigitalEngine:
    def __init__(self, *, step_count: int = ...) -> None: ...
    @property
    def step_count(self) -> int: ...
    def price(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
    ) -> PricingResult: ...

class FiniteDifferenceAccumulatorEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(self, instrument: Accumulator, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: Accumulator,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class FiniteDifferenceBarrierEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(self, instrument: BarrierOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class FiniteDifferenceDoubleBarrierEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(
        self, instrument: DoubleBarrierOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class FiniteDifferenceAutocallableEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class FiniteDifferenceDoubleTouchEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(
        self, instrument: DoubleTouchOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: DoubleTouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class FiniteDifferenceBinaryBarrierEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: BinaryBarrierOption | TouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class FiniteDifferenceHestonVanillaEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        variance_step_count: int = ...,
        time_step_count: int = ...,
        asset_upper_boundary: float | None = None,
        variance_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def variance_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    @property
    def variance_upper_boundary(self) -> float | None: ...
    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
    ) -> PricingResult: ...

class FiniteDifferenceVanillaEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class FiniteDifferenceDigitalEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        time_step_count: int = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: float | None = None,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def time_step_count(self) -> int: ...
    @property
    def scheme(self) -> _pricing.FiniteDifferenceScheme: ...
    @property
    def asset_upper_boundary(self) -> float | None: ...
    def price(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class MonteCarloVanillaEngine:
    def __init__(
        self,
        *,
        path_count: int = ...,
        step_count: int = ...,
        seed: int | None = None,
        thread_count: int = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    @property
    def path_count(self) -> int: ...
    @property
    def step_count(self) -> int: ...
    @property
    def seed(self) -> int | None: ...
    @property
    def thread_count(self) -> int: ...
    @property
    def sampling(self) -> _pricing.MonteCarloSampling: ...
    @property
    def antithetic(self) -> bool: ...
    def price(
        self, instrument: EuropeanOption | AmericanOption, context: PricingContext
    ) -> float: ...
    def price_with_statistics(
        self, instrument: EuropeanOption, context: PricingContext
    ) -> MonteCarloEstimate: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption | AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class MonteCarloEstimate:
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def price(self) -> float: ...
    @property
    def standard_error(self) -> float | None: ...
    @property
    def confidence_interval(self) -> tuple[float, float] | None: ...

class MonteCarloDigitalEngine:
    def __init__(
        self,
        *,
        path_count: int = ...,
        seed: int | None = None,
        thread_count: int = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    @property
    def path_count(self) -> int: ...
    @property
    def seed(self) -> int | None: ...
    @property
    def thread_count(self) -> int: ...
    @property
    def sampling(self) -> _pricing.MonteCarloSampling: ...
    @property
    def antithetic(self) -> bool: ...
    def price(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
    ) -> PricingResult: ...

class MonteCarloBarrierEngine:
    def __init__(
        self,
        *,
        path_count: int = ...,
        seed: int | None = None,
        thread_count: int = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    @property
    def path_count(self) -> int: ...
    @property
    def seed(self) -> int | None: ...
    @property
    def thread_count(self) -> int: ...
    @property
    def sampling(self) -> _pricing.MonteCarloSampling: ...
    @property
    def antithetic(self) -> bool: ...
    def price(
        self,
        instrument: BarrierOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float = ...,
    ) -> PricingResult: ...

class MonteCarloDoubleBarrierEngine:
    def __init__(
        self,
        *,
        path_count: int = ...,
        seed: int | None = None,
        thread_count: int = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    @property
    def path_count(self) -> int: ...
    @property
    def seed(self) -> int | None: ...
    @property
    def thread_count(self) -> int: ...
    @property
    def sampling(self) -> _pricing.MonteCarloSampling: ...
    @property
    def antithetic(self) -> bool: ...
    def price(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float = ...,
    ) -> PricingResult: ...

class MonteCarloAutocallableEngine:
    def __init__(
        self,
        *,
        path_count: int = ...,
        seed: int | None = None,
        thread_count: int = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    @property
    def path_count(self) -> int: ...
    @property
    def seed(self) -> int | None: ...
    @property
    def thread_count(self) -> int: ...
    @property
    def sampling(self) -> _pricing.MonteCarloSampling: ...
    @property
    def antithetic(self) -> bool: ...
    def price(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_statistics(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
    ) -> MonteCarloEstimate: ...
    def price_with_greeks(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class MonteCarloAccumulatorEngine:
    def __init__(
        self,
        *,
        path_count: int = ...,
        seed: int | None = None,
        thread_count: int = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    @property
    def path_count(self) -> int: ...
    @property
    def seed(self) -> int | None: ...
    @property
    def thread_count(self) -> int: ...
    @property
    def sampling(self) -> _pricing.MonteCarloSampling: ...
    @property
    def antithetic(self) -> bool: ...
    def price(self, instrument: Accumulator, context: PricingContext) -> float: ...
    def price_with_statistics(
        self, instrument: Accumulator, context: PricingContext
    ) -> MonteCarloEstimate: ...
    def price_with_greeks(
        self,
        instrument: Accumulator,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class MonteCarloAsianEngine:
    def __init__(
        self,
        *,
        path_count: int = ...,
        step_count: int = ...,
        seed: int | None = None,
        thread_count: int = ...,
        sampling: _pricing.MonteCarloSampling = ...,
        antithetic: bool = ...,
    ) -> None: ...
    @property
    def path_count(self) -> int: ...
    @property
    def step_count(self) -> int: ...
    @property
    def seed(self) -> int | None: ...
    @property
    def thread_count(self) -> int: ...
    @property
    def sampling(self) -> _pricing.MonteCarloSampling: ...
    @property
    def antithetic(self) -> bool: ...
    def price(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
    ) -> PricingResult: ...

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
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def price(self) -> float: ...
    @property
    def delta(self) -> float | None: ...
    @property
    def gamma(self) -> float | None: ...
    @property
    def speed(self) -> float | None: ...
    @property
    def theta(self) -> float | None: ...
    @property
    def charm(self) -> float | None: ...
    @property
    def color(self) -> float | None: ...
    @property
    def vega(self) -> float | None: ...
    @property
    def volga(self) -> float | None: ...
    @property
    def vanna(self) -> float | None: ...
    @property
    def zomma(self) -> float | None: ...
    @property
    def rho(self) -> float | None: ...
    @property
    def dividend_rho(self) -> float | None: ...
    def require(self, measure: Greek) -> float: ...
    def status(
        self, measure: Greek
    ) -> Literal["available", "not_requested", "unsupported", "undefined"]: ...
    @property
    def requested(self) -> list[Greek]: ...
    @property
    def supported(self) -> list[Greek]: ...

@overload
def implied_volatility(
    engine: QuadratureAccumulatorEngine
    | FiniteDifferenceAccumulatorEngine
    | MonteCarloAccumulatorEngine,
    instrument: Accumulator,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
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
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticRangeAccrualEngine,
    instrument: RangeAccrualOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticDoubleBarrierEngine
    | FiniteDifferenceDoubleBarrierEngine
    | MonteCarloDoubleBarrierEngine,
    instrument: DoubleBarrierOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticDoubleTouchEngine | FiniteDifferenceDoubleTouchEngine,
    instrument: DoubleTouchOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
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
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticGeometricAveragePriceEngine | MonteCarloAsianEngine,
    instrument: GeometricAveragePriceOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: TurnbullWakemanArithmeticAveragePriceEngine | MonteCarloAsianEngine,
    instrument: ArithmeticAveragePriceOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticEuropeanEngine | MonteCarloVanillaEngine,
    instrument: EuropeanOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
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
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: BjerksundStenslandVanillaEngine | MonteCarloVanillaEngine,
    instrument: AmericanOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: CoxRossRubinsteinVanillaEngine
    | FiniteDifferenceVanillaEngine
    | QuadratureVanillaEngine,
    instrument: EuropeanOption | AmericanOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
) -> float: ...
@overload
def implied_volatility(
    engine: AnalyticBinaryBarrierEngine | FiniteDifferenceBinaryBarrierEngine,
    instrument: BinaryBarrierOption | TouchOption,
    context: PricingContext,
    observed_price: float,
    *,
    lower_bound: float = ...,
    upper_bound: float = ...,
    price_tolerance: float = ...,
    parameter_tolerance: float = ...,
    max_iterations: int = ...,
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
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
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
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticRangeAccrualEngine,
    instrument: RangeAccrualOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
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
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticDoubleTouchEngine | FiniteDifferenceDoubleTouchEngine,
    instrument: DoubleTouchOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
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
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticGeometricAveragePriceEngine | MonteCarloAsianEngine,
    instrument: GeometricAveragePriceOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: TurnbullWakemanArithmeticAveragePriceEngine | MonteCarloAsianEngine,
    instrument: ArithmeticAveragePriceOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticEuropeanEngine | MonteCarloVanillaEngine,
    instrument: EuropeanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
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
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: BjerksundStenslandVanillaEngine | MonteCarloVanillaEngine,
    instrument: AmericanOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
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
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...
@overload
def numerical_greeks(
    engine: AnalyticBinaryBarrierEngine | FiniteDifferenceBinaryBarrierEngine,
    instrument: BinaryBarrierOption | TouchOption,
    context: PricingContext,
    greeks: Greek | Literal["all"] | Iterable[Greek],
    *,
    spot_shift: float | None = None,
    volatility_shift: float = ...,
    rate_shift: float = ...,
    time_shift_days: int = ...,
) -> PricingResult: ...

class GeometricAveragePriceOption:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: float | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_time: datetime,
        averaging_start_time: datetime,
        expiry_time: datetime,
        realized_average: float = ...,
    ) -> None: ...
    def replace_history(
        self, *, realized_average: float | EllipsisType = ...
    ) -> Self: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def averaging_start_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def realized_average(self) -> float: ...

class AnalyticGeometricAveragePriceEngine:
    def __init__(self) -> None: ...
    def price(
        self, instrument: GeometricAveragePriceOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: GeometricAveragePriceOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class ArithmeticAveragePriceOption:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: float | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_time: datetime,
        averaging_start_time: datetime,
        expiry_time: datetime,
        realized_average: float = ...,
    ) -> None: ...
    def replace_history(
        self, *, realized_average: float | EllipsisType = ...
    ) -> Self: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def averaging_start_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def realized_average(self) -> float: ...

class TurnbullWakemanArithmeticAveragePriceEngine:
    def __init__(self) -> None: ...
    def price(
        self, instrument: ArithmeticAveragePriceOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: ArithmeticAveragePriceOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class BarrierOption:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        barrier_level: float | EllipsisType = ...,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
        | EllipsisType = ...,
        rebate: float | EllipsisType = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: float,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        rebate: float = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_mode: Literal["continuous", "scheduled"] = ...,
        observation_times: Iterable[datetime] = ...,
        touch_state: Literal["untouched", "touched"] | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
    ) -> Self: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def barrier_level(self) -> float: ...
    @property
    def barrier_type(
        self,
    ) -> Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]: ...
    @property
    def rebate(self) -> float: ...
    @property
    def rebate_timing(self) -> Literal["at_hit", "at_expiry"]: ...
    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def observation_schedule(self) -> ObservationSchedule: ...
    @property
    def mean_observation_year_fraction(self) -> float: ...
    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None: ...

class AnalyticBarrierEngine:
    def __init__(self) -> None: ...
    def price(self, instrument: BarrierOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class DoubleBarrierOption:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        lower_barrier: float | EllipsisType = ...,
        upper_barrier: float | EllipsisType = ...,
        barrier_type: Literal["knock_in", "knock_out"] | EllipsisType = ...,
        rebate: float | EllipsisType = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_time: datetime,
        expiry_time: datetime,
        lower_barrier: float,
        upper_barrier: float,
        barrier_type: Literal["knock_in", "knock_out"],
        rebate: float = ...,
        rebate_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_mode: Literal["continuous", "scheduled"] = ...,
        observation_times: Iterable[datetime] = ...,
        touch_state: Literal["untouched", "touched"] | None = None,
    ) -> None: ...
    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
    ) -> Self: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def lower_barrier(self) -> float: ...
    @property
    def upper_barrier(self) -> float: ...
    @property
    def barrier_type(
        self,
    ) -> Literal["knock_in", "knock_out"]: ...
    @property
    def rebate(self) -> float: ...
    @property
    def rebate_timing(self) -> Literal["at_hit", "at_expiry"]: ...
    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def observation_schedule(self) -> ObservationSchedule: ...
    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None: ...

class RangeAccrualOption:
    def replace(
        self,
        *,
        notional: float | EllipsisType = ...,
        annual_coupon_rate: float | EllipsisType = ...,
        lower_bound: float | EllipsisType = ...,
        upper_bound: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_hit_count: int | None | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        notional: float,
        annual_coupon_rate: float,
        lower_bound: float,
        upper_bound: float,
        effective_time: datetime,
        expiry_time: datetime,
        realized_hit_count: int | None = ...,
    ) -> None: ...
    def replace_history(
        self, *, realized_hit_count: int | None | EllipsisType = ...
    ) -> Self: ...
    @property
    def notional(self) -> float: ...
    @property
    def annual_coupon_rate(self) -> float: ...
    @property
    def lower_bound(self) -> float: ...
    @property
    def upper_bound(self) -> float: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def realized_hit_count(self) -> int | None: ...

class AnalyticRangeAccrualEngine:
    def __init__(self) -> None: ...
    def price(
        self, instrument: RangeAccrualOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: RangeAccrualOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class AnalyticDoubleBarrierEngine:
    def __init__(self) -> None: ...
    def price(
        self, instrument: DoubleBarrierOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class AnalyticDoubleTouchEngine:
    def __init__(self) -> None: ...
    def price(
        self, instrument: DoubleTouchOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: DoubleTouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class BinaryBarrierOption:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        barrier_level: float | EllipsisType = ...,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
        | EllipsisType = ...,
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payout: float | EllipsisType = ...,
    ) -> Self: ...
    def __new__(cls, _token: Never, /) -> Self: ...
    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
    ) -> Self: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def barrier_level(self) -> float: ...
    @property
    def barrier_type(
        self,
    ) -> Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]: ...
    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def observation_schedule(self) -> ObservationSchedule: ...
    @property
    def mean_observation_year_fraction(self) -> float: ...
    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None: ...
    @property
    def payoff_type(self) -> Literal["cash", "asset"]: ...
    @property
    def payout(self) -> float | None: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...

class TouchOption:
    def replace(
        self,
        *,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        barrier_level: float | EllipsisType = ...,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
        | EllipsisType = ...,
        settlement_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payout: float | EllipsisType = ...,
    ) -> Self: ...
    def __new__(cls, _token: Never, /) -> Self: ...
    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
    ) -> Self: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def barrier_level(self) -> float: ...
    @property
    def barrier_type(
        self,
    ) -> Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]: ...
    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def observation_schedule(self) -> ObservationSchedule: ...
    @property
    def mean_observation_year_fraction(self) -> float: ...
    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None: ...
    @property
    def payoff_type(self) -> Literal["cash", "asset"]: ...
    @property
    def payout(self) -> float | None: ...
    @property
    def is_one_touch(self) -> bool: ...
    @property
    def is_up(self) -> bool: ...
    @property
    def settlement_timing(self) -> Literal["at_hit", "at_expiry"]: ...

def cash_binary_barrier_option(
    *,
    option_type: Literal["call", "put"],
    strike: float,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: float,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    payout: float,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
) -> BinaryBarrierOption: ...
def asset_binary_barrier_option(
    *,
    option_type: Literal["call", "put"],
    strike: float,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: float,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
) -> BinaryBarrierOption: ...
def cash_touch_option(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: float,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    payout: float,
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
) -> TouchOption: ...
def asset_touch_option(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: float,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
) -> TouchOption: ...

class AnalyticBinaryBarrierEngine:
    def __init__(self) -> None: ...
    def price(
        self, instrument: BinaryBarrierOption | TouchOption, context: PricingContext
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: BinaryBarrierOption | TouchOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class DoubleTouchOption:
    def replace(
        self,
        *,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        lower_barrier: float | EllipsisType = ...,
        upper_barrier: float | EllipsisType = ...,
        barrier_type: Literal["knock_in", "knock_out"] | EllipsisType = ...,
        settlement_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payout: float | EllipsisType = ...,
    ) -> Self: ...
    def __new__(cls, _token: Never, /) -> Self: ...
    def replace_history(
        self,
        *,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
    ) -> Self: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    @property
    def lower_barrier(self) -> float: ...
    @property
    def upper_barrier(self) -> float: ...
    @property
    def payout(self) -> float: ...
    @property
    def is_one_touch(self) -> bool: ...
    @property
    def barrier_type(self) -> Literal["knock_in", "knock_out"]: ...
    @property
    def settlement_timing(self) -> Literal["at_hit", "at_expiry"]: ...
    @property
    def observation_mode(self) -> Literal["continuous", "scheduled"]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def observation_schedule(self) -> ObservationSchedule: ...
    @property
    def touch_state(self) -> Literal["untouched", "touched"] | None: ...

def cash_double_touch(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    lower_barrier: float,
    upper_barrier: float,
    payout: float,
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
) -> DoubleTouchOption: ...
def cash_double_no_touch(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    lower_barrier: float,
    upper_barrier: float,
    payout: float,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
) -> DoubleTouchOption: ...

class QuadratureAutocallableEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        integration_standard_deviations: float = ...,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def integration_standard_deviations(self) -> float: ...
    def price(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: SnowballOption
        | BinarySnowballOption
        | TernarySnowballOption
        | PhoenixOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class AutocallableObservation:
    def __init__(
        self,
        time: datetime,
        knock_out_level: float,
        *,
        coupon_rate: float = ...,
        coupon_barrier_level: float = ...,
    ) -> None: ...
    @property
    def time(self) -> datetime: ...
    @property
    def knock_out_level(self) -> float: ...
    @property
    def coupon_rate(self) -> float: ...
    @property
    def coupon_barrier_level(self) -> float: ...

def autocallable_observations(
    observation_times: Iterable[datetime],
    knock_out_levels: Iterable[float],
    *,
    knock_out_coupon_rates: Iterable[float] | None = None,
    coupon_barrier_levels: Iterable[float] | None = None,
) -> list[AutocallableObservation]: ...

class SnowballOption:
    def replace(
        self,
        *,
        maturity_coupon_rate: float | EllipsisType = ...,
        initial_spot: float | EllipsisType = ...,
        knock_in_level: float | EllipsisType = ...,
        upper_strike: float | EllipsisType = ...,
        lower_strike: float | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(
        self, coupon: float, *, quote_convention: _pricing.CouponQuoteConvention
    ) -> Self: ...
    def __init__(
        self,
        *,
        maturity_coupon_rate: float,
        initial_spot: float,
        knock_in_level: float,
        upper_strike: float,
        lower_strike: float,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: float = ...,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
    ) -> SnowballOption: ...
    @property
    def observations(self) -> list[AutocallableObservation]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def knock_out_levels(self) -> list[float]: ...
    @property
    def knock_out_coupon_rates(self) -> list[float]: ...
    @property
    def maturity_coupon_rate(self) -> float: ...
    @property
    def initial_spot(self) -> float: ...
    @property
    def knock_in_level(self) -> float: ...
    @property
    def upper_strike(self) -> float: ...
    @property
    def lower_strike(self) -> float: ...
    @property
    def principal_ratio(self) -> float: ...
    @property
    def knock_in_observation_mode(
        self,
    ) -> Literal["every_trading_day", "at_expiry"]: ...
    @property
    def barrier_state(
        self,
    ) -> Literal["no_event", "knocked_in", "knocked_out"] | None: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

class PhoenixOption:
    @overload
    def replace(
        self,
        *,
        coupon_rate: float,
        coupon_per_period: EllipsisType = ...,
        initial_spot: float | EllipsisType = ...,
        knock_in_level: float | EllipsisType = ...,
        upper_strike: float | EllipsisType = ...,
        lower_strike: float | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
    ) -> Self: ...
    @overload
    def replace(
        self,
        *,
        coupon_rate: EllipsisType = ...,
        coupon_per_period: float,
        initial_spot: float | EllipsisType = ...,
        knock_in_level: float | EllipsisType = ...,
        upper_strike: float | EllipsisType = ...,
        lower_strike: float | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
    ) -> Self: ...
    @overload
    def replace(
        self,
        *,
        coupon_rate: EllipsisType = ...,
        coupon_per_period: EllipsisType = ...,
        initial_spot: float | EllipsisType = ...,
        knock_in_level: float | EllipsisType = ...,
        upper_strike: float | EllipsisType = ...,
        lower_strike: float | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(self, coupon: float) -> Self: ...
    @overload
    def __init__(
        self,
        *,
        coupon_rate: float,
        coupon_per_period: EllipsisType = ...,
        initial_spot: float,
        knock_in_level: float,
        upper_strike: float,
        lower_strike: float,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: float = ...,
    ) -> None: ...
    @overload
    def __init__(
        self,
        *,
        coupon_rate: EllipsisType = ...,
        coupon_per_period: float,
        initial_spot: float,
        knock_in_level: float,
        upper_strike: float,
        lower_strike: float,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: float = ...,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
    ) -> PhoenixOption: ...
    @property
    def observations(self) -> list[AutocallableObservation]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def knock_out_levels(self) -> list[float]: ...
    @property
    def coupon_barrier_levels(self) -> list[float]: ...
    @property
    def coupon_rate(self) -> float | None: ...
    @property
    def coupon_per_period(self) -> float | None: ...
    @property
    def initial_spot(self) -> float: ...
    @property
    def knock_in_level(self) -> float: ...
    @property
    def upper_strike(self) -> float: ...
    @property
    def lower_strike(self) -> float: ...
    @property
    def principal_ratio(self) -> float: ...
    @property
    def knock_in_observation_mode(
        self,
    ) -> Literal["every_trading_day", "at_expiry"]: ...
    @property
    def barrier_state(
        self,
    ) -> Literal["no_event", "knocked_in", "knocked_out"] | None: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

class BinarySnowballOption:
    def replace(
        self,
        *,
        maturity_coupon_rate: float | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(
        self, coupon: float, *, quote_convention: _pricing.CouponQuoteConvention
    ) -> Self: ...
    def __init__(
        self,
        *,
        maturity_coupon_rate: float,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: float = ...,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
    ) -> BinarySnowballOption: ...
    @property
    def observations(self) -> list[AutocallableObservation]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def knock_out_levels(self) -> list[float]: ...
    @property
    def knock_out_coupon_rates(self) -> list[float]: ...
    @property
    def maturity_coupon_rate(self) -> float: ...
    @property
    def principal_ratio(self) -> float: ...
    @property
    def barrier_state(
        self,
    ) -> Literal["no_event", "knocked_in", "knocked_out"] | None: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

class TernarySnowballOption:
    def replace(
        self,
        *,
        maturity_coupon_rate: float | EllipsisType = ...,
        minimum_coupon_rate: float | EllipsisType = ...,
        knock_in_level: float | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: float | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(
        self, coupon: float, *, quote_convention: _pricing.CouponQuoteConvention
    ) -> Self: ...
    def __init__(
        self,
        *,
        maturity_coupon_rate: float,
        minimum_coupon_rate: float,
        knock_in_level: float,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: float = ...,
    ) -> None: ...
    def replace_history(
        self,
        *,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
    ) -> TernarySnowballOption: ...
    @property
    def observations(self) -> list[AutocallableObservation]: ...
    @property
    def observation_times(self) -> list[datetime]: ...
    @property
    def knock_out_levels(self) -> list[float]: ...
    @property
    def knock_out_coupon_rates(self) -> list[float]: ...
    @property
    def maturity_coupon_rate(self) -> float: ...
    @property
    def knock_in_level(self) -> float: ...
    @property
    def minimum_coupon_rate(self) -> float: ...
    @property
    def principal_ratio(self) -> float: ...
    @property
    def knock_in_observation_mode(
        self,
    ) -> Literal["every_trading_day", "at_expiry"]: ...
    @property
    def barrier_state(
        self,
    ) -> Literal["no_event", "knocked_in", "knocked_out"] | None: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...

def standard_snowball(
    *,
    coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def european_snowball(
    *,
    coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def step_down_snowball(
    *,
    coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    initial_knock_out_level: float,
    knock_out_level_decrement: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def both_down_snowball(
    *,
    initial_coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    initial_knock_out_level: float,
    coupon_rate_decrement: float,
    knock_out_level_decrement: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def dual_coupon_snowball(
    *,
    knock_out_coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    maturity_coupon_rate: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def parachute_snowball(
    *,
    coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    final_knock_out_level: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def otm_snowball(
    *,
    coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    upper_strike: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def loss_capped_snowball(
    *,
    coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    lower_strike: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...

class Accumulator:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: float | EllipsisType = ...,
        knock_out_level: float | EllipsisType = ...,
        daily_quantity: float | EllipsisType = ...,
        acceleration_factor: float | EllipsisType = ...,
        accumulated_quantity: float | None | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        knock_out_observation_times: Iterable[datetime] | None | EllipsisType = ...,
        terminated: bool | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        strike: float,
        knock_out_level: float,
        daily_quantity: float,
        acceleration_factor: float,
        effective_time: datetime,
        expiry_time: datetime,
        accumulated_quantity: float | None = None,
        option_type: Literal["call", "put"] = ...,
        knock_out_observation_times: Iterable[datetime] | None = None,
        terminated: bool = ...,
    ) -> None: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def knock_out_observation_times(self) -> list[datetime] | None: ...
    @property
    def strike(self) -> float: ...
    @property
    def knock_out_level(self) -> float: ...
    @property
    def daily_quantity(self) -> float: ...
    @property
    def acceleration_factor(self) -> float: ...
    @property
    def accumulated_quantity(self) -> float | None: ...
    @property
    def terminated(self) -> bool: ...
    @property
    def effective_time(self) -> datetime: ...
    @property
    def expiry_time(self) -> datetime: ...
    def replace_history(
        self,
        *,
        accumulated_quantity: float | None | EllipsisType = ...,
        terminated: bool | EllipsisType = ...,
    ) -> Self: ...
    def __eq__(self, other: object) -> bool: ...

class QuadratureAccumulatorEngine:
    def __init__(
        self,
        *,
        asset_step_count: int = ...,
        integration_standard_deviations: float = ...,
    ) -> None: ...
    @property
    def asset_step_count(self) -> int: ...
    @property
    def integration_standard_deviations(self) -> float: ...
    def price(
        self,
        instrument: Accumulator,
        context: PricingContext,
    ) -> float: ...
    def price_with_greeks(
        self,
        instrument: Accumulator,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: float | None = None,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

def fixed_coupon_note(
    *,
    coupon_per_period: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> PhoenixOption: ...
def digital_coupon_note(
    *,
    coupon_per_period: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    coupon_barrier_level: float,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> PhoenixOption: ...
def early_profit_snowball(
    *,
    first_coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    second_coupon_rate: float,
    first_stage_end_time: datetime,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def butterfly_snowball(
    *,
    first_coupon_rate: float,
    initial_spot: float,
    knock_in_level: float,
    knock_out_level: float,
    second_coupon_rate: float,
    third_coupon_rate: float,
    first_stage_end_time: datetime,
    second_stage_end_time: datetime,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: float = ...,
) -> SnowballOption: ...
def _calendar_parts(
    calendar: TradingCalendar,
) -> tuple[str, TradingCalendar | None, list[datetime], list[datetime], int]: ...
def _observation_schedule(dates: Iterable[datetime]) -> ObservationSchedule: ...
def _pricing_result(
    price: float,
    greeks: dict[Greek, float],
    *,
    requested: Iterable[Greek] | None = None,
    supported: Iterable[Greek] | None = None,
) -> PricingResult: ...
def _monte_carlo_estimate(
    *, price: float, standard_error: float | None
) -> MonteCarloEstimate: ...
def _curve_parts(curve: RateTermStructure) -> tuple[list[float], float]: ...
