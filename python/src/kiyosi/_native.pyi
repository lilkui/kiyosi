"""Typed Python interfaces for the native contracts, market data, and pricing API."""

from collections.abc import Iterable, Iterator
from datetime import date, datetime
from numbers import Real as _RealNumber
from types import EllipsisType
from typing import Literal, Never, Self, SupportsIndex, overload

from . import pricing as _pricing

type _Real = float | _RealNumber

__version__: str

def _repr_previews(value: object) -> dict[str, str]: ...

class KiyosiError(Exception): ...
class InvalidInputError(KiyosiError): ...
class UnsupportedOperationError(KiyosiError): ...
class ComputationFailureError(KiyosiError): ...
class RootNotBracketedError(KiyosiError): ...
class NonConvergenceError(KiyosiError): ...

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
) -> float: ...
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
) -> float: ...

class EuropeanOption:
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
        factors: Iterable[_Real],
    ) -> None: ...
    @staticmethod
    def flat(
        rate: _Real, *, reference_time: datetime | None = None
    ) -> RateTermStructure: ...
    @staticmethod
    def bounded_flat(
        rate: _Real, *, reference_time: datetime, max_time: datetime
    ) -> RateTermStructure: ...
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
    def shifted(self, shift: _Real) -> RateTermStructure: ...

class BlackScholesMertonParameters:
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
    ) -> BlackScholesMertonParameters: ...
    @property
    def flat_risk_free_rate(self) -> float: ...
    @property
    def flat_dividend_yield(self) -> float: ...
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
    ) -> HestonParameters: ...
    @property
    def flat_risk_free_rate(self) -> float: ...
    @property
    def flat_dividend_yield(self) -> float: ...
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
        spots: Iterable[_Real],
        values: Iterable[Iterable[_Real]],
    ) -> None: ...
    @property
    def times(self) -> list[datetime]: ...
    @property
    def spots(self) -> list[float]: ...
    @property
    def values(self) -> list[list[float]]: ...
    @property
    def maximum_volatility(self) -> float: ...
    def volatility(self, time: datetime, spot: _Real) -> float: ...

class LocalVolatilityParameters:
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
    ) -> LocalVolatilityParameters: ...
    @property
    def surface(self) -> LocalVolatilitySurface: ...
    @property
    def flat_risk_free_rate(self) -> float: ...
    @property
    def flat_dividend_yield(self) -> float: ...
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
    trading_days_per_year: SupportsIndex | None = None,
) -> TradingCalendar: ...

class ObservationSchedule:
    def __init__(
        self,
        *,
        times: Iterable[datetime],
        start_time: datetime,
        end_time: datetime,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    @property
    def times(self) -> list[datetime]: ...
    def __len__(self) -> int: ...
    def __getitem__(self, index: SupportsIndex) -> datetime: ...
    def __iter__(self) -> Iterator[datetime]: ...

def fixed_interval_schedule(
    *,
    start_time: datetime,
    end_time: datetime,
    interval_days: SupportsIndex,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule: ...
def monthly_schedule(
    *,
    start_time: datetime,
    end_time: datetime,
    first_observation_month: SupportsIndex,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule: ...

class PricingContext:
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

class AnalyticEuropeanEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def price(self, instrument: EuropeanOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class AnalyticHestonEuropeanEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        absolute_tolerance: _Real = ...,
        relative_tolerance: _Real = ...,
        max_evaluations: SupportsIndex = ...,
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
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class BjerksundStenslandVanillaEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def price(self, instrument: AmericanOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: AmericanOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class AnalyticDigitalEngine:
    def __eq__(self, other: object) -> bool: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class CoxRossRubinsteinBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(self, *, step_count: SupportsIndex = ...) -> None: ...
    @property
    def step_count(self) -> int: ...
    def price(self, instrument: BarrierOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class QuadratureBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class QuadratureDigitalEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class QuadratureVanillaEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class CoxRossRubinsteinVanillaEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(self, *, step_count: SupportsIndex = ...) -> None: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class CoxRossRubinsteinDigitalEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(self, *, step_count: SupportsIndex = ...) -> None: ...
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
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceAccumulatorEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceDoubleBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceAutocallableEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceDoubleTouchEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceBinaryBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceHestonVanillaEngine:
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
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceVanillaEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class FiniteDifferenceDigitalEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        time_step_count: SupportsIndex = ...,
        scheme: _pricing.FiniteDifferenceScheme = ...,
        asset_upper_boundary: _Real | None = None,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class MonteCarloVanillaEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        step_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        thread_count: SupportsIndex = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class MonteCarloEstimate:
    def __eq__(self, other: object) -> bool: ...
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def price(self) -> float: ...
    @property
    def standard_error(self) -> float | None: ...
    @property
    def confidence_interval(self) -> tuple[float, float] | None: ...

class MonteCarloDigitalEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        thread_count: SupportsIndex = ...,
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
    def price_with_statistics(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
    ) -> MonteCarloEstimate: ...
    def price_with_greeks(
        self,
        instrument: CashOrNothingOption | AssetOrNothingOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class MonteCarloBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        thread_count: SupportsIndex = ...,
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
    def price_with_statistics(
        self,
        instrument: BarrierOption,
        context: PricingContext,
    ) -> MonteCarloEstimate: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class MonteCarloDoubleBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        thread_count: SupportsIndex = ...,
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
    def price_with_statistics(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
    ) -> MonteCarloEstimate: ...
    def price_with_greeks(
        self,
        instrument: DoubleBarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class MonteCarloAutocallableEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        thread_count: SupportsIndex = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class MonteCarloAccumulatorEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        thread_count: SupportsIndex = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class MonteCarloAsianEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        path_count: SupportsIndex = ...,
        step_count: SupportsIndex = ...,
        seed: SupportsIndex | None = None,
        thread_count: SupportsIndex = ...,
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
    def price_with_statistics(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
    ) -> MonteCarloEstimate: ...
    def price_with_greeks(
        self,
        instrument: GeometricAveragePriceOption | ArithmeticAveragePriceOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
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
    def __eq__(self, other: object) -> bool: ...
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
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
    price_tolerance: _Real = ...,
    parameter_tolerance: _Real = ...,
    max_iterations: SupportsIndex = ...,
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
    observed_price: _Real,
    *,
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    lower_bound: _Real = ...,
    upper_bound: _Real = ...,
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
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift_days: SupportsIndex = ...,
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
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...
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
    time_shift_days: SupportsIndex = ...,
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
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...
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
    time_shift_days: SupportsIndex = ...,
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
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...
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
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...
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
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...
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
    time_shift_days: SupportsIndex = ...,
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
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...
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
    time_shift_days: SupportsIndex = ...,
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
    spot_shift: _Real | None = None,
    volatility_shift: _Real = ...,
    rate_shift: _Real = ...,
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...
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
    time_shift_days: SupportsIndex = ...,
) -> PricingResult: ...

class GeometricAveragePriceOption:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: _Real | None | EllipsisType = ...,
    ) -> Self: ...
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
    def replace_history(
        self, *, realized_average: _Real | None | EllipsisType = ...
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
    def realized_average(self) -> float | None: ...

class AnalyticGeometricAveragePriceEngine:
    def __eq__(self, other: object) -> bool: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class ArithmeticAveragePriceOption:
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] | EllipsisType = ...,
        strike: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        averaging_start_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_average: _Real | None | EllipsisType = ...,
    ) -> Self: ...
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
    def replace_history(
        self, *, realized_average: _Real | None | EllipsisType = ...
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
    def realized_average(self) -> float | None: ...

class TurnbullWakemanArithmeticAveragePriceEngine:
    def __eq__(self, other: object) -> bool: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class BarrierOption:
    @property
    def calendar(self) -> TradingCalendar: ...
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
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
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
        observation_mode: Literal["continuous", "scheduled"] = ...,
        observation_times: Iterable[datetime] = ...,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar = ...,
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
    def __eq__(self, other: object) -> bool: ...
    def __init__(self) -> None: ...
    def price(self, instrument: BarrierOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: BarrierOption,
        context: PricingContext,
        greeks: Greek | Literal["all"] | Iterable[Greek],
        *,
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class DoubleBarrierOption:
    @property
    def calendar(self) -> TradingCalendar: ...
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
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
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
        observation_mode: Literal["continuous", "scheduled"] = ...,
        observation_times: Iterable[datetime] = ...,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar = ...,
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
        notional: _Real | EllipsisType = ...,
        annual_coupon_rate: _Real | EllipsisType = ...,
        lower_bound: _Real | EllipsisType = ...,
        upper_bound: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        realized_hit_count: SupportsIndex | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        notional: _Real,
        annual_coupon_rate: _Real,
        lower_bound: _Real,
        upper_bound: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        realized_hit_count: SupportsIndex | None = ...,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    def replace_history(
        self, *, realized_hit_count: SupportsIndex | None | EllipsisType = ...
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
    @property
    def calendar(self) -> TradingCalendar: ...

class AnalyticRangeAccrualEngine:
    def __eq__(self, other: object) -> bool: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class AnalyticDoubleBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class AnalyticDoubleTouchEngine:
    def __eq__(self, other: object) -> bool: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class BinaryBarrierOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: _Real,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        payout: _Real | None,
        observation_mode: Literal["continuous", "scheduled"] = ...,
        observation_times: Iterable[datetime] = ...,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    @property
    def calendar(self) -> TradingCalendar: ...
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
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payout: _Real | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
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
    def __init__(
        self,
        *,
        effective_time: datetime,
        expiry_time: datetime,
        barrier_level: _Real,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
        payout: _Real | None,
        settlement_timing: Literal["at_hit", "at_expiry"] = ...,
        observation_mode: Literal["continuous", "scheduled"] = ...,
        observation_times: Iterable[datetime] = ...,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    @property
    def calendar(self) -> TradingCalendar: ...
    def replace(
        self,
        *,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        barrier_level: _Real | EllipsisType = ...,
        barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
        | EllipsisType = ...,
        settlement_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payout: _Real | None | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
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
    strike: _Real,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    payout: _Real,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
) -> BinaryBarrierOption: ...
def asset_binary_barrier_option(
    *,
    option_type: Literal["call", "put"],
    strike: _Real,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
) -> BinaryBarrierOption: ...
def cash_touch_option(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    payout: _Real,
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
) -> TouchOption: ...
def asset_touch_option(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    barrier_level: _Real,
    barrier_type: Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"],
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
) -> TouchOption: ...

class AnalyticBinaryBarrierEngine:
    def __eq__(self, other: object) -> bool: ...
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class DoubleTouchOption:
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
        observation_mode: Literal["continuous", "scheduled"] = ...,
        observation_times: Iterable[datetime] = ...,
        touch_state: Literal["untouched", "touched"] | None = None,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    @property
    def calendar(self) -> TradingCalendar: ...
    def replace(
        self,
        *,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        lower_barrier: _Real | EllipsisType = ...,
        upper_barrier: _Real | EllipsisType = ...,
        barrier_type: Literal["knock_in", "knock_out"] | EllipsisType = ...,
        settlement_timing: Literal["at_hit", "at_expiry"] | EllipsisType = ...,
        observation_mode: Literal["continuous", "scheduled"] | EllipsisType = ...,
        observation_times: Iterable[datetime] | EllipsisType = ...,
        touch_state: Literal["untouched", "touched"] | None | EllipsisType = ...,
        payout: _Real | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
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
    lower_barrier: _Real,
    upper_barrier: _Real,
    payout: _Real,
    settlement_timing: Literal["at_hit", "at_expiry"] = ...,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
) -> DoubleTouchOption: ...
def cash_double_no_touch(
    *,
    effective_time: datetime,
    expiry_time: datetime,
    lower_barrier: _Real,
    upper_barrier: _Real,
    payout: _Real,
    observation_mode: Literal["continuous", "scheduled"] = ...,
    observation_times: Iterable[datetime] = ...,
    touch_state: Literal["untouched", "touched"] | None = None,
    calendar: TradingCalendar = ...,
) -> DoubleTouchOption: ...

class QuadratureAutocallableEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

class AutocallableObservation:
    def __init__(
        self,
        time: datetime,
        knock_out_level: _Real,
        *,
        coupon_rate: _Real = ...,
        coupon_barrier_level: _Real = ...,
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
    knock_out_levels: Iterable[_Real],
    *,
    knock_out_coupon_rates: Iterable[_Real] | None = None,
    coupon_barrier_levels: Iterable[_Real] | None = None,
) -> list[AutocallableObservation]: ...

class SnowballOption:
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
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(
        self, coupon: _Real, *, quote_convention: _pricing.CouponQuoteConvention
    ) -> Self: ...
    def __init__(
        self,
        *,
        maturity_coupon_rate: _Real,
        initial_spot: _Real,
        knock_in_level: _Real,
        upper_strike: _Real,
        lower_strike: _Real,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
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
    @property
    def calendar(self) -> TradingCalendar: ...

class PhoenixOption:
    @overload
    def replace(
        self,
        *,
        coupon_rate: _Real,
        coupon_per_period: EllipsisType = ...,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
    @overload
    def replace(
        self,
        *,
        coupon_rate: EllipsisType = ...,
        coupon_per_period: _Real,
        initial_spot: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        upper_strike: _Real | EllipsisType = ...,
        lower_strike: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
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
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(self, coupon: _Real) -> Self: ...
    @overload
    def __init__(
        self,
        *,
        coupon_rate: _Real,
        coupon_per_period: EllipsisType = ...,
        initial_spot: _Real,
        knock_in_level: _Real,
        upper_strike: _Real,
        lower_strike: _Real,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    @overload
    def __init__(
        self,
        *,
        coupon_rate: EllipsisType = ...,
        coupon_per_period: _Real,
        initial_spot: _Real,
        knock_in_level: _Real,
        upper_strike: _Real,
        lower_strike: _Real,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
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
    @property
    def calendar(self) -> TradingCalendar: ...

class BinarySnowballOption:
    def replace(
        self,
        *,
        maturity_coupon_rate: _Real | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(
        self, coupon: _Real, *, quote_convention: _pricing.CouponQuoteConvention
    ) -> Self: ...
    def __init__(
        self,
        *,
        maturity_coupon_rate: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
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
    @property
    def calendar(self) -> TradingCalendar: ...

class TernarySnowballOption:
    def replace(
        self,
        *,
        maturity_coupon_rate: _Real | EllipsisType = ...,
        minimum_coupon_rate: _Real | EllipsisType = ...,
        knock_in_level: _Real | EllipsisType = ...,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"]
        | EllipsisType = ...,
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"]
        | None
        | EllipsisType = ...,
        principal_ratio: _Real | EllipsisType = ...,
        effective_time: datetime | EllipsisType = ...,
        expiry_time: datetime | EllipsisType = ...,
        observations: Iterable[AutocallableObservation] | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
    def with_coupon(
        self, coupon: _Real, *, quote_convention: _pricing.CouponQuoteConvention
    ) -> Self: ...
    def __init__(
        self,
        *,
        maturity_coupon_rate: _Real,
        minimum_coupon_rate: _Real,
        knock_in_level: _Real,
        knock_in_observation_mode: Literal["every_trading_day", "at_expiry"] = ...,
        effective_time: datetime,
        expiry_time: datetime,
        observations: Iterable[AutocallableObservation],
        barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
        principal_ratio: _Real = ...,
        calendar: TradingCalendar = ...,
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
    @property
    def calendar(self) -> TradingCalendar: ...

def standard_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
def european_snowball(
    *,
    coupon_rate: _Real,
    initial_spot: _Real,
    knock_in_level: _Real,
    knock_out_level: _Real,
    observation_times: Iterable[datetime],
    effective_time: datetime,
    expiry_time: datetime,
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...

class Accumulator:
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
        knock_out_observation_times: Iterable[datetime] | None | EllipsisType = ...,
        terminated: bool | EllipsisType = ...,
        calendar: TradingCalendar | EllipsisType = ...,
    ) -> Self: ...
    def __init__(
        self,
        *,
        strike: _Real,
        knock_out_level: _Real,
        daily_quantity: _Real,
        acceleration_factor: _Real,
        effective_time: datetime,
        expiry_time: datetime,
        accumulated_quantity: _Real | None = None,
        option_type: Literal["call", "put"] = ...,
        knock_out_observation_times: Iterable[datetime] | None = None,
        terminated: bool = ...,
        calendar: TradingCalendar = ...,
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
        accumulated_quantity: _Real | None | EllipsisType = ...,
        terminated: bool | EllipsisType = ...,
    ) -> Self: ...
    def __eq__(self, other: object) -> bool: ...
    @property
    def calendar(self) -> TradingCalendar: ...

class QuadratureAccumulatorEngine:
    def __eq__(self, other: object) -> bool: ...
    def __init__(
        self,
        *,
        asset_step_count: SupportsIndex = ...,
        integration_standard_deviations: _Real = ...,
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
        spot_shift: _Real | None = None,
        volatility_shift: _Real = ...,
        rate_shift: _Real = ...,
        time_shift_days: SupportsIndex = ...,
    ) -> PricingResult: ...

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
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> PhoenixOption: ...
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
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> PhoenixOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
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
    barrier_state: Literal["no_event", "knocked_in", "knocked_out"] | None = None,
    principal_ratio: _Real = ...,
    calendar: TradingCalendar = ...,
) -> SnowballOption: ...
def _calendar_parts(
    calendar: TradingCalendar,
) -> tuple[str, TradingCalendar | None, list[datetime], list[datetime], int]: ...
def _pricing_result(
    price: _Real,
    greeks: dict[Greek, _Real],
    *,
    requested: Iterable[Greek] | None = None,
    supported: Iterable[Greek] | None = None,
) -> PricingResult: ...
def _monte_carlo_estimate(
    *, price: _Real, standard_error: _Real | None
) -> MonteCarloEstimate: ...
def _curve_parts(curve: RateTermStructure) -> tuple[list[float], float]: ...
