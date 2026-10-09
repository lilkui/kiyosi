import math
import unittest
from datetime import date, timedelta

from kiyosi import pricing
from kiyosi.instruments import ArithmeticAveragePriceOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class AsianRegressionTests(unittest.TestCase):
    def test_arithmetic_asians_retain_time_value_when_squared_volatility_underflows(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction in ("call", "put"):
            for spot, sigma in ((100, 1e-170), (1e160, 1e-160), (1e300, 1e-200)):
                for window in (0, 1, 2):
                    with self.subTest(
                        direction=direction, spot=spot, sigma=sigma, window=window
                    ):
                        future = 1 if window == 0 else 245 / 365
                        lead = 120 / 365 if window == 1 else 0
                        scale = future if window == 2 else 1
                        expected = (
                            scale
                            * spot
                            * sigma
                            * math.sqrt((lead + future / 3) / (2 * math.pi))
                        )
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0,
                                dividend_yield=0,
                                volatility=sigma,
                            ),
                            spot_price=spot,
                            valuation_time=start + timedelta(days=120)
                            if window == 2
                            else start,
                        )
                        option = ArithmeticAveragePriceOption(
                            option_type=direction,
                            strike=spot,
                            effective_date=start,
                            averaging_start_date=start + timedelta(days=120)
                            if window == 1
                            else start,
                            expiry_date=end,
                            realized_average=spot if window == 2 else 0,
                        )
                        actual = (
                            pricing.TurnbullWakemanArithmeticAveragePriceEngine().price(
                                option, context
                            )
                        )
                        self.assertAlmostEqual(actual, expected, delta=expected * 1e-12)
