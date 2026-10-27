"""Validated vanilla and European digital contracts."""

from typing import Literal

from ._native import (
    AmericanOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)

OptionType = Literal["call", "put"]

__all__ = [
    "AmericanOption",
    "AssetOrNothingOption",
    "CashOrNothingOption",
    "EuropeanOption",
    "OptionType",
]
