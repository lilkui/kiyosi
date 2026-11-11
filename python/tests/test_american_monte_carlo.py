"""American least-squares Monte Carlo through the installed public API."""

import importlib
import math
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from functools import partial

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import AmericanOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    CoxRossRubinsteinVanillaEngine,
    MonteCarloVanillaEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import DomainTestCase
from support.terminal import assert_terminal_greeks
from test_binomial import END, GREEKS, START, market


def contract(direction="put", strike=100):
    return AmericanOption(
        option_type=direction, strike=strike, effective_time=START, expiry_time=END
    )


class AmericanMonteCarloTests(DomainTestCase):
    def test_path_storage_limit_rejects_large_grids_before_sampling(self):
        context = market(rate=0.05, dividend=0)
        put = contract()
        for antithetic in (False, True):
            with self.subTest(antithetic=antithetic):
                engine = MonteCarloVanillaEngine(
                    path_count=15_001, step_count=10_000, seed=42, antithetic=antithetic
                )
                self.assert_rejection(InvalidInputError, engine.price, put, context)
                self.assert_rejection(
                    InvalidInputError, engine.price_with_greeks, put, context, "delta"
                )
                self.assert_rejection(
                    InvalidInputError,
                    implied_volatility,
                    engine,
                    put,
                    context,
                    1,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
                self.assertEqual(
                    engine.price(
                        put, context.replace(valuation_time=END, spot_price=90)
                    ),
                    10,
                )
                european = EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_time=START,
                    expiry_time=END,
                )
                expected = engine.price(european, context)
                self.assertGreater(expected, 0)
                self.assertEqual(engine.price(contract("call"), context), expected)
                self.assertEqual(engine.seed, 42)

    def test_nonfinite_continuation_regressions_preserve_engine_reuse(self):
        context = market(rate=-0.1, dividend=0, sigma=0.2, spot=100)
        engine = MonteCarloVanillaEngine(path_count=128, step_count=4, seed=42)
        ordinary = contract("call")
        before = engine.price(ordinary, context)
        for strike in (1e-100, 1e-300):
            with self.subTest(strike=strike):
                extreme = contract("call", strike)
                self.assert_rejection(
                    ComputationFailureError, engine.price, extreme, context
                )
                self.assert_rejection(
                    ComputationFailureError,
                    engine.price_with_greeks,
                    extreme,
                    context,
                    ["delta"],
                )
        self.assertEqual(engine.price(ordinary, context), before)
        self.assertEqual(engine.seed, 42)

    def test_step_counts_are_intervals_with_two_required_for_american_exercise(self):
        from kiyosi.pricing import MonteCarloAsianEngine

        option = contract()
        context = market()
        engine = MonteCarloVanillaEngine(path_count=64, step_count=2, seed=42)
        value = engine.price(option, context)
        self.assertGreater(value, 0)
        self.assertLess(value, 100)
        self.assertEqual(engine.step_count, 2)
        self.assertEqual(MonteCarloAsianEngine(step_count=2).step_count, 2)
        one = MonteCarloVanillaEngine(path_count=64, step_count=1, seed=42)
        self.assert_rejection(InvalidInputError, one.price, option, context)
        self.assertEqual(
            one.price(option, context.replace(valuation_time=END, spot_price=90)), 10
        )

    def test_immediate_exercise_and_minimum_grid_validation_timing(self):
        option = contract()
        context = market(rate=0.1, dividend=0, sigma=0.1, spot=50)
        engine = MonteCarloVanillaEngine(path_count=4096, seed=42)
        self.assertAlmostEqual(engine.price(option, context), 50, delta=1e-10)
        engine = MonteCarloVanillaEngine(step_count=1, seed=42)
        self.assert_rejection(InvalidInputError, engine.price, option, context)
        expiry = context.replace(valuation_time=END)
        self.assertEqual(engine.price(option, expiry), 50)
        result = engine.price_with_greeks(option, expiry, greeks="all")
        self.assertEqual(result.price, 50)
        assert_terminal_greeks(self, result, -1)
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            option,
            expiry,
            50,
            lower_bound=0.0,
            upper_bound=4.0,
        )
        for valuation in (
            START - timedelta(days=1),
            datetime(2026, 1, 1, 0, 0, 0, 1),
        ):
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                option,
                context.replace(valuation_time=valuation),
            )

    def test_sparse_rank_deficient_regressions_and_currency_scale(self):
        for paths in (1, 3, 4, 127):
            for antithetic in (False, True):
                engine = MonteCarloVanillaEngine(
                    path_count=paths, step_count=4, seed=42, antithetic=antithetic
                )
                context = market(
                    rate=0, dividend=0.1, sigma=sys.float_info.min, spot=50
                )
                price = engine.price(contract(), context)
                self.assertAlmostEqual(price, 100 - 50 * math.exp(-0.1), delta=1e-12)
                for scale in (0.01, 100):
                    self.assertAlmostEqual(
                        engine.price(
                            contract(strike=100 * scale),
                            context.replace(spot_price=50 * scale),
                        )
                        / scale,
                        price,
                        delta=1e-11,
                    )
        # A deterministic optimal exercise date lies strictly inside this three-year grid.
        option = contract().replace(expiry_time=datetime(2028, 1, 1))
        expected = max(
            100 * math.exp(-0.05 * (3 * step / 50))
            - 30 * math.exp(-0.2 * (3 * step / 50))
            for step in range(51)
        )
        for paths in (1, 2, 4, 2000):
            engine = MonteCarloVanillaEngine(path_count=paths, seed=42)
            self.assertAlmostEqual(
                engine.price(
                    option, market(rate=0.05, dividend=0.2, sigma=1e-8, spot=30)
                ),
                expected,
                delta=1e-6,
            )

    def test_terminal_call_shortcut_and_rate_stencil_crossing_zero(self):
        call = contract("call")
        european = EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                for rate, dividend in ((0, 0), (0.05, -0.02)):
                    context = market(rate=rate, dividend=dividend)
                    settings = {
                        "path_count": 1024,
                        "seed": 42,
                        "sampling": sampling,
                        "antithetic": antithetic,
                    }
                    price = MonteCarloVanillaEngine(**settings).price(european, context)
                    for steps in (3, 50, 10000):
                        self.assertEqual(
                            MonteCarloVanillaEngine(**settings, step_count=steps).price(
                                call, context
                            ),
                            price,
                        )
        engine = MonteCarloVanillaEngine(path_count=2048, step_count=6, seed=42)
        requested = 0.0001
        for rate in (-requested / 2, 0, requested / 2):
            context = market(rate=rate, dividend=0)
            result = engine.price_with_greeks(call, context, "rho")
            shift = (rate + requested) - rate
            if rate - (rate - shift) != shift:
                self.assertIsNone(result.rho)
                continue
            up = engine.price(call, market(rate=rate + shift, dividend=math.ulp(0.0)))
            down = engine.price(call, market(rate=rate - shift, dividend=math.ulp(0.0)))
            self.assertAlmostEqual(
                result.rho,
                (up - down) / (2 * shift),
                delta=1e-8,
            )
            self.assertEqual(result.price, engine.price(call, context))

    def test_shared_samples_defaults_solving_and_concurrent_calls(self):
        option, context = contract(), market()
        shifts = {
            "spot_shift": 8,
            "volatility_shift": 0.02,
            "rate_shift": 0.01,
            "time_shift": timedelta(days=2),
        }
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                engine = MonteCarloVanillaEngine(
                    path_count=1024,
                    step_count=6,
                    seed=42,
                    sampling=sampling,
                    antithetic=antithetic,
                )
                result = numerical_greeks(
                    engine, option, context, greeks="all", **shifts
                )
                self.assertEqual(result.price, engine.price(option, context))
                for name in GREEKS:
                    selected = numerical_greeks(engine, option, context, name, **shifts)
                    self.assertEqual(getattr(selected, name), getattr(result, name))
                default = engine.price_with_greeks(option, context, ["delta", "gamma"])
                explicit = numerical_greeks(
                    engine, option, context, ["delta", "gamma"], spot_shift=8
                )
                none = numerical_greeks(
                    engine, option, context, ["delta", "gamma"], spot_shift=None
                )
                self.assertEqual(
                    (default.delta, default.gamma), (explicit.delta, explicit.gamma)
                )
                self.assertEqual(
                    (none.delta, none.gamma), (explicit.delta, explicit.gamma)
                )
                solved = implied_volatility(
                    engine,
                    option,
                    context,
                    result.price,
                    lower_bound=0.15,
                    upper_bound=0.25,
                )
                self.assertAlmostEqual(solved, 0.2, delta=1e-7)
                self.assert_rejection(
                    RootNotBracketedError,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    -1,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
                unavailable = numerical_greeks(
                    engine, option, context, "gamma", spot_shift=200
                )
                self.assertIsNone(unavailable.gamma)
                self.assertEqual(unavailable.price, result.price)
                with ThreadPoolExecutor(max_workers=2) as pool:
                    values = list(
                        pool.map(
                            lambda _, engine=engine: numerical_greeks(
                                engine, option, context, greeks="all", **shifts
                            ),
                            range(2),
                        )
                    )
                for value in values:
                    self.assertEqual(value.price, result.price)
                    self.assertEqual(
                        [getattr(value, name) for name in GREEKS],
                        [getattr(result, name) for name in GREEKS],
                    )
        unseeded = MonteCarloVanillaEngine(path_count=1024, step_count=6)
        result = numerical_greeks(unseeded, option, context, greeks="all", **shifts)
        self.assertTrue(all(math.isfinite(getattr(result, name)) for name in GREEKS))
        self.assertIsNone(unseeded.seed)

    def test_validation_representations_and_failure_state(self):
        engine = MonteCarloVanillaEngine(path_count=127, step_count=6, seed=42)
        option, context = contract(), market()
        price = engine.price(option, context)
        for name in ("spot_shift", "volatility_shift", "rate_shift"):
            for value in (True, "1", object()):
                with self.assertRaises(TypeError):
                    numerical_greeks(engine, option, context, "delta", **{name: value})
            for value in (0, -1, math.inf, math.nan):
                self.assert_rejection(
                    InvalidInputError,
                    partial(numerical_greeks, engine),
                    option,
                    context,
                    "delta",
                    **{name: value},
                )
        for value in (True, 1.5, 2.0):
            with self.assertRaises(TypeError):
                numerical_greeks(engine, option, context, "theta", time_shift=value)
        with self.assertRaises(TypeError):
            numerical_greeks(engine, option, context, "theta", time_shift=2**31)
        curve = RateTermStructure(
            reference_time=START,
            times=[END, END + (END - START)],
            factors=[0.95, (0.95) * 0.5],
        )
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=curve)
        )
        self.assert_rejection(
            UnsupportedOperationError,
            engine.price_with_greeks,
            option,
            curved,
            [],
        )
        self.assert_rejection(
            ComputationFailureError, engine.price, option, market(sigma=1000)
        )
        self.assert_rejection(
            ComputationFailureError,
            partial(numerical_greeks, engine),
            option,
            context,
            "rho",
            rate_shift=1000,
        )
        digital = CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=1,
            effective_time=START,
            expiry_time=END,
        )
        with self.assertRaises(UnsupportedOperationError):
            engine.price(digital, context)
        with self.assertRaises(UnsupportedOperationError):
            engine.price_with_greeks(digital, context, "delta")
        with self.assertRaises(UnsupportedOperationError):
            implied_volatility(
                engine, digital, context, 1, lower_bound=0.0, upper_bound=4.0
            )
        self.assertEqual(engine.price(option, context), price)
        self.assertEqual(context.model_parameters.volatility, 0.2)
        with self.assertRaises(AttributeError):
            engine.step_count = 2

    def test_live_quantlib_refinement_and_existing_error_budgets(self):
        american = importlib.import_module("quantlib_reference.american")
        oracle = importlib.import_module("quantlib_reference.oracle")
        oracle.require_version()
        config = oracle.load_fixture("american.json")
        profile = next(
            p for p in config["profiles"] if p["engine"] == "MonteCarloVanillaEngine"
        )
        engine = MonteCarloVanillaEngine(**profile["settings"])
        for scenario in config["scenarios"]:
            inputs = scenario["inputs"]
            option = AmericanOption(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_time=datetime.fromisoformat(inputs["effective_time"]),
                expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
            )
            context = market(
                spot=inputs["spot"],
                rate=inputs["rate"],
                dividend=inputs["dividend"],
                sigma=inputs["volatility"],
                valuation=datetime.fromisoformat(inputs["valuation"]),
            )
            with ql.SavedSettings():
                reference, uncertainty = american.reference(inputs)
            # Wide bumps cross exercise boundaries near expiry.
            names = sorted(reference.keys() - {"price"}) if scenario["wrapper"] else []
            result = (
                numerical_greeks(engine, option, context, names, **profile["shifts"])
                if names
                else None
            )
            self.assertAlmostEqual(
                result.price if result is not None else engine.price(option, context),
                reference["price"],
                delta=profile["tolerances"]["price"],
            )
            for name in names:
                with self.subTest(case=scenario["case_id"], measure=name):
                    self.assertAlmostEqual(
                        getattr(result, name),
                        reference[name],
                        delta=profile["numerical_tolerances"][name]
                        + uncertainty[f"uncertainty_{name}"],
                    )

    def test_default_spot_greeks_converge_across_seeds_and_price_scales(self):
        reference = CoxRossRubinsteinVanillaEngine(step_count=3200).price_with_greeks(
            contract(), market(), ["delta", "gamma"]
        )
        for scale, seed in ((0.01, 1), (1, 7), (100, 42)):
            with self.subTest(scale=scale, seed=seed):
                option = contract(strike=100 * scale)
                context = market(spot=100 * scale)
                engine = MonteCarloVanillaEngine(seed=seed, max_active_workers=1)
                result = engine.price_with_greeks(option, context, ["delta", "gamma"])
                explicit = numerical_greeks(
                    engine, option, context, ["delta", "gamma"], spot_shift=8 * scale
                )
                self.assertEqual(
                    (result.delta, result.gamma), (explicit.delta, explicit.gamma)
                )
                self.assertAlmostEqual(result.delta, reference.delta, delta=0.02)
                self.assertAlmostEqual(
                    result.gamma * scale, reference.gamma, delta=0.004
                )

    def test_price_sampling_error_and_exercise_grid_convergence(self):
        option, context = contract(), market()
        reference = CoxRossRubinsteinVanillaEngine(step_count=3200).price(
            option, context
        )
        rmse = []
        for paths in (1024, 16384):
            errors = [
                MonteCarloVanillaEngine(
                    path_count=paths, seed=seed, max_active_workers=1
                ).price(option, context)
                - reference
                for seed in range(16)
            ]
            rmse.append(math.sqrt(sum(error**2 for error in errors) / len(errors)))
        self.assertLess(rmse[1], rmse[0])
        self.assertLess(rmse[1], 0.25)
        prices = [
            MonteCarloVanillaEngine(
                path_count=100000, step_count=steps, seed=42, max_active_workers=1
            ).price(option, context)
            for steps in (25, 50, 100)
        ]
        for price in prices:
            self.assertAlmostEqual(price, reference, delta=0.3)
        self.assertLess(max(prices) - min(prices), 0.15)
