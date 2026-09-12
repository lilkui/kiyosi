"""Idiomatic Python access to kiyosi's closed-form European option pricer."""

import math
from dataclasses import dataclass
from datetime import date
from typing import Any

from . import _native

__all__ = ["EuropeanOption", "Market", "black_scholes", "price"]


def _number(value: Any, field: str, *, positive: bool = False) -> float:
    if isinstance(value, bool):
        raise TypeError(f"{field} must be a finite number")
    try:
        result = float(value)
    except (TypeError, ValueError) as error:
        raise ValueError(f"{field} must be a finite number") from error
    if not math.isfinite(result) or (positive and result <= 0.0):
        qualifier = "finite and positive" if positive else "finite"
        raise ValueError(f"{field} must be {qualifier}")
    return result


def _calendar_date(value: date, field: str) -> date:
    if type(value) is not date:
        raise ValueError(f"{field} must be a datetime.date")
    return value


def _kind(value: str) -> str:
    if not isinstance(value, str) or value not in {"call", "put"}:
        raise ValueError("kind must be 'call' or 'put'")
    return value


@dataclass(frozen=True, slots=True)
class EuropeanOption:
    """A European vanilla call or put contract."""

    kind: str
    strike: float
    expiry: date

    def __post_init__(self) -> None:
        object.__setattr__(self, "kind", _kind(self.kind))
        object.__setattr__(
            self, "strike", _number(self.strike, "strike", positive=True)
        )
        object.__setattr__(self, "expiry", _calendar_date(self.expiry, "expiry"))


@dataclass(frozen=True, slots=True)
class Market:
    """Market inputs for Black-Scholes pricing."""

    spot: float
    risk_free_rate: float
    dividend_yield: float
    volatility: float
    valuation_date: date

    def __post_init__(self) -> None:
        object.__setattr__(self, "spot", _number(self.spot, "spot", positive=True))
        object.__setattr__(
            self, "risk_free_rate", _number(self.risk_free_rate, "risk_free_rate")
        )
        object.__setattr__(
            self, "dividend_yield", _number(self.dividend_yield, "dividend_yield")
        )
        object.__setattr__(
            self, "volatility", _number(self.volatility, "volatility", positive=True)
        )
        object.__setattr__(
            self,
            "valuation_date",
            _calendar_date(self.valuation_date, "valuation_date"),
        )


def price(option: EuropeanOption, market: Market) -> dict[str, float | None]:
    """Price an option and return its premium and standard Greeks."""
    if not isinstance(option, EuropeanOption):
        raise TypeError("option must be a EuropeanOption")
    if not isinstance(market, Market):
        raise TypeError("market must be a Market")
    if option.expiry < market.valuation_date:
        raise ValueError("expiry must not precede valuation_date")
    return _native.price(
        option.kind,
        market.spot,
        option.strike,
        market.valuation_date.year,
        market.valuation_date.month,
        market.valuation_date.day,
        option.expiry.year,
        option.expiry.month,
        option.expiry.day,
        market.risk_free_rate,
        market.dividend_yield,
        market.volatility,
    )


def black_scholes(
    kind: str,
    spot: float,
    strike: float,
    valuation_date: date,
    expiry: date,
    risk_free_rate: float,
    dividend_yield: float,
    volatility: float,
) -> dict[str, float | None]:
    """Price a European option using the closed-form Black-Scholes model."""
    return price(
        EuropeanOption(kind, strike, expiry),
        Market(spot, risk_free_rate, dividend_yield, volatility, valuation_date),
    )
