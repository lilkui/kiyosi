import unittest
from datetime import date, datetime, timezone

import kiyosi
from kiyosi.instruments import BarrierOption, BarrierType, CashOrNothingOption, EuropeanOption, OptionType
from kiyosi.market import BsmParameters, PricingContext, fixed_interval_schedule
from kiyosi.pricing import AnalyticDigitalEngine, AnalyticVanillaEngine, numerical_analytics


class KiyosiPythonTests(unittest.TestCase):
    def setUp(self):
        self.parameters = BsmParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2)
        self.context = PricingContext(parameters=self.parameters, asset_price=100.0, valuation_time=date(2025, 1, 1))
        self.option = EuropeanOption(type=OptionType.CALL, strike=100.0, effective=date(2025, 1, 1), expiry=date(2026, 1, 1))

    def test_default_and_explicit_pricing_share_result_surface(self):
        result = kiyosi.price(self.option, self.context)
        explicit = AnalyticVanillaEngine().price(self.option, self.context)
        self.assertEqual(len(result), 11)
        self.assertEqual(result.price, explicit.price)
        self.assertEqual(result["price"], result.price)
        self.assertIn("speed", result)

    def test_native_domain_errors_expose_categories(self):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BsmParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.0)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_VOLATILITY)

    def test_numeric_and_date_boundaries_are_checked(self):
        with self.assertRaises(TypeError):
            EuropeanOption(type=OptionType.CALL, strike="100", effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        with self.assertRaises(TypeError):
            PricingContext(parameters=self.parameters, asset_price=100, valuation_time=datetime(2025, 1, 1))
        aware = PricingContext(parameters=self.parameters, asset_price=100, valuation_time=datetime(2025, 1, 1, 8, tzinfo=timezone.utc))
        self.assertIsNotNone(kiyosi.price(self.option, aware).price)

    def test_digital_barrier_schedule_and_analytics(self):
        digital = CashOrNothingOption(type=OptionType.CALL, strike=100, payout=10, effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        self.assertGreater(AnalyticDigitalEngine().price(digital, self.context).price, 0)
        barrier = BarrierOption(type=OptionType.CALL, strike=100, effective=date(2025, 1, 1), expiry=date(2026, 1, 1), barrier=80, barrier_kind=BarrierType.DOWN_AND_OUT)
        self.assertGreater(kiyosi.price(barrier, self.context).price, 0)
        self.assertEqual(len(fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)), 5)
        self.assertIsNotNone(numerical_analytics(AnalyticVanillaEngine(), self.option, self.context).vega)


if __name__ == "__main__":
    unittest.main()
