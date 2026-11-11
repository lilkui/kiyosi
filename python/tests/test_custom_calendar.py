from datetime import date, datetime
from datetime import time as clock_time

import pytest
from kiyosi import InvalidInputError
from kiyosi.market import (
    custom_calendar,
    fixed_interval_schedule,
    sse_calendar,
    weekdays_calendar,
)
from support.assertions import assert_decimal_equal


def test_overrides_ownership_and_calendar_operations():
    base = weekdays_calendar()
    closed = [date(2026, 10, 2), date(2026, 10, 1), date(2026, 10, 1)]
    calendar = custom_calendar(
        base=base,
        closed_dates=closed,
        open_dates=(date(2026, 10, 10) for _ in range(2)),
    )
    closed.clear()
    for day, expected in (
        (1, False),
        (2, False),
        (9, True),
        (10, True),
        (11, False),
    ):
        assert calendar.is_trading_day(date(2026, 10, day)) == expected
    assert base.is_trading_day(date(2026, 10, 1))
    assert not (base.is_trading_day(date(2026, 10, 10)))
    assert calendar.trading_days_per_year == 252
    start, end = date(2026, 10, 1), date(2026, 10, 12)
    assert calendar.trading_days_between(start, end) == 6
    assert_decimal_equal(calendar.trading_year_fraction(start, end), 6 / 252)
    assert calendar.adjust(start, "following") == date(2026, 10, 5)
    assert list(
        fixed_interval_schedule(
            observation_clock_time=clock_time(),
            start_time=datetime(2026, 10, 9),
            end_time=datetime(2026, 10, 11),
            interval_days=1,
            calendar=calendar,
        )
    ) == [datetime(2026, 10, 10)]
    layered = custom_calendar(
        base=calendar,
        closed_dates=[date(2026, 10, 10)],
        open_dates=[start],
        trading_days_per_year=365,
    )
    del calendar
    assert layered.is_trading_day(start)
    assert not (layered.is_trading_day(date(2026, 10, 10)))
    assert layered.trading_days_per_year == 365


def test_empty_overrides_inherit_base():
    base = sse_calendar()
    calendar = custom_calendar(base=base, trading_days_per_year=None)
    assert calendar.trading_days_per_year == base.trading_days_per_year
    assert calendar.is_trading_day(date(2026, 10, 1)) == base.is_trading_day(
        date(2026, 10, 1)
    )


def test_domain_errors(subtests):
    for kwargs in (
        {"closed_dates": [date(2026, 10, 1)], "open_dates": [date(2026, 10, 1)]},
        {"trading_days_per_year": 0},
        {"trading_days_per_year": -1},
    ):
        with (
            subtests.test(kwargs=kwargs),
            pytest.raises(InvalidInputError) as error,
        ):
            custom_calendar(base=weekdays_calendar(), **kwargs)
        assert type(error.value) is InvalidInputError


def test_representation_errors(subtests):
    for field in ("closed_dates", "open_dates"):
        for value in (None, 1, [datetime(2026, 10, 1)], ["2026-10-01"], [True]):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                custom_calendar(base=weekdays_calendar(), **{field: value})
    for value in (True, 252.0, 252.5, "252"):
        with subtests.test(value=value), pytest.raises(TypeError):
            custom_calendar(base=weekdays_calendar(), trading_days_per_year=value)
    with pytest.raises(OverflowError):
        custom_calendar(base=weekdays_calendar(), trading_days_per_year=2**40)
    with pytest.raises(TypeError):
        custom_calendar()
