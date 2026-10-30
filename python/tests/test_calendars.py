"""Public calendar and schedule contracts."""

import numbers
import re
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta
from pathlib import Path

from kiyosi import InvalidInputError
from kiyosi.market import (
    all_days_calendar,
    custom_calendar,
    fixed_interval_schedule,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)
from support.custom_calendar import CustomCalendarTests  # noqa: F401

ROOT = Path(__file__).resolve().parents[2]


class CalendarTests(unittest.TestCase):
    def test_counts_adjustment_and_annualization(self):
        calendar = weekdays_calendar()
        start, end = date(2025, 1, 1), date(2025, 1, 6)
        self.assertEqual(calendar.trading_days_between(start, end), 3)
        self.assertEqual(calendar.trading_year_fraction(start, end), 3 / 252)
        self.assertEqual(calendar.trading_days_between(start, start), 0)
        self.assertEqual(calendar.trading_days_between(date(2025, 1, 4), end), 0)
        self.assertEqual(calendar.adjust(date(2025, 1, 5), "following"), end)
        self.assertEqual(
            calendar.adjust(date(2025, 1, 5), "preceding"), date(2025, 1, 3)
        )
        self.assertEqual(calendar.adjust(end, "following"), end)
        self.assertEqual(all_days_calendar().trading_days_per_year, 365)
        self.assertTrue(all_days_calendar().is_trading_day(date(2025, 1, 5)))
        self.assertEqual(all_days_calendar().trading_days_between(start, end), 5)

    def test_sse_known_history_forecast_and_fallback(self):
        calendar = sse_calendar()
        self.assertEqual(calendar.trading_days_per_year, 243)
        for value in (
            date(1996, 2, 29),
            date(2026, 2, 23),
            date(2027, 2, 12),
            date(2099, 6, 22),
            date(2031, 1, 4),
        ):
            self.assertFalse(calendar.is_trading_day(value), value)
        for value in (date(2026, 2, 24), date(2031, 1, 2)):
            self.assertTrue(calendar.is_trading_day(value), value)
        for value in (date(1990, 12, 25), date(2100, 2, 8)):
            self.assertEqual(
                calendar.is_trading_day(value),
                weekdays_calendar().is_trading_day(value),
            )

    def test_committed_generator_data_matches_public_queries(self):
        source = (ROOT / "crates/kiyosi/src/time/sse_holidays.rs").read_text()
        closures = {int(value) for value in re.findall(r"\d{8}", source)}
        calendar = sse_calendar()
        value = date(1991, 1, 1)
        while value < date(2100, 1, 1):
            encoded = value.year * 10000 + value.month * 100 + value.day
            self.assertEqual(
                calendar.is_trading_day(value),
                value.weekday() < 5 and encoded not in closures,
                value,
            )
            value += timedelta(days=1)

    def test_schedule_endpoints_clamping_offsets_and_empty_results(self):
        fixed = fixed_interval_schedule(
            start=date(2025, 1, 3), end=date(2025, 1, 7), interval_days=1
        )
        self.assertEqual(fixed.dates, [date(2025, 1, 6), date(2025, 1, 7)])
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 1), end=date(2025, 3, 1), first_observation_month=1
            ).dates,
            [date(2025, 2, 3)],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 31),
                end=date(2025, 4, 30),
                first_observation_month=1,
            ).dates,
            [date(2025, 2, 28), date(2025, 3, 31), date(2025, 4, 30)],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(2024, 1, 31),
                end=date(2024, 3, 31),
                first_observation_month=1,
                calendar=all_days_calendar(),
            ).dates,
            [date(2024, 2, 29), date(2024, 3, 31)],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 2), end=date(2025, 4, 2), first_observation_month=2
            ).dates,
            [date(2025, 3, 3), date(2025, 4, 2)],
        )
        for start, end in (
            (date(2025, 1, 1), date(2025, 1, 1)),
            (date(2025, 1, 1), date(2026, 1, 1)),
            (date(9999, 11, 1), date.max),
        ):
            for interval in (2**31 - 1, 2**31, 2**40, 2**63 - 1):
                with self.subTest(start=start, end=end, interval=interval):
                    self.assertEqual(
                        fixed_interval_schedule(
                            start=start, end=end, interval_days=interval
                        ).dates,
                        [],
                    )
            self.assertEqual(
                monthly_schedule(
                    start=start, end=end, first_observation_month=2**31 - 1
                ).dates,
                [],
            )
        self.assertEqual(
            fixed_interval_schedule(
                start=date(9999, 12, 30),
                end=date.max,
                interval_days=1,
                calendar=all_days_calendar(),
            ).dates,
            [date.max],
        )

    def test_closures_merge_candidates_and_stop_at_end(self):
        closed = [date(2025, 1, 1) + timedelta(days=i) for i in range(90)]
        calendar = custom_calendar(base=all_days_calendar(), closed_dates=closed)
        for schedule in (
            monthly_schedule(
                start=date(2025, 1, 1),
                end=date(2025, 4, 2),
                first_observation_month=1,
                calendar=calendar,
            ),
            fixed_interval_schedule(
                start=date(2025, 1, 1),
                end=date(2025, 4, 2),
                interval_days=31,
                calendar=calendar,
            ),
        ):
            self.assertEqual(schedule.dates, [date(2025, 4, 1)])
        self.assertEqual(
            fixed_interval_schedule(
                start=date(2025, 1, 1),
                end=date(2025, 3, 31),
                interval_days=1,
                calendar=calendar,
            ).dates,
            [],
        )

    def test_schedule_iteration_indexing_and_owning_copies(self):
        schedule = fixed_interval_schedule(
            start=date(2025, 1, 1),
            end=date(2025, 1, 5),
            interval_days=1,
            calendar=all_days_calendar(),
        )
        expected = [date(2025, 1, i) for i in range(2, 6)]
        self.assertEqual(len(schedule), 4)
        self.assertEqual(schedule[0], expected[0])
        self.assertEqual(schedule[-1], expected[-1])
        self.assertEqual(schedule[-4], expected[0])
        for index in (4, -5):
            with self.assertRaises(IndexError):
                _ = schedule[index]
        copied = schedule.dates
        copied.clear()
        self.assertEqual(schedule.dates, expected)
        with self.assertRaises(AttributeError):
            schedule.dates = []
        first, second = iter(schedule), iter(schedule)
        self.assertIs(iter(first), first)
        self.assertEqual(next(first), expected[0])
        del schedule
        self.assertEqual(list(first), expected[1:])
        self.assertEqual(list(second), expected)
        for _ in range(2):
            with self.assertRaises(StopIteration):
                next(first)

    def test_validation_errors_and_order(self):
        start, end = date(2025, 1, 1), date(2025, 2, 1)
        for factory, field, minimum in (
            (fixed_interval_schedule, "interval_days", -(2**63)),
            (monthly_schedule, "first_observation_month", -(2**31)),
        ):
            for value in (0, -1, minimum):
                with self.assertRaises(InvalidInputError) as error:
                    factory(start=start, end=end, **{field: value})
                self.assertIs(type(error.exception), InvalidInputError)
                with self.assertRaises(InvalidInputError) as error:
                    factory(start=end, end=start, **{field: value})
                self.assertIs(type(error.exception), InvalidInputError)
        for operation in (
            weekdays_calendar().trading_days_between,
            weekdays_calendar().trading_year_fraction,
        ):
            with self.assertRaises(InvalidInputError) as error:
                operation(end, start)
            self.assertIs(type(error.exception), InvalidInputError)

    def test_representations_use_native_errors_and_integer_protocol(self):
        start, end = date(2025, 1, 1), date(2025, 2, 1)

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
            self.assertEqual(
                factory(start=start, end=end, **{field: Index()}).dates,
                factory(start=start, end=end, **{field: 1}).dates,
            )
            for value in (True, 1.0, 1.5, "1", None, IndexOnly()):
                with self.assertRaises(TypeError):
                    factory(start=start, end=end, **{field: value})
            for value in (-(2**bits) - 1, 2**bits):
                with self.assertRaises(OverflowError):
                    factory(start=start, end=end, **{field: value})
            for value in (datetime(2025, 1, 1), "2025-01-01", True):
                with self.assertRaises(TypeError):
                    factory(start=value, end=end, **{field: 1})
        with self.assertRaises(ValueError):
            weekdays_calendar().adjust(start, "modified_following")
        with self.assertRaises(TypeError):
            weekdays_calendar().adjust(start, True)

    def test_shared_and_distinct_calendars_support_concurrent_reads(self):
        calendar = custom_calendar(
            base=weekdays_calendar(),
            closed_dates=[date(2026, 10, 1)],
            open_dates=[date(2026, 10, 10)],
        )

        def query(shared):
            return shared.trading_days_between(date(2026, 10, 1), date(2026, 10, 12))

        with ThreadPoolExecutor(max_workers=4) as pool:
            self.assertEqual(list(pool.map(query, [calendar] * 8)), [7] * 8)
            self.assertEqual(
                list(pool.map(query, [weekdays_calendar() for _ in range(8)])), [7] * 8
            )

    def test_date_conversion_boundaries_and_protocol_failures(self):
        for boundary, convention in ((date.min, "preceding"), (date.max, "following")):
            calendar = custom_calendar(
                base=weekdays_calendar(), closed_dates=[boundary]
            )
            with self.assertRaises(ValueError):
                calendar.adjust(boundary, convention)
            self.assertFalse(calendar.is_trading_day(boundary))

        class ConversionFailure(Exception):
            pass

        @numbers.Integral.register
        class BrokenInteger:
            def __index__(self):
                raise ConversionFailure

        with self.assertRaises(ConversionFailure):
            fixed_interval_schedule(
                start=date(2025, 1, 1),
                end=date(2025, 2, 1),
                interval_days=BrokenInteger(),
            )

        def broken_dates():
            yield date(2025, 1, 1)
            raise ConversionFailure

        base = weekdays_calendar()
        with self.assertRaises(ConversionFailure):
            custom_calendar(base=base, closed_dates=broken_dates())
        self.assertTrue(base.is_trading_day(date(2025, 1, 1)))
