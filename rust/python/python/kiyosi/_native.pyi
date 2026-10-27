from datetime import date, datetime
from enum import IntEnum
from typing import Literal, Never, Self

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

class BlackScholesMertonParameters:
    def __init__(
        self, *, risk_free_rate: float, dividend_yield: float, volatility: float
    ) -> None: ...
    @property
    def risk_free_rate(self) -> float: ...
    @property
    def dividend_yield(self) -> float: ...
    @property
    def volatility(self) -> float: ...

class TradingCalendar:
    def __new__(cls, _token: Never, /) -> Self: ...
    @property
    def trading_days_per_year(self) -> int: ...

def weekdays_calendar() -> TradingCalendar: ...

class PricingContext:
    def __init__(
        self,
        *,
        model_parameters: BlackScholesMertonParameters,
        spot_price: float,
        valuation_time: date | datetime,
        calendar: TradingCalendar = ...,
    ) -> None: ...
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
