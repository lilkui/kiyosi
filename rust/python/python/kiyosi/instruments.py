"""Validated vanilla, digital, single-barrier and continuous-average Asian contracts."""

from typing import Literal

from ._native import (
    AmericanOption,
    ArithmeticAveragePriceOption,
    AssetOrNothingOption,
    BarrierOption,
    CashOrNothingOption,
    EuropeanOption,
    GeometricAveragePriceOption,
)

OptionType = Literal["call", "put"]

__all__ = [
    "AmericanOption",
    "ArithmeticAveragePriceOption",
    "AssetOrNothingOption",
    "BarrierOption",
    "CashOrNothingOption",
    "EuropeanOption",
    "GeometricAveragePriceOption",
    "OptionType",
]
