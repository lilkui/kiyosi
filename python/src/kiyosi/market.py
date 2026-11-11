"""Market snapshots, rate curves, calendars, and observation schedules.

Notes
-----
Rates use annual decimal units and continuous compounding; volatility is
annualized in decimal units. Rate curves use Actual/365 Fixed. Calendar
trading-year fractions use the calendar's annualization basis.

`BlackScholesMertonParameters` accepts nonnegative volatility; zero selects
deterministic pricing. `LocalVolatilityParameters` uses an absolute time/spot
surface. `HestonParameters` uses risk-neutral variance parameters. Engine
support and numerical controls are documented on each engine.

Use ``risk_free_curve`` and ``dividend_curve`` to preserve coverage when passing
model rates to another constructor. Scalar rate queries discard coverage bounds.
Use `RateTermStructure.bounded_flat` when exact flatness and coverage bounds
are required; rounded node factors may imply slightly different forward rates.

Calendars classify dates without moving contract boundaries. Automatic
schedules require an explicit observation clock. `ObservationSchedule` supports
integer indexing, slicing, and ``reversed(schedule)``. Reverse iteration owns a
copy of the timestamps. Slices return independent schedules with the same
calendar and reject decreasing selections. Its ``calendar``
property preserves the validation calendar; plain lists and tuples contain no
calendar. Schedule-taking constructors inherit this calendar unless explicitly
overridden.

Examples
--------
Validate an irregular schedule with exact observation times:

>>> from datetime import datetime
>>> times = [datetime(2025, 1, 3, 15), datetime(2025, 1, 8, 14, 30)]
>>> schedule = ObservationSchedule(
...     times=times, start_time=datetime(2025, 1, 1),
...     end_time=datetime(2025, 1, 10),
... )
>>> list(schedule) == times
True

Build monthly observations from a month-end anchor. February is clamped
without moving the March anchor:

>>> from datetime import datetime, time
>>> schedule = monthly_schedule(
...     start_time=datetime(2025, 1, 31), end_time=datetime(2025, 3, 31),
...     first_observation_month=1, observation_clock_time=time(0),
...     calendar=all_days_calendar(),
... )
>>> list(schedule)
[datetime.datetime(2025, 2, 28, 0, 0), datetime.datetime(2025, 3, 31, 0, 0)]
"""

from typing import Literal

from ._native import (
    BlackScholesMertonParameters,
    HestonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    ObservationSchedule,
    PricingContext,
    RateTermStructure,
    TradingCalendar,
    all_days_calendar,
    custom_calendar,
    fixed_interval_schedule,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)

BusinessDayConvention = Literal["following", "preceding"]

__all__ = [
    "BlackScholesMertonParameters",
    "BusinessDayConvention",
    "HestonParameters",
    "LocalVolatilityParameters",
    "LocalVolatilitySurface",
    "ObservationSchedule",
    "PricingContext",
    "RateTermStructure",
    "TradingCalendar",
    "all_days_calendar",
    "custom_calendar",
    "fixed_interval_schedule",
    "monthly_schedule",
    "sse_calendar",
    "weekdays_calendar",
]
