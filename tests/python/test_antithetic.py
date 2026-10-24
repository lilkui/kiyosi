import math
import os
import unittest
from datetime import date

import kiyosi
import test_mc_shared_samples as shared
from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    BinarySnowballOption,
    EuropeanOption,
    PhoenixOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)

ENGINES = (
    pricing.MonteCarloVanillaEngine,
    pricing.MonteCarloAccumulatorEngine,
    pricing.MonteCarloAutocallableEngine,
)


class AntitheticTests(unittest.TestCase):
    def test_antithetic_defaults_strict_bool_readonly_and_repr(self):
        for engine_type in ENGINES:
            self.assertIs(engine_type().antithetic, False)
            for enabled in (False, True):
                engine = engine_type(antithetic=enabled)
                self.assertIs(engine.antithetic, enabled)
                self.assertIn(f"antithetic={enabled}", repr(engine))
                self.assertEqual(repr(eval(repr(engine), vars(pricing))), repr(engine))
                with self.assertRaises(AttributeError):
                    engine.antithetic = not enabled
            for value in (0, 1, 0.0, "true", b"false", None, [], object()):
                with (
                    self.subTest(engine=engine_type.__name__, value=value),
                    self.assertRaises(TypeError),
                ):
                    engine_type(antithetic=value)

    def test_antithetic_sobol_preserves_core_validation(self):
        for engine_type in ENGINES:
            for paths in (1, 2, 8_388_608):
                engine = engine_type(
                    path_count=paths, sampling="sobol", antithetic=True
                )
                self.assertEqual(engine.path_count, paths)
            for paths in (0, 3, 10_000_001):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine_type(path_count=paths, sampling="sobol", antithetic=True)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                )
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine_type(
                    path_count=2, sampling="sobol", backend="cuda", antithetic=True
                )
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION
            )

    def test_antithetic_sobol_products_match_cpp_explicit_payoffs(self):
        start, end = date(2025, 1, 1), date(2025, 1, 2)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        dt, z = 1 / 365, -0.10364940758940244
        positive = 100 * math.exp(0.03 * dt + 0.2 * math.sqrt(dt) * z)
        negative = 100 * math.exp(0.03 * dt - 0.2 * math.sqrt(dt) * z)
        common = {"effective_date": start, "expiry_date": end}
        cases = (
            (
                EuropeanOption(option_type="call", strike=90, **common),
                ENGINES[0],
                positive - 90,
                negative - 90,
            ),
            (
                AmericanOption(option_type="call", strike=100, **common),
                ENGINES[0],
                max(positive - 100, 0),
                max(negative - 100, 0),
            ),
            (
                Accumulator(
                    strike=100,
                    knock_out_level=100.05,
                    daily_quantity=1,
                    acceleration_factor=2,
                    **common,
                ),
                ENGINES[1],
                3 * (positive - 100),
                negative - 100,
            ),
            (
                BinarySnowballOption(
                    maturity_coupon_rate=0.06,
                    observations=autocallable_observations(
                        [end], [100], knock_out_coupon_rates=[0.08]
                    ),
                    **common,
                ),
                ENGINES[2],
                1 + 0.06 * dt,
                1 + 0.08 * dt,
            ),
        )
        for option, engine_type, first, second in cases:
            for paths, enabled in ((1, False), (1, True), (2, True)):
                with self.subTest(
                    option=type(option).__name__, paths=paths, enabled=enabled
                ):
                    engine = engine_type(
                        path_count=paths, seed=42, sampling="sobol", antithetic=enabled
                    )
                    expected = math.exp(-0.05 * dt) * (
                        first if paths == 1 else (first + second) / 2
                    )
                    self.assertAlmostEqual(
                        engine.price(option, context), expected, delta=1e-11
                    )
                    self.assertAlmostEqual(
                        engine.price_with_greeks(option, context, "delta").price,
                        expected,
                        delta=1e-11,
                    )

    def test_antithetic_all_products_preserve_shared_samples_and_threads(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        backends = (
            ("cpu", "cuda") if os.getenv("KIYOSI_TEST_CUDA") == "enabled" else ("cpu",)
        )
        for backend, sampling in (
            (backend, sampling)
            for backend in backends
            for sampling in ("pseudo_random", "sobol")
            if backend != "cuda" or sampling != "sobol"
        ):
            for enabled in (False, True):
                for option, engine_type in cases:
                    with self.subTest(
                        backend=backend,
                        sampling=sampling,
                        enabled=enabled,
                        option=type(option).__name__,
                    ):
                        settings = {
                            "path_count": 1025 if sampling == "pseudo_random" else 1024,
                            "seed": 42,
                            "backend": backend,
                            "sampling": sampling,
                            "antithetic": enabled,
                        }
                        if engine_type is ENGINES[0]:
                            settings["step_count"] = 7
                        engine = engine_type(**settings)
                        quote = engine.price(option, context)
                        actual = engine.price_with_greeks(
                            option, context, shared.GREEKS, spot_shift=0.5
                        )
                        self.assertEqual(actual.price, quote)
                        up = engine.price(
                            option, context.replace(spot_price=context.spot_price + 0.5)
                        )
                        down = engine.price(
                            option, context.replace(spot_price=context.spot_price - 0.5)
                        )
                        self.assertAlmostEqual(actual.delta, up - down, delta=1e-9)
                        self.assertAlmostEqual(
                            actual.gamma, (up - 2 * quote + down) / 0.25, delta=1e-9
                        )
                        volatility = pricing.implied_volatility(
                            engine,
                            option,
                            context,
                            quote,
                            lower_bound=0.1,
                            upper_bound=0.4,
                        )
                        shifted = context.replace(
                            model_parameters=context.model_parameters.replace(
                                volatility=volatility
                            )
                        )
                        self.assertAlmostEqual(
                            engine.price(option, shifted), quote, delta=1e-4
                        )
                        if engine_type is ENGINES[2]:
                            conventions = (
                                ({},)
                                if isinstance(option, PhoenixOption)
                                else (
                                    {"quote_convention": "preserve_maturity_coupon"},
                                    {"quote_convention": "shift_maturity_coupon"},
                                )
                            )
                            for convention in conventions:
                                coupon = pricing.implied_coupon(
                                    engine,
                                    option,
                                    context,
                                    quote,
                                    lower_bound=0.05,
                                    upper_bound=0.2,
                                    price_tolerance=1e-10,
                                    **convention,
                                )
                                self.assertAlmostEqual(coupon, 0.08, delta=1e-8)
                        if backend == "cpu":
                            serial = engine_type(
                                **settings, thread_count=1
                            ).price_with_greeks(
                                option, context, shared.GREEKS, spot_shift=0.5
                            )
                            self.assertEqual(
                                tuple(getattr(actual, name) for name in shared.GREEKS),
                                tuple(getattr(serial, name) for name in shared.GREEKS),
                            )


if __name__ == "__main__":
    unittest.main()
