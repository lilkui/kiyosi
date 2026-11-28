"""Market snapshots, rate curves, calendars, and observation schedules.

Notes
-----
Rates use annual decimal units and continuous compounding; volatility is
annualized in decimal units under the variance time convention. Models default to
``variance_time_convention="actual_365_fixed"``; BSM and local-volatility PDE models also
accept ``variance_time_convention="business", calendar=...``. Business time uses the
calendar's fixed basis and advances uniformly over trading dates, stopping on
closed dates. Rates and continuous dividend yields use Actual/365 Fixed by
default. Set ``calendar`` on ``RateTermStructure.bounded_flat`` for
calendar-based accrual independent of the model's variance clock. Changing
the model clock does not convert volatility values.

``BlackScholesMertonParameters`` accepts nonnegative volatility; zero selects
deterministic pricing. ``LocalVolatilityParameters`` uses an absolute time/spot
surface. ``HestonParameters`` uses risk-neutral variance parameters on an
Actual/365 Fixed variance clock. Each engine documents its support and controls.

Read model rates through ``risk_free_curve`` and ``dividend_curve`` for term
structure queries and to preserve coverage when reusing model rates.
Scalar rate queries discard coverage bounds.
Use ``RateTermStructure.bounded_flat`` with the default calendar when you
need an exact constant forward rate with coverage bounds. Rounded node factors
may imply slightly different forward rates.

Calendars classify dates without moving contract boundaries. Automatic
schedules require an explicit observation clock. ``ObservationSchedule`` supports
integer indexing, slicing, and ``reversed(schedule)``. Forward and reverse
iteration yield timestamps lazily and retain the schedule owner. Slices return
independent schedules with the same calendar and reject decreasing selections.
The ``calendar`` property preserves the validation calendar; plain lists and
tuples contain no calendar. Schedule-taking constructors inherit this calendar
unless explicitly overridden.

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
>>> schedule = ObservationSchedule.monthly(
...     start_time=datetime(2025, 1, 31), end_time=datetime(2025, 3, 31),
...     first_observation_month=1, observation_clock_time=time(0),
...     calendar=TradingCalendar.all_days(),
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
]
