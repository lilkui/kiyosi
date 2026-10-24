import unittest
from datetime import UTC, date, datetime

from kiyosi import pricing
from test_accumulator_quadrature import contract, exposure, market, terminal_reference


class AccumulatorFiniteDifferenceAccuracyTests(unittest.TestCase):
    def test_market_greeks_and_implied_volatility_preserve_independent_results(self):
        option = contract(date(2025, 4, 1))
        context = market(100.25, datetime(2025, 1, 4, 12, tzinfo=UTC))
        engine = pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=80, time_step_count=80
        )
        joint = engine.price_with_greeks(option, context, all_greeks=True)
        self.assertIsNotNone(joint.theta)
        for greek in (
            "delta",
            "gamma",
            "speed",
            "vega",
            "vanna",
            "zomma",
            "rho",
            "theta",
            "charm",
            "color",
        ):
            with self.subTest(greek=greek):
                single = engine.price_with_greeks(option, context, greek)
                self.assertEqual(single.price, joint.price)
                self.assertEqual(getattr(single, greek), getattr(joint, greek))
        quote = engine.price(option, context)
        recovered = pricing.implied_volatility(
            engine, option, context, quote, lower_bound=0.15, upper_bound=0.25
        )
        self.assertAlmostEqual(recovered, 0.2, delta=1e-6)

    def test_terminal_averaging_converges_against_analytic_prices_and_greeks(self):
        context = market(100.25, datetime(2025, 1, 3, 12, tzinfo=UTC))
        for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
            for knock_out in (99.7, 100, 100.3, 109.75, 110, 110.25):
                with self.subTest(scheme=scheme, knock_out=knock_out):
                    option = contract(date(2025, 1, 6), knock_out)
                    expected = terminal_reference(option, context)
                    h = 0.01
                    low = terminal_reference(
                        option, market(100.25 - h, context.valuation_time)
                    )
                    high = terminal_reference(
                        option, market(100.25 + h, context.valuation_time)
                    )
                    results = [
                        pricing.FiniteDifferenceAccumulatorEngine(
                            asset_step_count=nodes,
                            time_step_count=2000,
                            scheme=scheme,
                            asset_upper_boundary=440,
                        ).price_with_greeks(option, context, ["delta", "gamma"])
                        for nodes in (400, 1600)
                    ]
                    coarse, fine = results
                    self.assertLess(abs(coarse.price - expected), 0.01)
                    self.assertLess(abs(fine.price - expected), 0.001)
                    self.assertLess(
                        abs(fine.price - expected), abs(coarse.price - expected)
                    )
                    self.assertAlmostEqual(
                        fine.delta, (high - low) / (2 * h), delta=0.002
                    )
                    self.assertAlmostEqual(
                        fine.gamma, (high - 2 * expected + low) / h**2, delta=0.002
                    )

    def test_daily_event_averaging_and_damping_stabilize_prices_and_greeks(self):
        context = market()
        reference_engine = pricing.QuadratureAccumulatorEngine(asset_step_count=1024)
        for knock_out in (109.75, 110, 110.25):
            with self.subTest(knock_out=knock_out):
                option = contract(date(2025, 4, 1), knock_out)
                reference = reference_engine.price_with_greeks(
                    option, context, ["delta", "gamma"]
                )
                for scheme in ("explicit_euler", "implicit_euler"):
                    actual = pricing.FiniteDifferenceAccumulatorEngine(
                        asset_step_count=400,
                        time_step_count=2000,
                        scheme=scheme,
                        asset_upper_boundary=440,
                    ).price(option, context)
                    self.assertLess(
                        abs(actual - reference.price) / exposure(option, context), 1e-4
                    )
                coarse, fine = [
                    pricing.FiniteDifferenceAccumulatorEngine(
                        asset_step_count=nodes,
                        time_step_count=2000,
                        asset_upper_boundary=440,
                    ).price_with_greeks(option, context, ["delta", "gamma"])
                    for nodes in (800, 3200)
                ]
                scale = exposure(option, context)
                self.assertLess(abs(coarse.price - reference.price) / scale, 2e-5)
                self.assertLess(abs(fine.price - reference.price) / scale, 1e-6)
                for greek in ("delta", "gamma"):
                    coarse_error = abs(
                        getattr(coarse, greek) - getattr(reference, greek)
                    )
                    fine_error = abs(getattr(fine, greek) - getattr(reference, greek))
                    self.assertLess(fine_error, coarse_error)
                    self.assertLess(fine_error, 0.01)
                short_grid = pricing.FiniteDifferenceAccumulatorEngine(
                    asset_step_count=1600,
                    time_step_count=200,
                    asset_upper_boundary=440,
                ).price_with_greeks(option, context, ["gamma"])
                self.assertAlmostEqual(short_grid.gamma, reference.gamma, delta=0.15)
