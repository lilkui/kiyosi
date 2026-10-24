import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import date, timedelta
from typing import get_args

import kiyosi
import test_mc_shared_samples as shared
from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
    autocallable_observations,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext

ENGINES = (
    pricing.MonteCarloVanillaEngine,
    pricing.MonteCarloAccumulatorEngine,
    pricing.MonteCarloAutocallableEngine,
)


class SobolTests(unittest.TestCase):
    def test_sampling_defaults_properties_and_repr(self):
        self.assertEqual(
            get_args(pricing.MonteCarloSampling), ("pseudo_random", "sobol")
        )
        for engine_type in ENGINES:
            with self.subTest(engine=engine_type.__name__):
                default = engine_type()
                self.assertEqual(default.sampling, "pseudo_random")
                self.assertEqual(
                    default.path_count,
                    100_000 if engine_type is ENGINES[0] else 20_000,
                )
                engine = engine_type(path_count=1024, seed=42, sampling="sobol")
                self.assertEqual(engine.sampling, "sobol")
                self.assertEqual(engine.path_count, 1024)
                self.assertIn("sampling='sobol'", repr(engine))
                self.assertEqual(repr(eval(repr(engine), vars(pricing))), repr(engine))
                with self.assertRaises(AttributeError):
                    engine.sampling = "pseudo_random"

    def test_sampling_representation_errors_are_native(self):
        for engine_type in ENGINES:
            for value in (True, False, 1, 1.0, b"sobol", None, []):
                with (
                    self.subTest(engine=engine_type.__name__, sampling=value),
                    self.assertRaises(TypeError),
                ):
                    engine_type(path_count=1024, sampling=value)
            for value in ("Sobol", "", "pseudo-random"):
                with (
                    self.subTest(engine=engine_type.__name__, sampling=value),
                    self.assertRaises(ValueError),
                ):
                    engine_type(path_count=1024, sampling=value)

    def test_sobol_path_count_is_validated_at_construction(self):
        for engine_type in ENGINES:
            for paths in (1, 2, 1024, 8_388_608):
                with self.subTest(engine=engine_type.__name__, paths=paths):
                    self.assertEqual(
                        engine_type(path_count=paths, sampling="sobol").path_count,
                        paths,
                    )
            for paths in (0, 3, 20_000, 100_000, 8_388_609, 16_777_216):
                with self.subTest(engine=engine_type.__name__, paths=paths):
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        engine_type(path_count=paths, sampling="sobol")
                    self.assertEqual(
                        error.exception.category,
                        kiyosi.ErrorCategory.INVALID_PARAMETER,
                    )
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine_type(sampling="sobol")
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )

    def test_sobol_cuda_combination_is_rejected_at_construction(self):
        for engine_type in ENGINES:
            with self.subTest(engine=engine_type.__name__):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine_type(path_count=1024, backend="cuda", sampling="sobol")
                self.assertEqual(
                    error.exception.category,
                    kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                )

    def test_explicit_pseudorandom_preserves_seeded_prices(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        for option, engine_type in cases:
            with self.subTest(option=type(option).__name__):
                settings = {"path_count": 1025, "seed": 42}
                default = engine_type(**settings)
                explicit = engine_type(**settings, sampling="pseudo_random")
                self.assertEqual(
                    default.price(option, context), explicit.price(option, context)
                )

    def test_sobol_prices_and_greeks_are_invariant_to_thread_count(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        for option, engine_type in cases:
            settings = {"path_count": 1024, "seed": 42, "sampling": "sobol"}
            if engine_type is pricing.MonteCarloVanillaEngine:
                settings["step_count"] = 7
            serial = engine_type(**settings, thread_count=1)
            quote = serial.price(option, context)
            expected = serial.price_with_greeks(option, context, shared.GREEKS)
            self.assertAlmostEqual(expected.price, quote, delta=1e-12)
            for threads in (0, 2, 4):
                with self.subTest(option=type(option).__name__, threads=threads):
                    engine = engine_type(**settings, thread_count=threads)
                    self.assertEqual(engine.price(option, context), quote)
                    actual = engine.price_with_greeks(option, context, shared.GREEKS)
                    self.assertEqual(actual.price, expected.price)
                    self.assertEqual(
                        tuple(getattr(actual, name) for name in shared.GREEKS),
                        tuple(getattr(expected, name) for name in shared.GREEKS),
                    )

    def test_sobol_shared_engine_calls_are_independent(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        for option, engine_type in cases:
            with self.subTest(option=type(option).__name__):
                engine = engine_type(path_count=1024, seed=42, sampling="sobol")
                expected = engine.price(option, context)
                with ThreadPoolExecutor(max_workers=2) as executor:
                    tasks = [
                        executor.submit(engine.price, option, context) for _ in range(2)
                    ]
                    self.assertEqual(
                        [task.result() for task in tasks], [expected, expected]
                    )

    def test_sobol_european_price_and_delta_agree_with_analytic(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=32_768, seed=42, sampling="sobol"
        )
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction,
                strike=100,
                effective_date=start,
                expiry_date=end,
            )
            with self.subTest(direction=direction):
                actual = engine.price_with_greeks(option, context, "delta")
                reference = pricing.AnalyticVanillaEngine().price_with_greeks(
                    option, context, "delta"
                )
                self.assertAlmostEqual(actual.price, reference.price, delta=0.005)
                self.assertAlmostEqual(actual.delta, reference.delta, delta=0.0005)
                for steps in (2, 10_000):
                    self.assertEqual(
                        pricing.MonteCarloVanillaEngine(
                            path_count=32_768,
                            step_count=steps,
                            seed=42,
                            sampling="sobol",
                        ).price(option, context),
                        actual.price,
                    )

    def test_sobol_single_point_matches_cpp_reference_without_antithetic_pairing(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(
            option_type="call", strike=90, effective_date=start, expiry_date=end
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=1, seed=42, sampling="sobol"
        )
        expected = math.exp(-0.05) * (
            100 * math.exp(0.01 + 0.2 * (-0.10364940758940244)) - 90
        )
        self.assertAlmostEqual(engine.price(option, context), expected, delta=1e-11)

    def test_sobol_greeks_reuse_scalar_samples(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        shift = 0.5
        for option, engine_type in cases:
            with self.subTest(option=type(option).__name__):
                engine = engine_type(path_count=4096, seed=42, sampling="sobol")
                actual = engine.price_with_greeks(
                    option, context, ["delta", "gamma"], spot_shift=shift
                )
                up = engine.price(
                    option, context.replace(spot_price=context.spot_price + shift)
                )
                down = engine.price(
                    option, context.replace(spot_price=context.spot_price - shift)
                )
                center = engine.price(option, context)
                self.assertAlmostEqual(actual.price, center, delta=1e-12)
                self.assertAlmostEqual(
                    actual.delta, (up - down) / (2 * shift), delta=1e-10
                )
                self.assertAlmostEqual(
                    actual.gamma, (up - 2 * center + down) / shift**2, delta=1e-10
                )

    def test_sobol_implied_volatility_reuses_seeded_samples(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        for option, engine_type in cases:
            with self.subTest(option=type(option).__name__):
                engine = engine_type(path_count=1024, seed=42, sampling="sobol")
                quote = engine.price(option, context)
                volatility = pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    quote,
                    lower_bound=0.1,
                    upper_bound=0.4,
                )
                repriced = engine.price(
                    option,
                    context.replace(
                        model_parameters=context.model_parameters.replace(
                            volatility=volatility
                        )
                    ),
                )
                self.assertAlmostEqual(repriced, quote, delta=1e-4)
                self.assertEqual(
                    pricing.implied_volatility(
                        engine,
                        option,
                        context,
                        quote,
                        lower_bound=0.1,
                        upper_bound=0.4,
                    ),
                    volatility,
                )

    def test_sobol_extreme_seed_and_single_path_are_finite(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        for option, engine_type in cases:
            with self.subTest(option=type(option).__name__):
                for seed in (0, 2**64 - 1):
                    engine = engine_type(path_count=1, seed=seed, sampling="sobol")
                    self.assertTrue(math.isfinite(engine.price(option, context)))

    def test_sobol_implied_coupon_recovers_seeded_quotes(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        for option, engine_type in cases:
            if engine_type is not pricing.MonteCarloAutocallableEngine:
                continue
            conventions = (
                ({},)
                if isinstance(option, PhoenixOption)
                else (
                    {"quote_convention": "preserve_maturity_coupon"},
                    {"quote_convention": "shift_maturity_coupon"},
                )
            )
            engine = engine_type(path_count=1024, seed=42, sampling="sobol")
            quote = engine.price(option, context)
            for convention in conventions:
                with self.subTest(option=type(option).__name__, **convention):
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

    def test_sobol_dimension_limit_and_failed_call_preserve_engine(self):
        context, cases = shared.SharedSampleTests.parallel_cases()
        start = date(2025, 1, 1)
        for option, engine_type in cases[:2]:
            engine = engine_type(path_count=1, seed=42, sampling="sobol")
            expected = engine.price(option, context)
            for dimensions in (21_201, 21_203):
                end = start + timedelta(days=dimensions)
                if engine_type is pricing.MonteCarloAccumulatorEngine:
                    extended = Accumulator(
                        strike=100,
                        knock_out_level=1000,
                        daily_quantity=1,
                        acceleration_factor=2,
                        effective_date=start,
                        expiry_date=end,
                    )
                else:
                    extended = SnowballOption(
                        maturity_coupon_rate=0.06,
                        initial_spot=100,
                        knock_in_level=80,
                        upper_strike=100,
                        lower_strike=60,
                        knock_in_observation_mode="every_trading_day",
                        barrier_state="none",
                        effective_date=start,
                        expiry_date=end,
                        observations=autocallable_observations(
                            [end], [110], knock_out_coupon_rates=[0.08]
                        ),
                    )
                with self.subTest(engine=engine_type.__name__, dimensions=dimensions):
                    if dimensions == 21_201:
                        self.assertTrue(math.isfinite(engine.price(extended, context)))
                    else:
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            engine.price(extended, context)
                        self.assertEqual(
                            error.exception.category,
                            kiyosi.ErrorCategory.INVALID_PARAMETER,
                        )
                    self.assertEqual(engine.price(option, context), expected)


if __name__ == "__main__":
    unittest.main()
