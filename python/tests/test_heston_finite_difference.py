from datetime import datetime

"""Heston fixed-grid pricing and Python boundary contracts."""

import inspect
import json
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import timedelta
from pathlib import Path

from kiyosi import ComputationFailureError, InvalidInputError, UnsupportedOperationError
from kiyosi.instruments import AmericanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticHestonEuropeanEngine,
    FiniteDifferenceHestonVanillaEngine,
    implied_volatility,
)
from test_heston import END, START, context, model, option, reference


class HestonFiniteDifferenceTests(unittest.TestCase):
    def test_american_reference_and_exercise_bounds(self):
        fixture = json.loads(
            (
                Path(__file__).parent
                / "quantlib_reference"
                / "heston_finite_difference.json"
            ).read_text()
        )
        reference_price = fixture["price"]
        budget = 0.001 + 1e-4 * abs(reference_price)
        self.assertAlmostEqual(
            fixture["refinement_uncertainty"],
            abs(
                fixture["extrapolated_prices"][-1] - fixture["extrapolated_prices"][-2]
            ),
            delta=1e-15,
        )
        self.assertLess(fixture["refinement_uncertainty"], budget / 10)
        engine = FiniteDifferenceHestonVanillaEngine()
        contract = AmericanOption(
            option_type="put", strike=100, effective_time=START, expiry_time=END
        )
        actual = engine.price(contract, context())
        self.assertAlmostEqual(actual, reference_price, delta=budget)
        temporal = [
            FiniteDifferenceHestonVanillaEngine(time_step_count=n).price(
                contract, context()
            )
            for n in (100, 200)
        ] + [actual]
        self.assertLess(abs(temporal[2] - temporal[1]), abs(temporal[1] - temporal[0]))
        self.assertGreaterEqual(
            actual, engine.price(option(option_type="put"), context())
        )
        call = AmericanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        snapshot = context(model(dividend_yield=0))
        expected = AnalyticHestonEuropeanEngine().price(option(), snapshot)
        self.assertAlmostEqual(
            engine.price(call, snapshot), expected, delta=0.001 + 1e-4 * expected
        )
        immediate = context(
            model(risk_free_rate=0.03, dividend_yield=0.1), spot_price=200
        )
        self.assertAlmostEqual(engine.price(call, immediate), 100, delta=1e-10)

    def test_defaults_signatures_and_lossless_conversion(self):
        engine = FiniteDifferenceHestonVanillaEngine()
        self.assertEqual(
            (
                engine.asset_step_count,
                engine.variance_step_count,
                engine.time_step_count,
            ),
            (400, 100, 400),
        )
        self.assertIsNone(engine.asset_upper_boundary)
        self.assertIsNone(engine.variance_upper_boundary)
        signature = inspect.signature(FiniteDifferenceHestonVanillaEngine)
        self.assertEqual(
            tuple(signature.parameters),
            (
                "asset_step_count",
                "variance_step_count",
                "time_step_count",
                "asset_upper_boundary",
                "variance_upper_boundary",
            ),
        )
        self.assertTrue(
            all(
                p.kind is inspect.Parameter.KEYWORD_ONLY
                for p in signature.parameters.values()
            )
        )
        for name in ("asset_step_count", "variance_step_count", "time_step_count"):
            for value, error in (
                (True, TypeError),
                (1.5, TypeError),
                ("10", TypeError),
                (1 << 100, OverflowError),
            ):
                with self.subTest(name=name, value=value), self.assertRaises(error):
                    FiniteDifferenceHestonVanillaEngine(**{name: value})
        for name in ("asset_upper_boundary", "variance_upper_boundary"):
            with self.subTest(name=name), self.assertRaises(TypeError):
                FiniteDifferenceHestonVanillaEngine(**{name: True})
            for value in (0, -1, math.nan, math.inf):
                with (
                    self.subTest(name=name, value=value),
                    self.assertRaises(InvalidInputError),
                ):
                    FiniteDifferenceHestonVanillaEngine(**{name: value})
        for kwargs in (
            {"asset_step_count": 2},
            {"variance_step_count": 2},
            {"time_step_count": 0},
        ):
            with self.subTest(kwargs=kwargs), self.assertRaises(InvalidInputError):
                FiniteDifferenceHestonVanillaEngine(**kwargs)
        with self.assertRaises(AttributeError):
            engine.time_step_count = 1

    def test_european_prices_match_shared_independent_anchors(self):
        engine = FiniteDifferenceHestonVanillaEngine()
        for changes, expected in (
            ({}, 9.01127840754174),
            ({"vol_of_vol": 0.9}, 7.783760588260141),
            (
                {"mean_reversion_speed": 0.2, "vol_of_vol": 1.0, "correlation": -0.9},
                6.256279042682394,
            ),
        ):
            parameters = model(**changes)
            snapshot = context(parameters)
            for side in ("call", "put"):
                contract = option(option_type=side)
                ref = (
                    expected
                    if side == "call"
                    else expected - 100 * (math.exp(-0.02) - math.exp(-0.05))
                )
                with self.subTest(changes=changes, side=side):
                    self.assertAlmostEqual(
                        engine.price(contract, snapshot),
                        ref,
                        delta=0.001 + 1e-4 * abs(ref),
                    )

    def test_absorbing_zero_and_node_curves_keep_interior_exercise(self):
        parameters = model(
            risk_free_rate=0.05,
            dividend_yield=0.2,
            initial_variance=0,
            long_run_variance=0,
            mean_reversion_speed=0,
        )
        snapshot = context(parameters, spot_price=30)
        contract = AmericanOption(
            option_type="put",
            strike=100,
            effective_time=START,
            expiry_time=datetime(2028, 1, 1),
        )
        optimum = math.log(5 / 6) / -0.15
        expected = 100 * math.exp(-0.05 * optimum) - 30 * math.exp(-0.2 * optimum)
        engine = FiniteDifferenceHestonVanillaEngine(time_step_count=1)
        self.assertAlmostEqual(engine.price(contract, snapshot), expected, delta=1e-12)
        self.assertGreater(expected, 70)
        for side, rate, dividend in (("call", 1e-18, 0), ("put", 0, 1e-18)):
            parameters = model(
                risk_free_rate=rate,
                dividend_yield=dividend,
                initial_variance=0,
                long_run_variance=0,
            )
            self.assertAlmostEqual(
                engine.price(
                    option(option_type=side, strike=1e18),
                    context(parameters, spot_price=1e18),
                ),
                1,
                delta=1e-12,
            )
        middle = datetime(2025, 7, 1)
        curve = RateTermStructure(
            reference_time=START, times=[middle, END], factors=[math.exp(0.1), 1]
        )
        parameters = model(
            risk_free_rate=curve,
            dividend_yield=0,
            initial_variance=0,
            long_run_variance=0,
        )
        contract = AmericanOption(
            option_type="put", strike=100, effective_time=START, expiry_time=END
        )
        self.assertAlmostEqual(
            engine.price(contract, context(parameters, spot_price=0.01)),
            100 * math.exp(0.1) - 0.01,
            delta=1e-12,
        )

    def test_expiry_applicability_errors_and_state(self):
        engine = FiniteDifferenceHestonVanillaEngine()
        snapshot = context()
        saved_model = snapshot.model_parameters
        end = snapshot.replace(valuation_time=END, spot_price=110)
        self.assertEqual(engine.price(option(), end), 10)
        for when in (END + timedelta(microseconds=1), END + timedelta(days=1)):
            with self.subTest(when=when), self.assertRaises(InvalidInputError):
                engine.price(option(), snapshot.replace(valuation_time=when))
        bsm = end.replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            )
        )
        with self.assertRaises(UnsupportedOperationError):
            engine.price(option(), bsm)
        result = engine.price_with_greeks(option(), snapshot, "delta")
        self.assertIsNone(result.delta)
        with self.assertRaises(UnsupportedOperationError):
            implied_volatility(
                engine, option(), snapshot, 9, lower_bound=0.0, upper_bound=4.0
            )
        for controls in (
            {"asset_upper_boundary": 100},
            {"variance_upper_boundary": 0.04},
        ):
            with self.subTest(controls=controls), self.assertRaises(InvalidInputError):
                FiniteDifferenceHestonVanillaEngine(**controls).price(
                    option(), snapshot
                )
        broken = FiniteDifferenceHestonVanillaEngine(
            asset_step_count=3,
            variance_step_count=3,
            time_step_count=1,
            asset_upper_boundary=1e308,
        )
        for _ in range(2):
            with self.assertRaises(ComputationFailureError):
                broken.price(option(), snapshot)
        extreme = context(
            model(
                risk_free_rate=0,
                dividend_yield=0,
                initial_variance=2000,
                long_run_variance=2000,
                mean_reversion_speed=0,
                vol_of_vol=0,
            ),
            spot_price=1e200,
        )
        put = option(option_type="put", strike=1e-200)
        self.assertGreater(AnalyticHestonEuropeanEngine().price(put, extreme), 0.9e-200)
        with self.assertRaises(ComputationFailureError):
            engine.price(put, extreme)
        self.assertEqual(snapshot.model_parameters, saved_model)
        self.assertEqual(snapshot.spot_price, 100)
        self.assertEqual(engine.time_step_count, 400)
        short = RateTermStructure(
            reference_time=START, times=[datetime(2025, 7, 1)], factors=[0.98]
        )
        with self.assertRaises(InvalidInputError):
            engine.price(
                option(), end.replace(model_parameters=model(risk_free_rate=short))
            )

    def test_shared_engine_concurrency_and_price_scaling(self):
        engine = FiniteDifferenceHestonVanillaEngine(
            asset_step_count=100, variance_step_count=25, time_step_count=50
        )
        expected = engine.price(option(), context())
        with ThreadPoolExecutor(max_workers=4) as pool:
            actual = list(
                pool.map(lambda _: engine.price(option(), context()), range(4))
            )
        self.assertEqual(actual, [expected] * 4)
        for scale in (1e-100, 1e100):
            with self.subTest(scale=scale):
                value = engine.price(
                    option(strike=100 * scale), context(spot_price=100 * scale)
                )
                self.assertAlmostEqual(value / scale, expected, delta=1e-11)

    def test_deterministic_variance_and_nonabsorbing_zero(self):
        engine = FiniteDifferenceHestonVanillaEngine()
        for xi in (1e-8, 1e-200):
            with self.subTest(xi=xi):
                self.assertAlmostEqual(
                    engine.price(option(), context(model(vol_of_vol=xi))),
                    9.227005508154036,
                    delta=0.002,
                )
        for changes in (
            {"vol_of_vol": 0, "initial_variance": 0.01, "long_run_variance": 0.09},
            {"initial_variance": 0},
            {"correlation": -1},
            {"correlation": 1},
        ):
            snapshot = context(model(**changes))
            ref = AnalyticHestonEuropeanEngine().price(option(), snapshot)
            with self.subTest(changes=changes):
                self.assertAlmostEqual(
                    engine.price(option(), snapshot), ref, delta=0.001 + 1e-4 * ref
                )

    def test_maturity_moneyness_and_nonflat_rate_prices(self):
        engine = FiniteDifferenceHestonVanillaEngine()
        for expiry, strike in (
            (START + timedelta(days=7), 100),
            (datetime(2030, 1, 1), 120),
            (END, 50),
            (END, 150),
        ):
            contract = option(expiry_time=expiry, strike=strike)
            ref = AnalyticHestonEuropeanEngine().price(contract, context())
            with self.subTest(expiry=expiry, strike=strike):
                self.assertAlmostEqual(
                    engine.price(contract, context()),
                    ref,
                    delta=1e-5 * strike + 1e-4 * ref,
                )
        risk = RateTermStructure(
            reference_time=START,
            times=[datetime(2025, 7, 1), END],
            factors=[1.01, 0.98],
        )
        dividend = RateTermStructure(
            reference_time=START,
            times=[datetime(2025, 4, 1), END],
            factors=[0.99, 0.97],
        )
        snapshot = context(model(risk_free_rate=risk, dividend_yield=dividend))
        ref = AnalyticHestonEuropeanEngine().price(option(), snapshot)
        self.assertAlmostEqual(
            engine.price(option(), snapshot), ref, delta=0.001 + 1e-4 * ref
        )

    def test_spatial_time_and_domain_refinement_are_checked_separately(self):
        snapshot = context()
        ref = 9.01127840754174
        base = {
            "asset_step_count": 400,
            "variance_step_count": 100,
            "time_step_count": 100,
        }

        def price(**changes):
            return FiniteDifferenceHestonVanillaEngine(**(base | changes)).price(
                option(), snapshot
            )

        spatial = [price(asset_step_count=n) for n in (100, 200, 400)]
        self.assertLess(abs(spatial[2] - ref), abs(spatial[1] - ref))
        self.assertLess(abs(spatial[1] - ref), abs(spatial[0] - ref))
        variance = [price(variance_step_count=n) for n in (25, 50, 100)]
        self.assertLess(abs(variance[2] - variance[1]), abs(variance[1] - variance[0]))
        temporal = [price(time_step_count=n) for n in (25, 50, 100)]
        self.assertLess(
            abs(temporal[2] - temporal[1]), 0.4 * abs(temporal[1] - temporal[0])
        )
        for controls in (
            {"asset_upper_boundary": 800},
            {"variance_upper_boundary": 0.6},
        ):
            with self.subTest(controls=controls):
                enlarged = price(**controls)
                self.assertAlmostEqual(enlarged, spatial[-1], delta=0.0005)
                self.assertAlmostEqual(enlarged, ref, delta=0.001 + 1e-4 * ref)

    def test_refined_tail_price_uses_relative_acceptance(self):
        contract = option(
            option_type="put", strike=70, expiry_time=START + timedelta(days=30)
        )
        expected = 3.105513031411244e-6
        estimates = [
            reference(model(), contract, tolerance) for tolerance in (1e-10, 1e-12)
        ]
        self.assertLess(abs(estimates[1] - estimates[0]), expected * 0.001)
        self.assertAlmostEqual(estimates[1], expected, delta=expected * 0.001)
        engine = FiniteDifferenceHestonVanillaEngine(
            asset_step_count=1200, variance_step_count=200, time_step_count=100
        )
        actual = engine.price(contract, context())
        self.assertLess(abs(actual / expected - 1), 0.01)


if __name__ == "__main__":
    unittest.main()
