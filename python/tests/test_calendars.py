"""Public calendar and schedule contracts."""

import numbers
import pickle
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta

from kiyosi import InvalidInputError
from kiyosi.market import (
    BlackScholesMertonParameters,
    ObservationSchedule,
    PricingContext,
    all_days_calendar,
    custom_calendar,
    fixed_interval_schedule,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)
from support.custom_calendar import CustomCalendarTests  # noqa: F401


class CalendarTests(unittest.TestCase):
    def test_explicit_schedules_preserve_times_and_roundtrip(self):
        start, end = datetime(2025, 1, 2, 15), datetime(2025, 1, 10, 15)
        times = [start, datetime(2025, 1, 6, 14, 30, 0, 123456), end]
        schedule = ObservationSchedule(
            times=iter(times), start_time=start, end_time=end
        )
        self.assertEqual(list(schedule), times)
        self.assertEqual(schedule[-1], end)
        self.assertEqual(pickle.loads(pickle.dumps(schedule)), schedule)
        times.clear()
        copied = schedule.times
        copied.clear()
        self.assertEqual(len(schedule), 3)

    def test_explicit_schedules_validate_calendar_order_and_life(self):
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
            with self.subTest(case=case), self.assertRaises(InvalidInputError):
                ObservationSchedule(times=times, start_time=lower, end_time=upper)
        with self.assertRaises(TypeError):
            ObservationSchedule(times=[start.date()], start_time=start, end_time=end)

    def test_explicit_schedules_accept_empty_and_custom_calendar(self):
        at = datetime(2025, 1, 4, 15)
        empty = ObservationSchedule(times=[], start_time=at, end_time=at)
        self.assertEqual(list(empty), [])
        self.assertEqual(pickle.loads(pickle.dumps(empty)), empty)
        calendar = custom_calendar(base=weekdays_calendar(), open_dates=[at.date()])
        schedule = ObservationSchedule(
            times=[at], start_time=at, end_time=at, calendar=calendar
        )
        self.assertEqual(list(schedule), [at])
        self.assertEqual(pickle.loads(pickle.dumps(schedule)), schedule)

    def test_equivalent_custom_calendars_and_contexts_compare_equal(self):
        base = weekdays_calendar()
        closed, opened = date(2026, 10, 1), date(2026, 10, 10)
        left = custom_calendar(
            base=base, closed_dates=[closed] * 2, open_dates=[opened]
        )
        right = custom_calendar(
            base=base, closed_dates=[closed], open_dates=[opened] * 2
        )
        self.assertEqual(left, right)
        self.assertEqual(custom_calendar(base=base), base)
        self.assertNotEqual(custom_calendar(base=base, trading_days_per_year=365), base)
        self.assertNotEqual(left, custom_calendar(base=base, closed_dates=[closed]))
        self.assertNotEqual(
            left,
            custom_calendar(
                base=base,
                closed_dates=[closed],
                open_dates=[opened],
                trading_days_per_year=365,
            ),
        )
        nested = custom_calendar(base=left, closed_dates=[opened], open_dates=[closed])
        self.assertEqual(
            nested,
            custom_calendar(base=right, closed_dates=[opened] * 2, open_dates=[closed]),
        )
        self.assertFalse(nested.is_trading_day(opened))
        self.assertTrue(nested.is_trading_day(closed))
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime.combine(closed, datetime.min.time()),
            metadata_calendar=left,
        )
        self.assertEqual(context, context.replace(metadata_calendar=right))

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

    def test_schedule_endpoints_clamping_offsets_and_empty_results(self):
        fixed = fixed_interval_schedule(
            start_time=datetime(2025, 1, 3),
            end_time=datetime(2025, 1, 7),
            interval_days=1,
        )
        self.assertEqual(fixed.times, [datetime(2025, 1, 6), datetime(2025, 1, 7)])
        self.assertEqual(
            monthly_schedule(
                start_time=datetime(2025, 1, 1),
                end_time=datetime(2025, 3, 1),
                first_observation_month=1,
            ).times,
            [datetime(2025, 2, 3)],
        )
        self.assertEqual(
            monthly_schedule(
                start_time=datetime(2025, 1, 31),
                end_time=datetime(2025, 4, 30),
                first_observation_month=1,
            ).times,
            [datetime(2025, 2, 28), datetime(2025, 3, 31), datetime(2025, 4, 30)],
        )
        self.assertEqual(
            monthly_schedule(
                start_time=datetime(2024, 1, 31),
                end_time=datetime(2024, 3, 31),
                first_observation_month=1,
                calendar=all_days_calendar(),
            ).times,
            [datetime(2024, 2, 29), datetime(2024, 3, 31)],
        )
        self.assertEqual(
            monthly_schedule(
                start_time=datetime(2025, 1, 2),
                end_time=datetime(2025, 4, 2),
                first_observation_month=2,
            ).times,
            [datetime(2025, 3, 3), datetime(2025, 4, 2)],
        )
        for start, end in (
            (datetime(2025, 1, 1), datetime(2025, 1, 1)),
            (datetime(2025, 1, 1), datetime(2026, 1, 1)),
            (datetime(9999, 11, 1), datetime(9999, 12, 31)),
        ):
            for interval in (2**31 - 1, 2**31, 2**40, 2**63 - 1):
                with self.subTest(start_time=start, end_time=end, interval=interval):
                    self.assertEqual(
                        fixed_interval_schedule(
                            start_time=start, end_time=end, interval_days=interval
                        ).times,
                        [],
                    )
            self.assertEqual(
                monthly_schedule(
                    start_time=start, end_time=end, first_observation_month=2**31 - 1
                ).times,
                [],
            )
        self.assertEqual(
            fixed_interval_schedule(
                start_time=datetime(9999, 12, 30),
                end_time=datetime(9999, 12, 31),
                interval_days=1,
                calendar=all_days_calendar(),
            ).times,
            [datetime(9999, 12, 31)],
        )

    def test_closures_merge_candidates_and_stop_at_end(self):
        closed = [date(2025, 1, 1) + timedelta(days=i) for i in range(90)]
        calendar = custom_calendar(base=all_days_calendar(), closed_dates=closed)
        for schedule in (
            monthly_schedule(
                start_time=datetime(2025, 1, 1),
                end_time=datetime(2025, 4, 2),
                first_observation_month=1,
                calendar=calendar,
            ),
            fixed_interval_schedule(
                start_time=datetime(2025, 1, 1),
                end_time=datetime(2025, 4, 2),
                interval_days=31,
                calendar=calendar,
            ),
        ):
            self.assertEqual(schedule.times, [datetime(2025, 4, 1)])
        self.assertEqual(
            fixed_interval_schedule(
                start_time=datetime(2025, 1, 1),
                end_time=datetime(2025, 3, 31),
                interval_days=1,
                calendar=calendar,
            ).times,
            [],
        )

    def test_schedule_iteration_indexing_and_owning_copies(self):
        schedule = fixed_interval_schedule(
            start_time=datetime(2025, 1, 1),
            end_time=datetime(2025, 1, 5),
            interval_days=1,
            calendar=all_days_calendar(),
        )
        expected = [datetime(2025, 1, i) for i in range(2, 6)]
        self.assertEqual(len(schedule), 4)
        self.assertEqual(schedule[0], expected[0])
        self.assertEqual(schedule[-1], expected[-1])
        self.assertEqual(schedule[-4], expected[0])

        class HugeIndex:
            def __index__(self):
                return 10**100

        for index in (4, -5, 10**100, -(10**100), HugeIndex()):
            with self.assertRaises(IndexError):
                _ = schedule[index]
        failure = OverflowError("custom index failure")

        class BrokenIndex:
            def __index__(self):
                raise failure

        with self.assertRaises(OverflowError) as error:
            _ = schedule[BrokenIndex()]
        self.assertIs(error.exception, failure)
        copied = schedule.times
        copied.clear()
        self.assertEqual(schedule.times, expected)
        with self.assertRaises(AttributeError):
            schedule.times = []
        first, second = iter(schedule), schedule.__iter__()
        self.assertIs(iter(first), first)
        self.assertEqual(next(first), expected[0])
        del schedule
        self.assertEqual(list(first), expected[1:])
        self.assertEqual(list(second), expected)
        for _ in range(2):
            with self.assertRaises(StopIteration):
                next(first)

    def test_schedule_indices_reject_booleans_and_nonintegers(self):
        schedule = fixed_interval_schedule(
            start_time=datetime(2025, 1, 1),
            end_time=datetime(2025, 1, 5),
            interval_days=1,
            calendar=all_days_calendar(),
        )

        class Index:
            def __index__(self):
                return -1

        self.assertEqual(schedule[Index()], datetime(2025, 1, 5))
        for index in (True, False, 1.0, 1.5, "1", None, slice(None)):
            with self.subTest(index=index), self.assertRaises(TypeError):
                _ = schedule[index]
        self.assertEqual(list(schedule), [datetime(2025, 1, i) for i in range(2, 6)])

    def test_validation_errors_and_order(self):
        start, end = datetime(2025, 1, 1), datetime(2025, 2, 1)
        for factory, field, minimum in (
            (fixed_interval_schedule, "interval_days", -(2**63)),
            (monthly_schedule, "first_observation_month", -(2**31)),
        ):
            for value in (0, -1, minimum):
                with self.assertRaises(InvalidInputError) as error:
                    factory(start_time=start, end_time=end, **{field: value})
                self.assertIs(type(error.exception), InvalidInputError)
                with self.assertRaises(InvalidInputError) as error:
                    factory(start_time=end, end_time=start, **{field: value})
                self.assertIs(type(error.exception), InvalidInputError)
        for operation in (
            weekdays_calendar().trading_days_between,
            weekdays_calendar().trading_year_fraction,
        ):
            with self.assertRaises(InvalidInputError) as error:
                operation(end.date(), start.date())
            self.assertIs(type(error.exception), InvalidInputError)

    def test_representations_use_native_errors_and_integer_protocol(self):
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
                self.assertEqual(
                    factory(start_time=start, end_time=end, **{field: value}).times,
                    factory(start_time=start, end_time=end, **{field: 1}).times,
                )
            for value in (True, 1.0, 1.5, "1", None):
                with self.assertRaises(TypeError):
                    factory(start_time=start, end_time=end, **{field: value})
            for value in (-(2**bits) - 1, 2**bits):
                with self.assertRaises(OverflowError):
                    factory(start_time=start, end_time=end, **{field: value})
            for value in (date(2025, 1, 1), "2025-01-01", True):
                with self.assertRaises(TypeError):
                    factory(start_time=value, end_time=end, **{field: 1})
        with self.assertRaises(ValueError):
            weekdays_calendar().adjust(start.date(), "modified_following")
        with self.assertRaises(TypeError):
            weekdays_calendar().adjust(start.date(), True)

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
                start_time=datetime(2025, 1, 1),
                end_time=datetime(2025, 2, 1),
                interval_days=BrokenInteger(),
            )

        def broken_dates():
            yield date(2025, 1, 1)
            raise ConversionFailure

        base = weekdays_calendar()
        with self.assertRaises(ConversionFailure):
            custom_calendar(base=base, closed_dates=broken_dates())
        self.assertTrue(base.is_trading_day(date(2025, 1, 1)))
