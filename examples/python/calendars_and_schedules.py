"""Build calendars and observation schedules with explicit observation clocks."""

from datetime import date, datetime, time

from kiyosi.market import (
    ObservationSchedule,
    custom_calendar,
    fixed_interval_schedule,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)

calendar = sse_calendar()
holiday = date(2025, 1, 1)
print(f"元旦是否交易：{calendar.is_trading_day(holiday)}")
print(f"顺延至交易日：{calendar.adjust(holiday, 'following')}")

# 日历只分类或调整日期；不会自动修改合约的生效、到期时间。
start, end = datetime(2025, 1, 2, 15), datetime(2025, 7, 2, 15)
monthly = monthly_schedule(
    start_time=start,
    end_time=end,
    first_observation_month=1,
    observation_clock_time=time(15),
    calendar=calendar,
)
print("月度观察日程：", list(monthly))
fortnightly = fixed_interval_schedule(
    start_time=start,
    end_time=end,
    interval_days=14,
    observation_clock_time=time(15),
    calendar=calendar,
)
print("每 14 个自然日锚定的观察日程：", list(fortnightly))

# 自定义日历可覆盖基础日历中的开、休市日期；此处仅演示覆盖机制。
custom = custom_calendar(
    base=weekdays_calendar(),
    closed_dates=[date(2025, 1, 3)],
    open_dates=[date(2025, 1, 4)],
)
print(f"自定义周六开市：{custom.is_trading_day(date(2025, 1, 4))}")

# 不规则观察时间也可显式验证；日程可迭代、索引和切片。
irregular = ObservationSchedule(
    times=[datetime(2025, 1, 2, 15), datetime(2025, 1, 4, 14, 30)],
    start_time=start,
    end_time=end,
    calendar=custom,
)
print("不规则观察日程：", list(irregular))
assert calendar.adjust(holiday, "following") == date(2025, 1, 2)
assert all(
    calendar.is_trading_day(at.date()) and at.time() == time(15) for at in monthly
)
