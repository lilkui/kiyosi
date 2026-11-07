"""Validated derivative contracts, autocallable presets and observation adapters.

``AutocallableObservation.snowball(..., coupon_rate=...)`` supplies snowball
knock-out coupons. ``AutocallableObservation.phoenix(..., coupon_barrier_level=...)``
supplies Phoenix thresholds; the note supplies the coupon amount. The basic
``AutocallableObservation(time, knock_out_level)`` constructor sets both coupon
fields to zero. Mixed nonzero coupon fields are rejected immediately.
Observation constructors and ``autocallable_observations`` validate positive
knock-out levels, finite signed coupon rates and nonnegative coupon barriers
on construction. Notes additionally validate ordering, life bounds and their
product-specific coupon rules. Observation ``replace()`` validates supplied
fields and preserves omitted values, like contract replacement.

``OptionType`` selects ``"call"`` or ``"put"``. Contract effective and expiry
boundaries are inclusive ``datetime.datetime`` values. Microseconds participate
in expiry, observation and elapsed-time calculations. Calendars classify dates
without moving contract boundaries. European vanilla and digital options permit valuation
before effective_time with model coverage through expiry. Other built-in contracts
require valuation within inclusive effective and expiry times, including range accruals.
Finite-difference pricing raises ``ComputationFailureError`` if distinct event times
cannot be represented distinctly by its numerical grid.
Barrier and touch contracts, range accruals, accumulators and autocallable notes own their ``calendar``
(weekdays by default). Supply the contractual calendar when constructing them or
use ``replace(calendar=...)`` to amend an existing validated contract. A contract
with observations on dates closed by the default calendar must
receive the appropriate calendar at construction; replacement cannot repair a
constructor rejection. Replacement, coupon shifts and pickle
preserve the contract calendar.
Inspect a range accrual's ``observation_schedule``, an accumulator's
``accumulation_schedule`` and ``knock_out_observation_schedule``, or an
autocallable's ``knock_in_observation_schedule`` for owned whole-life schedules.
Daily knock-in and accumulation include the effective instant; range accruals
exclude it. Expiry-only knock-in returns one observation; binary snowballs
return None because they have no knock-in. Schedules retain their timestamps
independently of their contract and are not filtered by history.
Explicit observations use their supplied timestamps. Automatic observations require
``observation_clock_time``, a timezone-free ``datetime.time`` with microsecond precision.
They are filtered against the full contract bounds. Snowballs and accumulators
include the effective instant; range accruals exclude it. Maturity settlement adds
no daily observation. Range accruals require a nonempty observation schedule. Explicit accumulator knock-out observations do not add quantity;
coincident events check knock-out before accumulating. Constructors validate terms and contractual calendar compatibility, including
explicit observation dates and expiry-only knock-in observations. Settlement-only
expiry dates may be closed without adding observations. Pricing validates
consistency with the market snapshot and supplied history.
Supplied history requires ``history_time`` at pricing. It describes events strictly
before that timestamp. Set it in the constructor or together with the history through
``replace_history(..., history_time=...)``. Replacements and pickle preserve it.
Future-dated history is rejected.
Reuse between discrete observations is allowed; crossing an unaccounted observation
raises ``InvalidInputError``. Continuous barrier and Asian history require the same
instant, unless a prior barrier hit resolves monitoring. Settled contracts need no
further observations. Numerical time sensitivities hold history fixed internally.
History describes events strictly before valuation; a current observation
is evaluated from the current spot. ``replace()`` replaces constructor terms on every contract. Omitted terms and
history are preserved when their observation terms are retained; supplied terms are validated together in Rust.
Schedule, expiry and calendar edits confined to events at or after ``history_time``
preserve dated history. Changing terms that affect past observations requires an
explicit history value or None to clear it. Such terms include barrier levels/direction,
observation times, averaging start, range bounds and daily monitoring/accumulation rules.
Accumulators require both
``accumulated_quantity`` and ``knock_out_settled``. Reasserting existing history explicitly
is allowed. Changes affecting only future payoff amounts retain history.
For example, ``barrier.replace(strike=105)`` preserves monitoring and history,
and changing monitoring to scheduled requires supplying the observation times in the same call.
Changing barrier or touch monitoring to continuous clears omitted observation times.
Explicit times must agree with the selected mode; scheduled monitoring requires nonempty times.
Phoenix replacements accept at most one non-None coupon choice and can change coupon units.
None leaves a coupon choice unspecified, so both coupon properties can be forwarded.
Double-touch contracts expose ``barrier_type="knock_in"`` or ``"knock_out"``
for inspection and replacement;
binary and touch constructors require explicit ``payoff_type="cash"`` or
``"asset"``. Replacements preserve this kind unless ``payoff_type`` is supplied.
Cash settlement requires a positive payout; ``payout=None`` cannot switch its kind.
``replace_history()`` updates only prior history on path-dependent contracts.
Asian averages use ``realized_average=None`` for unknown history; supplied averages
must be finite and positive. History is required strictly after averaging starts.
``replace()`` and ``replace_history()`` return new contracts and expose named
keyword-only fields through ``inspect.signature``.
Snowball ``with_shifted_coupons(first_observation_coupon, quote_convention=...)``
shifts every observation coupon by the change to the first observation's annual
rate, including past observations. The convention controls the maturity coupon.
It applies the quote returned by ``pricing.implied_snowball_coupon``.
Phoenix ``replace(coupon_rate=...)`` sets an annual rate;
``replace(coupon_per_period=...)`` sets a fixed payment per observation.
``pricing.implied_phoenix_coupon`` returns a value in the contract's existing units.
Apply it with ``with_quoted_coupon(coupon)`` to preserve those units.
Omitted fields, or the displayed default ``...`` (``Ellipsis``), retain their
values. Explicit ``None`` clears nullable history and is rejected for fields
that require a value.
For autocallables, ``barrier_state=None`` means unknown history;
``barrier_state="no_event"`` records known absence of prior events. Accumulators
use ``accumulated_quantity=None`` for unknown history and require an explicit
quantity after a strictly prior accumulation or knock-out observation; zero means no outstanding units.
``knock_out_settled=None`` means unknown knock-out settlement history. After a strictly prior
knock-out observation, supply ``knock_out_settled=False`` for a known active contract
or ``knock_out_settled=True`` for a settled contract, independently of its quantity.
Active accumulators reject positive quantity before a strictly prior accumulation observation.
Set ``knock_out_settled=True`` for a previously settled accumulator knock-out. Pricing
requires a knock-out observation strictly before valuation, including for
settled contracts; an empty knock-out schedule cannot have this history. Its remaining price
is zero; stored quantity is ignored. ``replace_history(knock_out_settled=True)`` records
settlement without changing the original contract. Rust uses the same operation
with ``AccumulatorHistoryChanges`` to replace quantity, knock-out settlement or both;
omitted fields retain their values.
Plain snowball, Phoenix and ternary snowball constructors default knock-in
monitoring to ``"every_trading_day"``, matching Rust terms. Daily monitoring requires
an explicit ``observation_clock_time``; expiry-only monitoring allows an omitted clock or None.
Replacing daily monitoring with ``"at_expiry"`` clears its clock; switching to daily
monitoring requires the clock in the same replacement.

``BinaryBarrierOption``, ``TouchOption`` and ``DoubleTouchOption`` have keyword-only
constructors, like other contracts. Binary barriers and single-touch contracts
require ``payoff_type``: ``"cash"`` requires a positive ``payout``; ``"asset"``
has no cash amount. Double-touch contracts require a cash payout.
Single-touch ``barrier_type`` uses ``up_and_in`` or ``down_and_in`` for one-touch,
and ``up_and_out`` or ``down_and_out`` for no-touch. Double-touch contracts use
``knock_in`` or ``knock_out`` respectively. The named cash/asset factory functions
are convenience forms of these constructors.

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
Pass ``payoff_type="asset"`` for asset settlement. At-hit settlement is available
only for one-touch contracts (``up_and_in`` or ``down_and_in``):

>>> touch = TouchOption(
...     effective_time=datetime(2025, 1, 1), expiry_time=datetime(2026, 1, 1),
...     barrier_level=80, barrier_type="down_and_out", payoff_type="cash", payout=10,
... )
>>> (touch.barrier_type, touch.is_one_touch, touch.settlement_timing)
('down_and_out', False, 'at_expiry')

Construct an asset binary barrier with explicit ``payoff_type="asset"``:

>>> binary = BinaryBarrierOption(
...     option_type="call", strike=100,
...     effective_time=touch.effective_time, expiry_time=touch.expiry_time,
...     barrier_level=120, barrier_type="up_and_in", payoff_type="asset",
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
