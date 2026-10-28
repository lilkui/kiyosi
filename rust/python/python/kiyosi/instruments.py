"""Validated vanilla, European digital and continuous-average Asian contracts."""

from typing import Literal

from ._native import (
    AmericanOption,
    ArithmeticAveragePriceOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
    GeometricAveragePriceOption,
)

OptionType = Literal["call", "put"]

__all__ = [
    "AmericanOption",
    "ArithmeticAveragePriceOption",
    "AssetOrNothingOption",
    "CashOrNothingOption",
    "EuropeanOption",
    "GeometricAveragePriceOption",
    "OptionType",
]
