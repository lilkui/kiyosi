import math
import unittest
from datetime import date

from kiyosi import ComputationFailureError, pricing
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class FiniteDifferenceRannacherTests(unittest.TestCase):
    def test_damping_still_rejects_oscillatory_prices(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = CashOrNothingOption(
            option_type="call",
            strike=100.5,
            payout=100,
            effective_date=start,
            expiry_date=end,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=10
            ),
            spot_price=20,
            valuation_time=start,
        )
        engine = pricing.FiniteDifferenceDigitalEngine(
            asset_step_count=1000, time_step_count=3, asset_upper_boundary=400
        )
        with self.assertRaises(ComputationFailureError) as rejected:
            engine.price(option, context)
        self.assertIs(type(rejected.exception), ComputationFailureError)

    def test_payoff_bounds_allow_known_carry_truncation(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        for rate in (-0.05, 0.05):
            dividend = 0.4 * rate
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=dividend, volatility=0.2
                ),
                spot_price=100,
                valuation_time=start,
            )
            for steps in (1, 200):
                engine = pricing.FiniteDifferenceDigitalEngine(
                    asset_step_count=800, time_step_count=steps
                )
                terms = {
                    "option_type": "call",
                    "strike": 1,
                    "effective_date": start,
                    "expiry_date": end,
                }
                for option, upper, delta in (
                    (
                        CashOrNothingOption(**terms, payout=100),
                        100 * math.exp(-rate),
                        0,
                    ),
                    (
                        AssetOrNothingOption(**terms),
                        100 * math.exp(-dividend),
                        math.exp(-dividend),
                    ),
                ):
                    with self.subTest(
                        rate=rate, steps=steps, option=type(option).__name__
                    ):
                        actual = engine.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        self.assertGreaterEqual(actual.price, 0)
                        self.assertLessEqual(actual.price, upper)
                        self.assertAlmostEqual(actual.price, upper, delta=1e-8)
                        self.assertAlmostEqual(actual.delta, delta, delta=0.001)
                        self.assertAlmostEqual(actual.gamma, 0, delta=1e-8)

    def test_short_cn_grids_match_implicit_half_steps(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.12, dividend_yield=0.02, volatility=0.3
            ),
            spot_price=99.25,
            valuation_time=start,
        )
        for steps in (1, 2):
            for option_type in ("call", "put"):
                terms = {
                    "option_type": option_type,
                    "strike": 100,
                    "effective_date": start,
                    "expiry_date": end,
                }
                for option, factory in (
                    (EuropeanOption(**terms), pricing.FiniteDifferenceVanillaEngine),
                    (AmericanOption(**terms), pricing.FiniteDifferenceVanillaEngine),
                    (
                        CashOrNothingOption(**terms, payout=10),
                        pricing.FiniteDifferenceDigitalEngine,
                    ),
                    (
                        AssetOrNothingOption(**terms),
                        pricing.FiniteDifferenceDigitalEngine,
                    ),
                ):
                    with self.subTest(
                        steps=steps,
                        option_type=option_type,
                        option=type(option).__name__,
                    ):
                        engine = factory(
                            asset_step_count=400,
                            time_step_count=steps,
                            asset_upper_boundary=400,
                        )
                        reference = factory(
                            asset_step_count=400,
                            time_step_count=2 * steps,
                            asset_upper_boundary=400,
                            scheme="implicit_euler",
                        )
                        actual = engine.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        expected = reference.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        self.assertEqual(actual.price, expected.price)
                        self.assertEqual(actual.price, engine.price(option, context))
                        self.assertEqual(actual.delta, expected.delta)
                        self.assertEqual(actual.gamma, expected.gamma)

    def test_coarse_cn_time_grids_track_analytic_greeks(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=start,
        )
        for steps in (8, 32):
            for option_type in ("call", "put"):
                terms = {
                    "option_type": option_type,
                    "strike": 100,
                    "effective_date": start,
                    "expiry_date": end,
                }
                for option, factory, analytic, tolerances in (
                    (
                        EuropeanOption(**terms),
                        pricing.FiniteDifferenceVanillaEngine,
                        pricing.AnalyticVanillaEngine,
                        (0.02, 0.001, 0.0005),
                    ),
                    (
                        CashOrNothingOption(**terms, payout=10),
                        pricing.FiniteDifferenceDigitalEngine,
                        pricing.AnalyticDigitalEngine,
                        (0.02, 0.002, 0.0002),
                    ),
                    (
                        AssetOrNothingOption(**terms),
                        pricing.FiniteDifferenceDigitalEngine,
                        pricing.AnalyticDigitalEngine,
                        (0.1, 0.02, 0.002),
                    ),
                ):
                    with self.subTest(
                        steps=steps,
                        option_type=option_type,
                        option=type(option).__name__,
                    ):
                        engine = factory(
                            asset_step_count=1600,
                            time_step_count=steps,
                            asset_upper_boundary=400,
                        )
                        actual = engine.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        expected = analytic().price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        for name, tolerance in zip(
                            ("price", "delta", "gamma"), tolerances, strict=True
                        ):
                            self.assertAlmostEqual(
                                getattr(actual, name),
                                getattr(expected, name),
                                delta=tolerance,
                            )
