import math
import os
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import BarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)

START = date(2025, 1, 1)
END = START + timedelta(days=360)
KINDS = ("up_and_in", "up_and_out", "down_and_in", "down_and_out")
OTHER_GREEKS = ("speed", "theta", "charm", "color", "vega", "vanna", "zomma", "rho")


def market(spot=100, valuation=START, rate=0.03, volatility=0.2, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
        calendar=all_days_calendar(),
    )


def option(kind="down_and_out", direction="call", **changes):
    terms = {
        "option_type": direction,
        "strike": 100,
        "effective_date": START,
        "expiry_date": END,
        "barrier_level": 120 if kind.startswith("up") else 80,
        "barrier_type": kind,
    }
    terms.update(changes)
    return BarrierOption(**terms)


def engines():
    return (
        pricing.MonteCarloBarrierEngine(path_count=65_536, seed=42, sampling="sobol"),
        pricing.CoxRossRubinsteinBarrierEngine(step_count=720),
        pricing.QuadratureBarrierEngine(),
    )


class BarrierNumericalTests(unittest.TestCase):
    def assert_category(self, category, operation, *args, **kwargs):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            operation(*args, **kwargs)
        self.assertEqual(error.exception.category, category)

    def test_continuous_prices_and_native_greeks_match_analytic(self):
        context = market()
        numerical = engines()
        for kind in KINDS:
            for direction in ("call", "put"):
                contract = option(kind, direction, rebate=2)
                expected = pricing.AnalyticBarrierEngine().price_with_greeks(
                    contract, context, ("delta", "gamma")
                )
                for engine, price_error, delta_error, gamma_error in zip(
                    numerical,
                    (0.03, 0.6, 0.0002),
                    (0.015, 0.07, 0.0002),
                    (0.004, 0.01, 0.0002),
                    strict=True,
                ):
                    with self.subTest(kind=kind, direction=direction, engine=engine):
                        result = engine.price_with_greeks(
                            contract, context, ("delta", "gamma")
                        )
                        self.assertAlmostEqual(
                            result.price, expected.price, delta=price_error
                        )
                        self.assertAlmostEqual(
                            result.delta, expected.delta, delta=delta_error
                        )
                        self.assertAlmostEqual(
                            result.gamma, expected.gamma, delta=gamma_error
                        )

    def test_scheduled_dates_and_early_monitoring_end_match_refined_fd(self):
        context = market()
        dates = [START + timedelta(days=90), START + timedelta(days=180)]
        reference = pricing.FiniteDifferenceBarrierEngine(
            asset_step_count=1600, time_step_count=1600
        )
        for kind in KINDS:
            contract = option(
                kind, rebate=2, observation_mode="scheduled", observation_dates=dates
            )
            expected = reference.price(contract, context)
            for engine, tolerance in zip(engines(), (0.09, 0.5, 0.03), strict=True):
                with self.subTest(kind=kind, engine=engine):
                    self.assertAlmostEqual(
                        engine.price(contract, context), expected, delta=tolerance
                    )

    def test_crr_requires_exact_future_observation_alignment(self):
        contract = option(
            observation_mode="scheduled",
            observation_dates=[START + timedelta(days=91), END],
        )
        self.assert_category(
            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
            pricing.CoxRossRubinsteinBarrierEngine(step_count=256).price,
            contract,
            market(),
        )
        aligned = pricing.CoxRossRubinsteinBarrierEngine(step_count=360)
        self.assertGreater(aligned.price(contract, market()), 0)
        intraday = datetime(2025, 1, 1, 12, tzinfo=UTC)
        self.assert_category(
            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
            aligned.price,
            contract,
            market(valuation=intraday),
        )

    def test_current_touch_and_paid_history_resolve_before_backend_or_grid(self):
        for engine in (*engines(), pricing.MonteCarloBarrierEngine(backend="cuda")):
            for kind in ("up_and_out", "down_and_out"):
                level = 120 if kind.startswith("up") else 80
                for timing in ("at_hit", "at_expiry"):
                    contract = option(kind, rebate=7, rebate_timing=timing)
                    expected = (
                        7 if timing == "at_hit" else 7 * math.exp(-0.03 * 360 / 365)
                    )
                    self.assertAlmostEqual(
                        engine.price(contract, market(spot=level)),
                        expected,
                        delta=1e-12,
                    )
                    result = engine.price_with_greeks(
                        contract, market(spot=level), all_greeks=True
                    )
                    for greek in ("delta", "gamma", *OTHER_GREEKS):
                        self.assertIsNone(getattr(result, greek))
                paid = option(
                    kind, rebate=7, rebate_timing="at_hit", touch_state="touched"
                )
                self.assertEqual(
                    engine.price(paid, market(valuation=START + timedelta(days=1))), 0
                )

    def test_history_is_required_and_failure_does_not_change_engine(self):
        for engine in engines():
            contract = option()
            expected = engine.price(contract, market())
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                engine.price,
                contract,
                market(valuation=START + timedelta(days=1)),
            )
            self.assertEqual(engine.price(contract, market()), expected)
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_OPTION,
                engine.price,
                option(touch_state="touched"),
                market(),
            )

    def test_scheduled_touch_occurs_only_at_original_midnight(self):
        observation = START + timedelta(days=90)
        midday = datetime.combine(
            observation, datetime.min.time(), tzinfo=UTC
        ) + timedelta(hours=12)
        for kind in KINDS:
            level = 120 if kind.startswith("up") else 80
            terms = {
                "rebate": 5,
                "rebate_timing": "at_hit" if kind.endswith("out") else "at_expiry",
                "observation_mode": "scheduled",
                "observation_dates": [observation],
            }
            contract = option(kind, **terms)
            prior_untouched = option(kind, **terms, touch_state="untouched")
            vanilla = EuropeanOption(
                option_type="call", strike=100, effective_date=START, expiry_date=END
            )
            at_observation = market(spot=level, valuation=observation)
            after_observation = market(spot=level, valuation=midday)
            for engine in engines():
                current = (
                    5
                    if kind.endswith("out")
                    else pricing.AnalyticVanillaEngine().price(vanilla, at_observation)
                )
                later = (
                    pricing.AnalyticVanillaEngine().price(vanilla, after_observation)
                    if kind.endswith("out")
                    else 5 * math.exp(-0.03 * 269.5 / 365)
                )
                self.assertAlmostEqual(
                    engine.price(contract, at_observation), current, delta=1e-12
                )
                self.assertAlmostEqual(
                    engine.price(prior_untouched, after_observation), later, delta=1e-12
                )

    def test_completed_monitoring_returns_vanilla_or_expiry_rebate(self):
        dates = [START + timedelta(days=90)]
        valuation = START + timedelta(days=180)
        context = market(valuation=valuation)
        vanilla = EuropeanOption(
            option_type="call", strike=100, effective_date=START, expiry_date=END
        )
        value = pricing.AnalyticVanillaEngine().price(vanilla, context)
        for engine in engines():
            for kind in KINDS:
                contract = option(
                    kind,
                    rebate=3,
                    observation_mode="scheduled",
                    observation_dates=dates,
                    touch_state="untouched",
                )
                expected = (
                    3 * math.exp(-0.03 * 180 / 365) if kind.endswith("in") else value
                )
                self.assertAlmostEqual(
                    engine.price(contract, context), expected, delta=1e-12
                )

    def test_negative_rate_at_hit_rebates_have_finite_accurate_values(self):
        context = market(rate=-0.02, dividend=-0.02)
        for kind in ("up_and_out", "down_and_out"):
            contract = option(kind, rebate=7, rebate_timing="at_hit")
            expected = pricing.FiniteDifferenceBarrierEngine(
                asset_step_count=1800, time_step_count=1600
            ).price(contract, context)
            for engine, tolerance in zip(engines(), (0.04, 0.6, 0.02), strict=True):
                self.assertAlmostEqual(
                    engine.price(contract, context), expected, delta=tolerance
                )

    def test_mc_seed_threads_antithetic_and_steps_preserve_prices(self):
        contract = option(
            observation_mode="scheduled",
            observation_dates=[START + timedelta(days=90), END],
        )
        for sampling, paths in (("pseudo_random", 4097), ("sobol", 4096)):
            kwargs = {
                "path_count": paths,
                "seed": 42,
                "sampling": sampling,
                "antithetic": True,
            }
            serial = pricing.MonteCarloBarrierEngine(**kwargs, thread_count=1)
            parallel = pricing.MonteCarloBarrierEngine(
                **kwargs, thread_count=4, step_count=10_000
            )
            expected = serial.price_with_greeks(contract, market(), ("delta", "gamma"))
            actual = parallel.price_with_greeks(contract, market(), ("delta", "gamma"))
            for field in ("price", "delta", "gamma"):
                self.assertEqual(getattr(actual, field), getattr(expected, field))
            with ThreadPoolExecutor(max_workers=2) as pool:
                values = list(
                    pool.map(
                        lambda _, engine=parallel: engine.price(contract, market()),
                        range(2),
                    )
                )
            self.assertEqual(values, [expected.price, expected.price])

    def test_mc_and_crr_return_only_requested_supported_greeks(self):
        for engine in engines()[:2]:
            result = engine.price_with_greeks(option(), market(), all_greeks=True)
            self.assertIsNotNone(result.delta)
            self.assertIsNotNone(result.gamma)
            for name in OTHER_GREEKS:
                self.assertIsNone(getattr(result, name))
            selected = engine.price_with_greeks(option(), market(), "delta")
            self.assertEqual(selected.delta, result.delta)
            self.assertIsNone(selected.gamma)
        one = pricing.CoxRossRubinsteinBarrierEngine(step_count=1)
        self.assertIsNone(one.price_with_greeks(option(), market(), "gamma").gamma)

    def test_quadrature_refinement_and_requested_higher_greeks(self):
        contract, context = option(rebate=2), market()
        names = ("delta", "gamma", "vega", "rho")
        expected = pricing.AnalyticBarrierEngine().price_with_greeks(
            contract, context, names
        )
        for steps, extent in ((256, 8), (512, 8), (512, 10)):
            engine = pricing.QuadratureBarrierEngine(
                asset_step_count=steps, integration_standard_deviations=extent
            )
            result = engine.price_with_greeks(contract, context, names)
            for name in ("price", *names):
                self.assertAlmostEqual(
                    getattr(result, name),
                    getattr(expected, name),
                    delta=0.001 if steps == 256 else 0.0002,
                )
            self.assertIsNone(result.speed)

    def test_valuation_and_greek_validation_preserve_core_categories(self):
        for engine in engines():
            for valuation in (START - timedelta(days=1), END + timedelta(days=1)):
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_TIME_RANGE,
                    engine.price,
                    option(),
                    market(valuation=valuation),
                )
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                engine.price_with_greeks,
                option(),
                market(),
                [],
            )
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                engine.price_with_greeks,
                option(),
                market(),
                "delta",
                spot_shift=0,
            )

    def test_expiry_settlement_has_no_greeks(self):
        for engine in engines():
            for kind in KINDS:
                contract = option(kind, rebate=3, touch_state="untouched")
                result = engine.price_with_greeks(
                    contract, market(spot=110, valuation=END), all_greeks=True
                )
                self.assertEqual(result.price, 3 if kind.endswith("in") else 10)
                for greek in ("delta", "gamma", *OTHER_GREEKS):
                    self.assertIsNone(getattr(result, greek))

    def test_settings_defaults_and_representation_validation(self):
        mc, tree, quadrature = (
            pricing.MonteCarloBarrierEngine(),
            pricing.CoxRossRubinsteinBarrierEngine(),
            pricing.QuadratureBarrierEngine(),
        )
        self.assertEqual(
            (
                mc.path_count,
                mc.step_count,
                mc.seed,
                mc.backend,
                mc.thread_count,
                mc.sampling,
                mc.antithetic,
            ),
            (100_000, 50, None, "cpu", 0, "pseudo_random", False),
        )
        self.assertEqual(tree.step_count, 256)
        self.assertEqual(
            (quadrature.asset_step_count, quadrature.integration_standard_deviations),
            (512, 8),
        )
        for constructor, name in (
            (pricing.MonteCarloBarrierEngine, "path_count"),
            (pricing.CoxRossRubinsteinBarrierEngine, "step_count"),
            (pricing.QuadratureBarrierEngine, "asset_step_count"),
        ):
            for value in (True, 4.5):
                with self.assertRaises(TypeError):
                    constructor(**{name: value})
            with self.assertRaises(OverflowError):
                constructor(**{name: 2**80})
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER, constructor, **{name: 0}
            )
        self.assert_category(
            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
            pricing.MonteCarloBarrierEngine,
            path_count=1024,
            sampling="sobol",
            backend="cuda",
        )

    def test_implied_volatility_reprices_and_rejects_fixed_cashflows(self):
        context = market()
        contract = option(barrier_level=60)
        for engine in engines():
            quote = engine.price(contract, context)
            volatility = pricing.implied_volatility(
                engine,
                contract,
                context,
                quote,
                lower_bound=0.15,
                upper_bound=0.25,
                price_tolerance=1e-7,
            )
            self.assertAlmostEqual(volatility, 0.2, delta=0.0002)
            fixed = option(rebate=7, rebate_timing="at_expiry", touch_state="touched")
            later = market(valuation=START + timedelta(days=1))
            self.assert_category(
                kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                pricing.implied_volatility,
                engine,
                fixed,
                later,
                engine.price(fixed, later),
            )

    def test_cuda_accuracy_or_explicit_unavailability(self):
        engine = pricing.MonteCarloBarrierEngine(
            path_count=262_144, seed=42, backend="cuda"
        )
        try:
            price = engine.price(option(), market())
        except kiyosi.KiyosiError as error:
            if os.environ.get("KIYOSI_TEST_CUDA") == "enabled":
                raise
            self.assertEqual(error.category, kiyosi.ErrorCategory.BACKEND_UNAVAILABLE)
            return
        self.assertNotEqual(os.environ.get("KIYOSI_TEST_CUDA"), "disabled")
        self.assertAlmostEqual(
            price, pricing.AnalyticBarrierEngine().price(option(), market()), delta=0.2
        )
        for contract in (
            option(),
            option(
                observation_mode="scheduled",
                observation_dates=[START + timedelta(days=90), END],
            ),
        ):
            expected = pricing.QuadratureBarrierEngine().price_with_greeks(
                contract, market(), ("delta", "gamma")
            )
            actual = engine.price_with_greeks(contract, market(), all_greeks=True)
            for name, tolerance in (("price", 0.2), ("delta", 0.06), ("gamma", 0.02)):
                self.assertAlmostEqual(
                    getattr(actual, name), getattr(expected, name), delta=tolerance
                )
