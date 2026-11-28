from datetime import date, datetime
from datetime import time as clock_time

import pytest
from kiyosi import InvalidInputError
from kiyosi.market import (
    ObservationSchedule,
    TradingCalendar,
)
from support.assertions import assert_decimal_equal


def test_overrides_ownership_and_calendar_operations():
    base = TradingCalendar.weekdays()
    closed = [date(2026, 10, 2), date(2026, 10, 1), date(2026, 10, 1)]
    calendar = TradingCalendar.custom(
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
    assert calendar.trading_days_per_year == 261
    start, end = date(2026, 10, 1), date(2026, 10, 12)
    assert calendar.trading_days_between(start, end) == 6
    assert_decimal_equal(calendar.trading_year_fraction(start, end), 6 / 261)
    assert calendar.adjust(start, "following") == date(2026, 10, 5)
    assert list(
        ObservationSchedule.fixed_interval(
            observation_clock_time=clock_time(),
            start_time=datetime(2026, 10, 9),
            end_time=datetime(2026, 10, 11),
            interval_days=1,
            calendar=calendar,
        )
    ) == [datetime(2026, 10, 10)]
    layered = TradingCalendar.custom(
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
    base = TradingCalendar.sse()
    calendar = TradingCalendar.custom(base=base, trading_days_per_year=None)
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
            TradingCalendar.custom(base=TradingCalendar.weekdays(), **kwargs)
        assert type(error.value) is InvalidInputError


def test_representation_errors(subtests):
    for field in ("closed_dates", "open_dates"):
        for value in (None, 1, [datetime(2026, 10, 1)], ["2026-10-01"], [True]):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                TradingCalendar.custom(
                    base=TradingCalendar.weekdays(), **{field: value}
                )
    for value in (True, 252.0, 252.5, "252"):
        with subtests.test(value=value), pytest.raises(TypeError):
            TradingCalendar.custom(
                base=TradingCalendar.weekdays(), trading_days_per_year=value
            )
    with pytest.raises(OverflowError):
        TradingCalendar.custom(
            base=TradingCalendar.weekdays(), trading_days_per_year=2**40
        )
    with pytest.raises(TypeError):
        TradingCalendar.custom()
