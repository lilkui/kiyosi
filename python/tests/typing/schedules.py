"""Explicit schedule construction uses the same timestamp types as generated schedules."""

from datetime import datetime
from typing import assert_type

from kiyosi.instruments import (
    Accumulator,
    BinarySnowballOption,
    PhoenixOption,
    RangeAccrualOption,
    SnowballOption,
    TernarySnowballOption,
)
from kiyosi.market import ObservationSchedule, all_days_calendar


def explicit_schedule() -> None:
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 10)
    schedule = ObservationSchedule(
        times=(time for time in (start, end)),
        start_time=start,
        end_time=end,
        calendar=all_days_calendar(),
    )
    assert_type(schedule.times, list[datetime])
    assert_type(schedule[0], datetime)
    assert_type(schedule[::-1], list[datetime])


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
