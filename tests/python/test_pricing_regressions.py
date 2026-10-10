import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import AmericanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class PricingRegressionTests(unittest.TestCase):
    def test_american_fd_domains_preserve_long_expiry_exercise_value(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0.05, volatility=1.0
            ),
            spot_price=100,
            valuation_time=start,
        )
        option = AmericanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        # QuantLib 1.43 FD, 3200 time/asset steps and 2 damping steps.
        reference = 69.73204196706111
        coarse = pricing.FiniteDifferenceVanillaEngine().price(option, context)
        fine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=800, time_step_count=1600
        )
        refined = fine.price(option, context)
        self.assertAlmostEqual(coarse, reference, delta=0.2)
        self.assertAlmostEqual(refined, reference, delta=0.03)
        self.assertLess(abs(refined - reference), abs(coarse - reference))
        joint = fine.price_with_greeks(option, context, "vega")
        self.assertEqual(joint.price, refined)
        self.assertIsNotNone(joint.vega)
