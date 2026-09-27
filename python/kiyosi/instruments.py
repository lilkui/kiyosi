"""Validated derivative instruments."""

from typing import Literal

from ._native import (
    Accumulator,
    AmericanOption,
    ArithmeticAveragePriceOption,
    AssetOrNothingOption,
    BarrierOption,
    BinaryBarrierOption,
    BinarySnowballOption,
    CashOrNothingOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    TouchOption,
    asset_binary_barrier_option,
    asset_no_touch_down,
    asset_no_touch_up,
    asset_one_touch_down,
    asset_one_touch_up,
    both_down_snowball,
    cash_binary_barrier_option,
    cash_no_touch_down,
    cash_no_touch_up,
    cash_one_touch_down,
    cash_one_touch_up,
    dual_coupon_snowball,
    european_snowball,
    loss_capped_snowball,
    otm_snowball,
    parachute_snowball,
    standard_snowball,
    step_down_snowball,
)

AutocallableBarrierState = Literal["none", "knocked_out", "knocked_in"]
BarrierTouchState = Literal["untouched", "touched"]
BarrierType = Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
KnockInObservationMode = Literal["every_trading_day", "at_expiry"]
ObservationMode = Literal["continuous", "scheduled"]
OptionType = Literal["call", "put"]
PayoffType = Literal["cash", "asset"]
RebateTiming = Literal["at_hit", "at_expiry"]
SettlementTiming = Literal["at_hit", "at_expiry"]

__all__ = [
    "Accumulator",
    "AmericanOption",
    "ArithmeticAveragePriceOption",
    "AssetOrNothingOption",
    "AutocallableBarrierState",
    "BarrierOption",
    "BarrierTouchState",
    "BarrierType",
    "BinaryBarrierOption",
    "BinarySnowballOption",
    "CashOrNothingOption",
    "EuropeanOption",
    "GeometricAveragePriceOption",
    "KnockInObservationMode",
    "ObservationMode",
    "OptionType",
    "PayoffType",
    "PhoenixOption",
    "RebateTiming",
    "SettlementTiming",
    "SnowballOption",
    "TernarySnowballOption",
    "TouchOption",
    "asset_binary_barrier_option",
    "asset_no_touch_down",
    "asset_no_touch_up",
    "asset_one_touch_down",
    "asset_one_touch_up",
    "both_down_snowball",
    "cash_binary_barrier_option",
    "cash_no_touch_down",
    "cash_no_touch_up",
    "cash_one_touch_down",
    "cash_one_touch_up",
    "dual_coupon_snowball",
    "european_snowball",
    "loss_capped_snowball",
    "otm_snowball",
    "parachute_snowball",
    "standard_snowball",
    "step_down_snowball",
]
