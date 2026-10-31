"""Validated derivative contracts, snowball presets and observation adapters.

``OptionType`` selects ``"call"`` or ``"put"``. Contract dates are inclusive,
with expiry at midnight UTC. Constructors validate terms; pricing validates
their consistency with the market snapshot, calendar and supplied history.
History describes events strictly before valuation; a current observation
is evaluated from the current spot. Replacement returns a new contract.
``replace()`` exposes named keyword-only fields through ``inspect.signature``.
Omitted fields, or the displayed default ``...`` (``Ellipsis``), retain their
values. Explicit ``None`` clears nullable history and is rejected for fields
that require a value.

Examples
--------
Construct a digital call, then create an independent strike replacement.
Cash and asset digitals pay zero when the expiry spot equals the strike:

>>> from datetime import date
>>> option = CashOrNothingOption(
...     option_type="call", strike=100, payout=10,
...     effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
... )
>>> replacement = option.replace(strike=105)
>>> (option.strike, replacement.strike, replacement.payout)
(100.0, 105.0, 10.0)

Construct a cash no-touch contract using the same barrier choices as barrier options.
Use ``asset_touch_option`` for asset settlement. At-hit settlement is available
only for one-touch contracts (``up_and_in`` or ``down_and_in``):

>>> touch = cash_touch_option(
...     effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
...     barrier_level=80, barrier_type="down_and_out", payout=10,
... )
>>> (touch.barrier_type, touch.is_one_touch, touch.settlement_timing)
('down_and_out', False, 'at_expiry')
"""

from typing import Literal

from ._native import (
    Accumulator,
    AmericanOption,
    ArithmeticAveragePriceOption,
    AssetOrNothingOption,
    AutocallableObservation,
    BarrierOption,
    BinaryBarrierOption,
    BinarySnowballOption,
    CashOrNothingOption,
    DoubleBarrierOption,
    DoubleTouchOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    PhoenixOption,
    RangeAccrualOption,
    SnowballOption,
    TernarySnowballOption,
    TouchOption,
    asset_binary_barrier_option,
    asset_touch_option,
    autocallable_observations,
    both_down_snowball,
    cash_binary_barrier_option,
    cash_double_no_touch,
    cash_double_touch,
    cash_touch_option,
    dual_coupon_snowball,
    european_snowball,
    loss_capped_snowball,
    otm_snowball,
    parachute_snowball,
    standard_snowball,
    step_down_snowball,
)

OptionType = Literal["call", "put"]

__all__ = [
    "Accumulator",
    "AmericanOption",
    "ArithmeticAveragePriceOption",
    "AssetOrNothingOption",
    "AutocallableObservation",
    "BarrierOption",
    "BinaryBarrierOption",
    "BinarySnowballOption",
    "CashOrNothingOption",
    "DoubleBarrierOption",
    "DoubleTouchOption",
    "EuropeanOption",
    "GeometricAveragePriceOption",
    "OptionType",
    "PhoenixOption",
    "RangeAccrualOption",
    "SnowballOption",
    "TernarySnowballOption",
    "TouchOption",
    "asset_binary_barrier_option",
    "asset_touch_option",
    "autocallable_observations",
    "both_down_snowball",
    "cash_binary_barrier_option",
    "cash_double_no_touch",
    "cash_double_touch",
    "cash_touch_option",
    "dual_coupon_snowball",
    "european_snowball",
    "loss_capped_snowball",
    "otm_snowball",
    "parachute_snowball",
    "standard_snowball",
    "step_down_snowball",
]
