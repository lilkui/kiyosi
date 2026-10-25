import math
import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import (
    AssetOrNothingOption,
    CashOrNothingOption,
    asset_binary_barrier_option,
    cash_binary_barrier_option,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class DigitalGreekTests(unittest.TestCase):
    def test_digital_and_binary_barrier_terminal_payoffs_share_strict_strike_boundaries(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)

        def check(engine, option, context, expected):
            with self.subTest(
                spot=context.spot_price,
                engine=type(engine).__name__,
                option=repr(option),
            ):
                self.assertEqual(engine.price(option, context), expected)
                result = engine.price_with_greeks(option, context, "delta")
                self.assertEqual(result.price, expected)
                self.assertIsNone(result.delta)

        for direction in ("call", "put"):
            for spot in (
                99,
                math.nextafter(100, 0),
                100,
                math.nextafter(100, 101),
                101,
            ):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                    ),
                    spot_price=spot,
                    valuation_time=end,
                )
                terms = {
                    "option_type": direction,
                    "strike": 100,
                    "effective_date": start,
                    "expiry_date": end,
                }
                exercised = spot > 100 if direction == "call" else spot < 100

                for engine in (
                    pricing.MonteCarloDigitalEngine(),
                    pricing.CoxRossRubinsteinDigitalEngine(),
                    pricing.AnalyticDigitalEngine(),
                    pricing.QuadratureDigitalEngine(),
                    pricing.FiniteDifferenceDigitalEngine(),
                ):
                    check(
                        engine,
                        CashOrNothingOption(**terms, payout=7),
                        context,
                        7 if exercised else 0,
                    )
                    check(
                        engine,
                        AssetOrNothingOption(**terms),
                        context,
                        spot if exercised else 0,
                    )
                for knock_in in (False, True):
                    for touched in (False, True):
                        barrier_terms = terms | {
                            "barrier_level": 50,
                            "barrier_type": "down_and_in"
                            if knock_in
                            else "down_and_out",
                            "touch_state": "touched" if touched else "untouched",
                        }
                        pays = exercised and knock_in == touched
                        engine = pricing.AnalyticBinaryBarrierEngine()
                        check(
                            engine,
                            cash_binary_barrier_option(**barrier_terms, payout=7),
                            context,
                            7 if pays else 0,
                        )
                        check(
                            engine,
                            asset_binary_barrier_option(**barrier_terms),
                            context,
                            spot if pays else 0,
                        )

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
