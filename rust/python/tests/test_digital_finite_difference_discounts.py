import math
import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import CashOrNothingOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class FiniteDifferenceDiscountTests(unittest.TestCase):
    def test_finite_difference_digital_preserves_scaled_discounts(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        payout = 1e-308
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=-710, dividend_yield=-710, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.FiniteDifferenceDigitalEngine(
            asset_step_count=100, time_step_count=100_000
        )
        for direction, sign in (("call", 1), ("put", -1)):
            with self.subTest(direction=direction):
                option = CashOrNothingOption(
                    option_type=direction,
                    strike=100,
                    payout=payout,
                    effective_date=start,
                    expiry_date=end,
                )
                expected = (
                    math.exp(math.log(payout) + 710)
                    * 0.5
                    * math.erfc(sign * 0.1 / math.sqrt(2))
                )
                self.assertTrue(
                    math.isclose(engine.price(option, context), expected, rel_tol=0.006)
                )
