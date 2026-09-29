"""Market snapshots, calendars, and observation schedules."""

from typing import Literal as _Literal

from ._native import (
    BlackScholesMertonParameters,
    ObservationSchedule,
    PricingContext,
    TradingCalendar,
    all_days_calendar,
    fixed_interval_schedule,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)

BusinessDayConvention = _Literal["following", "preceding"]
