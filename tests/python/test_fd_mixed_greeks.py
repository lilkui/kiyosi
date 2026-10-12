import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class FiniteDifferenceMixedGreekTests(unittest.TestCase):
    def test_one_sided_native_time_greeks_remain_selective_and_accurate(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
            ),
            spot_price=100,
            valuation_time=start,
        )
        for option_type in ("call", "put"):
            option = EuropeanOption(
                option_type=option_type, strike=100, effective_date=start, expiry_date=end
            )
            for engine in (
                pricing.CoxRossRubinsteinVanillaEngine(step_count=1000),
                pricing.FiniteDifferenceVanillaEngine(),
            ):
                for greek, dependency in (("charm", "delta"), ("color", "gamma")):
                    with self.subTest(
                        option_type=option_type, engine=type(engine), greek=greek
                    ):
                        single = engine.price_with_greeks(option, context, [greek])
                        joint = engine.price_with_greeks(option, context, [greek, dependency])
                        expected = pricing.AnalyticVanillaEngine().price_with_greeks(
                            option, context, [greek]
                        )
                        self.assertEqual(single.price, joint.price)
                        self.assertEqual(getattr(single, greek), getattr(joint, greek))
                        self.assertIsNone(single.delta)
                        self.assertIsNone(single.gamma)
                        reference = getattr(expected, greek)
                        self.assertAlmostEqual(
                            getattr(single, greek),
                            reference,
                            delta=abs(reference) * 0.02 + 1e-7,
                        )

    def test_charm_at_effective_date_remains_selective(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
            ),
            spot_price=100,
            valuation_time=start,
        )
        expected = pricing.AnalyticVanillaEngine().price_with_greeks(
            option, context, ["charm"]
        )
        actual = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=400, time_step_count=400
        ).price_with_greeks(option, context, ["charm"])
        self.assertAlmostEqual(
            actual.charm, expected.charm, delta=abs(expected.charm) * 0.02 + 1e-7
        )
        self.assertIsNone(actual.delta)
        self.assertIsNone(actual.gamma)
        self.assertIsNone(actual.color)

    def test_mixed_greeks_track_analytic_sensitivities(self):
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
        request = ["vanna", "zomma", "charm", "color"]
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
