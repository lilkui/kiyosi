import math
import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import (
    asset_no_touch_down,
    asset_no_touch_up,
    asset_one_touch_down,
    asset_one_touch_up,
    cash_no_touch_down,
    cash_no_touch_up,
    cash_one_touch_down,
    cash_one_touch_up,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class AuditFixTests(unittest.TestCase):
    def test_touch_prices_survive_volatility_variance_underflow(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticBinaryBarrierEngine()
        for up in (True, False):
            barrier = 101 if up else 99
            terms = {
                "effective_date": start,
                "expiry_date": end,
                "barrier_level": barrier,
            }
            for sigma in (1e-200, math.ulp(0.0)):
                for crossing in (False, True):
                    dividend = (0 if up else 0.1) if crossing else 0.05
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.05,
                            dividend_yield=dividend,
                            volatility=sigma,
                        ),
                        spot_price=100,
                        valuation_time=start,
                    )
                    hit_time = (
                        math.log1p((barrier - 100) / 100) / (0.05 - dividend)
                        if crossing
                        else 0
                    )
                    for timing in ("at_hit", "at_expiry"):
                        with self.subTest(
                            up=up, sigma=sigma, crossing=crossing, timing=timing
                        ):
                            cash = (cash_one_touch_up if up else cash_one_touch_down)(
                                **terms, payout=1, settlement_timing=timing
                            )
                            asset = (
                                asset_one_touch_up if up else asset_one_touch_down
                            )(**terms, settlement_timing=timing)
                            discount = math.exp(
                                -0.05 * (hit_time if timing == "at_hit" else 1)
                            )
                            self.assertAlmostEqual(
                                engine.price(cash, context),
                                discount if crossing else 0,
                                delta=1e-12,
                            )
                            expected = (
                                (
                                    barrier * discount
                                    if timing == "at_hit"
                                    else 100 * math.exp(-dividend)
                                )
                                if crossing
                                else 0
                            )
                            self.assertAlmostEqual(
                                engine.price(asset, context), expected, delta=1e-10
                            )
                    cash = (cash_no_touch_up if up else cash_no_touch_down)(
                        **terms, payout=1
                    )
                    asset = (asset_no_touch_up if up else asset_no_touch_down)(**terms)
                    self.assertAlmostEqual(
                        engine.price(cash, context),
                        0 if crossing else math.exp(-0.05),
                        delta=1e-12,
                    )
                    self.assertAlmostEqual(
                        engine.price(asset, context),
                        0 if crossing else 100 * math.exp(-dividend),
                        delta=1e-10,
                    )
            drift = math.log1p((barrier - 100) / 100)
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=-drift, volatility=1e-200
                ),
                spot_price=100,
                valuation_time=start,
            )
            boundary = (cash_one_touch_up if up else cash_one_touch_down)(
                **terms, payout=1, settlement_timing="at_expiry"
            )
            self.assertEqual(engine.price(boundary, context), 0.5)
