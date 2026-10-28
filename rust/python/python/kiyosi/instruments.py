"""Validated vanilla, digital, barrier, touch and continuous-average Asian contracts."""

from typing import Literal

from ._native import (
    AmericanOption,
    ArithmeticAveragePriceOption,
    AssetOrNothingOption,
    BarrierOption,
    BinaryBarrierOption,
    CashOrNothingOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    TouchOption,
    asset_binary_barrier_option,
    asset_no_touch_down,
    asset_no_touch_up,
    asset_one_touch_down,
    asset_one_touch_up,
    cash_binary_barrier_option,
    cash_no_touch_down,
    cash_no_touch_up,
    cash_one_touch_down,
    cash_one_touch_up,
)

OptionType = Literal["call", "put"]

__all__ = [
    "AmericanOption",
    "ArithmeticAveragePriceOption",
    "AssetOrNothingOption",
    "BarrierOption",
    "BinaryBarrierOption",
    "CashOrNothingOption",
    "EuropeanOption",
    "GeometricAveragePriceOption",
    "OptionType",
    "TouchOption",
    "asset_binary_barrier_option",
    "asset_no_touch_down",
    "asset_no_touch_up",
    "asset_one_touch_down",
    "asset_one_touch_up",
    "cash_binary_barrier_option",
    "cash_no_touch_down",
    "cash_no_touch_up",
    "cash_one_touch_down",
    "cash_one_touch_up",
]
