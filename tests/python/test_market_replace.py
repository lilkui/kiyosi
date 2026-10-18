import unittest
from datetime import UTC, date, datetime, timedelta, timezone

from kiyosi import ErrorCategory, KiyosiError
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)


class MarketReplaceTests(unittest.TestCase):
    def setUp(self):
        self.parameters = BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0.01, volatility=0.2
        )
        self.time = datetime(2025, 1, 3, 12, 30, 1, 123456, tzinfo=UTC)
        self.context = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=self.time,
            calendar=all_days_calendar(),
        )

    def test_parameters_replace_preserves_fields_and_accepts_zero_rates(self):
        changed = self.parameters.replace(volatility=0.25)
        self.assertEqual(changed.risk_free_rate, 0.03)
        self.assertEqual(changed.dividend_yield, 0.01)
        self.assertEqual(changed.volatility, 0.25)
        zero_rates = changed.replace(risk_free_rate=0, dividend_yield=0)
        self.assertEqual(zero_rates.risk_free_rate, 0)
        self.assertEqual(zero_rates.dividend_yield, 0)
        self.assertEqual(zero_rates.volatility, 0.25)
        self.assertEqual(self.parameters.volatility, 0.2)
        self.assertEqual(self.parameters.replace(), self.parameters)

    def test_context_replace_preserves_calendar_and_exact_timestamp(self):
        for changed in (self.context.replace(), self.context.replace(spot_price=105)):
            self.assertEqual(changed.model_parameters, self.parameters)
            self.assertEqual(changed.valuation_time, self.time)
            self.assertEqual(changed.calendar.trading_days_per_year, 365)
            self.assertTrue(changed.calendar.is_trading_day(date(2025, 1, 4)))
        self.assertEqual(self.context.replace().spot_price, 100)
        self.assertEqual(self.context.replace(spot_price=105).spot_price, 105)
        changed = self.context.replace(
            model_parameters=self.parameters.replace(volatility=0.25),
            spot_price=110,
            valuation_time=datetime(2025, 1, 6, 9, tzinfo=timezone(timedelta(hours=8))),
            calendar=weekdays_calendar(),
        )
        self.assertEqual(changed.model_parameters.volatility, 0.25)
        self.assertEqual(changed.spot_price, 110)
        self.assertEqual(changed.valuation_time, datetime(2025, 1, 6, 1, tzinfo=UTC))
        self.assertFalse(changed.calendar.is_trading_day(date(2025, 1, 4)))
        self.assertEqual(
            self.context.replace(valuation_time=date(2025, 1, 6)).valuation_time,
            datetime(2025, 1, 6, tzinfo=UTC),
        )
        self.assertEqual(self.context.spot_price, 100)
        self.assertEqual(self.context.model_parameters, self.parameters)
        self.assertEqual(self.context.valuation_time, self.time)
        self.assertTrue(self.context.calendar.is_trading_day(date(2025, 1, 4)))

    def test_replace_rejects_invalid_domains_without_mutation(self):
        for field, value, category in (
            ("risk_free_rate", float("nan"), ErrorCategory.INVALID_RISK_FREE_RATE),
            ("dividend_yield", float("inf"), ErrorCategory.INVALID_DIVIDEND_YIELD),
            ("volatility", 0, ErrorCategory.INVALID_VOLATILITY),
        ):
            with self.subTest(field=field):
                with self.assertRaises(KiyosiError) as caught:
                    self.parameters.replace(**{field: value})
                self.assertEqual(caught.exception.category, category)
        with self.assertRaises(KiyosiError) as caught:
            self.context.replace(spot_price=0, calendar=weekdays_calendar())
        self.assertEqual(caught.exception.category, ErrorCategory.INVALID_SPOT_PRICE)
        self.assertEqual(self.parameters.risk_free_rate, 0.03)
        self.assertEqual(self.parameters.dividend_yield, 0.01)
        self.assertEqual(self.parameters.volatility, 0.2)
        self.assertEqual(self.context.spot_price, 100)
        self.assertTrue(self.context.calendar.is_trading_day(date(2025, 1, 4)))

    def test_replace_rejects_invalid_representations_and_signatures(self):
        for obj, fields in (
            (self.parameters, ("risk_free_rate", "dividend_yield", "volatility")),
            (
                self.context,
                ("model_parameters", "spot_price", "valuation_time", "calendar"),
            ),
        ):
            for field in fields:
                for value in (None, True, "invalid", Ellipsis):
                    with (
                        self.subTest(field=field, value=value),
                        self.assertRaises(TypeError),
                    ):
                        obj.replace(**{field: value})
            with self.assertRaises(TypeError):
                obj.replace(1)
            with self.assertRaises(TypeError):
                obj.replace(unknown=1)
        with self.assertRaises(TypeError):
            self.context.replace(valuation_time=datetime(2025, 1, 6))  # noqa: DTZ001 - reject naive timestamps
        with self.assertRaises(OverflowError):
            self.parameters.replace(volatility=10**1000)


if __name__ == "__main__":
    unittest.main()
