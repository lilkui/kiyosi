"""Public calendar and schedule contracts."""

import numbers
import pickle
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import InvalidInputError
from kiyosi.market import (
    ObservationSchedule,
    all_days_calendar,
    custom_calendar,
    fixed_interval_schedule,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)


def test_schedules_retain_calendars_for_contract_composition(subtests):
    from kiyosi import instruments as i
    from test_contract_replacements import contract_inputs

    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    saturday = datetime(2025, 1, 4, 15)
    calendar = custom_calendar(base=weekdays_calendar(), open_dates=[saturday.date()])
    schedule = ObservationSchedule(
        times=[saturday], start_time=start, end_time=end, calendar=calendar
    )
    assert schedule.calendar == calendar
    assert pickle.loads(pickle.dumps(schedule)).calendar == calendar
    empty = ObservationSchedule(
        times=[], start_time=start, end_time=end, calendar=calendar
    )
    assert pickle.loads(pickle.dumps(empty)).calendar == calendar
    copied = ObservationSchedule(times=schedule, start_time=start, end_time=end)
    assert copied == schedule
    for factory, fields in contract_inputs():
        key = (
            "knock_out_observation_times"
            if "knock_out_observation_times" in fields
            else "observation_times"
        )
        if key not in fields:
            continue
        fields = fields | {
            name: None
            for name in ("touch_state", "accumulated_quantity", "knock_out_settled")
            if name in fields
        }
        with subtests.test(factory=factory.__name__):
            original = factory(**(fields | {key: None}))
            inherited = factory(**(fields | {key: schedule}))
            assert inherited.calendar == calendar
            assert (
                factory(**fields | {key: schedule, "calendar": ...}).calendar
                == calendar
            )
            assert original.replace(**{key: schedule}).calendar == calendar
            with pytest.raises(InvalidInputError):
                original.replace(**{key: schedule, "calendar": weekdays_calendar()})
            assert (
                original.replace(
                    **{key: schedule, "calendar": all_days_calendar()}
                ).calendar
                == all_days_calendar()
            )
            with pytest.raises(InvalidInputError):
                factory(**(fields | {key: schedule, "calendar": weekdays_calendar()}))
            override = factory(
                **(fields | {key: schedule, "calendar": all_days_calendar()})
            )
            assert override.calendar == all_days_calendar()
    note = i.standard_snowball(
        coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        knock_out_level=110,
        effective_time=start,
        expiry_time=end,
        autocall_times=schedule,
        knock_in_observation_clock_time=clock_time(15),
    )
    assert note.calendar == calendar
    phoenix = i.fixed_coupon_note(
        coupon_per_period=0.01,
        initial_spot=100,
        knock_in_level=80,
        knock_out_level=110,
        effective_time=start,
        expiry_time=end,
        autocall_times=schedule,
    )
    assert phoenix.calendar == calendar
    for generated in (
        fixed_interval_schedule(
            start_time=start,
            end_time=end,
            interval_days=30,
            observation_clock_time=clock_time(15),
            calendar=calendar,
        ),
        monthly_schedule(
            start_time=start,
            end_time=end,
            first_observation_month=1,
            observation_clock_time=clock_time(15),
            calendar=calendar,
        ),
    ):
        assert generated.calendar == calendar


def test_explicit_schedules_preserve_times_and_roundtrip():
    start, end = datetime(2025, 1, 2, 15), datetime(2025, 1, 10, 15)
    times = [start, datetime(2025, 1, 6, 14, 30, 0, 123456), end]
    schedule = ObservationSchedule(times=iter(times), start_time=start, end_time=end)
    assert list(schedule) == times
    assert schedule[-1] == end
    assert pickle.loads(pickle.dumps(schedule)) == schedule
    times.clear()
    copied = list(schedule.times)
    copied.clear()
    assert len(schedule) == 3


def test_explicit_schedules_validate_calendar_order_and_life(subtests):
    start, end = datetime(2025, 1, 2), datetime(2025, 1, 10)
    cases = (
        ("reversed_life", [], end, start),
        ("duplicate", [start, start], start, end),
        ("unordered", [end, start], start, end),
        ("before_life", [start - timedelta(microseconds=1)], start, end),
        ("after_life", [end + timedelta(microseconds=1)], start, end),
        ("closed_day", [datetime(2025, 1, 4)], start, end),
    )
    for case, times, lower, upper in cases:
        with subtests.test(case=case), pytest.raises(InvalidInputError):
            ObservationSchedule(times=times, start_time=lower, end_time=upper)
    with pytest.raises(TypeError):
        ObservationSchedule(times=[start.date()], start_time=start, end_time=end)


def test_explicit_schedules_accept_empty_and_custom_calendar():
    at = datetime(2025, 1, 4, 15)
    empty = ObservationSchedule(times=[], start_time=at, end_time=at)
    assert list(empty) == []
    assert pickle.loads(pickle.dumps(empty)) == empty
    calendar = custom_calendar(base=weekdays_calendar(), open_dates=[at.date()])
    schedule = ObservationSchedule(
        times=[at], start_time=at, end_time=at, calendar=calendar
    )
    assert list(schedule) == [at]
    assert pickle.loads(pickle.dumps(schedule)) == schedule


def test_equivalent_custom_calendars_compare_equal():
    base = weekdays_calendar()
    closed, opened = date(2026, 10, 1), date(2026, 10, 10)
    left = custom_calendar(base=base, closed_dates=[closed] * 2, open_dates=[opened])
    right = custom_calendar(base=base, closed_dates=[closed], open_dates=[opened] * 2)
    assert left == right
    assert custom_calendar(base=base) == base
    assert custom_calendar(base=base, trading_days_per_year=365) != base
    assert left != custom_calendar(base=base, closed_dates=[closed])
    assert left != custom_calendar(
        base=base,
        closed_dates=[closed],
        open_dates=[opened],
        trading_days_per_year=365,
    )
    nested = custom_calendar(base=left, closed_dates=[opened], open_dates=[closed])
    assert nested == custom_calendar(
        base=right, closed_dates=[opened] * 2, open_dates=[closed]
    )
    assert not (nested.is_trading_day(opened))
    assert nested.is_trading_day(closed)


def test_calendar_amendments_merge_and_serialize_effective_overrides():
    closed, opened = date(2026, 10, 1), date(2026, 10, 10)
    original = custom_calendar(
        base=weekdays_calendar(),
        closed_dates=[closed],
        open_dates=[opened],
        trading_days_per_year=260,
    )
    amended = original
    for _ in range(500):
        amended = custom_calendar(
            base=amended, closed_dates=[opened], open_dates=[closed]
        )
    expected = custom_calendar(
        base=weekdays_calendar(),
        closed_dates=[opened],
        open_dates=[closed],
        trading_days_per_year=260,
    )
    assert amended == expected
    assert pickle.loads(pickle.dumps(amended)) == expected
    assert repr(amended) == repr(expected)
    assert amended.is_trading_day(closed)
    assert not (amended.is_trading_day(opened))
    assert amended.trading_days_per_year == 260
    assert not (original.is_trading_day(closed))
    assert original.is_trading_day(opened)


def test_counts_adjustment_and_annualization():
    calendar = weekdays_calendar()
    start, end = date(2025, 1, 1), date(2025, 1, 6)
    assert calendar.trading_days_between(start, end) == 3
    assert calendar.trading_year_fraction(start, end) == 3 / 252
    assert calendar.trading_days_between(start, start) == 0
    assert calendar.trading_days_between(date(2025, 1, 4), end) == 0
    assert calendar.adjust(date(2025, 1, 5), "following") == end
    assert calendar.adjust(date(2025, 1, 5), "preceding") == date(2025, 1, 3)
    assert calendar.adjust(end, "following") == end
    assert all_days_calendar().trading_days_per_year == 365
    assert all_days_calendar().is_trading_day(date(2025, 1, 5))
    assert all_days_calendar().trading_days_between(start, end) == 5


def test_sse_known_history_forecast_and_fallback():
    calendar = sse_calendar()
    assert calendar.trading_days_per_year == 243
    for value in (
        date(1996, 2, 29),
        date(2026, 2, 23),
        date(2027, 2, 12),
        date(2099, 6, 22),
        date(2031, 1, 4),
    ):
        assert not (calendar.is_trading_day(value)), value
    for value in (date(2026, 2, 24), date(2031, 1, 2)):
        assert calendar.is_trading_day(value), value
    for value in (date(1990, 12, 25), date(2100, 2, 8)):
        assert calendar.is_trading_day(value) == weekdays_calendar().is_trading_day(
            value
        )


def test_schedule_endpoints_clamping_offsets_and_empty_results(subtests):
    fixed = fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 3),
        end_time=datetime(2025, 1, 7),
        interval_days=1,
    )
    assert fixed.times == ((datetime(2025, 1, 6), datetime(2025, 1, 7)))
    assert monthly_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 1),
        end_time=datetime(2025, 3, 1),
        first_observation_month=1,
    ).times == ((datetime(2025, 2, 3),))
    assert monthly_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 31),
        end_time=datetime(2025, 4, 30),
        first_observation_month=1,
    ).times == ((datetime(2025, 2, 28), datetime(2025, 3, 31), datetime(2025, 4, 30)))
    assert monthly_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2024, 1, 31),
        end_time=datetime(2024, 3, 31),
        first_observation_month=1,
        calendar=all_days_calendar(),
    ).times == ((datetime(2024, 2, 29), datetime(2024, 3, 31)))
    assert monthly_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 2),
        end_time=datetime(2025, 4, 2),
        first_observation_month=2,
    ).times == ((datetime(2025, 3, 3), datetime(2025, 4, 2)))
    for start, end in (
        (datetime(2025, 1, 1), datetime(2025, 1, 1)),
        (datetime(2025, 1, 1), datetime(2026, 1, 1)),
        (datetime(9999, 11, 1), datetime(9999, 12, 31)),
    ):
        for interval in (2**31 - 1, 2**31, 2**40, 2**63 - 1):
            with subtests.test(start_time=start, end_time=end, interval=interval):
                assert fixed_interval_schedule(
                    observation_clock_time=clock_time(),
                    start_time=start,
                    end_time=end,
                    interval_days=interval,
                ).times == (())
        assert monthly_schedule(
            observation_clock_time=clock_time(),
            start_time=start,
            end_time=end,
            first_observation_month=2**31 - 1,
        ).times == (())
    assert fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(9999, 12, 30),
        end_time=datetime(9999, 12, 31),
        interval_days=1,
        calendar=all_days_calendar(),
    ).times == ((datetime(9999, 12, 31),))


def test_closures_merge_candidates_and_stop_at_end():
    closed = [date(2025, 1, 1) + timedelta(days=i) for i in range(90)]
    calendar = custom_calendar(base=all_days_calendar(), closed_dates=closed)
    for schedule in (
        monthly_schedule(
            observation_clock_time=clock_time(),
            start_time=datetime(2025, 1, 1),
            end_time=datetime(2025, 4, 2),
            first_observation_month=1,
            calendar=calendar,
        ),
        fixed_interval_schedule(
            observation_clock_time=clock_time(),
            start_time=datetime(2025, 1, 1),
            end_time=datetime(2025, 4, 2),
            interval_days=31,
            calendar=calendar,
        ),
    ):
        assert schedule.times == ((datetime(2025, 4, 1),))
    assert fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 1),
        end_time=datetime(2025, 3, 31),
        interval_days=1,
        calendar=calendar,
    ).times == (())


def test_schedule_iteration_indexing_and_owning_copies():
    schedule = fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 1),
        end_time=datetime(2025, 1, 5),
        interval_days=1,
        calendar=all_days_calendar(),
    )
    expected = [datetime(2025, 1, i) for i in range(2, 6)]
    assert len(schedule) == 4
    assert schedule[0] == expected[0]
    assert schedule[-1] == expected[-1]
    assert schedule[-4] == expected[0]

    class HugeIndex:
        def __index__(self):
            return 10**100

    for index in (4, -5, 10**100, -(10**100), HugeIndex()):
        with pytest.raises(IndexError):
            _ = schedule[index]
    failure = OverflowError("custom index failure")

    class BrokenIndex:
        def __index__(self):
            raise failure

    with pytest.raises(OverflowError) as error:
        _ = schedule[BrokenIndex()]
    assert error.value is failure
    copied = list(schedule.times)
    copied.clear()
    assert schedule.times == tuple(expected)
    with pytest.raises(AttributeError):
        schedule.times = []
    first, second = iter(schedule), schedule.__iter__()
    assert iter(first) is first
    assert next(first) == expected[0]
    del schedule
    assert list(first) == expected[1:]
    assert list(second) == expected
    for _ in range(2):
        with pytest.raises(StopIteration):
            next(first)


def test_schedule_reverse_iteration_preserves_order_and_owns_timestamps(subtests):
    start = datetime(2025, 1, 2, 15, 0, 0, 123456)
    end = datetime(2025, 1, 3, 15)
    for times in ((), (start,), (start, end)):
        with subtests.test(times=times):
            schedule = ObservationSchedule(times=times, start_time=start, end_time=end)
            reverse = reversed(schedule)
            assert iter(reverse) is reverse
            assert schedule.times == times
            del schedule
            assert list(reverse) == list(times)[::-1]
            with pytest.raises(StopIteration):
                next(reverse)


def test_schedule_indices_reject_booleans_and_nonintegers(subtests):
    schedule = fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 1),
        end_time=datetime(2025, 1, 5),
        interval_days=1,
        calendar=all_days_calendar(),
    )

    class Index:
        def __index__(self):
            return -1

    assert schedule[Index()] == datetime(2025, 1, 5)
    for index in (True, False, 1.0, 1.5, "1", None):
        with subtests.test(index=index), pytest.raises(TypeError):
            _ = schedule[index]
    assert list(schedule) == ([datetime(2025, 1, i) for i in range(2, 6)])


def test_schedule_slices_preserve_order_calendar_and_ownership(subtests):
    schedule = fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 1),
        end_time=datetime(2025, 1, 5),
        interval_days=1,
        calendar=all_days_calendar(),
    )
    expected = list(schedule)
    for selection in (
        slice(None),
        slice(1, 3),
        slice(-3, None, 2),
        slice(3, 1),
        slice(-(10**100), 10**100),
        slice(None, None, 10**100),
    ):
        with subtests.test(selection=selection):
            result = schedule[selection]
            assert isinstance(result, ObservationSchedule)
            assert list(result) == expected[selection]
            assert result.calendar == schedule.calendar
    result = schedule[:]
    assert result is not schedule
    del result
    assert list(schedule) == expected
    assert list(schedule[0:0]) == []
    assert list(schedule[0:0][:]) == []
    assert list(schedule[:: -(10**100)]) == expected[:: -(10**100)]
    with pytest.raises(InvalidInputError):
        _ = schedule[::-1]
    with pytest.raises(ValueError):
        _ = schedule[::0]
    for selection in (
        slice(True),
        slice(None, False),
        slice(None, None, True),
        slice(1.5),
    ):
        with subtests.test(selection=selection), pytest.raises(TypeError):
            _ = schedule[selection]


def test_sliced_schedule_preserves_snowball_monitoring_dates():
    from kiyosi.instruments import standard_snowball

    start, end = datetime(2025, 1, 3), datetime(2025, 1, 10)
    closed = date(2025, 1, 6)
    calendar = custom_calendar(base=weekdays_calendar(), closed_dates=[closed])
    schedule = ObservationSchedule(
        times=[end], start_time=start, end_time=end, calendar=calendar
    )
    note = standard_snowball(
        coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        knock_out_level=110,
        effective_time=start,
        expiry_time=end,
        knock_in_observation_clock_time=clock_time(15),
        autocall_times=schedule[:],
    )
    assert note.calendar == calendar
    assert closed not in ([at.date() for at in note.knock_in_observation_schedule])


def test_validation_errors_and_order():
    start, end = datetime(2025, 1, 1), datetime(2025, 2, 1)
    for factory, field, minimum in (
        (fixed_interval_schedule, "interval_days", -(2**63)),
        (monthly_schedule, "first_observation_month", -(2**31)),
    ):
        for value in (0, -1, minimum):
            with pytest.raises(InvalidInputError) as error:
                factory(
                    observation_clock_time=clock_time(),
                    start_time=start,
                    end_time=end,
                    **{field: value},
                )
            assert type(error.value) is InvalidInputError
            with pytest.raises(InvalidInputError) as error:
                factory(
                    observation_clock_time=clock_time(),
                    start_time=end,
                    end_time=start,
                    **{field: value},
                )
            assert type(error.value) is InvalidInputError
    for operation in (
        weekdays_calendar().trading_days_between,
        weekdays_calendar().trading_year_fraction,
    ):
        with pytest.raises(InvalidInputError) as error:
            operation(end.date(), start.date())
        assert type(error.value) is InvalidInputError


def test_representations_use_native_errors_and_integer_protocol():
    start, end = datetime(2025, 1, 1), datetime(2025, 2, 1)

    @numbers.Integral.register
    class Index:
        def __index__(self):
            return 1

    class IndexOnly:
        def __index__(self):
            return 1

    for factory, field, bits in (
        (fixed_interval_schedule, "interval_days", 63),
        (monthly_schedule, "first_observation_month", 31),
    ):
        for value in (Index(), IndexOnly()):
            assert (
                factory(
                    observation_clock_time=clock_time(),
                    start_time=start,
                    end_time=end,
                    **{field: value},
                ).times
                == factory(
                    observation_clock_time=clock_time(),
                    start_time=start,
                    end_time=end,
                    **{field: 1},
                ).times
            )
        for value in (True, 1.0, 1.5, "1", None):
            with pytest.raises(TypeError):
                factory(
                    observation_clock_time=clock_time(),
                    start_time=start,
                    end_time=end,
                    **{field: value},
                )
        for value in (-(2**bits) - 1, 2**bits):
            with pytest.raises(OverflowError):
                factory(
                    observation_clock_time=clock_time(),
                    start_time=start,
                    end_time=end,
                    **{field: value},
                )
        for value in (date(2025, 1, 1), "2025-01-01", True):
            with pytest.raises(TypeError):
                factory(
                    observation_clock_time=clock_time(),
                    start_time=value,
                    end_time=end,
                    **{field: 1},
                )
    with pytest.raises(ValueError):
        weekdays_calendar().adjust(start.date(), "modified_following")
    with pytest.raises(TypeError):
        weekdays_calendar().adjust(start.date(), True)


def test_shared_and_distinct_calendars_support_concurrent_reads():
    calendar = custom_calendar(
        base=weekdays_calendar(),
        closed_dates=[date(2026, 10, 1)],
        open_dates=[date(2026, 10, 10)],
    )

    def query(shared):
        return shared.trading_days_between(date(2026, 10, 1), date(2026, 10, 12))

    with ThreadPoolExecutor(max_workers=4) as pool:
        assert list(pool.map(query, [calendar] * 8)) == [7] * 8
        assert list(pool.map(query, [weekdays_calendar() for _ in range(8)])) == [7] * 8


def test_date_conversion_boundaries_and_protocol_failures():
    for boundary, convention in ((date.min, "preceding"), (date.max, "following")):
        calendar = custom_calendar(base=weekdays_calendar(), closed_dates=[boundary])
        with pytest.raises(ValueError):
            calendar.adjust(boundary, convention)
        assert not (calendar.is_trading_day(boundary))

    class ConversionFailure(Exception):
        pass

    @numbers.Integral.register
    class BrokenInteger:
        def __index__(self):
            raise ConversionFailure

    with pytest.raises(ConversionFailure):
        fixed_interval_schedule(
            observation_clock_time=clock_time(),
            start_time=datetime(2025, 1, 1),
            end_time=datetime(2025, 2, 1),
            interval_days=BrokenInteger(),
        )

    def broken_dates():
        yield date(2025, 1, 1)
        raise ConversionFailure

    base = weekdays_calendar()
    with pytest.raises(ConversionFailure):
        custom_calendar(base=base, closed_dates=broken_dates())
    assert base.is_trading_day(date(2025, 1, 1))
