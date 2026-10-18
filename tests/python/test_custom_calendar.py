import unittest
from datetime import date, datetime
from pathlib import Path

from kiyosi import ErrorCategory, KiyosiError, _native
from kiyosi.market import (
    custom_calendar,
    fixed_interval_schedule,
    sse_calendar,
    weekdays_calendar,
)


class CustomCalendarTests(unittest.TestCase):
    def test_overrides_ownership_and_calendar_operations(self):
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
            self.assertEqual(calendar.is_trading_day(date(2026, 10, day)), expected)
        self.assertTrue(base.is_trading_day(date(2026, 10, 1)))
        self.assertFalse(base.is_trading_day(date(2026, 10, 10)))
        self.assertEqual(calendar.trading_days_per_year, 252)
        start, end = date(2026, 10, 1), date(2026, 10, 12)
        self.assertEqual(calendar.trading_days_between(start, end), 6)
        self.assertAlmostEqual(calendar.trading_year_fraction(start, end), 6 / 252)
        self.assertEqual(calendar.adjust(start, "following"), date(2026, 10, 5))
        self.assertEqual(
            list(
                fixed_interval_schedule(
                    start=date(2026, 10, 9),
                    end=date(2026, 10, 11),
                    interval_days=1,
                    calendar=calendar,
                )
            ),
            [date(2026, 10, 10)],
        )
        layered = custom_calendar(
            base=calendar,
            closed_dates=[date(2026, 10, 10)],
            open_dates=[start],
            trading_days_per_year=365,
        )
        del calendar
        self.assertTrue(layered.is_trading_day(start))
        self.assertFalse(layered.is_trading_day(date(2026, 10, 10)))
        self.assertEqual(layered.trading_days_per_year, 365)

    def test_empty_overrides_inherit_base(self):
        base = sse_calendar()
        calendar = custom_calendar(base=base, trading_days_per_year=None)
        self.assertEqual(calendar.trading_days_per_year, base.trading_days_per_year)
        self.assertEqual(
            calendar.is_trading_day(date(2026, 10, 1)),
            base.is_trading_day(date(2026, 10, 1)),
        )

    def test_domain_errors(self):
        for kwargs in (
            {"closed_dates": [date(2026, 10, 1)], "open_dates": [date(2026, 10, 1)]},
            {"trading_days_per_year": 0},
            {"trading_days_per_year": -1},
        ):
            with self.subTest(kwargs=kwargs), self.assertRaises(KiyosiError) as error:
                custom_calendar(base=weekdays_calendar(), **kwargs)
            self.assertEqual(error.exception.category, ErrorCategory.INVALID_CALENDAR)

    def test_representation_errors(self):
        for field in ("closed_dates", "open_dates"):
            for value in (None, 1, [datetime(2026, 10, 1)], ["2026-10-01"], [True]):  # noqa: DTZ001 - invalid date input
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(TypeError),
                ):
                    custom_calendar(base=weekdays_calendar(), **{field: value})
        for value in (True, 252.0, 252.5, "252"):
            with self.subTest(value=value), self.assertRaises(TypeError):
                custom_calendar(base=weekdays_calendar(), trading_days_per_year=value)
        with self.assertRaises(OverflowError):
            custom_calendar(base=weekdays_calendar(), trading_days_per_year=2**40)
        with self.assertRaises(TypeError):
            custom_calendar()

    def test_generated_stub_exports_factory(self):
        stub = (
            Path(_native.__file__).with_name("_native.pyi").read_text(encoding="utf-8")
        )
        self.assertIn("def custom_calendar(", stub)


if __name__ == "__main__":
    unittest.main()
