"""Single-barrier CPU Monte Carlo through the installed public Python API."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime, timedelta

from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import RateTermStructure
from test_barrier_binomial import END, KINDS, OTHER_GREEKS, START, market, option


class BarrierMonteCarloTests(unittest.TestCase):
    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_continuous_prices_and_native_greeks_match_analytic(self):
        for sampling in ("pseudo_random", "sobol"):
            engine = pricing.MonteCarloBarrierEngine(
                path_count=131072, seed=42, sampling=sampling
            )
            for kind in KINDS:
                for direction in ("call", "put"):
                    contract = option(kind, direction, rebate=2)
                    actual = engine.price_with_greeks(
                        contract, market(), all_greeks=True
                    )
                    expected = pricing.AnalyticBarrierEngine().price_with_greeks(
                        contract, market(), ("delta", "gamma")
                    )
                    for name, budget in (
                        ("price", 0.12),
                        ("delta", 0.04),
                        ("gamma", 0.012),
                    ):
                        with self.subTest(
                            sampling=sampling, kind=kind, direction=direction, name=name
                        ):
                            self.assertAlmostEqual(
                                getattr(actual, name),
                                getattr(expected, name),
                                delta=budget,
                            )
                    self.assertEqual(actual.price, engine.price(contract, market()))
                    for name in OTHER_GREEKS:
                        self.assertIsNone(getattr(actual, name))

    def test_scheduled_prices_greeks_and_early_monitoring_match_quadrature(self):
        engine = pricing.MonteCarloBarrierEngine(
            path_count=131072, seed=42, sampling="sobol"
        )
        for early in (False, True):
            for kind in KINDS:
                dates = [START + timedelta(days=90), START + timedelta(days=180)]
                if not early:
                    dates.append(END)
                contract = option(
                    kind,
                    "put",
                    rebate=2,
                    observation_mode="scheduled",
                    observation_dates=dates,
                )
                actual = engine.price_with_greeks(contract, market(), all_greeks=True)
                expected = pricing.QuadratureBarrierEngine().price_with_greeks(
                    contract, market(), ("delta", "gamma")
                )
                for name, budget in (
                    ("price", 0.08),
                    ("delta", 0.03),
                    ("gamma", 0.015),
                ):
                    with self.subTest(early=early, kind=kind, name=name):
                        self.assertAlmostEqual(
                            getattr(actual, name), getattr(expected, name), delta=budget
                        )
        contract = option(
            "up_and_out",
            strike=130,
            observation_mode="scheduled",
            observation_dates=[START + timedelta(days=90)],
        )
        self.assertGreater(engine.price(contract, market()), 0.01)

    def test_hit_rebates_and_negative_rates_match_analytic_values(self):
        engine = pricing.MonteCarloBarrierEngine(
            path_count=65536, seed=17, sampling="sobol"
        )
        for kind in ("up_and_out", "down_and_out"):
            contract = option(kind, rebate=10, rebate_timing="at_hit")
            for rate in (-0.02, 0.04):
                context = market(rate=rate)
                actual = engine.price_with_greeks(contract, context, all_greeks=True)
                expected = pricing.AnalyticBarrierEngine().price_with_greeks(
                    contract, context, ("delta", "gamma")
                )
                for name, budget in (
                    ("price", 0.015),
                    ("delta", 0.006),
                    ("gamma", 0.003),
                ):
                    self.assertAlmostEqual(
                        getattr(actual, name), getattr(expected, name), delta=budget
                    )
        context = market(rate=0.04)
        observation = START + timedelta(days=180)
        terms = {
            "rebate_timing": "at_hit",
            "observation_mode": "scheduled",
            "observation_dates": [observation],
        }
        with_rebate = option("up_and_out", "put", rebate=10, **terms)
        without_rebate = option("up_and_out", "put", **terms)
        engine = pricing.MonteCarloBarrierEngine(path_count=131072, seed=42)
        time = 180 / 365
        threshold = (math.log(1.2) - 0.01 * time) / (0.2 * math.sqrt(time))
        expected = (
            10 * math.exp(-0.04 * time) * 0.5 * math.erfc(threshold / math.sqrt(2))
        )
        self.assertAlmostEqual(
            engine.price(with_rebate, context) - engine.price(without_rebate, context),
            expected,
            delta=0.025,
        )

    def test_steps_threads_pairing_and_concurrent_calls_preserve_samples(self):
        context = market()
        for scheduled in (False, True):
            contract = option(
                "up_and_out",
                "put",
                rebate=3,
                rebate_timing="at_hit",
                **(
                    {
                        "observation_mode": "scheduled",
                        "observation_dates": [START + timedelta(days=90), END],
                    }
                    if scheduled
                    else {}
                ),
            )
            for sampling in ("pseudo_random", "sobol"):
                for antithetic in (False, True):
                    settings = {
                        "path_count": 1024,
                        "seed": 17,
                        "sampling": sampling,
                        "antithetic": antithetic,
                    }
                    serial = pricing.MonteCarloBarrierEngine(thread_count=1, **settings)
                    parallel = pricing.MonteCarloBarrierEngine(
                        thread_count=4, **settings
                    )
                    expected = serial.price_with_greeks(
                        contract, context, all_greeks=True
                    )

                    def evaluate(
                        index, parallel=parallel, serial=serial, contract=contract
                    ):
                        engine = parallel if index % 2 else serial
                        result = engine.price_with_greeks(
                            contract, context, all_greeks=True
                        )
                        return result.price, result.delta, result.gamma

                    with ThreadPoolExecutor(max_workers=4) as pool:
                        for actual in pool.map(evaluate, range(8)):
                            self.assertEqual(
                                actual, (expected.price, expected.delta, expected.gamma)
                            )
        vanilla = EuropeanOption(
            option_type="call", strike=100, effective_date=START, expiry_date=END
        )
        for paths in (1, 2, 3, 5, 257):
            settings = {"path_count": paths, "seed": 42, "antithetic": True}
            engine = pricing.MonteCarloBarrierEngine(**settings)
            total = engine.price(option("down_and_in"), context) + engine.price(
                option(), context
            )
            expected = pricing.MonteCarloVanillaEngine(**settings).price(
                vanilla, context
            )
            self.assertAlmostEqual(total, expected, delta=1e-12)

    def test_history_current_equality_expiry_and_finished_monitoring(self):
        engine = pricing.MonteCarloBarrierEngine(path_count=1, seed=42)
        for kind in KINDS:
            for timing in (
                ("at_expiry",) if kind.endswith("in") else ("at_hit", "at_expiry")
            ):
                contract = option(kind, rebate=7, rebate_timing=timing)
                context = market(spot=contract.barrier_level)
                result = engine.price_with_greeks(contract, context, all_greeks=True)
                self.assertEqual(
                    result.price,
                    pricing.AnalyticBarrierEngine().price(contract, context),
                )
                for name in ("delta", "gamma", *OTHER_GREEKS):
                    self.assertIsNone(getattr(result, name))
                later = context.replace(
                    valuation_time=datetime(2025, 1, 1, tzinfo=UTC)
                    + timedelta(microseconds=1)
                )
                self.assert_rejection(
                    InvalidInputError,
                    engine.price,
                    contract,
                    later,
                )
                touched = contract.replace(touch_state="touched")
                self.assert_rejection(InvalidInputError, engine.price, touched, context)
                self.assertEqual(
                    engine.price(touched, later),
                    pricing.AnalyticBarrierEngine().price(touched, later),
                )
                for history in ("touched", "untouched"):
                    known = contract.replace(touch_state=history)
                    expired = market(spot=contract.barrier_level, valuation=END)
                    result = engine.price_with_greeks(known, expired, all_greeks=True)
                    self.assertEqual(
                        result.price,
                        pricing.AnalyticBarrierEngine().price(known, expired),
                    )
                    self.assertIsNone(result.delta)
                    self.assertIsNone(result.gamma)
                observation = START + timedelta(days=90)
                scheduled = option(
                    kind,
                    rebate=7,
                    rebate_timing=timing,
                    observation_mode="scheduled",
                    observation_dates=[observation],
                )
                on_date = market(spot=contract.barrier_level, valuation=observation)
                self.assertEqual(
                    engine.price(scheduled, on_date),
                    pricing.AnalyticBarrierEngine().price(scheduled, on_date),
                )
                after = on_date.replace(
                    valuation_time=datetime.combine(
                        observation, datetime.min.time(), UTC
                    )
                    + timedelta(microseconds=1)
                )
                untouched = scheduled.replace(touch_state="untouched")
                self.assertEqual(
                    engine.price(untouched, after),
                    pricing.AnalyticBarrierEngine().price(untouched, after),
                )
                self.assertIsNone(contract.touch_state)
        near_barrier = market(spot=80.005)
        result = engine.price_with_greeks(option(), near_barrier, all_greeks=True)
        self.assertIsNone(result.delta)
        self.assertIsNone(result.gamma)

    def test_defaults_and_strict_representations(self):
        factory = pricing.MonteCarloBarrierEngine
        engine, vanilla = factory(), pricing.MonteCarloVanillaEngine()
        for name in (
            "path_count",
            "seed",
            "thread_count",
            "sampling",
            "antithetic",
        ):
            self.assertEqual(getattr(engine, name), getattr(vanilla, name))
            with self.assertRaises(AttributeError):
                setattr(engine, name, getattr(engine, name))
        self.assertIn("MonteCarloBarrierEngine", pricing.__all__)
        for name in ("path_count", "seed", "thread_count"):
            for value in (
                True,
                False,
                1.5,
                2.0,
                "3",
                None if name != "seed" else object(),
            ):
                with self.assertRaises(TypeError):
                    factory(**{name: value})
            with self.assertRaises(OverflowError):
                factory(**{name: 1 << 100})
        for fields in (
            {"antithetic": 1},
            {"unknown": 1},
            {"backend": "cpu"},
            {"sampling": True},
        ):
            with self.assertRaises(TypeError):
                factory(**fields)
        with self.assertRaises(ValueError):
            factory(sampling="unknown")
        for fields in (
            {"path_count": 0},
            {"path_count": 10000001},
            {"thread_count": -1},
            {"path_count": 3, "sampling": "sobol"},
        ):
            self.assert_rejection(InvalidInputError, factory, **fields)
        for seed in (-1, 2**64):
            with self.assertRaises(OverflowError):
                factory(seed=seed)
        for seed in (0, 2**64 - 1):
            self.assertEqual(factory(seed=seed).seed, seed)
        for fields in (
            {"spot_shift": True},
            {"time_shift_days": True},
            {"time_shift_days": 1.5},
            {"unknown": 1},
        ):
            with self.assertRaises(TypeError):
                engine.price_with_greeks(option(), market(), "delta", **fields)
        with self.assertRaises(OverflowError):
            engine.price_with_greeks(option(), market(), "delta", time_shift_days=2**31)
        vanilla = EuropeanOption(
            option_type="call", strike=100, effective_date=START, expiry_date=END
        )
        for operation, args in (
            (engine.price, (vanilla, market())),
            (engine.price_with_greeks, (vanilla, market(), "delta")),
            (pricing.implied_volatility, (engine, vanilla, market(), 10)),
        ):
            with self.assertRaises(UnsupportedOperationError):
                operation(*args)

    def test_applicability_errors_validation_order_and_failure_state(self):
        engine = pricing.MonteCarloBarrierEngine(path_count=32, seed=42)
        contract, context = option(), market()
        quote = engine.price(contract, context)
        curve = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.03 * 360 / 365)]
        )
        for changes in ({"rate": curve}, {"dividend": curve}):
            curved = market(**changes)
            self.assert_rejection(
                UnsupportedOperationError,
                engine.price,
                contract,
                curved,
            )
            self.assert_rejection(
                UnsupportedOperationError,
                engine.price_with_greeks,
                contract,
                curved,
                [],
            )
            self.assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                contract,
                curved,
                math.nan,
            )
        for valuation in (START - timedelta(days=1), END + timedelta(days=1)):
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                contract,
                market(valuation=valuation),
            )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            contract,
            context,
            [],
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            contract,
            context,
            "delta",
            spot_shift=0,
        )
        for sigma, rate in ((50, 0.03), (0.2, -1000)):
            self.assert_rejection(
                ComputationFailureError,
                engine.price,
                contract,
                market(volatility=sigma, rate=rate),
            )
        self.assertEqual(engine.price(contract, context), quote)
        self.assertEqual(context.model_parameters.volatility, 0.2)
        self.assertIsNone(contract.touch_state)

    def test_solver_pins_draws_checks_fixed_cashflows_and_keeps_inputs(self):
        engine = pricing.MonteCarloBarrierEngine(
            path_count=8192, seed=42, sampling="sobol"
        )
        contract, context = option(), market()
        quote = engine.price(contract, context)

        def solve(index):
            return pricing.implied_volatility(
                engine, contract, context, quote, lower_bound=0.15, upper_bound=0.25
            )

        with ThreadPoolExecutor(max_workers=4) as pool:
            for sigma in pool.map(solve, range(8)):
                self.assertAlmostEqual(sigma, 0.2, delta=1e-5)
        for observed, controls, expected_error in (
            (-1, {}, RootNotBracketedError),
            (math.nan, {}, InvalidInputError),
            (quote, {"max_iterations": 0}, InvalidInputError),
        ):
            self.assert_rejection(
                expected_error,
                pricing.implied_volatility,
                engine,
                contract,
                context,
                observed,
                **controls,
            )
        fixed = contract.replace(touch_state="touched")
        later = market(valuation=START + timedelta(days=1))
        self.assert_rejection(
            UnsupportedOperationError,
            pricing.implied_volatility,
            engine,
            fixed,
            later,
            engine.price(fixed, later),
        )
        self.assertEqual(engine.price(contract, context), quote)
        self.assertEqual(context.model_parameters.volatility, 0.2)

    def test_sobol_dimension_limit_includes_the_unobserved_expiry_leg(self):
        dates = [START + timedelta(days=i) for i in range(1, 21202)]
        contract = option(
            "up_and_out",
            expiry_date=dates[-1] + timedelta(days=1),
            observation_mode="scheduled",
            observation_dates=dates,
        )
        engine = pricing.MonteCarloBarrierEngine(
            path_count=1, seed=42, sampling="sobol"
        )
        self.assert_rejection(InvalidInputError, engine.price, contract, market())

    def test_scaled_rare_hits_and_only_requested_greek_failures(self):
        engine = pricing.MonteCarloBarrierEngine(path_count=1, seed=42)
        contract = option("down_and_in", strike=1, barrier_level=1e300 * math.exp(-20))
        context = market(spot=1e300, rate=0.5, dividend=0, volatility=1)
        self.assertGreater(engine.price(contract, context), 0)
        contract = option()
        context = market(volatility=1e-200)
        price = engine.price(contract, context)
        self.assertEqual(
            engine.price_with_greeks(contract, context, "rho").price, price
        )
        self.assert_rejection(
            ComputationFailureError,
            engine.price_with_greeks,
            contract,
            context,
            "gamma",
        )
        from kiyosi.market import weekdays_calendar

        invalid = option(
            observation_mode="scheduled", observation_dates=[START + timedelta(days=3)]
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            invalid,
            market().replace(calendar=weekdays_calendar()),
        )
