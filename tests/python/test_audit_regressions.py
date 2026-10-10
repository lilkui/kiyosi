import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class AuditRegressionTests(unittest.TestCase):
    def test_numerical_greeks_use_representable_symmetric_bumps(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0,
            dividend_yield=0,
            volatility=0.01,
        )
        for spot in (100, 1e12, 1e13, 1e14):
            with self.subTest(spot=spot):
                option = EuropeanOption(
                    option_type="call",
                    strike=spot * 0.01,
                    effective_date=start,
                    expiry_date=end,
                )
                context = PricingContext(
                    model_parameters=parameters,
                    spot_price=spot,
                    valuation_time=start,
                )
                for result in (
                    pricing.calculate_numerical_greeks(
                        pricing.AnalyticVanillaEngine(), option, context
                    ),
                    pricing.QuadratureVanillaEngine().price_with_greeks(
                        option,
                        context,
                        ["delta", "gamma", "speed"],
                    ),
                ):
                    self.assertAlmostEqual(result.delta, 1, delta=1e-12)
                    self.assertEqual(result.gamma, 0)
                    self.assertAlmostEqual(result.speed, 0, delta=1e-6)
                if spot == 1e14:
                    collapsed = pricing.calculate_numerical_greeks(
                        pricing.AnalyticVanillaEngine(),
                        option,
                        context,
                        spot_shift=0.004,
                    )
                    self.assertIsNone(collapsed.delta)
                    self.assertIsNone(collapsed.gamma)
                    self.assertIsNone(collapsed.speed)
                    self.assertEqual(collapsed.vega, 0)
