import math
import unittest
from datetime import date

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class DigitalOverflowTests(unittest.TestCase):
    def test_deep_in_the_money_digital_prices_survive_overflowing_discount_factors(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        settlement = 1e-300
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=-710, dividend_yield=-710, volatility=0.2
            ),
            spot_price=settlement,
            valuation_time=start,
        )
        expected = math.exp(math.log(settlement) + 710)
        for direction in ("call", "put"):
            terms = {
                "option_type": direction,
                "strike": 1e-304 if direction == "call" else 1e-296,
                "effective_date": start,
                "expiry_date": end,
            }
            for option in (
                CashOrNothingOption(**terms, payout=settlement),
                AssetOrNothingOption(**terms),
            ):
                for engine in (
                    pricing.AnalyticDigitalEngine(),
                    pricing.QuadratureDigitalEngine(),
                ):
                    with self.subTest(
                        direction=direction,
                        option=type(option).__name__,
                        engine=type(engine).__name__,
                    ):
                        self.assertTrue(
                            math.isclose(
                                engine.price(option, context), expected, rel_tol=1e-12
                            )
                        )

    def test_digital_prices_survive_overflowing_discounted_settlements(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        settlement = 1.75e308
        for direction in ("call", "put"):
            probability = 0.5 * math.erfc(
                (-0.2 if direction == "call" else 0.2) / math.sqrt(2)
            )
            for asset in (False, True):
                for overflow in (False, True):
                    discount_rate = -1.0 if overflow else -0.05
                    parameters = BlackScholesMertonParameters(
                        risk_free_rate=discount_rate + 0.02 if asset else discount_rate,
                        dividend_yield=discount_rate if asset else discount_rate - 0.06,
                        volatility=0.2,
                    )
                    spot = settlement if asset else 100
                    context = PricingContext(
                        model_parameters=parameters,
                        spot_price=spot,
                        valuation_time=start,
                    )
                    terms = {
                        "option_type": direction,
                        "strike": spot,
                        "effective_date": start,
                        "expiry_date": end,
                    }
                    option = (
                        AssetOrNothingOption(**terms)
                        if asset
                        else CashOrNothingOption(**terms, payout=settlement)
                    )
                    for engine in (
                        pricing.AnalyticDigitalEngine(),
                        pricing.QuadratureDigitalEngine(),
                    ):
                        with self.subTest(
                            direction=direction,
                            asset=asset,
                            overflow=overflow,
                            engine=type(engine).__name__,
                        ):
                            if overflow:
                                with self.assertRaises(kiyosi.KiyosiError) as error:
                                    engine.price(option, context)
                                self.assertEqual(
                                    error.exception.category,
                                    kiyosi.ErrorCategory.INVALID_RESULT,
                                )
                            else:
                                self.assertTrue(
                                    math.isclose(
                                        engine.price(option, context),
                                        settlement * (math.exp(0.05) * probability),
                                        rel_tol=1e-10,
                                    )
                                )
