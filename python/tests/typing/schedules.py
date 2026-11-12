"""Explicit schedule construction uses the same timestamp types as generated schedules."""

from datetime import datetime
from typing import assert_type

from kiyosi.instruments import (
    Accumulator,
    BarrierOption,
    BinaryBarrierOption,
    BinarySnowballOption,
    DoubleBarrierOption,
    DoubleTouchOption,
    PhoenixOption,
    RangeAccrualOption,
    SnowballOption,
    TernarySnowballOption,
    TouchOption,
)
from kiyosi.market import ObservationSchedule, TradingCalendar, all_days_calendar


def explicit_schedule() -> None:
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 10)
    schedule = ObservationSchedule(
        times=(time for time in (start, end)),
        start_time=start,
        end_time=end,
        calendar=all_days_calendar(),
    )
    assert_type(schedule.calendar, TradingCalendar)
    assert_type(schedule.times, tuple[datetime, ...])
    assert_type(schedule[0], datetime)
    assert_type(schedule[:], ObservationSchedule)
    assert_type(next(reversed(schedule)), datetime)


def contractual_schedules(
    accrual: RangeAccrualOption,
    accumulator: Accumulator,
    note: SnowballOption | PhoenixOption | TernarySnowballOption,
    binary: BinarySnowballOption,
) -> None:
    assert_type(accrual.observation_schedule, ObservationSchedule)
    assert_type(accumulator.accumulation_schedule, ObservationSchedule)
    assert_type(accumulator.knock_out_observation_schedule, ObservationSchedule)
    assert_type(note.knock_in_observation_schedule, ObservationSchedule)
    assert_type(binary.knock_in_observation_schedule, None)
    assert_type(note.autocall_times, tuple[datetime, ...])
    assert_type(binary.autocall_times, tuple[datetime, ...])
    for at in note.autocall_times:
        assert_type(at, datetime)
    for at in binary.autocall_times:
        assert_type(at, datetime)


def monitoring_schedule(
    option: BarrierOption
    | DoubleBarrierOption
    | BinaryBarrierOption
    | TouchOption
    | DoubleTouchOption,
) -> None:
    schedule = option.observation_schedule
    assert_type(schedule, ObservationSchedule | None)
    if schedule is not None:
        assert_type(schedule.times, tuple[datetime, ...])
    option.replace(observation_times=schedule)
