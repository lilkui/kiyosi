"""Validated derivative contracts and autocallable presets.

Notes
-----
Constructors validate terms and contractual calendars. Pricing checks valuation
and prior history. Contract boundaries are inclusive. European vanilla and
digital options permit pre-effective valuation with model coverage through
expiry; other contracts require valuation within their life.

Barrier and touch contracts, range accruals, accumulators, and autocallables own
their calendars. Constructors inherit an ``ObservationSchedule`` calendar;
explicit calendars override it, and plain timestamp iterables default to weekdays.
Replacement inherits a supplied schedule's calendar, otherwise retaining the
contract's calendar.
Explicit observations use exact timestamps; automatic observations require
``observation_clock_time``. Daily knock-in and accumulation include the effective
instant; range accrual observations exclude it. Settlement adds no observation.
Barrier and touch contracts use ``observation_times=None`` for continuous
monitoring and a nonempty iterable for scheduled monitoring. Their read-only
``observation_mode`` property reports the selected mode. Both ``observation_times``
and ``observation_schedule`` return ``None`` for continuous monitoring. A scheduled
contract's ``observation_schedule`` can be passed as ``observation_times`` with its calendar intact.
Timestamp tuples contain no calendar; forward the contract's calendar with them.

Supplied history requires ``history_time`` and covers events strictly before it.
Current observations use the current spot. Discrete history can be reused until
the next observation; continuous barrier and Asian history require the same
instant unless monitoring is resolved. Refresh payload and timestamp together
with ``replace_history``. Numerical time sensitivities hold history fixed.
Changing a dated snapshot's timestamp requires explicitly supplying its history
payloads, even when their values stay the same.

``replace`` returns a validated copy. Omission or Ellipsis retains a field;
``None`` clears nullable fields. Changes affecting past observations require an
explicit history choice; schedule edits confined to future events retain dated
history. Accumulators require choices for both quantity and knock-out settlement.
Clearing the final history payload clears an omitted timestamp; an explicit
timestamp is validated and retained.

Use `AutocallableObservation.snowball` or `AutocallableObservation.phoenix` to
construct product-specific observations. `PhoenixOption` accepts a finite signed
``coupon_per_period`` rate applied at each qualifying observation without time accrual. Principal redemption is
per unit of original notional; scale the complete price and Greeks for position
size.

Phoenix ``lockout_periods`` disables knock-out for the first specified number of
observations, while coupons and knock-in remain active. At least one observation
must allow knock-out. ``standard_phoenix`` uses a constant knock-out barrier;
``step_down_phoenix`` starts its absolute barrier ladder at the first eligible
observation. Both use daily knock-in and initial spot as the upper loss strike.
Replacing the lockout changes eligibility without regenerating barrier levels.

`cash_touch_option` and `asset_touch_option` pay on a touch and require an
``up_and_in`` or ``down_and_in`` barrier. `cash_no_touch_option` and
`asset_no_touch_option` pay when the barrier is never reached and require
``up_and_out`` or ``down_and_out``. `TouchOption` accepts either payment condition.

Examples
--------
Edit a preset's observation and maturity coupons together:

>>> from datetime import datetime, time
>>> note = standard_snowball(
...     coupon_rate=0.12, initial_spot=100, knock_in_level=80, knock_out_level=110,
...     effective_time=datetime(2025, 1, 2), expiry_time=datetime(2026, 1, 2),
...     observation_times=[datetime(2026, 1, 2)], observation_clock_time=time(15),
... )
>>> edited = note.with_quoted_coupon(
...     first_observation_coupon=0.15, quote_convention="shift_maturity_coupon",
... )
>>> round(edited.knock_out_coupon_rates[0], 6), round(edited.maturity_coupon_rate, 6)
(0.15, 0.15)
>>> note.maturity_coupon_rate
0.12

Refresh a continuous average and its snapshot timestamp together, then price it:

>>> from datetime import datetime
>>> from kiyosi.market import BlackScholesMertonParameters, PricingContext
>>> from kiyosi.pricing import AnalyticGeometricAveragePriceEngine
>>> average = GeometricAveragePriceOption(
...     option_type="call", strike=100, effective_time=datetime(2025, 1, 1),
...     averaging_start_time=datetime(2025, 1, 1), expiry_time=datetime(2026, 1, 1),
... )
>>> at = datetime(2025, 2, 1)
>>> seasoned = average.replace_history(realized_average=105, history_time=at)
>>> context = PricingContext(
...     model_parameters=BlackScholesMertonParameters(
...         risk_free_rate=0, dividend_yield=0, volatility=0),
...     spot_price=105, valuation_time=at,
... )
>>> round(AnalyticGeometricAveragePriceEngine().price(seasoned, context), 6)
5.0
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
    asset_no_touch_option,
    asset_touch_option,
    autocallable_observations,
    both_down_snowball,
    butterfly_snowball,
    cash_binary_barrier_option,
    cash_double_no_touch,
    cash_double_touch,
    cash_no_touch_option,
    cash_touch_option,
    digital_coupon_note,
    dual_coupon_snowball,
    early_profit_snowball,
    european_snowball,
    fixed_coupon_note,
    loss_capped_snowball,
    otm_snowball,
    parachute_snowball,
    standard_phoenix,
    standard_snowball,
    step_down_phoenix,
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
    "asset_no_touch_option",
    "asset_touch_option",
    "autocallable_observations",
    "both_down_snowball",
    "butterfly_snowball",
    "cash_binary_barrier_option",
    "cash_double_no_touch",
    "cash_double_touch",
    "cash_no_touch_option",
    "cash_touch_option",
    "digital_coupon_note",
    "dual_coupon_snowball",
    "early_profit_snowball",
    "european_snowball",
    "fixed_coupon_note",
    "loss_capped_snowball",
    "otm_snowball",
    "parachute_snowball",
    "standard_phoenix",
    "standard_snowball",
    "step_down_phoenix",
    "step_down_snowball",
]
