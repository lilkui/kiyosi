"""Validated European vanilla and digital contracts."""

from typing import Literal

from ._native import AssetOrNothingOption, CashOrNothingOption, EuropeanOption

OptionType = Literal["call", "put"]

__all__ = [
    "AssetOrNothingOption",
    "CashOrNothingOption",
    "EuropeanOption",
    "OptionType",
]
