"""Market snapshots, calendars, and observation schedules.

Rates and volatility use annual decimal units (0.05 means 5%). Rate curves
use Actual/365 Fixed time; a calendar's trading-day basis applies only to
trading-year fractions. ``BusinessDayConvention`` accepts ``"following"``
or ``"preceding"``, including the supplied date when it is open.
Model and context ``replace()`` methods expose named keyword-only fields.
Omitted fields retain their values; explicit ``None`` is rejected.

Examples
--------
Build monthly observations from a month-end anchor. February is clamped
without moving the March anchor:

>>> from datetime import date
>>> schedule = monthly_schedule(
...     start=date(2025, 1, 31), end=date(2025, 3, 31),
...     first_observation_month=1, calendar=all_days_calendar(),
... )
>>> schedule.dates
[datetime.date(2025, 2, 28), datetime.date(2025, 3, 31)]

Combine a discount curve with a scalar dividend yield:

>>> curve = RateTermStructure(
...     reference_time=date(2025, 1, 1), times=[date(2026, 1, 1)],
...     factors=[0.95],
... )
>>> model = BlackScholesMertonParameters(
...     risk_free_rate=curve, dividend_yield=0.02, volatility=0.2,
... )
>>> context = PricingContext(
...     model_parameters=model, spot_price=100, valuation_time=date(2025, 1, 1),
... )
>>> round(context.model_parameters.risk_free_curve.factor(
...     date(2025, 1, 1), date(2026, 1, 1),
... ), 2)
0.95
"""

from typing import Literal

from ._native import (
    BlackScholesMertonParameters,
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
