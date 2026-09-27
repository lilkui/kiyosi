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
    sse_calendar_data_first_year,
    sse_calendar_data_last_year,
    sse_calendar_data_version,
    weekdays_calendar,
)

BusinessDayConvention = _Literal["following", "preceding"]
