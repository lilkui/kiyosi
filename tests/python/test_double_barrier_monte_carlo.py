"""Double-barrier CPU Monte Carlo through the installed public Python API."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime, timedelta
from itertools import product

from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import RateTermStructure, weekdays_calendar
from test_double_barrier import END, START, contract, market, reference
from test_double_barrier_finite_difference import (
    FIRST,
    first_fixing_reference,
    terminal_reference,
    vanilla_reference,
)


class DoubleBarrierMonteCarloTests(unittest.TestCase):
    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_continuous_prices_and_shared_sample_greeks_match_quantlib(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=32768, seed=42, sampling="sobol"
        )
        for direction in ("call", "put"):
            for kind in ("knock_in", "knock_out"):
                option = contract(option_type=direction, barrier_type=kind)
                actual = engine.price_with_greeks(option, market(), all_greeks=True)
                self.assertAlmostEqual(
                    actual.price, reference(direction, kind), delta=0.01
                )
                expected = pricing.AnalyticDoubleBarrierEngine().price_with_greeks(
                    option, market(), ("delta", "gamma")
                )
                self.assertAlmostEqual(actual.delta, expected.delta, delta=0.003)
                self.assertAlmostEqual(actual.gamma, expected.gamma, delta=0.003)
                self.assertEqual(actual.price, engine.price(option, market()))
                self.assertIsNone(actual.vega)

    def test_scheduled_prices_match_independent_integrals_and_greeks_match_fd(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=262144, seed=42, sampling="sobol"
        )
        for direction, timing, final in product(
            ("call", "put"), ("at_hit", "at_expiry"), (False, True)
        ):
            option = contract(
                option_type=direction,
                lower_barrier=80.125,
                upper_barrier=120.125,
                rebate=2,
                rebate_timing=timing,
                observation_mode="scheduled",
                observation_dates=[FIRST, END] if final else [FIRST],
            )
            expected = first_fixing_reference(
                100.25, direction, timing, final_fixing=final
            )
            refined = first_fixing_reference(
                100.25, direction, timing, final_fixing=final, count=2048
            )
            self.assertAlmostEqual(expected, refined, delta=1e-8)
            context = market(100.25)
            result = engine.price_with_greeks(option, context, all_greeks=True)
            self.assertAlmostEqual(result.price, expected, delta=0.025)
            fd = pricing.FiniteDifferenceDoubleBarrierEngine(
                asset_step_count=1200, time_step_count=1200
            ).price_with_greeks(option, context, ("delta", "gamma"))
            self.assertAlmostEqual(result.delta, fd.delta, delta=0.025)
            self.assertAlmostEqual(result.gamma, fd.gamma, delta=0.01)
        for direction in ("call", "put"):
            option = contract(
                option_type=direction,
                rebate=2,
                observation_mode="scheduled",
                observation_dates=[END],
            )
            self.assertAlmostEqual(
                engine.price(option, market()),
                terminal_reference(100, direction, 80, 120, 1),
                delta=0.002,
            )
            for final in (False, True):
                inside = contract(
                    option_type=direction,
                    barrier_type="knock_in",
                    rebate=2,
                    lower_barrier=80.125,
                    upper_barrier=120.125,
                    observation_mode="scheduled",
                    observation_dates=[FIRST, END] if final else [FIRST],
                )
                expected = (
                    vanilla_reference(100.25, direction, 1)
                    + 2 * math.exp(-0.04)
                    - first_fixing_reference(
                        100.25, direction, "at_expiry", final_fixing=final
                    )
                )
                self.assertAlmostEqual(
                    engine.price(inside, market(100.25)), expected, delta=0.025
                )
        option = contract(
            strike=130, observation_mode="scheduled", observation_dates=[FIRST]
        )
        self.assertGreater(engine.price(option, market()), 0.01)

    def test_series_switch_extreme_bands_and_tiny_volatility(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=65536, seed=42, sampling="sobol"
        )
        for volatility, price, delta, gamma in (
            (
                0.20273255202675663,
                1.045594059652574,
                -0.009496602827940848,
                -0.006356529009821088,
            ),
            (
                0.20273255608140772,
                1.0455940036319245,
                -0.009496602869507598,
                -0.006356530679596517,
            ),
            (
                0.3040988310811233,
                0.21271328442609105,
                -0.002243229044052164,
                -0.0012635228285873268,
            ),
        ):
            result = engine.price_with_greeks(
                contract(), market(volatility=volatility), all_greeks=True
            )
            for actual, expected in (
                (result.price, price),
                (result.delta, delta),
                (result.gamma, gamma),
            ):
                self.assertAlmostEqual(actual, expected, delta=0.003)
        self.assertAlmostEqual(
            engine.price(contract(lower_barrier=99.99, upper_barrier=100.01), market()),
            0,
            delta=1e-12,
        )
        wide = contract(lower_barrier=0.01, upper_barrier=10000)
        self.assertAlmostEqual(
            engine.price(wide, market()),
            pricing.AnalyticDoubleBarrierEngine().price(wide, market()),
            delta=0.025,
        )
        for up, timing in product((False, True), ("at_hit", "at_expiry")):
            option = contract(
                lower_barrier=80 if up else 98,
                upper_barrier=102 if up else 120,
                rebate=3,
                rebate_timing=timing,
            )
            time = (
                math.log(1.02 if up else 100 / 98) / 0.03 if timing == "at_hit" else 1
            )
            self.assertAlmostEqual(
                engine.price(
                    option, market(dividend=0.01 if up else 0.07, volatility=1e-12)
                ),
                3 * math.exp(-0.04 * time),
                delta=1e-7,
            )

    def test_rebates_match_independent_exit_series_with_negative_rates(self):
        def rebate_reference(rate, count):
            width = math.log(125 / 80)
            survival, integrated = [], []
            for index in range(count):
                frequency = 2 * index + 1
                coefficient = 4 * (-1 if index % 2 else 1) / (frequency * math.pi)
                decay = 0.5 * 0.2**2 * (frequency * math.pi / width) ** 2
                survival.append(coefficient * math.exp(-decay))
                integrated.append(
                    coefficient * -math.expm1(-(rate + decay)) / (rate + decay)
                )
            discounted = math.exp(-rate) * math.fsum(survival)
            return (
                3 * (math.exp(-rate) - discounted),
                3 * (1 - discounted - rate * math.fsum(integrated)),
                3 * discounted,
            )

        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=65536, seed=42, sampling="sobol"
        )
        for rate in (-0.02, 0.04):
            refs, refined = rebate_reference(rate, 4097), rebate_reference(rate, 8193)
            for a, b in zip(refs, refined, strict=True):
                self.assertAlmostEqual(a, b, delta=1e-11)
            context = market(rate=rate, dividend=rate - 0.02)
            for index, (kind, timing) in enumerate(
                (
                    ("knock_out", "at_expiry"),
                    ("knock_out", "at_hit"),
                    ("knock_in", "at_expiry"),
                )
            ):
                terms = {
                    "upper_barrier": 125,
                    "barrier_type": kind,
                    "rebate_timing": timing,
                }
                option = contract(**terms, rebate=3)
                actual = engine.price_with_greeks(option, context, all_greeks=True)
                self.assertAlmostEqual(
                    actual.price - engine.price(contract(**terms), context),
                    refs[index],
                    delta=0.004,
                )
                expected = pricing.AnalyticDoubleBarrierEngine().price_with_greeks(
                    option, context, ("delta", "gamma")
                )
                self.assertAlmostEqual(actual.delta, expected.delta, delta=0.006)
                self.assertAlmostEqual(actual.gamma, expected.gamma, delta=0.003)

    def test_steps_threads_pairing_and_concurrent_calls_preserve_samples(self):
        context = market()
        for scheduled, sampling, paired in product(
            (False, True), ("pseudo_random", "sobol"), (False, True)
        ):
            option = contract(
                option_type="put",
                rebate=3,
                rebate_timing="at_hit",
                **(
                    {"observation_mode": "scheduled", "observation_dates": [FIRST]}
                    if scheduled
                    else {}
                ),
            )
            settings = {
                "path_count": 1024,
                "seed": 17,
                "sampling": sampling,
                "antithetic": paired,
            }
            serial = pricing.MonteCarloDoubleBarrierEngine(thread_count=1, **settings)
            parallel = pricing.MonteCarloDoubleBarrierEngine(
                thread_count=4, step_count=10000, **settings
            )
            expected = serial.price_with_greeks(option, context, all_greeks=True)

            def evaluate(index, parallel=parallel, serial=serial, option=option):
                result = (parallel if index % 2 else serial).price_with_greeks(
                    option, context, all_greeks=True
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
            engine = pricing.MonteCarloDoubleBarrierEngine(**settings)
            self.assertAlmostEqual(
                sum(
                    engine.price(contract(barrier_type=kind), context)
                    for kind in ("knock_in", "knock_out")
                ),
                pricing.MonteCarloVanillaEngine(**settings).price(vanilla, context),
                delta=1e-12,
            )

    def test_current_equality_prior_history_expiry_and_finished_monitoring(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(path_count=1, seed=42)
        analytic = pricing.AnalyticDoubleBarrierEngine()
        for kind, timing in (
            ("knock_out", "at_hit"),
            ("knock_out", "at_expiry"),
            ("knock_in", "at_expiry"),
        ):
            option = contract(barrier_type=kind, rebate=7, rebate_timing=timing)
            for spot in (79, 80, 120, 121):
                context = market(spot)
                self.assertEqual(
                    engine.price(option, context), analytic.price(option, context)
                )
                if spot in (80, 120):
                    result = engine.price_with_greeks(option, context, all_greeks=True)
                    self.assertIsNone(result.delta)
                    self.assertIsNone(result.gamma)
                later = context.replace(
                    valuation_time=datetime(2025, 1, 1, tzinfo=UTC)
                    + timedelta(microseconds=1)
                )
                self.assert_rejection(InvalidInputError, engine.price, option, later)
                touched = option.replace(touch_state="touched")
                self.assert_rejection(InvalidInputError, engine.price, touched, context)
                self.assertEqual(
                    engine.price(touched, later), analytic.price(touched, later)
                )
                for history in ("touched", "untouched"):
                    known = option.replace(touch_state=history)
                    expired = market(spot, time=END)
                    result = engine.price_with_greeks(known, expired, all_greeks=True)
                    self.assertEqual(result.price, analytic.price(known, expired))
                    self.assertIsNone(result.delta)
                    self.assertIsNone(result.gamma)
            scheduled = contract(
                barrier_type=kind,
                rebate=7,
                rebate_timing=timing,
                observation_mode="scheduled",
                observation_dates=[FIRST],
            )
            on_date = market(120, time=FIRST)
            self.assertEqual(
                engine.price(scheduled, on_date), analytic.price(scheduled, on_date)
            )
            after = on_date.replace(
                valuation_time=datetime.combine(FIRST, datetime.min.time(), UTC)
                + timedelta(microseconds=1)
            )
            untouched = scheduled.replace(touch_state="untouched")
            self.assertEqual(
                engine.price(untouched, after), analytic.price(untouched, after)
            )
            self.assertIsNone(option.touch_state)
        for spot in (80.005, 119.995):
            result = engine.price_with_greeks(contract(), market(spot), all_greeks=True)
            self.assertIsNone(result.delta)
            self.assertIsNone(result.gamma)

    def test_defaults_exports_and_strict_representations(self):
        factory = pricing.MonteCarloDoubleBarrierEngine
        engine, vanilla = factory(), pricing.MonteCarloVanillaEngine()
        for name in (
            "path_count",
            "step_count",
            "seed",
            "thread_count",
            "sampling",
            "antithetic",
        ):
            self.assertEqual(getattr(engine, name), getattr(vanilla, name))
            with self.assertRaises(AttributeError):
                setattr(engine, name, getattr(engine, name))
        self.assertIn("MonteCarloDoubleBarrierEngine", pricing.__all__)
        self.assertFalse(hasattr(engine, "price_with_statistics"))
        for name in ("path_count", "step_count", "seed", "thread_count"):
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
            {"step_count": 1},
            {"step_count": 10001},
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
                engine.price_with_greeks(contract(), market(), "delta", **fields)
        with self.assertRaises(OverflowError):
            engine.price_with_greeks(
                contract(), market(), "delta", time_shift_days=2**31
            )
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=START, expiry_date=END
        )
        for operation, args in (
            (engine.price, (option, market())),
            (engine.price_with_greeks, (option, market(), "delta")),
            (pricing.implied_volatility, (engine, option, market(), 10)),
        ):
            with self.assertRaises(UnsupportedOperationError):
                operation(*args)
        for fields in ({"lower_barrier": True}, {"upper_barrier": "120"}):
            with self.assertRaises(TypeError):
                contract(**fields)
        for fields in (
            {"lower_barrier": 0},
            {"upper_barrier": 80},
            {"upper_barrier": math.inf},
        ):
            self.assert_rejection(InvalidInputError, contract, **fields)
        self.assert_rejection(
            InvalidInputError,
            contract,
            barrier_type="knock_in",
            rebate_timing="at_hit",
        )

    def test_errors_validation_order_and_failure_state(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(path_count=32, seed=42)
        option, context = contract(), market()
        quote = engine.price(option, context)
        curve = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.03)]
        )
        for changes in ({"rate": curve}, {"dividend": curve}):
            curved = market(**changes)
            self.assert_rejection(
                UnsupportedOperationError, engine.price, option, curved
            )
            self.assert_rejection(
                UnsupportedOperationError,
                engine.price_with_greeks,
                option,
                curved,
                [],
            )
            self.assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                option,
                curved,
                math.nan,
            )
        for time in (START - timedelta(days=1), END + timedelta(days=1)):
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                option,
                market(time=time),
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
        for rate, volatility in ((-1000, 0.2), (0.04, 50)):
            self.assert_rejection(
                ComputationFailureError,
                engine.price,
                option,
                market(rate=rate, dividend=rate, volatility=volatility),
            )
        tiny = market(volatility=1e-200)
        price = engine.price(option, tiny)
        self.assertEqual(engine.price_with_greeks(option, tiny, "rho").price, price)
        self.assert_rejection(
            ComputationFailureError,
            engine.price_with_greeks,
            option,
            tiny,
            "gamma",
        )
        invalid = contract(
            observation_mode="scheduled", observation_dates=[START + timedelta(days=3)]
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            invalid,
            context.replace(calendar=weekdays_calendar()),
        )
        self.assertEqual(engine.price(option, context), quote)
        self.assertEqual(context.model_parameters.volatility, 0.2)
        self.assertIsNone(option.touch_state)

    def test_solver_replays_draws_rejects_fixed_cashflows_and_preserves_inputs(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=8192, seed=42, sampling="sobol"
        )
        option = contract(lower_barrier=50, upper_barrier=200)
        context = market()
        quote = engine.price(option, context)

        def solve(index):
            return pricing.implied_volatility(
                engine, option, context, quote, lower_bound=0.15, upper_bound=0.25
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
                option,
                context,
                observed,
                **controls,
            )
        later = market(time=FIRST + timedelta(days=1))
        fixed = contract(rebate=3, touch_state="touched")
        finished = contract(
            barrier_type="knock_in",
            observation_mode="scheduled",
            observation_dates=[FIRST],
            touch_state="untouched",
        )
        for option, context in (
            (fixed, later),
            (finished, later),
            (contract(strike=125), market()),
            (contract(option_type="put", strike=75), market()),
            (contract(touch_state="untouched"), market(time=END)),
        ):
            self.assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                option,
                context,
                engine.price(option, context),
            )
        self.assertEqual(
            engine.price(contract(lower_barrier=50, upper_barrier=200), market()), quote
        )

    def test_sobol_dimension_limit_includes_unobserved_expiry_leg(self):
        dates = [START + timedelta(days=i) for i in range(1, 21202)]
        option = contract(
            expiry_date=dates[-1] + timedelta(days=1),
            observation_mode="scheduled",
            observation_dates=dates,
        )
        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=1, seed=42, sampling="sobol"
        )
        self.assert_rejection(InvalidInputError, engine.price, option, market())
