import math
import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class DigitalGreekTests(unittest.TestCase):
    def test_analytic_digital_speed_at_large_price_scales(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        spot, payout, width = 100000.0, 1e8, 0.2
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=width
            ),
            spot_price=spot,
            valuation_time=start,
        )
        engine = pricing.AnalyticDigitalEngine()
        for direction, sign in (("call", 1), ("put", -1)):
            terms = {
                "option_type": direction,
                "strike": spot,
                "effective_date": start,
                "expiry_date": end,
            }
            for asset, option in (
                (False, CashOrNothingOption(**terms, payout=payout)),
                (True, AssetOrNothingOption(**terms)),
            ):
                with self.subTest(direction=direction, asset=asset):
                    d = 0.03 / width + (0.5 if asset else -0.5) * width
                    density = math.exp(-0.5 * d * d) / math.sqrt(2 * math.pi)
                    if asset:
                        expected = (
                            sign
                            * math.exp(-0.02)
                            * density
                            / (spot**2 * width)
                            * ((d * d - 1) / width**2 - 1)
                        )
                    else:
                        expected = (
                            sign
                            * payout
                            * math.exp(-0.05)
                            * density
                            / (spot**3 * width)
                            * (2 + 3 * d / width + (d * d - 1) / width**2)
                        )
                    result = engine.price_with_greeks(option, context, "speed")
                    self.assertTrue(math.isclose(result.speed, expected, rel_tol=1e-7))
                    self.assertEqual(result.price, engine.price(option, context))
                    self.assertIsNone(result.gamma)
