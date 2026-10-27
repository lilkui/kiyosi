from collections.abc import Iterable, Iterator
from datetime import date, datetime
from enum import IntEnum
from typing import Literal, Never, Self, TypeAlias, overload

__version__: str

class ErrorCategory(IntEnum):
    INVALID_STRIKE = 2
    INVALID_VOLATILITY = 3
    INVALID_RISK_FREE_RATE = 4
    INVALID_DIVIDEND_YIELD = 5
    INVALID_SPOT_PRICE = 6
    INVALID_DATE = 7
    INVALID_TIME_RANGE = 8
    INVALID_RESULT = 9
    INVALID_SCHEDULE = 10
    INVALID_CALENDAR = 11
    INVALID_PARAMETER = 12
    UNBRACKETED_VOLATILITY = 13
    SOLVER_NON_CONVERGENCE = 14
    SOLVER_NON_FINITE = 15
    UNSUPPORTED_OPERATION = 19

class KiyosiError(Exception):
    category: ErrorCategory

class EuropeanOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_date: date,
        expiry_date: date,
    ) -> None: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_date(self) -> date: ...
    @property
    def expiry_date(self) -> date: ...

class AmericanOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_date: date,
        expiry_date: date,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] = ...,
        strike: float = ...,
        effective_date: date = ...,
        expiry_date: date = ...,
    ) -> AmericanOption: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_date(self) -> date: ...
    @property
    def expiry_date(self) -> date: ...

class CashOrNothingOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        payout: float,
        effective_date: date,
        expiry_date: date,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] = ...,
        strike: float = ...,
        payout: float = ...,
        effective_date: date = ...,
        expiry_date: date = ...,
    ) -> CashOrNothingOption: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def payout(self) -> float: ...
    @property
    def effective_date(self) -> date: ...
    @property
    def expiry_date(self) -> date: ...

class AssetOrNothingOption:
    def __init__(
        self,
        *,
        option_type: Literal["call", "put"],
        strike: float,
        effective_date: date,
        expiry_date: date,
    ) -> None: ...
    def replace(
        self,
        *,
        option_type: Literal["call", "put"] = ...,
        strike: float = ...,
        effective_date: date = ...,
        expiry_date: date = ...,
    ) -> AssetOrNothingOption: ...
    @property
    def option_type(self) -> Literal["call", "put"]: ...
    @property
    def strike(self) -> float: ...
    @property
    def effective_date(self) -> date: ...
    @property
    def expiry_date(self) -> date: ...

class RateTermStructure:
    def __init__(
        self,
        *,
        reference_time: date | datetime,
        times: Iterable[date | datetime],
        factors: Iterable[float],
    ) -> None: ...
    @staticmethod
    def flat(
        rate: float, *, reference_time: date | datetime | None = None
    ) -> RateTermStructure: ...
    @property
    def is_flat(self) -> bool: ...
    @property
    def reference_time(self) -> datetime | None: ...
    @property
    def max_time(self) -> datetime | None: ...
    @property
    def nodes(self) -> list[datetime]: ...
    @property
    def factors(self) -> list[float]: ...
    def factor(self, start: date | datetime, end: date | datetime) -> float: ...
    def log_factor(self, start: date | datetime, end: date | datetime) -> float: ...
    def forward_rate(self, time: date | datetime) -> float: ...
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
        risk_free_rate: float | RateTermStructure = ...,
        dividend_yield: float | RateTermStructure = ...,
        volatility: float = ...,
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
    def dates(self) -> list[date]: ...
    def __len__(self) -> int: ...
    def __getitem__(self, index: int) -> date: ...
    def __iter__(self) -> Iterator[date]: ...

def fixed_interval_schedule(
    *,
    start: date,
    end: date,
    interval_days: int,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule: ...
def monthly_schedule(
    *,
    start: date,
    end: date,
    first_observation_month: int,
    calendar: TradingCalendar = ...,
) -> ObservationSchedule: ...

class PricingContext:
    def __init__(
        self,
        *,
        model_parameters: BlackScholesMertonParameters,
        spot_price: float,
        valuation_time: date | datetime,
        calendar: TradingCalendar = ...,
    ) -> None: ...
    def replace(
        self,
        *,
        model_parameters: BlackScholesMertonParameters = ...,
        spot_price: float = ...,
        valuation_time: date | datetime = ...,
        calendar: TradingCalendar = ...,
    ) -> PricingContext: ...
    @property
    def model_parameters(self) -> BlackScholesMertonParameters: ...
    @property
    def spot_price(self) -> float: ...
    @property
    def valuation_date(self) -> date: ...
    @property
    def valuation_time(self) -> datetime: ...
    @property
    def calendar(self) -> TradingCalendar: ...

class AnalyticVanillaEngine:
    def __init__(self) -> None: ...
    def price(self, instrument: EuropeanOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: EuropeanOption,
        context: PricingContext,
        greeks: Greek | Iterable[Greek] | None = None,
        *,
        all_greeks: bool = False,
        spot_shift: float = ...,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

class BjerksundStenslandVanillaEngine:
    def __init__(self) -> None: ...
    def price(self, instrument: AmericanOption, context: PricingContext) -> float: ...
    def price_with_greeks(
        self,
        instrument: AmericanOption,
        context: PricingContext,
        greeks: Greek | Iterable[Greek] | None = None,
        *,
        all_greeks: bool = False,
        spot_shift: float = ...,
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
        greeks: Greek | Iterable[Greek] | None = None,
        *,
        all_greeks: bool = False,
        spot_shift: float = ...,
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
        greeks: Greek | Iterable[Greek] | None = None,
        *,
        all_greeks: bool = False,
        spot_shift: float = ...,
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
        greeks: Greek | Iterable[Greek] | None = None,
        *,
        all_greeks: bool = False,
        spot_shift: float = ...,
        volatility_shift: float = ...,
        rate_shift: float = ...,
        time_shift_days: int = ...,
    ) -> PricingResult: ...

Greek: TypeAlias = Literal[
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "vanna",
    "zomma",
    "rho",
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
    def vanna(self) -> float | None: ...
    @property
    def zomma(self) -> float | None: ...
    @property
    def rho(self) -> float | None: ...
    def require(self, measure: Greek) -> float: ...

@overload
def implied_volatility(
    engine: AnalyticVanillaEngine,
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
    engine: AnalyticDigitalEngine | CoxRossRubinsteinDigitalEngine,
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
    engine: BjerksundStenslandVanillaEngine,
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
    engine: CoxRossRubinsteinVanillaEngine,
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
