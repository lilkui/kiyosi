import math
import os
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext

START, END = (date(2025, 1, 1), date(2026, 1, 1))
GREEKS = (
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "vanna",
    "zomma",
    "rho",
)


def market(spot=100, volatility=0.2, valuation=START, rate=0.05, dividend=0.02):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def options():
    for direction in ("call", "put"):
        terms = {
            "option_type": direction,
            "strike": 100,
            "effective_date": START,
            "expiry_date": END,
        }
        yield (CashOrNothingOption(**terms, payout=10), False)
        yield (AssetOrNothingOption(**terms), True)


class DigitalNumericalTests(unittest.TestCase):
    def assert_category(self, category, operation, *args, **kwargs):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            operation(*args, **kwargs)
        self.assertEqual(error.exception.category, category)

    def test_prices_and_supported_greeks_match_analytic_contracts(self):
        for remaining in (1, 365):
            time = remaining / 365
            width = 0.2 * math.sqrt(time)
            for spot in (95, 100, 105):
                context = market(spot=spot, valuation=END - timedelta(days=remaining))
                for option, asset in options():
                    expected = pricing.AnalyticDigitalEngine().price_with_greeks(
                        option, context, ("delta", "gamma")
                    )
                    for sampling in ("pseudo_random", "sobol"):
                        engine = pricing.MonteCarloDigitalEngine(
                            path_count=65536, seed=42, sampling=sampling
                        )
                        with self.subTest(
                            remaining=remaining,
                            spot=spot,
                            asset=asset,
                            sampling=sampling,
                        ):
                            actual = engine.price_with_greeks(
                                option, context, all_greeks=True
                            )
                            second = (
                                spot**2 * math.exp(-0.04 * time + width**2)
                                if asset
                                else 100 * math.exp(-0.1 * time)
                            )
                            errors = (
                                6 * math.sqrt(second / 65536),
                                6
                                * math.sqrt(
                                    second * (1 + 4 * width**2 if asset else 1) / 65536
                                )
                                / spot
                                / width,
                                6
                                * math.sqrt(
                                    second
                                    * (
                                        2 + 9 * width**2 + 4 * width**4
                                        if asset
                                        else 2 + width**2
                                    )
                                    / 65536
                                )
                                / spot**2
                                / width**2,
                            )
                            for name, error in zip(
                                ("price", "delta", "gamma"), errors, strict=True
                            ):
                                self.assertAlmostEqual(
                                    getattr(actual, name),
                                    getattr(expected, name),
                                    delta=error,
                                )
                            self.assertEqual(
                                actual.price, engine.price(option, context)
                            )
                            for name in GREEKS[2:]:
                                self.assertIsNone(getattr(actual, name))
                    if remaining == 365:
                        for steps in (255, 256, 4095, 4096):
                            engine = pricing.CoxRossRubinsteinDigitalEngine(
                                step_count=steps
                            )
                            actual = engine.price_with_greeks(
                                option, context, all_greeks=True
                            )
                            error = (spot if asset else 10) / math.sqrt(steps)
                            self.assertAlmostEqual(
                                actual.price, expected.price, delta=error
                            )
                            self.assertAlmostEqual(
                                actual.delta, expected.delta, delta=error / spot
                            )
                            self.assertAlmostEqual(
                                actual.gamma,
                                expected.gamma,
                                delta=error / spot**2 / 0.04,
                            )
                            for name in GREEKS[2:]:
                                self.assertIsNone(getattr(actual, name))

    def test_negative_rates_dividends_and_carry_match_analytic_values(self):
        for rate, dividend, volatility in (
            (-0.03, 0.01, 0.45),
            (0.01, 0.07, 0.3),
            (-0.02, -0.05, 0.15),
        ):
            context = market(rate=rate, dividend=dividend, volatility=volatility)
            for option, asset in options():
                expected = pricing.AnalyticDigitalEngine().price_with_greeks(
                    option, context, ("delta", "gamma")
                )
                for engine, error in (
                    (
                        pricing.MonteCarloDigitalEngine(
                            path_count=65_536, seed=42, sampling="sobol"
                        ),
                        1e-3,
                    ),
                    (
                        pricing.CoxRossRubinsteinDigitalEngine(step_count=4095),
                        1 / math.sqrt(4095),
                    ),
                ):
                    actual = engine.price_with_greeks(option, context, all_greeks=True)
                    scale = 100 if asset else 10
                    self.assertAlmostEqual(
                        actual.price, expected.price, delta=error * scale
                    )
                    self.assertAlmostEqual(
                        actual.delta,
                        expected.delta,
                        delta=error * scale / 100 / volatility,
                    )
                    self.assertAlmostEqual(
                        actual.gamma,
                        expected.gamma,
                        delta=error * scale / 10_000 / volatility**2,
                    )

    def test_one_and_two_step_tree_values_preserve_strike_equality(self):
        for steps in (1, 2):
            up = math.exp(0.2 / math.sqrt(steps))
            probability = math.expm1(0.03 / steps + math.log(up)) / math.expm1(
                2 * math.log(up)
            )
            discount = math.exp(-0.05 / steps)
            for option, asset in options():
                values = []
                for node in range(steps + 1):
                    terminal = 100 * up ** (2 * node - steps)
                    pays = (
                        terminal > 100
                        if option.option_type == "call"
                        else terminal < 100
                    )
                    values.append((terminal if asset else 10) if pays else 0)
                second = values[:] if steps == 2 else None
                if steps == 2:
                    values = [
                        discount
                        * ((1 - probability) * values[n] + probability * values[n + 1])
                        for n in range(2)
                    ]
                delta = (values[1] - values[0]) / (100 * (up - 1 / up))
                price = discount * (
                    (1 - probability) * values[0] + probability * values[1]
                )
                actual = pricing.CoxRossRubinsteinDigitalEngine(
                    step_count=steps
                ).price_with_greeks(option, market(), all_greeks=True)
                self.assertAlmostEqual(actual.price, price, delta=1e-12)
                self.assertAlmostEqual(actual.delta, delta, delta=1e-12)
                if second is None:
                    self.assertIsNone(actual.gamma)
                else:
                    gamma = (
                        (second[2] - second[1]) / (100 * (up**2 - 1))
                        - (second[1] - second[0]) / (100 * (1 - up ** (-2)))
                    ) / (50 * (up**2 - up ** (-2)))
                    self.assertAlmostEqual(actual.gamma, gamma, delta=1e-12)

    def test_mc_threads_steps_selections_and_concurrency_share_samples(self):
        for sampling, paths in (("pseudo_random", 1025), ("sobol", 1024)):
            for antithetic in (False, True):
                settings = {
                    "path_count": paths,
                    "seed": 42,
                    "sampling": sampling,
                    "antithetic": antithetic,
                }
                for option, _ in options():
                    engine = pricing.MonteCarloDigitalEngine(**settings)
                    expected = engine.price_with_greeks(
                        option, market(), all_greeks=True
                    )
                    for threads in (0, 1, 4):
                        other = pricing.MonteCarloDigitalEngine(
                            **settings, step_count=10000, thread_count=threads
                        )
                        for name in ("delta", "gamma", "rho"):
                            actual = other.price_with_greeks(option, market(), name)
                            self.assertEqual(actual.price, expected.price)
                            self.assertEqual(
                                getattr(actual, name), getattr(expected, name)
                            )
                            self.assertIsNone(
                                getattr(actual, "gamma" if name == "delta" else "delta")
                            )
                    with ThreadPoolExecutor(max_workers=2) as pool:
                        futures = [
                            pool.submit(engine.price, option, market())
                            for _ in range(2)
                        ]
                        results = [future.result() for future in futures]
                    self.assertEqual(results, [expected.price, expected.price])

    def test_configuration_defaults_exports_repr_and_native_representation_errors(self):
        mc = pricing.MonteCarloDigitalEngine()
        vanilla = pricing.MonteCarloVanillaEngine()
        for name in (
            "path_count",
            "step_count",
            "seed",
            "backend",
            "thread_count",
            "sampling",
            "antithetic",
        ):
            self.assertEqual(getattr(mc, name), getattr(vanilla, name))
        tree = pricing.CoxRossRubinsteinDigitalEngine()
        self.assertEqual(
            tree.step_count, pricing.CoxRossRubinsteinVanillaEngine().step_count
        )
        for engine in (mc, tree):
            self.assertIn(type(engine).__name__, pricing.__all__)
            self.assertEqual(repr(eval(repr(engine), vars(pricing))), repr(engine))
            with self.assertRaises(AttributeError):
                engine.step_count = 1
        for engine_type, fields in (
            (
                pricing.MonteCarloDigitalEngine,
                ("path_count", "step_count", "seed", "thread_count"),
            ),
            (pricing.CoxRossRubinsteinDigitalEngine, ("step_count",)),
        ):
            for field in fields:
                for value in (True, 1.5, "3"):
                    with (
                        self.subTest(field=field, value=value),
                        self.assertRaises(TypeError),
                    ):
                        engine_type(**{field: value})
                with self.assertRaises(OverflowError):
                    engine_type(**{field: 1 << 100})
        for field in ("backend", "sampling"):
            with self.assertRaises(TypeError):
                pricing.MonteCarloDigitalEngine(**{field: True})
            with self.assertRaises(ValueError):
                pricing.MonteCarloDigitalEngine(**{field: "unknown"})
        with self.assertRaises(TypeError):
            pricing.MonteCarloDigitalEngine(antithetic=1)
        for settings in (
            {"path_count": 0},
            {"path_count": 10000001},
            {"step_count": 1},
            {"step_count": 10001},
            {"thread_count": -1},
            {"path_count": 3, "sampling": "sobol"},
        ):
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                pricing.MonteCarloDigitalEngine,
                **settings,
            )
        for steps in (0, -1, 1000001):
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                pricing.CoxRossRubinsteinDigitalEngine,
                step_count=steps,
            )
        self.assert_category(
            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
            pricing.MonteCarloDigitalEngine,
            path_count=1024,
            sampling="sobol",
            backend="cuda",
        )

    def test_requests_lifetimes_tiny_volatility_and_greek_overflow(self):
        for option, asset in options():
            for engine in (
                pricing.MonteCarloDigitalEngine(path_count=2, seed=42, antithetic=True),
                pricing.CoxRossRubinsteinDigitalEngine(),
            ):
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_PARAMETER,
                    engine.price_with_greeks,
                    option,
                    market(),
                    [],
                )
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_PARAMETER,
                    engine.price_with_greeks,
                    option,
                    market(),
                    "rho",
                    spot_shift=0,
                )
                with self.assertRaises(TypeError):
                    engine.price_with_greeks(option, market(), "delta", spot_shift=True)
                for valuation in (START - timedelta(days=1), END + timedelta(days=1)):
                    self.assert_category(
                        kiyosi.ErrorCategory.INVALID_TIME_RANGE,
                        engine.price,
                        option,
                        market(valuation=valuation),
                    )
            for sigma in (1e-200, math.ulp(0.0)):
                context = market(
                    volatility=sigma,
                    rate=0,
                    dividend=0,
                    valuation=datetime(2026, 1, 1, tzinfo=UTC)
                    - timedelta(microseconds=1),
                )
                engine = pricing.MonteCarloDigitalEngine(
                    path_count=2, seed=42, antithetic=True
                )
                self.assertEqual(engine.price(option, context), 50 if asset else 5)
                self.assertIsNone(engine.price_with_greeks(option, context, "rho").rho)
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_RESULT,
                    engine.price_with_greeks,
                    option,
                    context,
                    "gamma",
                )
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_RESULT,
                    pricing.CoxRossRubinsteinDigitalEngine().price,
                    option,
                    context,
                )

    def test_implied_volatility_retains_brackets_and_tree_domain(self):
        option = next(options())[0]
        context = market()
        for engine, tolerance in (
            (pricing.MonteCarloDigitalEngine(path_count=65536, seed=42), 0.001),
            (pricing.CoxRossRubinsteinDigitalEngine(step_count=1), 1e-06),
        ):
            quote = engine.price(option, context)
            result = pricing.implied_volatility(
                engine, option, context, quote, lower_bound=0.1, upper_bound=0.3
            )
            self.assertAlmostEqual(result, 0.2, delta=tolerance)
            self.assert_category(
                kiyosi.ErrorCategory.UNBRACKETED_VOLATILITY,
                pricing.implied_volatility,
                engine,
                option,
                context,
                20,
                lower_bound=0.1,
                upper_bound=0.3,
            )
        self.assert_category(
            kiyosi.ErrorCategory.UNBRACKETED_VOLATILITY,
            pricing.implied_volatility,
            pricing.CoxRossRubinsteinDigitalEngine(step_count=1),
            option,
            context,
            1,
            lower_bound=0.001,
            upper_bound=0.02,
        )

    def test_cuda_accuracy_availability_and_expiry_settlement(self):
        engine = pricing.MonteCarloDigitalEngine(
            path_count=262144, seed=42, backend="cuda"
        )
        for option, asset in options():
            if os.environ.get("KIYOSI_TEST_CUDA") == "enabled":
                actual = engine.price_with_greeks(option, market(), all_greeks=True)
                expected = pricing.AnalyticDigitalEngine().price_with_greeks(
                    option, market(), ("delta", "gamma")
                )
                scale = 10 if asset else 1
                for name, error in (
                    ("price", 0.08),
                    ("delta", 0.003),
                    ("gamma", 0.0005),
                ):
                    self.assertAlmostEqual(
                        getattr(actual, name),
                        getattr(expected, name),
                        delta=error * scale,
                    )
                gamma_only = engine.price_with_greeks(option, market(), "gamma")
                self.assertEqual(gamma_only.gamma, actual.gamma)
                self.assertIsNone(gamma_only.delta)
                tiny = market(
                    volatility=math.ulp(0.0),
                    rate=0,
                    dividend=0,
                    valuation=datetime(2026, 1, 1, tzinfo=UTC)
                    - timedelta(microseconds=1),
                )
                paired = pricing.MonteCarloDigitalEngine(
                    path_count=2, seed=42, backend="cuda", antithetic=True
                )
                result = paired.price_with_greeks(option, tiny, "rho")
                self.assertEqual(result.price, 50 if asset else 5)
                self.assertIsNone(result.rho)
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_RESULT,
                    paired.price_with_greeks,
                    option,
                    tiny,
                    "gamma",
                )
            elif os.environ.get("KIYOSI_TEST_CUDA") == "disabled":
                self.assert_category(
                    kiyosi.ErrorCategory.BACKEND_UNAVAILABLE,
                    engine.price,
                    option,
                    market(),
                )
            for spot in (math.nextafter(100, 0), 100, math.nextafter(100, 101)):
                actual = engine.price_with_greeks(
                    option, market(spot=spot, valuation=END), all_greeks=True
                )
                pays = spot > 100 if option.option_type == "call" else spot < 100
                self.assertEqual(actual.price, (spot if asset else 10) if pays else 0)
                for name in GREEKS:
                    self.assertIsNone(getattr(actual, name))
