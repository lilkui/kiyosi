"""Validated derivative instruments."""

from typing import Literal as _Literal

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

AutocallableBarrierState = _Literal["none", "knocked_out", "knocked_in"]
BarrierTouchState = _Literal["untouched", "touched"]
BarrierType = _Literal["up_and_in", "up_and_out", "down_and_in", "down_and_out"]
KnockInObservationMode = _Literal["every_trading_day", "at_expiry"]
ObservationMode = _Literal["continuous", "scheduled"]
OptionType = _Literal["call", "put"]
PayoffType = _Literal["cash", "asset"]
RebateTiming = _Literal["at_hit", "at_expiry"]
SettlementTiming = _Literal["at_hit", "at_expiry"]
