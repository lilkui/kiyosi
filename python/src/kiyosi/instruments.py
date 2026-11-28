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
Raw autocallable note constructors require an explicit ``calendar`` because
observation records carry no calendar. Pass ``calendar=schedule.calendar`` when
building records from a schedule; presets inherit the schedule's calendar.
Replacement inherits a supplied schedule's calendar, otherwise retaining the
contract's calendar.

Explicit observations use exact timestamps; automatic observations require
``observation_clock_time``. Autocallable daily knock-in owns its clock in
``KnockInObservation.every_trading_day``; daily presets accept
``knock_in_observation_clock_time``. Daily knock-in and accumulation include
the effective instant; range accrual observations exclude it. Settlement adds
no observation.
Barrier and touch contracts use ``observation_schedule=None`` for continuous
monitoring and a schedule or nonempty iterable for scheduled monitoring.
Read ``observation_schedule`` to get the contractual times and calendar together,
or ``None`` for continuous monitoring. Pass the same value as
``observation_schedule`` when constructing or replacing another contract.
For direct reads, ``observation_mode`` reports
the mode and ``observation_times`` returns a timestamp tuple or ``None``.
Forward the contract's calendar explicitly when passing a timestamp tuple.

Supplied history describes events strictly before the requested valuation.
Observations exactly at valuation use current spot once. Bookkeeping owns event
records, lifecycle and settlement progression, and consistency with valuation time
and contract terms, including preventing stale-state or future-state misuse.
Kiyosi neither checks state age nor reconstructs history. Missing history is
unknown; explicit zero, false, untouched, and no-event states are known history.
Range accruals accept prior hit counts; accumulators accept prior quantity and
knock-out settlement state.
Asian options accept the realized average for the elapsed averaging interval.
Numerical time sensitivities hold supplied history fixed.

``replace`` returns a validated copy. Omission or Ellipsis retains a field;
``None`` clears nullable fields to unknown. Terms and history are validated
together, and each omitted history field retains its value independently.

Read and replace snowball knock-out terms through ``autocall_observations``,
whose records keep each observation time, knock-out level and coupon together.
Phoenix uses separate ``KnockOutObservation`` and ``CouponObservation`` records,
passed through ``knock_out_observations`` and ``coupon_observations``. Each list
is nonempty and strictly ordered; their times and thresholds are independent.
``knock_out_times`` and ``coupon_times`` provide direct timestamp queries.
Knock-in monitoring is defined independently by ``knock_in_observation`` and
``knock_in_observation_schedule``.

``PhoenixOption`` accepts a finite signed ``coupon_per_period`` rate applied at
each qualifying coupon observation without time accrual. A knock-out-only
observation adds no coupon. Coincident knock-out and coupon barriers are assessed
independently, and expiry adds no implicit observation. Principal redemption is
per unit of original notional; scale the complete price and Greeks for position size.

The four Phoenix preset factories accept one ``autocall_times`` sequence and a
``lockout_periods`` count, defaulting to zero. They expand the entire sequence
into coupon observations and omit the initial count from knock-out observations.
The knock-out list must contain at least one observation. The contract owns both
lists, and replacements edit either list directly. ``PhoenixOption.standard`` uses a
constant knock-out barrier; ``PhoenixOption.step_down`` starts its absolute barrier
ladder at the first generated knock-out observation. Both use daily knock-in
and initial spot as the upper loss strike.

``TouchOption`` pays on a touch with an ``up_and_in`` or ``down_and_in`` barrier,
or when the barrier is never reached with ``up_and_out`` or ``down_and_out``.
Set ``payoff_type`` to ``cash`` or ``asset``; cash settlement requires ``payout``.

Examples
--------
Edit a preset's observation and maturity coupons together:

>>> from datetime import datetime, time
>>> note = SnowballOption.standard(
...     coupon_rate=0.12, initial_spot=100, knock_in_level=80, knock_out_level=110,
...     effective_time=datetime(2025, 1, 2), expiry_time=datetime(2026, 1, 2),
...     autocall_times=[datetime(2026, 1, 2)], knock_in_observation_clock_time=time(15),
... )
>>> edited = note.with_coupon(
...     first_observation_coupon=0.15, quote_convention="shift_maturity_coupon",
... )
>>> first = edited.autocall_observations[0]
>>> round(first.coupon_rate, 6), round(edited.maturity_coupon_rate, 6)
(0.15, 0.15)
>>> note.maturity_coupon_rate
0.12

Supply the realized average for an elapsed averaging interval, then price it:

>>> from datetime import datetime
>>> from kiyosi.market import BlackScholesMertonParameters, PricingContext
>>> from kiyosi.pricing import AnalyticGeometricAveragePriceEngine
>>> average = GeometricAveragePriceOption(
...     option_type="call", strike=100, effective_time=datetime(2025, 1, 1),
...     averaging_start_time=datetime(2025, 1, 1), expiry_time=datetime(2026, 1, 1),
... )
>>> at = datetime(2025, 2, 1)
>>> seasoned = average.replace(realized_average=105)
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
    BarrierOption,
    BinaryBarrierOption,
    BinarySnowballOption,
    CashOrNothingOption,
    CouponObservation,
    DoubleBarrierOption,
    DoubleTouchOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    KnockInObservation,
    KnockOutObservation,
    PhoenixOption,
    RangeAccrualOption,
    SnowballObservation,
    SnowballOption,
    TernarySnowballOption,
    TouchOption,
)

OptionType = Literal["call", "put"]

__all__ = [
    "Accumulator",
    "AmericanOption",
    "ArithmeticAveragePriceOption",
    "AssetOrNothingOption",
    "BarrierOption",
    "BinaryBarrierOption",
    "BinarySnowballOption",
    "CashOrNothingOption",
    "CouponObservation",
    "DoubleBarrierOption",
    "DoubleTouchOption",
    "EuropeanOption",
    "GeometricAveragePriceOption",
    "KnockInObservation",
    "KnockOutObservation",
    "OptionType",
    "PhoenixOption",
    "RangeAccrualOption",
    "SnowballObservation",
    "SnowballOption",
    "TernarySnowballOption",
    "TouchOption",
]
