"""Idiomatic Python access to kiyosi's closed-form European option pricer."""

from dataclasses import dataclass
from datetime import date
from typing import Any

from . import _native

ErrorCategory = _native.ErrorCategory

__all__ = ["ErrorCategory", "EuropeanOption", "KiyosiError", "Market", "black_scholes", "price"]


class KiyosiError(Exception):
    """A domain error reported by the C++ pricing core."""

    def __init__(self, category: ErrorCategory | int, message: str) -> None:
        self.category = category
        super().__init__(message)


def _number(value: Any, field: str) -> float:
    if isinstance(value, bool):
        raise TypeError(f"{field} must be a finite number")
    try:
        result = float(value)
    except (TypeError, ValueError, OverflowError) as error:
        raise ValueError(f"{field} must be a finite number") from error
    return result


def _calendar_date(value: date, field: str) -> date:
    if type(value) is not date:
        raise ValueError(f"{field} must be a datetime.date")
    return value


def _kind(value: str) -> str:
    if not isinstance(value, str):
        raise TypeError("kind must be a string")
    return value


@dataclass(frozen=True, slots=True)
class EuropeanOption:
    """A European vanilla call or put contract."""

    kind: str
    strike: float
    expiry: date
    effective: date


@dataclass(frozen=True, slots=True)
class Market:
    """Market inputs for Black-Scholes pricing."""

    spot: float
    risk_free_rate: float
    dividend_yield: float
    volatility: float
    valuation_date: date


def price(option: EuropeanOption, market: Market) -> dict[str, float | None]:
    """Price an option and return its premium and standard Greeks."""
    if not isinstance(option, EuropeanOption):
        raise TypeError("option must be a EuropeanOption")
    if not isinstance(market, Market):
        raise TypeError("market must be a Market")
    kind = _kind(option.kind)
    strike = _number(option.strike, "strike")
    expiry = _calendar_date(option.expiry, "expiry")
    valuation = _calendar_date(market.valuation_date, "valuation_date")
    spot = _number(market.spot, "spot")
    risk_free_rate = _number(market.risk_free_rate, "risk_free_rate")
    dividend_yield = _number(market.dividend_yield, "dividend_yield")
    volatility = _number(market.volatility, "volatility")
    effective = _calendar_date(option.effective, "effective")
    result = _native.price(
        kind, spot, strike,
        valuation.year, valuation.month, valuation.day,
        expiry.year, expiry.month, expiry.day,
        risk_free_rate, dividend_yield, volatility,
        effective.year, effective.month, effective.day,
    )
    if result.get("__kiyosi_error__"):
        raise KiyosiError(result["category"], str(result["message"]))
    return result


def black_scholes(
    kind: str,
    spot: float,
    strike: float,
    valuation_date: date,
    expiry: date,
    risk_free_rate: float,
    dividend_yield: float,
    volatility: float,
    effective: date,
) -> dict[str, float | None]:
    """Price a European option using the closed-form Black-Scholes model."""
    return price(
        EuropeanOption(kind, strike, expiry, effective),
        Market(spot, risk_free_rate, dividend_yield, volatility, valuation_date),
    )
