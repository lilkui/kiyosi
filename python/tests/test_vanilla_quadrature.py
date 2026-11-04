"""Vanilla quadrature contracts and independent numerical references."""

import inspect
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import RateTermStructure
from quantlib_reference import american, oracle
from support.terminal import assert_terminal_greeks
from test_binomial import END, GREEKS, START, contract, market


class VanillaQuadratureTests(unittest.TestCase):
    def test_settings_signatures_and_conversion_boundaries(self):
        factory = pricing.QuadratureVanillaEngine
        engine = factory()
        self.assertEqual(engine.asset_step_count, 512)
        self.assertEqual(engine.time_step_count, 200)
        self.assertEqual(engine.integration_standard_deviations, 8)
        signature = inspect.signature(factory)
        self.assertEqual(
            tuple(signature.parameters),
            ("asset_step_count", "time_step_count", "integration_standard_deviations"),
        )
        for name in signature.parameters:
            self.assertEqual(signature.parameters[name].default, getattr(engine, name))
            self.assertIs(
                signature.parameters[name].kind, inspect.Parameter.KEYWORD_ONLY
            )
        self.assertEqual(
            repr(engine),
            "QuadratureVanillaEngine(asset_step_count=512, time_step_count=200, "
            "integration_standard_deviations=8)",
        )
        for field in ("asset_step_count", "time_step_count"):
            for value, error in (
                (True, TypeError),
                (4.5, TypeError),
                (2**31, OverflowError),
            ):
                with self.subTest(field=field, value=value), self.assertRaises(error):
                    factory(**{field: value})
        for fields in (
            {"asset_step_count": 3},
            {"time_step_count": 0},
            {"integration_standard_deviations": 0},
            {"integration_standard_deviations": math.nan},
            {"integration_standard_deviations": math.inf},
        ):
            with self.subTest(fields=fields), self.assertRaises(InvalidInputError):
                factory(**fields)
        for value in (True, "8", None):
            with self.assertRaises(TypeError):
                factory(integration_standard_deviations=value)
        with self.assertRaises(AttributeError):
            engine.time_step_count = 400

    def test_european_prices_and_all_greeks_match_analytic(self):
        engine = pricing.QuadratureVanillaEngine()
        analytic = pricing.AnalyticEuropeanEngine()
        for direction in ("call", "put"):
            for spot, rate, dividend, sigma in (
                (80, 0.04, 0.01, 0.3),
                (100, 0.05, 0.02, 0.2),
                (120, -0.02, -0.03, 0.4),
                (100, 0, 0, 0.01),
            ):
                option = contract(EuropeanOption, direction)
                context = market(spot, rate, dividend, sigma)
                actual = engine.price_with_greeks(option, context, "all")
                expected = analytic.price_with_greeks(option, context, "all")
                self.assertAlmostEqual(actual.price, expected.price, delta=1e-6)
                for name in GREEKS:
                    with self.subTest(direction=direction, spot=spot, greek=name):
                        self.assertAlmostEqual(
                            getattr(actual, name), getattr(expected, name), delta=1e-4
                        )

    def test_time_space_and_extent_converge_separately(self):
        option, context = contract(AmericanOption, "put"), market()

        def price(space, time, extent):
            return pricing.QuadratureVanillaEngine(
                asset_step_count=space,
                time_step_count=time,
                integration_standard_deviations=extent,
            ).price(option, context)

        inputs = {
            "option": "put",
            "strike": 100,
            "spot": 100,
            "rate": 0.05,
            "dividend": 0.02,
            "volatility": 0.2,
            "effective_time": START.date().isoformat(),
            "valuation": START.date().isoformat(),
            "expiry_time": END.date().isoformat(),
        }
        with ql.SavedSettings():
            reference = american.measure(inputs, "price")
        for coarse, fine in (
            ((128, 800, 8), (768, 800, 8)),
            ((768, 25, 8), (768, 800, 8)),
            ((768, 800, 1), (768, 800, 8)),
        ):
            coarse_error = abs(price(*coarse) - reference)
            fine_error = abs(price(*fine) - reference)
            self.assertLess(fine_error, coarse_error)
            self.assertLess(fine_error, 0.01)
        european = contract(EuropeanOption)
        self.assertEqual(
            pricing.QuadratureVanillaEngine(time_step_count=1).price(european, context),
            pricing.QuadratureVanillaEngine(time_step_count=400).price(
                european, context
            ),
        )

    def test_quantlib_european_and_american_reference_profiles(self):
        oracle.require_version()
        american_config = oracle.load_fixture("american.json")
        for kind, rows, profiles, reference_fn in (
            (
                EuropeanOption,
                oracle.load_fixture("scenarios.json")["scenarios"],
                oracle.load_fixture("numerical_engines.json")["profiles"],
                oracle.reference,
            ),
            (
                AmericanOption,
                american_config["scenarios"],
                american_config["profiles"],
                american.reference,
            ),
        ):
            profile = next(
                p for p in profiles if p["engine"] == "QuadratureVanillaEngine"
            )
            engine = pricing.QuadratureVanillaEngine(**profile["settings"])
            for scenario in rows:
                inputs = scenario["inputs"]
                option = kind(
                    option_type=inputs["option"],
                    strike=inputs["strike"],
                    effective_time=datetime.fromisoformat(inputs["effective_time"]),
                    expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
                )
                context = market(
                    inputs["spot"],
                    inputs["rate"],
                    inputs["dividend"],
                    inputs["volatility"],
                    datetime.fromisoformat(inputs["valuation"]),
                )
                with ql.SavedSettings():
                    reference, metadata = reference_fn(inputs)
                if kind is EuropeanOption and oracle.exclusions(inputs):
                    reference = {
                        k: v
                        for k, v in reference.items()
                        if k in ("price", "delta", "gamma")
                    }
                result = engine.price_with_greeks(
                    option,
                    context,
                    sorted(reference.keys() - {"price"}),
                    **profile["shifts"],
                )
                for name, expected in reference.items():
                    with self.subTest(case=scenario["case_id"], measure=name):
                        self.assertAlmostEqual(
                            getattr(result, name),
                            expected,
                            delta=profile["numerical_tolerances"][name]
                            + metadata[f"uncertainty_{name}"],
                        )

    def test_implied_volatility_reprices_both_exercise_styles(self):
        engine, context = pricing.QuadratureVanillaEngine(), market()
        for kind in (EuropeanOption, AmericanOption):
            option = contract(kind, "put")
            quote = engine.price(option, context)
            sigma = pricing.implied_volatility(
                engine,
                option,
                context,
                quote,
                lower_bound=0.1,
                upper_bound=0.3,
                price_tolerance=1e-8,
            )
            self.assertAlmostEqual(sigma, 0.2, delta=1e-6)

    def test_american_negative_carry_and_european_limit(self):
        engine = pricing.QuadratureVanillaEngine(
            asset_step_count=768,
            time_step_count=800,
            integration_standard_deviations=10,
        )
        for direction, spot, rate, dividend in (
            ("put", 80, -0.02, -0.05),
            ("call", 120, -0.04, -0.01),
            ("call", 100, 0.05, 0),
        ):
            option = contract(AmericanOption, direction)
            context = market(spot, rate, dividend, 0.3)
            inputs = {
                "option": direction,
                "strike": 100,
                "spot": spot,
                "rate": rate,
                "dividend": dividend,
                "volatility": 0.3,
                "effective_time": START.date().isoformat(),
                "valuation": START.date().isoformat(),
                "expiry_time": END.date().isoformat(),
            }
            with ql.SavedSettings():
                expected = american.measure(inputs, "price")
            price = engine.price(option, context)
            self.assertAlmostEqual(price, expected, delta=0.01)
            if direction == "call" and dividend == 0:
                european = pricing.AnalyticEuropeanEngine().price(
                    contract(EuropeanOption), context
                )
                self.assertAlmostEqual(price, european, delta=0.0005)

    def test_american_volga_and_dividend_rho_match_quantlib_bumps(self):
        engine = pricing.QuadratureVanillaEngine(
            asset_step_count=768,
            time_step_count=800,
            integration_standard_deviations=10,
        )
        inputs = {
            "option": "put",
            "strike": 100,
            "spot": 100,
            "rate": 0.04,
            "dividend": 0.01,
            "volatility": 0.3,
            "effective_time": START.date().isoformat(),
            "valuation": START.date().isoformat(),
            "expiry_time": END.date().isoformat(),
        }
        with ql.SavedSettings():
            base = american.measure(inputs, "price")
            vol_up = american.measure(dict(inputs, volatility=0.31), "price")
            vol_down = american.measure(dict(inputs, volatility=0.29), "price")
            q_up = american.measure(dict(inputs, dividend=0.011), "price")
            q_down = american.measure(dict(inputs, dividend=0.009), "price")
        result = engine.price_with_greeks(
            contract(AmericanOption, "put"),
            market(100, 0.04, 0.01, 0.3),
            ["volga", "dividend_rho"],
            volatility_shift=0.01,
            rate_shift=0.001,
        )
        self.assertAlmostEqual(result.volga, vol_up - 2 * base + vol_down, delta=5e-5)
        self.assertAlmostEqual(result.dividend_rho, (q_up - q_down) / 0.2, delta=0.002)

    def test_life_curve_rejection_and_numerical_failures_preserve_state(self):
        engine = pricing.QuadratureVanillaEngine()
        curve = RateTermStructure(
            reference_time=START,
            times=[END, END + (END - START)],
            factors=[math.exp(-0.05), (math.exp(-0.05)) * 0.5],
        )
        for kind in (EuropeanOption, AmericanOption):
            option = contract(kind, "put")
            baseline = engine.price(option, market())
            for at in (START - timedelta(days=1), END + timedelta(days=1)):
                with self.assertRaises(InvalidInputError):
                    engine.price(option, market(valuation=at))
            expiry = engine.price_with_greeks(
                option, market(spot=80, valuation=END), "all"
            )
            self.assertEqual(expiry.price, 20)
            assert_terminal_greeks(self, expiry, -1)
            for at in (START, END):
                for fields in ({"rate": curve}, {"dividend": curve}):
                    context = market(valuation=at, **fields)
                    with self.assertRaises(UnsupportedOperationError):
                        engine.price(option, context)
                    with self.assertRaises(UnsupportedOperationError):
                        engine.price_with_greeks(option, context, [])
            with self.assertRaises(ComputationFailureError):
                engine.price(option, market(sigma=1e200))
            almost_expired = datetime(2025, 12, 31, 23, 59, 59, 999999)
            self.assertGreaterEqual(
                engine.price(option, market(spot=80, valuation=almost_expired)), 19.999
            )
            self.assertEqual(engine.price(option, market()), baseline)

    def test_shared_engine_supports_concurrent_pricing(self):
        engine = pricing.QuadratureVanillaEngine(
            asset_step_count=128, time_step_count=50
        )
        option = contract(AmericanOption, "put")
        contexts = [market(spot=spot) for spot in (80, 100, 120)]
        expected = [engine.price(option, context) for context in contexts]
        with ThreadPoolExecutor(max_workers=3) as pool:
            actual = list(
                pool.map(lambda context: engine.price(option, context), contexts)
            )
        self.assertEqual(actual, expected)
