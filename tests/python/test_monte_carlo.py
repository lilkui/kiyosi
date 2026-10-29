"""European CPU Monte Carlo contracts through the installed public API."""

import importlib
import math
import sys
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta
from fractions import Fraction
from pathlib import Path

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    AnalyticVanillaEngine,
    MonteCarloVanillaEngine,
    implied_volatility,
)
from test_binomial import END, GREEKS, START, market


def contract(direction="call", strike=100):
    return EuropeanOption(
        option_type=direction, strike=strike, effective_date=START, expiry_date=END
    )


class MonteCarloTests(unittest.TestCase):
    def assert_rejection(self, expected, operation, *args, **kwargs):
        with self.assertRaises(expected) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def test_defaults_terminal_prices_and_statistics(self):
        default = MonteCarloVanillaEngine()
        self.assertEqual(
            (
                default.path_count,
                default.step_count,
                default.seed,
                default.thread_count,
            ),
            (100_000, 50, None, 0),
        )
        for direction, reference in (
            ("call", 9.227005508154036),
            ("put", 6.330080627549918),
        ):
            engine = MonteCarloVanillaEngine(seed=42, thread_count=4)
            option = contract(direction)
            estimate = engine.price_with_statistics(option, market())
            self.assertEqual(estimate.price, engine.price(option, market()))
            self.assertAlmostEqual(estimate.price, reference, delta=0.2)
            self.assertGreater(estimate.standard_error, 0)
            lower, upper = estimate.confidence_interval
            self.assertAlmostEqual(
                upper - lower, 2 * 1.959963984540054 * estimate.standard_error
            )
            for steps in (2, 7, 10_000):
                other = MonteCarloVanillaEngine(seed=42, step_count=steps)
                self.assertEqual(other.price(option, market()), estimate.price)
            one = MonteCarloVanillaEngine(path_count=1, seed=42)
            estimate = one.price_with_statistics(option, market())
            self.assertIsNone(estimate.standard_error)
            self.assertIsNone(estimate.confidence_interval)

    def test_strict_representations_and_domain_boundaries(self):
        for field in ("path_count", "step_count", "thread_count", "seed"):
            for value in (True, False, 1.5, 2.0, Fraction(2, 1), "2", object()):
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(TypeError),
                ):
                    MonteCarloVanillaEngine(**{field: value})
            for value in (-(2**64), 2**64):
                with self.assertRaises(OverflowError):
                    MonteCarloVanillaEngine(**{field: value})
        for field, values in (
            ("path_count", (0, -1, 10_000_001)),
            ("step_count", (0, 1, 10_001)),
            ("thread_count", (-1,)),
        ):
            for value in values:
                self.assert_rejection(
                    InvalidInputError,
                    MonteCarloVanillaEngine,
                    **{field: value},
                )
        for seed in (-1, 2**64):
            with self.assertRaises(OverflowError):
                MonteCarloVanillaEngine(seed=seed)
        for paths in (1, 10_000_000):
            for steps in (2, 10_000):
                engine = MonteCarloVanillaEngine(
                    path_count=paths,
                    step_count=steps,
                    seed=2**64 - 1,
                    thread_count=2**31 - 1,
                )
                self.assertEqual(
                    (
                        engine.path_count,
                        engine.step_count,
                        engine.seed,
                        engine.thread_count,
                    ),
                    (paths, steps, 2**64 - 1, 2**31 - 1),
                )
        for kwargs in ({"backend": "cuda"},):
            with self.assertRaises(TypeError):
                MonteCarloVanillaEngine(**kwargs)
        with self.assertRaises(TypeError):
            MonteCarloVanillaEngine(100)

    def test_exact_path_counts_and_thread_block_boundaries(self):
        option, context = contract(), market()
        for paths in (1, 2, 255, 256, 257, 511, 512, 513, 769, 1025, 3073, 4097):
            for seed in (0, 42, 2**64 - 1):
                engine = MonteCarloVanillaEngine(
                    path_count=paths, seed=seed, thread_count=1
                )
                expected = engine.price_with_statistics(option, context)
                expected = (
                    expected.price,
                    expected.standard_error,
                    expected.confidence_interval,
                )
                for threads in (0, 2, 4, 2**31 - 1):
                    other = MonteCarloVanillaEngine(
                        path_count=paths, seed=seed, thread_count=threads
                    )
                    actual = other.price_with_statistics(option, context)
                    self.assertEqual(
                        (
                            actual.price,
                            actual.standard_error,
                            actual.confidence_interval,
                        ),
                        expected,
                    )
        option = contract(strike=1)
        first = MonteCarloVanillaEngine(path_count=1, seed=42).price(option, context)
        pair = MonteCarloVanillaEngine(path_count=2, seed=42).price_with_statistics(
            option, context
        )
        # For two observations the standard error equals half their absolute difference.
        self.assertAlmostEqual(
            pair.standard_error, abs(first - pair.price), delta=1e-12
        )

    def test_shared_sample_greeks_match_independent_scalar_stencils(self):

        differences = importlib.import_module("price_differences").price_differences
        shifts = {"spot_shift": 1, "volatility_shift": 0.01, "rate_shift": 0.001}
        for direction in ("call", "put"):
            option = contract(direction)
            for paths in (1, 257, 1025):
                for seed in (42, 2**64 - 1):
                    engine = MonteCarloVanillaEngine(
                        path_count=paths, seed=seed, thread_count=4
                    )
                    context = market(
                        spot=100.25, valuation=datetime(2025, 1, 1, 12, tzinfo=UTC)
                    )
                    expected = differences(engine, option, context, **shifts)
                    actual = engine.price_with_greeks(
                        option, context, all_greeks=True, **shifts
                    )
                    self.assertEqual(actual.price, expected.price)
                    for name in GREEKS:
                        self.assertAlmostEqual(
                            getattr(actual, name), getattr(expected, name), delta=1e-9
                        )
                        selected = engine.price_with_greeks(
                            option, context, name, **shifts
                        )
                        self.assertEqual(selected.require(name), getattr(actual, name))
        engine = MonteCarloVanillaEngine(path_count=1025)
        result = engine.price_with_greeks(
            contract(strike=1e-100), market(), ["delta", "gamma"]
        )
        self.assertAlmostEqual(result.delta, result.price / 100, delta=1e-9)
        self.assertAlmostEqual(result.gamma, 0, delta=1e-7)
        self.assertIsNone(engine.seed)

    def test_expiry_validation_failures_and_ownership(self):
        engine = MonteCarloVanillaEngine(path_count=257, seed=42, thread_count=4)
        option, context = contract(), market()
        quote = engine.price(option, context)
        expiry = market(spot=110, valuation=END)
        estimate = engine.price_with_statistics(option, expiry)
        self.assertEqual(
            (estimate.price, estimate.standard_error, estimate.confidence_interval),
            (10, 0, (10, 10)),
        )
        result = engine.price_with_greeks(option, expiry, all_greeks=True)
        self.assertEqual(result.price, 10)
        for name in GREEKS:
            self.assertIsNone(getattr(result, name))
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            option,
            expiry,
            10,
        )
        for valuation in (
            START - timedelta(days=1),
            datetime(2026, 1, 1, 0, 0, 0, 1, tzinfo=UTC),
        ):
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                option,
                market(valuation=valuation),
            )
        before = datetime(2025, 12, 31, 23, 59, 59, 999999, tzinfo=UTC)
        self.assertGreaterEqual(engine.price(option, market(valuation=before)), 0)
        nodes = RateTermStructure(reference_time=START, times=[END], factors=[0.95])
        node_context = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=nodes),
            valuation_time=END,
        )
        self.assert_rejection(
            UnsupportedOperationError, engine.price, option, node_context
        )
        self.assert_rejection(
            UnsupportedOperationError,
            engine.price_with_greeks,
            option,
            node_context,
            [],
        )
        for invalid_quote in (0, math.nan):
            self.assert_rejection(
                UnsupportedOperationError,
                implied_volatility,
                engine,
                option,
                node_context,
                invalid_quote,
                max_iterations=0,
            )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            context,
            [],
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            context,
            "delta",
            spot_shift=0,
        )
        self.assert_rejection(
            ComputationFailureError,
            engine.price_with_greeks,
            option,
            context,
            "rho",
            rate_shift=1000,
        )
        self.assert_rejection(
            ComputationFailureError, engine.price, option, market(sigma=1e308)
        )
        missing = engine.price_with_greeks(
            option, context, ["delta", "vega"], spot_shift=100, volatility_shift=0.2
        )
        self.assertIsNone(missing.delta)
        self.assertIsNone(missing.vega)
        self.assertEqual(missing.price, quote)
        with self.assertRaises(TypeError):
            engine.price_with_greeks(option, context, "delta", spot_shift=True)
        self.assertEqual(engine.price(option, context), quote)
        for obj, field in (
            (engine, "path_count"),
            (estimate, "price"),
            (result, "delta"),
        ):
            with self.assertRaises(AttributeError):
                setattr(obj, field, 0)
        del engine, option, context, expiry
        self.assertEqual(estimate.confidence_interval, (10, 10))
        self.assertEqual(result.price, 10)

    def test_implied_volatility_seed_sharing_failures_and_thread_invariance(self):
        option, context = contract(), market()
        for seed in (42, 2**64 - 1):
            engine = MonteCarloVanillaEngine(path_count=4097, seed=seed, thread_count=1)
            quote = engine.price(option, context)
            bounds = {"lower_bound": 0.1, "upper_bound": 0.4}
            expected = implied_volatility(engine, option, context, quote, **bounds)
            self.assertAlmostEqual(expected, 0.2, delta=1e-8)
            for threads in (0, 2, 4):
                other = MonteCarloVanillaEngine(
                    path_count=4097, seed=seed, thread_count=threads
                )
                self.assertEqual(
                    implied_volatility(other, option, context, quote, **bounds),
                    expected,
                )
            self.assert_rejection(
                RootNotBracketedError,
                implied_volatility,
                engine,
                option,
                context,
                -1,
                **bounds,
            )
            self.assert_rejection(
                InvalidInputError,
                implied_volatility,
                engine,
                option,
                context,
                math.nan,
            )
            self.assert_rejection(
                InvalidInputError,
                implied_volatility,
                engine,
                option,
                context,
                quote,
                max_iterations=0,
            )
            self.assert_rejection(
                NonConvergenceError,
                implied_volatility,
                engine,
                option,
                context,
                quote,
                lower_bound=0.11,
                upper_bound=0.23,
                max_iterations=1,
                price_tolerance=1e-15,
                parameter_tolerance=1e-15,
            )
            with self.assertRaises(TypeError):
                implied_volatility(engine, option, context, quote, max_iterations=True)
            self.assertEqual(engine.price(option, context), quote)
            self.assertEqual(engine.seed, seed)
        unseeded = MonteCarloVanillaEngine(path_count=4097, thread_count=4)
        sigma = implied_volatility(
            unseeded,
            option,
            context,
            9.227005508154036,
            lower_bound=0.1,
            upper_bound=0.4,
        )
        self.assertTrue(0.1 < sigma < 0.4)
        self.assertIsNone(unseeded.seed)
        self.assertEqual(context.model_parameters.volatility, 0.2)

    def test_existing_quantlib_monte_carlo_profile(self):
        root = Path(__file__).resolve().parent

        scenarios = importlib.import_module("quantlib_reference.scenarios")
        oracle = importlib.import_module("quantlib_reference.oracle")
        oracle.require_version()
        profile = next(
            p
            for p in scenarios.numerical_profiles(
                root / "quantlib_reference/numerical_engines.json"
            )
            if p["engine"] == "MonteCarloVanillaEngine"
        )
        for scenario in scenarios.load_scenarios(
            root / "quantlib_reference/scenarios.json"
        ):
            inputs = scenario["inputs"]
            if oracle.exclusions(inputs):
                continue
            option = EuropeanOption(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_date=date.fromisoformat(inputs["effective_date"]),
                expiry_date=date.fromisoformat(inputs["expiry_date"]),
            )
            context = market(
                inputs["spot"],
                inputs["rate"],
                inputs["dividend"],
                inputs["volatility"],
                date.fromisoformat(inputs["valuation"]),
            )
            with ql.SavedSettings():
                reference, uncertainty = oracle.reference(inputs)
            engine = MonteCarloVanillaEngine(**profile["settings"], thread_count=4)
            # Retain the legacy wrapper matrix; wide bumps are biased at one-day expiries.
            wrapped = inputs["spot"] == 100 and inputs["expiry_date"] == "2026-01-06"
            names = sorted(reference.keys() - {"price"}) if wrapped else []
            result = (
                engine.price_with_greeks(option, context, names, **profile["shifts"])
                if wrapped
                else None
            )
            for name, expected in reference.items():
                if name != "price" and name not in names:
                    continue
                with self.subTest(case=scenario["case_id"], measure=name):
                    self.assertAlmostEqual(
                        getattr(result, name)
                        if result is not None
                        else engine.price(option, context),
                        expected,
                        delta=profile["numerical_tolerances"][name]
                        + uncertainty[f"uncertainty_{name}"],
                    )

    def test_statistical_coverage_and_inverse_square_root_error(self):
        for antithetic in (False, True):
            observations, deviation = (
                (4096, 7.286842354) if antithetic else (8192, 13.831466750894657)
            )
            option, context = contract(), market()
            covered, standardized = 0, []
            for seed in range(32):
                engine = MonteCarloVanillaEngine(
                    path_count=8192, seed=seed, thread_count=1, antithetic=antithetic
                )
                estimate = engine.price_with_statistics(option, context)
                self.assertAlmostEqual(
                    estimate.standard_error * math.sqrt(observations),
                    deviation,
                    delta=1.5,
                )
                lower, upper = estimate.confidence_interval
                covered += lower <= 9.227005508154036 <= upper
                standardized.append(
                    (estimate.price - 9.227005508154036) / estimate.standard_error
                )
            self.assertGreaterEqual(covered, 26)
            self.assertLess(abs(sum(standardized) / len(standardized)), 0.5)
            small = MonteCarloVanillaEngine(
                path_count=8192, seed=42, antithetic=antithetic
            ).price_with_statistics(option, context)
            large = MonteCarloVanillaEngine(
                path_count=32768, seed=42, antithetic=antithetic
            ).price_with_statistics(option, context)
            self.assertAlmostEqual(
                small.standard_error / large.standard_error, 2, delta=0.15
            )

    def test_concurrent_shared_and_distinct_engines(self):
        option, context = contract("put"), market()
        engine = MonteCarloVanillaEngine(path_count=4097, seed=42, thread_count=4)
        expected = engine.price_with_greeks(option, context, all_greeks=True)

        def compute(index):
            selected = (
                engine
                if index % 2
                else MonteCarloVanillaEngine(path_count=4097, seed=42, thread_count=1)
            )
            result = selected.price_with_greeks(option, context, all_greeks=True)
            return result.price, tuple(result.require(name) for name in GREEKS)

        with ThreadPoolExecutor(max_workers=4) as executor:
            self.assertEqual(
                list(executor.map(compute, range(8))),
                [(expected.price, tuple(expected.require(name) for name in GREEKS))]
                * 8,
            )

    def test_core_calls_release_the_gil(self):
        engine = MonteCarloVanillaEngine(path_count=100_000, seed=42, thread_count=1)
        option, context = contract(), market()
        quote = engine.price(option, context)
        operations = (
            lambda: engine.price(option, context),
            lambda: engine.price_with_statistics(option, context),
            lambda: engine.price_with_greeks(option, context, "delta"),
            lambda: implied_volatility(
                engine, option, context, quote, lower_bound=0.1, upper_bound=0.4
            ),
        )
        for operation in operations:
            ready, start, observed = (
                threading.Event(),
                threading.Event(),
                threading.Event(),
            )

            def python_worker(ready=ready, start=start, observed=observed):
                ready.set()
                start.wait()
                observed.set()

            worker = threading.Thread(target=python_worker)
            worker.start()
            ready.wait()
            previous = sys.getswitchinterval()
            try:
                # Prevent a bytecode scheduling handoff between start.set() and the native call.
                sys.setswitchinterval(100)
                start.set()
                operation()
                self.assertTrue(
                    observed.is_set(), "Python worker could not run during core work"
                )
            finally:
                sys.setswitchinterval(previous)
                start.set()
                worker.join()


class SamplingTests(unittest.TestCase):
    def test_sampling_choices_and_exact_representations(self):
        from typing import get_args

        from kiyosi.pricing import MonteCarloSampling

        self.assertEqual(get_args(MonteCarloSampling), ("pseudo_random", "sobol"))
        default = MonteCarloVanillaEngine()
        self.assertEqual(default.sampling, "pseudo_random")
        self.assertIs(default.antithetic, False)
        for antithetic in (False, True):
            engine = MonteCarloVanillaEngine(
                path_count=1024, sampling="sobol", antithetic=antithetic
            )
            self.assertEqual(engine.sampling, "sobol")
            self.assertIs(engine.antithetic, antithetic)
            for field in ("sampling", "antithetic"):
                with self.assertRaises(AttributeError):
                    setattr(engine, field, getattr(engine, field))
        for value in (True, 1, 1.0, b"sobol", None, []):
            with self.subTest(value=value), self.assertRaises(TypeError):
                MonteCarloVanillaEngine(path_count=1024, sampling=value)
        for value in ("Sobol", "", "pseudo-random"):
            with self.assertRaises(ValueError):
                MonteCarloVanillaEngine(path_count=1024, sampling=value)
        for value in (0, 1, 1.0, "true", None, [], object()):
            with self.subTest(value=value), self.assertRaises(TypeError):
                MonteCarloVanillaEngine(antithetic=value)
        # Representation conversion precedes core validation.
        with self.assertRaises(TypeError):
            MonteCarloVanillaEngine(path_count=0, antithetic=1)

    def test_sobol_path_boundaries_and_single_point_pair(self):
        for antithetic in (False, True):
            for paths in (1, 2, 1024, 8_388_608):
                self.assertEqual(
                    MonteCarloVanillaEngine(
                        path_count=paths, sampling="sobol", antithetic=antithetic
                    ).path_count,
                    paths,
                )
            for paths in (0, 3, 100_000, 8_388_609, 16_777_216):
                with (
                    self.subTest(paths=paths),
                    self.assertRaises(InvalidInputError) as error,
                ):
                    MonteCarloVanillaEngine(
                        path_count=paths, sampling="sobol", antithetic=antithetic
                    )
                self.assertIs(type(error.exception), InvalidInputError)
            for paths in (1, 2):
                engine = MonteCarloVanillaEngine(
                    path_count=paths,
                    seed=42,
                    sampling="sobol",
                    antithetic=antithetic,
                )
                # Independent SciPy ndtri reference from the legacy scenario.
                first = -0.10364940758940244
                positive = 100 * math.exp(0.01 + 0.2 * first) - 90
                negative = 100 * math.exp(0.01 - 0.2 * first) - 90
                expected = math.exp(-0.05) * (
                    (positive + negative) / 2 if antithetic and paths == 2 else positive
                )
                if antithetic or paths == 1:
                    self.assertAlmostEqual(
                        engine.price(contract(strike=90), market()),
                        expected,
                        delta=1e-11,
                    )

    def test_exact_antithetic_counts_and_pair_uncertainty(self):
        option, context = contract(strike=1e-100), market()

        def engine(paths, antithetic):
            return MonteCarloVanillaEngine(
                path_count=paths, seed=42, antithetic=antithetic, thread_count=1
            )

        self.assertEqual(
            engine(1, True).price(option, context),
            engine(1, False).price(option, context),
        )
        for samples in (1, 2, 3, 128, 129, 256, 257, 513):
            positive = samples * engine(samples, False).price(option, context) - (
                0
                if samples == 1
                else (samples - 1) * engine(samples - 1, False).price(option, context)
            )
            opposite = 10_000 * math.exp(-0.08) / positive
            paired = engine(2 * samples, True).price_with_statistics(option, context)
            odd = engine(2 * samples - 1, True).price_with_statistics(option, context)
            self.assertAlmostEqual(
                2 * samples * paired.price - (2 * samples - 1) * odd.price,
                opposite,
                delta=1e-7,
            )
            self.assertIsNone(odd.standard_error)
            self.assertIsNone(odd.confidence_interval)
            self.assertEqual(paired.standard_error is not None, samples > 1)
        first = engine(2, True).price(option, context)
        two = engine(4, True).price_with_statistics(option, context)
        self.assertAlmostEqual(two.standard_error, abs(first - two.price), delta=1e-12)

    def test_all_sampling_combinations_replay_analytics_and_preserve_failure_state(
        self,
    ):

        differences = importlib.import_module("price_differences").price_differences
        option, context = contract(), market()
        shifts = {"spot_shift": 1, "volatility_shift": 0.01, "rate_shift": 0.001}
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                with self.subTest(sampling=sampling, antithetic=antithetic):
                    settings = {
                        "path_count": 1024,
                        "seed": 42,
                        "sampling": sampling,
                        "antithetic": antithetic,
                    }
                    engine = MonteCarloVanillaEngine(**settings, thread_count=1)
                    reference = differences(engine, option, context, **shifts)
                    expected = engine.price_with_greeks(
                        option, context, all_greeks=True, **shifts
                    )
                    self.assertEqual(expected.price, reference.price)
                    for name in GREEKS:
                        self.assertAlmostEqual(
                            getattr(expected, name),
                            getattr(reference, name),
                            delta=1e-9,
                        )
                    sigma = implied_volatility(
                        engine,
                        option,
                        context,
                        expected.price,
                        lower_bound=0.1,
                        upper_bound=0.4,
                    )
                    self.assertAlmostEqual(sigma, 0.2, delta=1e-8)
                    for threads in (0, 2, 4):
                        other = MonteCarloVanillaEngine(
                            **settings, thread_count=threads
                        )
                        actual = other.price_with_greeks(
                            option, context, all_greeks=True, **shifts
                        )
                        self.assertEqual(actual.price, expected.price)
                        self.assertEqual(
                            tuple(getattr(actual, name) for name in GREEKS),
                            tuple(getattr(expected, name) for name in GREEKS),
                        )
                        self.assertEqual(
                            implied_volatility(
                                other,
                                option,
                                context,
                                expected.price,
                                lower_bound=0.1,
                                upper_bound=0.4,
                            ),
                            sigma,
                        )
                    with self.assertRaises(ComputationFailureError) as error:
                        engine.price_with_greeks(
                            option, context, "rho", rate_shift=1000
                        )
                    self.assertIs(type(error.exception), ComputationFailureError)
                    self.assertEqual(engine.price(option, context), expected.price)
                    for steps in (2, 10_000):
                        self.assertEqual(
                            MonteCarloVanillaEngine(**settings, step_count=steps).price(
                                option, context
                            ),
                            expected.price,
                        )
                    for seed in (0, 2**64 - 1, 2**32 + 42):
                        self.assertNotEqual(
                            MonteCarloVanillaEngine(
                                **(settings | {"seed": seed})
                            ).price(option, context),
                            expected.price,
                        )
                    unseeded = MonteCarloVanillaEngine(**(settings | {"seed": None}))
                    analysis = unseeded.price_with_greeks(
                        contract(strike=1e-100), context, ["delta", "gamma"]
                    )
                    self.assertAlmostEqual(
                        analysis.delta, analysis.price / 100, delta=1e-9
                    )
                    self.assertAlmostEqual(analysis.gamma, 0, delta=1e-7)
                    sigma = implied_volatility(
                        unseeded,
                        option,
                        context,
                        9.227005508154036,
                        lower_bound=0.1,
                        upper_bound=0.4,
                    )
                    self.assertTrue(0.1 < sigma < 0.4)
                    self.assertIsNone(unseeded.seed)
                    with ThreadPoolExecutor(max_workers=3) as pool:
                        self.assertEqual(
                            list(
                                pool.map(
                                    lambda _, engine=engine: engine.price(
                                        option, context
                                    ),
                                    range(6),
                                )
                            ),
                            [expected.price] * 6,
                        )

    def test_sampling_statistics_at_expiry_and_block_boundaries(self):
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                for paths in (1, 2, 256, 512, 1024):
                    for seed in (0, 42, 2**64 - 1):
                        engine = MonteCarloVanillaEngine(
                            path_count=paths,
                            seed=seed,
                            sampling=sampling,
                            antithetic=antithetic,
                            thread_count=1,
                        )
                        for direction in ("call", "put"):
                            option = contract(direction)
                            expected = engine.price_with_statistics(option, market())
                            self.assertEqual(
                                engine.price(option, market()), expected.price
                            )
                            if sampling == "sobol":
                                self.assertIsNone(expected.standard_error)
                                self.assertIsNone(expected.confidence_interval)
                            for threads in (0, 2, 4, 2**31 - 1):
                                actual = MonteCarloVanillaEngine(
                                    path_count=paths,
                                    seed=seed,
                                    sampling=sampling,
                                    antithetic=antithetic,
                                    thread_count=threads,
                                ).price_with_statistics(option, market())
                                self.assertEqual(
                                    (
                                        actual.price,
                                        actual.standard_error,
                                        actual.confidence_interval,
                                    ),
                                    (
                                        expected.price,
                                        expected.standard_error,
                                        expected.confidence_interval,
                                    ),
                                )
                engine = MonteCarloVanillaEngine(
                    path_count=1024, seed=42, sampling=sampling, antithetic=antithetic
                )
                option, expiry = contract(strike=90), market(valuation=END)
                known = engine.price_with_statistics(option, expiry)
                self.assertEqual(known.price, 10)
                self.assertEqual(
                    known.standard_error, None if sampling == "sobol" else 0
                )
                self.assertEqual(
                    known.confidence_interval, None if sampling == "sobol" else (10, 10)
                )
                analysis = engine.price_with_greeks(option, expiry, all_greeks=True)
                self.assertTrue(all(getattr(analysis, name) is None for name in GREEKS))
                with self.assertRaises(UnsupportedOperationError) as error:
                    implied_volatility(engine, option, expiry, 10)
                self.assertIs(type(error.exception), UnsupportedOperationError)

    def test_sobol_greek_accuracy_and_multiseed_rmse(self):
        tolerances = {
            "delta": 2e-4,
            "gamma": 1e-5,
            "speed": 5e-6,
            "vega": 5e-5,
            "vanna": 1e-5,
            "zomma": 5e-6,
            "rho": 2e-5,
            "theta": 2e-5,
            "charm": 5e-7,
            "color": 2e-7,
        }
        for antithetic in (False, True):
            for direction in ("call", "put"):
                option, context = contract(direction), market()
                reference = AnalyticVanillaEngine().price_with_greeks(
                    option, context, all_greeks=True
                )
                engine = MonteCarloVanillaEngine(
                    path_count=65_536, seed=42, sampling="sobol", antithetic=antithetic
                )
                actual = engine.price_with_greeks(
                    option,
                    context,
                    all_greeks=True,
                    spot_shift=1,
                    volatility_shift=0.001,
                    rate_shift=0.001,
                )
                self.assertAlmostEqual(actual.price, reference.price, delta=0.002)
                for name, tolerance in tolerances.items():
                    self.assertAlmostEqual(
                        getattr(actual, name),
                        getattr(reference, name),
                        delta=tolerance,
                        msg=f"{direction} {antithetic} {name}",
                    )
            for sampling in ("pseudo_random", "sobol"):
                prices = [
                    MonteCarloVanillaEngine(
                        path_count=131_072,
                        seed=seed,
                        sampling=sampling,
                        antithetic=antithetic,
                        thread_count=1,
                    ).price(contract(), market())
                    for seed in range(16)
                ]
                errors = [price - 9.227005508154036 for price in prices]
                rmse = math.sqrt(sum(error**2 for error in errors) / len(errors))
                bias = sum(errors) / len(errors)
                self.assertLessEqual(rmse, 0.05)
                self.assertLessEqual(abs(bias), 0.05)

    def test_companion_failure_and_domain_validation_preserve_sampling_state(self):
        from kiyosi.market import BlackScholesMertonParameters

        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                engine = MonteCarloVanillaEngine(
                    path_count=1024, seed=42, sampling=sampling, antithetic=antithetic
                )
                option, context = contract(), market()
                baseline = engine.price(option, context)
                for valuation in (START - timedelta(days=1), END + timedelta(days=1)):
                    with self.assertRaises(InvalidInputError) as error:
                        engine.price(option, market(valuation=valuation))
                    self.assertIs(type(error.exception), InvalidInputError)
                curve = RateTermStructure(
                    reference_time=START, times=[END], factors=[0.95]
                )
                node = context.replace(
                    model_parameters=context.model_parameters.replace(
                        risk_free_rate=curve
                    ),
                    valuation_time=END,
                )
                with self.assertRaises(UnsupportedOperationError) as error:
                    engine.price_with_greeks(option, node, [])
                self.assertIs(type(error.exception), UnsupportedOperationError)
                with self.assertRaises(InvalidInputError) as error:
                    engine.price_with_greeks(option, context, [])
                self.assertIs(type(error.exception), InvalidInputError)
                self.assertEqual(engine.price(option, context), baseline)
        # First Sobol path is finite; its opposite overflows, so the paired call fails.
        context = market().replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.5, dividend_yield=0, volatility=1
            ),
            spot_price=1.7e308,
        )
        one = MonteCarloVanillaEngine(
            path_count=1, seed=42, sampling="sobol", antithetic=True
        )
        self.assertTrue(math.isfinite(one.price(contract(), context)))
        paired = MonteCarloVanillaEngine(
            path_count=2, seed=42, sampling="sobol", antithetic=True
        )
        baseline = paired.price(contract(), market())
        with self.assertRaises(ComputationFailureError) as error:
            paired.price(contract(), context)
        self.assertIs(type(error.exception), ComputationFailureError)
        self.assertEqual(paired.price(contract(), market()), baseline)
