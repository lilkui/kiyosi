"""Validated derivative contracts, autocallable presets and observation adapters.

``OptionType`` selects ``"call"`` or ``"put"``. Boundary arguments and properties
``effective_midnight_utc`` and ``expiry_midnight_utc`` are dates interpreted at
00:00 UTC, both included. They accept ``datetime.date``, excluding datetime.
An expiry date does not include the rest of that day. These are fixed UTC
boundaries, independent of the trading calendar and the caller's time zone.
Date-valued valuation inputs mean midnight UTC; aware datetimes are converted
to UTC before checking contract life. For example, noon UTC on expiry_midnight_utc
is outside contract life, as is midnight UTC+08:00 on effective_midnight_utc.
Contract dates cannot specify an intraday expiry. Constructors validate terms; pricing validates
their consistency with the market snapshot, calendar and supplied history.
History describes events strictly before valuation; a current observation
is evaluated from the current spot. ``replace()`` replaces constructor terms on vanilla and digital contracts.
``replace_history()`` updates only prior history on path-dependent contracts.
Both return new contracts and expose named keyword-only fields through ``inspect.signature``.
Autocallable ``with_coupon(coupon, ...)`` returns a new note using the same
quote convention and coupon units as ``pricing.implied_coupon``. Snowballs
require ``quote_convention``; Phoenix preserves annual or per-period units.
Omitted fields, or the displayed default ``...`` (``Ellipsis``), retain their
values. Explicit ``None`` clears nullable history and is rejected for fields
that require a value.
For autocallables, ``barrier_state=None`` means unknown history;
``barrier_state="no_event"`` records known absence of prior events. Accumulators
use ``accumulated_quantity=None`` for unknown history and require an explicit
quantity after a strictly prior trading-date observation; zero means no outstanding units.
Plain snowball, Phoenix and ternary snowball constructors default knock-in
monitoring to ``"every_trading_day"``, matching Rust terms.

``BinaryBarrierOption``, ``TouchOption`` and ``DoubleTouchOption`` are factory-only
result types; construct them with the corresponding cash/asset factory functions.

``fixed_coupon_note`` and ``digital_coupon_note`` return Phoenix contracts
with fixed ``coupon_per_period`` and expiry-only knock-in. Explicit Phoenix
construction requires exactly one of annual ``coupon_rate`` or fixed
``coupon_per_period``; the unused property is None. ``early_profit_snowball``
and ``butterfly_snowball`` use two or three annual coupon stages with inclusive
cutoff dates. Knock-out applies the selected rate over the whole holding period;
maturity uses the final stage rate.

Examples
--------
Construct a digital call, then create an independent strike replacement.
Cash and asset digitals pay zero when the expiry spot equals the strike:

>>> from datetime import date
>>> option = CashOrNothingOption(
...     option_type="call", strike=100, payout=10,
...     effective_midnight_utc=date(2025, 1, 1), expiry_midnight_utc=date(2026, 1, 1),
... )
>>> replacement = option.replace(strike=105)
>>> (option.strike, replacement.strike, replacement.payout)
(100.0, 105.0, 10.0)

Construct a cash no-touch contract using the same barrier choices as barrier options.
Use ``asset_touch_option`` for asset settlement. At-hit settlement is available
only for one-touch contracts (``up_and_in`` or ``down_and_in``):

>>> touch = cash_touch_option(
...     effective_midnight_utc=date(2025, 1, 1), expiry_midnight_utc=date(2026, 1, 1),
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
    butterfly_snowball,
    cash_binary_barrier_option,
    cash_double_no_touch,
    cash_double_touch,
    cash_touch_option,
    digital_coupon_note,
    dual_coupon_snowball,
    early_profit_snowball,
    european_snowball,
    fixed_coupon_note,
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
    "butterfly_snowball",
    "cash_binary_barrier_option",
    "cash_double_no_touch",
    "cash_double_touch",
    "cash_touch_option",
    "digital_coupon_note",
    "dual_coupon_snowball",
    "early_profit_snowball",
    "european_snowball",
    "fixed_coupon_note",
    "loss_capped_snowball",
    "otm_snowball",
    "parachute_snowball",
    "standard_snowball",
    "step_down_snowball",
]
