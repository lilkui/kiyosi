"""Validated derivative contracts, autocallable presets and observation adapters.

``AutocallableObservation`` and ``autocallable_observations`` validate positive
knock-out levels, finite signed coupon rates and nonnegative coupon barriers
on construction. Notes additionally validate ordering, life bounds and their
product-specific coupon rules.

``OptionType`` selects ``"call"`` or ``"put"``. Contract effective and expiry
boundaries are inclusive ``datetime.datetime`` values. Microseconds participate
in expiry, observation and elapsed-time calculations. Calendars classify dates
without moving contract boundaries. Pricing requires valuation within the
inclusive effective and expiry times for every built-in contract, including range accruals.
Finite-difference pricing raises ``ComputationFailureError`` if distinct event times
cannot be represented distinctly by its numerical grid.
Barrier and touch contracts, range accruals, accumulators and autocallable notes own their ``calendar``
(weekdays by default). Supply the contractual calendar when constructing them or
use ``replace(calendar=...)`` to amend an existing validated contract. A contract
with weekend observations or a weekend expiry that requires a trading day must
receive the appropriate calendar at construction; replacement cannot repair a
constructor rejection. Replacement, coupon shifts and pickle
preserve the contract calendar.
Explicit observations use their supplied times; daily observations use the expiry
clock time. Snowballs and accumulators include the effective instant; range accruals
exclude it. Explicit accumulator knock-out observations do not add quantity;
coincident events check knock-out before accumulating. Constructors validate terms and contractual calendar compatibility, including
explicit observation dates and required trading-day expiries. Pricing validates
consistency with the market snapshot and supplied history.
History describes events strictly before valuation; a current observation
is evaluated from the current spot. ``replace()`` replaces constructor terms on every contract. Omitted terms and
history are preserved; supplied terms are validated together in Rust.
For example, ``barrier.replace(strike=105)`` preserves monitoring and history,
and changing monitoring to scheduled requires supplying the observation times in the same call.
Phoenix replacements accept at most one coupon choice and can change coupon units.
Double-touch contracts expose ``barrier_type="knock_in"`` or ``"knock_out"``
for inspection and replacement;
binary and touch replacements use the constructor's settlement convention:
``payout=None`` selects asset settlement, a positive payout selects cash settlement,
and omission preserves the existing settlement.
``replace_history()`` updates only prior history on path-dependent contracts.
Asian averages use ``realized_average=None`` for unknown history; supplied averages
must be finite and positive. History is required strictly after averaging starts.
Both return new contracts and expose named keyword-only fields through ``inspect.signature``.
Autocallable ``with_coupon(coupon, ...)`` returns a new note using the same
quote convention and coupon units as ``pricing.implied_snowball_coupon`` or ``pricing.implied_phoenix_coupon``. Snowballs
require ``quote_convention``; Phoenix preserves annual or per-period units.
Omitted fields, or the displayed default ``...`` (``Ellipsis``), retain their
values. Explicit ``None`` clears nullable history and is rejected for fields
that require a value.
For autocallables, ``barrier_state=None`` means unknown history;
``barrier_state="no_event"`` records known absence of prior events. Accumulators
use ``accumulated_quantity=None`` for unknown history and require an explicit
quantity after a strictly prior trading-date observation; zero means no outstanding units.
Set ``terminated=True`` for a previously settled accumulator knock-out. Pricing
requires a knock-out observation strictly before valuation, including for
terminated contracts; an empty knock-out schedule cannot have this history. Its remaining price
is zero; stored quantity is ignored. ``replace_history(terminated=True)`` records
settlement without changing the original contract.
Plain snowball, Phoenix and ternary snowball constructors default knock-in
monitoring to ``"every_trading_day"``, matching Rust terms.

``BinaryBarrierOption``, ``TouchOption`` and ``DoubleTouchOption`` have keyword-only
constructors, like other contracts. Binary barriers and single-touch contracts
require ``payout``: a positive cash amount selects cash settlement; explicit
``None`` selects asset settlement. Double-touch contracts require a cash payout.
Single-touch ``barrier_type`` uses ``up_and_in`` or ``down_and_in`` for one-touch,
and ``up_and_out`` or ``down_and_out`` for no-touch. Double-touch contracts use
``knock_in`` or ``knock_out`` respectively. The named cash/asset factory functions
remain convenience forms of these constructors.

``fixed_coupon_note`` and ``digital_coupon_note`` return Phoenix contracts
with fixed ``coupon_per_period`` and expiry-only knock-in. Explicit Phoenix
construction requires exactly one of annual ``coupon_rate`` or fixed
``coupon_per_period``; the unused property is None. ``early_profit_snowball``
and ``butterfly_snowball`` use two or three annual coupon stages with inclusive
cutoff times. Knock-out applies the selected rate over the whole holding period;
maturity uses the final stage rate.

Examples
--------
Construct a digital call, then create an independent strike replacement.
Cash and asset digitals pay zero when the expiry spot equals the strike:

>>> from datetime import datetime
>>> option = CashOrNothingOption(
...     option_type="call", strike=100, payout=10,
...     effective_time=datetime(2025, 1, 1), expiry_time=datetime(2026, 1, 1),
... )
>>> replacement = option.replace(strike=105)
>>> (option.strike, replacement.strike, replacement.payout)
(100.0, 105.0, 10.0)

Construct a cash no-touch contract using the same barrier choices as barrier options.
Pass ``payout=None`` for asset settlement. At-hit settlement is available
only for one-touch contracts (``up_and_in`` or ``down_and_in``):

>>> touch = TouchOption(
...     effective_time=datetime(2025, 1, 1), expiry_time=datetime(2026, 1, 1),
...     barrier_level=80, barrier_type="down_and_out", payout=10,
... )
>>> (touch.barrier_type, touch.is_one_touch, touch.settlement_timing)
('down_and_out', False, 'at_expiry')

Construct an asset binary barrier with explicit ``payout=None``:

>>> binary = BinaryBarrierOption(
...     option_type="call", strike=100,
...     effective_time=touch.effective_time, expiry_time=touch.expiry_time,
...     barrier_level=120, barrier_type="up_and_in", payout=None,
... )
>>> binary.payoff_type
'asset'

Construct a double no-touch with the same class-based interface:

>>> double_no_touch = DoubleTouchOption(
...     effective_time=touch.effective_time, expiry_time=touch.expiry_time,
...     lower_barrier=80, upper_barrier=120, barrier_type="knock_out", payout=10,
... )
>>> double_no_touch.is_one_touch
False

Select an all-days contractual calendar independently of the market snapshot:

>>> from kiyosi.market import all_days_calendar
>>> daily_touch = touch.replace(calendar=all_days_calendar())
>>> daily_touch.calendar == all_days_calendar()
True
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
