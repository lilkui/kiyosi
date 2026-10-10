import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class FiniteDifferenceMixedGreekTests(unittest.TestCase):
    def test_mixed_volatility_greeks_track_analytic_sensitivities(self):
        option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
            ),
            spot_price=100,
            valuation_time=date(2025, 7, 1),
        )
        request = ["vanna", "zomma"]
        expected = pricing.AnalyticVanillaEngine().price_with_greeks(
            option, context, request
        )
        actual = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=400, time_step_count=400
        ).price_with_greeks(option, context, request)
        for name in request:
            with self.subTest(greek=name):
                self.assertAlmostEqual(
                    getattr(actual, name),
                    getattr(expected, name),
                    delta=abs(getattr(expected, name)) * 0.02 + 1e-6,
                )
        self.assertIsNone(actual.delta)
        self.assertIsNone(actual.gamma)
