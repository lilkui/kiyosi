import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import ArithmeticAveragePriceOption, GeometricAveragePriceOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext

START, END = date(2025, 1, 1), date(2026, 1, 1)
KINDS = (ArithmeticAveragePriceOption, GeometricAveragePriceOption)


def market(spot=100, volatility=0.2, valuation=START, rate=0.04, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def option(kind, direction="call", strike=100, averaging=START, realized=0):
    return kind(
        option_type=direction,
        strike=strike,
        effective_date=START,
        averaging_start_date=averaging,
        expiry_date=END,
        realized_average=realized,
    )


class AsianMonteCarloTests(unittest.TestCase):
    def assert_category(self, category, operation, *args, **kwargs):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            operation(*args, **kwargs)
        self.assertEqual(error.exception.category, category)

    def test_constructor_exports_defaults_and_representation_errors(self):
        cls = pricing.MonteCarloAsianEngine
        self.assertIn("MonteCarloAsianEngine", pricing.__all__)
        engine, vanilla = cls(), pricing.MonteCarloVanillaEngine()
        for name in (
            "path_count",
            "step_count",
            "seed",
            "backend",
            "thread_count",
            "sampling",
            "antithetic",
        ):
            self.assertEqual(getattr(engine, name), getattr(vanilla, name))
        self.assertIn("MonteCarloAsianEngine(", repr(engine))
        with self.assertRaises(AttributeError):
            engine.seed = 5
        for name in ("path_count", "step_count", "thread_count", "seed"):
            for value in (True, 2.5):
                with self.subTest(name=name, value=value), self.assertRaises(TypeError):
                    cls(**{name: value})
            with self.subTest(name=name), self.assertRaises(OverflowError):
                cls(**{name: 1 << 100})
        with self.assertRaises(TypeError):
            cls(100)
        with self.assertRaises(TypeError):
            cls(antithetic=1)
        for settings in (
            {"path_count": 0},
            {"step_count": 1},
            {"step_count": 10001},
            {"thread_count": -1},
            {"backend": "cuda", "thread_count": 1},
            {"sampling": "sobol", "path_count": 100},
        ):
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER, cls, **settings
            )
        self.assert_category(
            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
            cls,
            path_count=64,
            sampling="sobol",
            backend="cuda",
        )

    def test_history_start_intraday_life_and_expiry(self):
        engine = pricing.MonteCarloAsianEngine(path_count=32, seed=42)
        averaging = START + timedelta(days=90)
        instant = datetime(2025, 4, 1, tzinfo=UTC)
        for kind in KINDS:
            for shift in (-1, 0, 1):
                for realized in (0, 100):
                    instrument = option(kind, averaging=averaging, realized=realized)
                    context = market(valuation=instant + timedelta(seconds=shift))
                    if (shift > 0) == (realized > 0):
                        self.assertGreaterEqual(engine.price(instrument, context), 0)
                    else:
                        self.assert_category(
                            kiyosi.ErrorCategory.INVALID_PARAMETER,
                            engine.price,
                            instrument,
                            context,
                        )
            for when in (START - timedelta(days=1), END + timedelta(days=1)):
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_TIME_RANGE,
                    engine.price,
                    option(kind),
                    market(valuation=when),
                )
            for backend in ("cpu", "cuda"):
                settled = pricing.MonteCarloAsianEngine(
                    path_count=1, seed=42, backend=backend
                )
                for direction, expected in (("call", 10), ("put", 0)):
                    result = settled.price_with_greeks(
                        option(kind, direction, realized=110),
                        market(valuation=END),
                        all_greeks=True,
                    )
                    self.assertEqual(result.price, expected)
                    self.assertIsNone(result.delta)
                    self.assertIsNone(result.gamma)
                    self.assertEqual(
                        settled.price(
                            option(kind, direction, averaging=END),
                            market(110, valuation=END),
                        ),
                        expected,
                    )

    def test_geometric_independent_reference_and_arithmetic_published_convergence(self):
        for seed in (7, 42, 103):
            engine = pricing.MonteCarloAsianEngine(
                path_count=65536,
                step_count=64,
                seed=seed,
                sampling="sobol",
                antithetic=True,
            )
            self.assertAlmostEqual(
                engine.price(option(GeometricAveragePriceOption), market()),
                5.035867546515384,
                delta=0.002,
            )
            # Published Vecer continuous references, one year in matching ACT/365 time.
            for spot, expected in ((1.9, 0.193174), (2.0, 0.246416), (2.1, 0.306220)):
                context = market(spot, 0.5, rate=0.05, dividend=0)
                call = option(ArithmeticAveragePriceOption, strike=2)
                actual = engine.price(call, context)
                self.assertAlmostEqual(actual, expected, delta=0.001)
                coarse = pricing.MonteCarloAsianEngine(
                    path_count=65536,
                    step_count=2,
                    seed=seed,
                    sampling="sobol",
                    antithetic=True,
                )
                self.assertLess(
                    abs(actual - expected), abs(coarse.price(call, context) - expected)
                )
                put = option(ArithmeticAveragePriceOption, "put", strike=2)
                parity = math.exp(-0.05) * (spot * math.expm1(0.05) / 0.05 - 2)
                self.assertAlmostEqual(
                    actual - engine.price(put, context), parity, delta=0.001
                )

    def test_requested_greeks_forward_and_seasoned_fixed_history(self):
        engine = pricing.MonteCarloAsianEngine(
            path_count=65536, step_count=32, seed=42, sampling="sobol", antithetic=True
        )
        for kind in KINDS:
            for direction in ("call", "put"):
                for window in (0, 1, 2):
                    averaging = START + timedelta(days=120) if window == 1 else START
                    valuation = START + timedelta(days=180) if window == 2 else START
                    instrument = option(
                        kind,
                        direction,
                        averaging=averaging,
                        realized=105 if window == 2 else 0,
                    )
                    context = market(valuation=valuation, rate=-0.02)
                    result = engine.price_with_greeks(
                        instrument, context, all_greeks=True
                    )
                    self.assertEqual(result.price, engine.price(instrument, context))
                    for name in (
                        "speed",
                        "theta",
                        "charm",
                        "color",
                        "vega",
                        "vanna",
                        "zomma",
                        "rho",
                    ):
                        self.assertIsNone(getattr(result, name))
                    if kind is GeometricAveragePriceOption:
                        expected = pricing.AnalyticGeometricAveragePriceEngine().price_with_greeks(
                            instrument, context, greeks=["delta", "gamma"]
                        )
                        self.assertAlmostEqual(
                            result.price, expected.price, delta=0.003
                        )
                        self.assertAlmostEqual(
                            result.delta, expected.delta, delta=0.001
                        )
                        self.assertAlmostEqual(
                            result.gamma, expected.gamma, delta=0.0005
                        )
                    else:
                        for h in (0.05, 0.025):
                            up = engine.price(
                                instrument,
                                market(100 + h, valuation=valuation, rate=-0.02),
                            )
                            down = engine.price(
                                instrument,
                                market(100 - h, valuation=valuation, rate=-0.02),
                            )
                            self.assertAlmostEqual(
                                result.delta, (up - down) / (2 * h), delta=0.00002
                            )
                            self.assertAlmostEqual(
                                result.gamma,
                                (up - 2 * result.price + down) / h**2,
                                delta=0.00002,
                            )
                    selected = engine.price_with_greeks(
                        instrument, context, greeks="gamma"
                    )
                    self.assertEqual(selected.gamma, result.gamma)
                    self.assertIsNone(selected.delta)
                    self.assert_category(
                        kiyosi.ErrorCategory.INVALID_PARAMETER,
                        engine.price_with_greeks,
                        instrument,
                        context,
                        greeks="delta",
                        spot_shift=0,
                    )

    def test_sampling_threading_concurrency_and_exact_distribution_step_invariance(
        self,
    ):
        for sampling, paths in (("pseudo_random", 4097), ("sobol", 4096)):
            engine = pricing.MonteCarloAsianEngine(
                path_count=paths,
                step_count=16,
                seed=42,
                sampling=sampling,
                antithetic=True,
                thread_count=1,
            )
            parallel = pricing.MonteCarloAsianEngine(
                path_count=paths,
                step_count=16,
                seed=42,
                sampling=sampling,
                antithetic=True,
                thread_count=4,
            )
            for kind in KINDS:
                instrument = option(kind)
                expected = engine.price_with_greeks(
                    instrument, market(), all_greeks=True
                )
                with ThreadPoolExecutor(max_workers=4) as pool:
                    results = list(
                        pool.map(
                            lambda _, parallel=parallel, instrument=instrument: (
                                parallel.price_with_greeks(
                                    instrument, market(), all_greeks=True
                                )
                            ),
                            range(8),
                        )
                    )
                for result in results:
                    self.assertEqual(
                        (result.price, result.delta, result.gamma),
                        (expected.price, expected.delta, expected.gamma),
                    )
                for steps in (2, 10000):
                    changed = pricing.MonteCarloAsianEngine(
                        path_count=paths,
                        step_count=steps,
                        seed=42,
                        sampling=sampling,
                        antithetic=True,
                    )
                    single = option(kind, averaging=END)
                    self.assertEqual(
                        changed.price(single, market()), engine.price(single, market())
                    )
                    if kind is GeometricAveragePriceOption:
                        self.assertEqual(
                            changed.price(instrument, market()), expected.price
                        )

    def test_tiny_volatility_scaled_prices_and_negative_seasoned_geometric_gamma(self):
        engine = pricing.MonteCarloAsianEngine(
            path_count=65536, step_count=64, seed=42, sampling="sobol", antithetic=True
        )
        for spot, volatility in (
            (100, 1e-170),
            (1e160, 1e-160),
            (1e300, 1e-200),
            (1e300, math.ulp(0.0)),
        ):
            expected = math.exp(math.log(spot) + math.log(volatility)) / math.sqrt(
                6 * math.pi
            )
            for kind in KINDS:
                for direction in ("call", "put"):
                    actual = engine.price(
                        option(kind, direction, strike=spot),
                        market(spot, volatility, rate=0, dividend=0),
                    )
                    self.assertTrue(
                        math.isclose(actual, expected, rel_tol=0.002),
                        (actual, expected),
                    )
        instrument = option(GeometricAveragePriceOption, strike=10, realized=100)
        context = market(valuation=START + timedelta(days=180))
        actual = engine.price_with_greeks(instrument, context, greeks="gamma")
        expected = pricing.AnalyticGeometricAveragePriceEngine().price_with_greeks(
            instrument, context, greeks="gamma"
        )
        self.assertLess(actual.gamma, 0)
        self.assertAlmostEqual(actual.gamma, expected.gamma, delta=0.0001)

    def test_implied_volatility_recovery_and_affine_backend_independence(self):
        engine = pricing.MonteCarloAsianEngine(
            path_count=16384, step_count=16, seed=42, sampling="sobol"
        )
        for kind in KINDS:
            instrument = option(kind)
            observed = engine.price(instrument, market())
            solved = pricing.implied_volatility(
                engine, instrument, market(volatility=0.35), observed
            )
            self.assertAlmostEqual(solved, 0.2, delta=0.00001)
        for backend in ("cpu", "cuda"):
            settled = pricing.MonteCarloAsianEngine(
                path_count=1, backend=backend, seed=42
            )
            instrument = option(ArithmeticAveragePriceOption, strike=25, realized=100)
            context = market(valuation=START + timedelta(days=180))
            actual = settled.price_with_greeks(instrument, context, all_greeks=True)
            self.assertEqual(
                actual.price,
                pricing.TurnbullWakemanArithmeticAveragePriceEngine().price(
                    instrument, context
                ),
            )
            self.assertEqual(actual.gamma, 0)
            future = 185 / 365
            self.assertAlmostEqual(
                actual.delta,
                math.exp(-0.04 * future) * math.expm1(0.03 * future) / 0.03,
                delta=1e-12,
            )
            self.assert_category(
                kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                pricing.implied_volatility,
                settled,
                instrument,
                context,
                actual.price,
            )

    def test_cuda_contracts_prices_and_gamma_only_slot(self):
        cpu = pricing.MonteCarloAsianEngine(
            path_count=131072, step_count=32, seed=42, antithetic=True
        )
        gpu = pricing.MonteCarloAsianEngine(
            path_count=131072, step_count=32, seed=42, backend="cuda", antithetic=True
        )
        for kind in KINDS:
            for direction in ("call", "put"):
                for window in (0, 1, 2):
                    instrument = option(
                        kind,
                        direction,
                        averaging=START + timedelta(days=120) if window == 1 else START,
                        realized=100 if window == 2 else 0,
                    )
                    context = market(
                        valuation=START + timedelta(days=180) if window == 2 else START
                    )
                    try:
                        actual = gpu.price_with_greeks(
                            instrument, context, all_greeks=True
                        )
                    except kiyosi.KiyosiError as error:
                        self.assertEqual(
                            error.category, kiyosi.ErrorCategory.BACKEND_UNAVAILABLE
                        )
                        continue
                    expected = cpu.price_with_greeks(
                        instrument, context, all_greeks=True
                    )
                    self.assertAlmostEqual(actual.price, expected.price, delta=0.08)
                    self.assertAlmostEqual(actual.delta, expected.delta, delta=0.01)
                    self.assertAlmostEqual(actual.gamma, expected.gamma, delta=0.002)
                    self.assertEqual(actual.price, gpu.price(instrument, context))
                    selected = gpu.price_with_greeks(
                        instrument, context, greeks="gamma"
                    )
                    self.assertEqual(selected.gamma, actual.gamma)
                    self.assertIsNone(selected.delta)


if __name__ == "__main__":
    unittest.main()
