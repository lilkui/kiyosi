"""Analytical and Monte Carlo double barriers against independent references."""

import math
import os
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import timedelta
from itertools import product

from kiyosi import ErrorCategory, KiyosiError, pricing
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)
from test_double_barrier import (
    END,
    FIRST,
    START,
    continuous_reference,
    contract,
    first_fixing_reference,
    terminal_reference,
)


def market(spot=100, *, time=START, rate=0.04, dividend=0.01, volatility=0.2):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=time,
        calendar=all_days_calendar(),
    )


def engines():
    return (
        pricing.AnalyticDoubleBarrierEngine(),
        pricing.MonteCarloDoubleBarrierEngine(
            path_count=32_768, seed=42, sampling="sobol"
        ),
    )


def rebate_reference(rate, count):
    """Independent driftless killed-Brownian Fourier survival and exit integral."""
    width = math.log(125 / 80)
    survival, integrated = [], []
    for index in range(count):
        frequency = 2 * index + 1
        coefficient = 4 * (-1 if index % 2 else 1) / (frequency * math.pi)
        decay = 0.5 * 0.2**2 * (frequency * math.pi / width) ** 2
        survival.append(coefficient * math.exp(-decay))
        integrated.append(coefficient * -math.expm1(-(rate + decay)) / (rate + decay))
    discounted_survival = math.exp(-rate) * math.fsum(survival)
    return {
        "at_expiry": 3 * (math.exp(-rate) - discounted_survival),
        "at_hit": 3 * (1 - discounted_survival - rate * math.fsum(integrated)),
        "untouched": 3 * discounted_survival,
    }


class DoubleBarrierNumericalTests(unittest.TestCase):
    def assert_category(self, category, operation, *args, **kwargs):
        with self.assertRaises(KiyosiError) as caught:
            operation(*args, **kwargs)
        self.assertEqual(caught.exception.category, category)

    def test_analytic_prices_and_greeks_match_quantlib(self):
        engine = pricing.AnalyticDoubleBarrierEngine()
        for direction, kind in product(("call", "put"), ("knock_in", "knock_out")):
            option = contract(option_type=direction, barrier_type=kind)
            expected = continuous_reference(direction, kind, series=20)
            self.assertAlmostEqual(engine.price(option, market()), expected, delta=1e-9)
            result = engine.price_with_greeks(option, market(), ("delta", "gamma"))
            low = continuous_reference(direction, kind, spot=99.99, series=20)
            high = continuous_reference(direction, kind, spot=100.01, series=20)
            self.assertAlmostEqual(result.delta, (high - low) / 0.02, delta=1e-7)
            self.assertAlmostEqual(
                result.gamma, (high - 2 * expected + low) / 0.01**2, delta=1e-6
            )
            self.assertIsNone(result.vega)

    def test_mc_continuous_prices_and_greeks_across_fixed_seeds(self):
        context = market()
        for direction, kind in product(("call", "put"), ("knock_in", "knock_out")):
            option = contract(option_type=direction, barrier_type=kind)
            expected = continuous_reference(direction, kind, series=20)
            low = continuous_reference(direction, kind, spot=99.99, series=20)
            high = continuous_reference(direction, kind, spot=100.01, series=20)
            for seed in (17, 42, 103):
                engine = pricing.MonteCarloDoubleBarrierEngine(
                    path_count=131_072, seed=seed, antithetic=True
                )
                actual = engine.price_with_greeks(option, context, all_greeks=True)
                with self.subTest(direction=direction, kind=kind, seed=seed):
                    self.assertAlmostEqual(actual.price, expected, delta=0.12)
                    self.assertAlmostEqual(
                        actual.delta, (high - low) / 0.02, delta=0.015
                    )
                    self.assertAlmostEqual(
                        actual.gamma,
                        (high - 2 * expected + low) / 0.01**2,
                        delta=0.003,
                    )
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
                        self.assertIsNone(getattr(actual, name))

    def test_rebate_increments_match_independent_exit_series_including_negative_rates(
        self,
    ):
        for rate in (-0.02, 0.04):
            expected = rebate_reference(rate, 4097)
            refined = rebate_reference(rate, 8193)
            for timing in expected:
                self.assertAlmostEqual(expected[timing], refined[timing], delta=1e-11)
            context = market(rate=rate, dividend=rate - 0.02)
            for engine in engines():
                for kind, timing in (
                    ("knock_out", "at_hit"),
                    ("knock_out", "at_expiry"),
                    ("knock_in", "at_expiry"),
                ):
                    terms = {
                        "upper_barrier": 125,
                        "barrier_type": kind,
                        "rebate_timing": timing,
                    }
                    actual = engine.price(contract(**terms, rebate=3), context)
                    base = engine.price(contract(**terms), context)
                    reference = expected["untouched" if kind == "knock_in" else timing]
                    tolerance = (
                        1e-9
                        if isinstance(engine, pricing.AnalyticDoubleBarrierEngine)
                        else 0.004
                    )
                    with self.subTest(
                        rate=rate, engine=engine, kind=kind, timing=timing
                    ):
                        self.assertAlmostEqual(
                            actual - base, reference, delta=tolerance
                        )

    def test_image_and_spectral_series_match_quantlib_across_switch(self):
        # Independent QuantLib 20-term values immediately below/above the series switch.
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
            for engine in engines():
                analytic = isinstance(engine, pricing.AnalyticDoubleBarrierEngine)
                result = engine.price_with_greeks(
                    contract(), market(volatility=volatility), ("delta", "gamma")
                )
                self.assertAlmostEqual(
                    result.price, price, delta=1e-9 if analytic else 0.003
                )
                self.assertAlmostEqual(
                    result.delta, delta, delta=1e-7 if analytic else 0.003
                )
                self.assertAlmostEqual(
                    result.gamma, gamma, delta=1e-6 if analytic else 0.003
                )

    def test_mc_scheduled_prices_match_one_and_two_fixing_integrals(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=262_144, seed=42, sampling="sobol"
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
            self.assertAlmostEqual(
                engine.price(option, market(100.25)), expected, delta=0.025
            )
        for direction in ("call", "put"):
            option = contract(
                option_type=direction,
                rebate=2,
                observation_mode="scheduled",
                observation_dates=[END],
            )
            expected = terminal_reference(100, direction, 80, 120, 1)
            self.assertAlmostEqual(
                engine.price(option, market()), expected, delta=0.002
            )

    def test_analytic_rejects_unresolved_schedules_but_resolves_finished_monitoring(
        self,
    ):
        engine = pricing.AnalyticDoubleBarrierEngine()
        for dates in ([FIRST], [FIRST, END]):
            option = contract(observation_mode="scheduled", observation_dates=dates)
            self.assert_category(
                ErrorCategory.UNSUPPORTED_OPERATION, engine.price, option, market()
            )
        context = market(130, time=FIRST + timedelta(days=1))
        for engine in engines():
            for kind in ("knock_in", "knock_out"):
                option = contract(
                    barrier_type=kind,
                    rebate=3,
                    observation_mode="scheduled",
                    observation_dates=[FIRST],
                    touch_state="untouched",
                )
                vanilla = pricing.AnalyticVanillaEngine().price(
                    EuropeanOption(
                        option_type="call",
                        strike=100,
                        effective_date=START,
                        expiry_date=END,
                    ),
                    context,
                )
                expected = (
                    3 * math.exp(-0.04 * 184 / 365) if kind == "knock_in" else vanilla
                )
                self.assertAlmostEqual(
                    engine.price(option, context), expected, delta=1e-12
                )

    def test_current_boundary_touches_prior_payments_history_and_expiry(self):
        later = market(time=START + timedelta(days=1))
        for engine in engines():
            for spot, timing in product((79, 80, 120, 121), ("at_hit", "at_expiry")):
                option = contract(rebate=3, rebate_timing=timing)
                expected = 3 if timing == "at_hit" else 3 * math.exp(-0.04)
                self.assertAlmostEqual(
                    engine.price(option, market(spot)), expected, delta=1e-12
                )
                past = option.replace(touch_state="touched")
                expected = 0 if timing == "at_hit" else 3 * math.exp(-0.04 * 364 / 365)
                self.assertAlmostEqual(engine.price(past, later), expected, delta=1e-12)
                result = engine.price_with_greeks(option, market(spot), all_greeks=True)
                if spot in (80, 120):
                    self.assertIsNone(result.delta)
                    self.assertIsNone(result.gamma)
            self.assert_category(
                ErrorCategory.INVALID_PARAMETER, engine.price, contract(), later
            )
            self.assert_category(
                ErrorCategory.INVALID_OPTION,
                engine.price,
                contract(touch_state="touched"),
                market(),
            )
            option = contract(
                barrier_type="knock_in", rebate=2, touch_state="untouched"
            )
            result = engine.price_with_greeks(option, market(time=END), all_greeks=True)
            self.assertEqual(result.price, 2)
            self.assertIsNone(result.delta)
            self.assertIsNone(result.gamma)
            activated = option.replace(touch_state="touched")
            vanilla = pricing.AnalyticVanillaEngine().price(
                EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_date=START,
                    expiry_date=END,
                ),
                later,
            )
            self.assertEqual(engine.price(activated, later), vanilla)

    def test_narrow_wide_bands_and_strikes_outside_surviving_payoff(self):
        context = market()
        vanilla = pricing.AnalyticVanillaEngine().price(
            EuropeanOption(
                option_type="call", strike=100, effective_date=START, expiry_date=END
            ),
            context,
        )
        for engine in engines():
            tolerance = (
                1e-9
                if isinstance(engine, pricing.AnalyticDoubleBarrierEngine)
                else 0.025
            )
            self.assertAlmostEqual(
                engine.price(
                    contract(lower_barrier=99.99, upper_barrier=100.01), context
                ),
                0,
                delta=1e-12,
            )
            self.assertAlmostEqual(
                engine.price(
                    contract(lower_barrier=0.01, upper_barrier=10_000), context
                ),
                vanilla,
                delta=tolerance,
            )
            for direction, strike in (("call", 125), ("put", 75)):
                self.assertEqual(
                    engine.price(
                        contract(option_type=direction, strike=strike), context
                    ),
                    0,
                )
            for spot in (80.001, 119.999):
                actual = engine.price(contract(), market(spot))
                expected = continuous_reference(
                    "call", "knock_out", spot=spot, series=20
                )
                self.assertAlmostEqual(actual, expected, delta=tolerance)

    def test_tiny_positive_volatility_future_hits_preserve_rebate_timing(self):
        for engine in engines():
            for up, timing in product((False, True), ("at_hit", "at_expiry")):
                context = market(volatility=1e-12, dividend=0.01 if up else 0.07)
                hit_time = math.log(102 / 100 if up else 100 / 98) / 0.03
                expected = 3 * math.exp(-0.04 * (hit_time if timing == "at_hit" else 1))
                option = contract(
                    lower_barrier=80 if up else 98,
                    upper_barrier=102 if up else 120,
                    rebate=3,
                    rebate_timing=timing,
                )
                self.assertAlmostEqual(
                    engine.price(option, context), expected, delta=1e-7
                )

    def test_seeded_thread_step_antithetic_and_concurrent_reproducibility(self):
        option, context = contract(), market()
        for sampling, paired in product(("pseudo_random", "sobol"), (False, True)):
            kwargs = {
                "path_count": 1024,
                "seed": 17,
                "sampling": sampling,
                "antithetic": paired,
            }
            serial = pricing.MonteCarloDoubleBarrierEngine(**kwargs, thread_count=1)
            parallel = pricing.MonteCarloDoubleBarrierEngine(
                **kwargs, thread_count=4, step_count=10_000
            )
            expected = serial.price_with_greeks(option, context, ("delta", "gamma"))
            actual = parallel.price_with_greeks(option, context, ("delta", "gamma"))
            for name in ("price", "delta", "gamma"):
                self.assertEqual(getattr(actual, name), getattr(expected, name))
            with ThreadPoolExecutor(max_workers=3) as pool:
                values = list(
                    pool.map(
                        lambda _, engine=parallel: engine.price(option, context),
                        range(3),
                    )
                )
            self.assertEqual(values, [expected.price] * 3)

    def test_mc_odd_antithetic_paths_preserve_in_out_payoff_parity(self):
        context = market()

        def terminal_mean(paths, paired):
            engine = pricing.MonteCarloDoubleBarrierEngine(
                path_count=paths, seed=42, antithetic=paired
            )
            return engine.price(
                contract(lower_barrier=0.01, upper_barrier=10_000), context
            )

        for paths in (1, 2, 3, 5, 257):
            engine = pricing.MonteCarloDoubleBarrierEngine(
                path_count=paths, seed=42, antithetic=True
            )
            paired_paths = paths - paths % 2
            expected = (
                terminal_mean(paired_paths, True) * paired_paths if paired_paths else 0
            )
            if paths % 2:
                base = (paths + 1) // 2
                expected += terminal_mean(base, False) * base
                if base > 1:
                    expected -= terminal_mean(base - 1, False) * (base - 1)
            expected /= paths
            actual = engine.price(contract(), context) + engine.price(
                contract(barrier_type="knock_in"), context
            )
            self.assertAlmostEqual(actual, expected, delta=1e-11)

    def test_mc_setting_defaults_representations_and_domain_errors(self):
        default = pricing.MonteCarloDoubleBarrierEngine()
        single = pricing.MonteCarloBarrierEngine()
        for name in (
            "path_count",
            "step_count",
            "seed",
            "backend",
            "thread_count",
            "sampling",
            "antithetic",
        ):
            self.assertEqual(getattr(default, name), getattr(single, name))
        constructor = pricing.MonteCarloDoubleBarrierEngine
        for name in ("path_count", "step_count", "thread_count"):
            for value in (True, 4.5):
                with self.assertRaises(TypeError):
                    constructor(**{name: value})
            with self.assertRaises(OverflowError):
                constructor(**{name: 2**80})
        for kwargs in (
            {"path_count": 0},
            {"step_count": 1},
            {"thread_count": -1},
            {"path_count": 1000, "sampling": "sobol"},
        ):
            self.assert_category(ErrorCategory.INVALID_PARAMETER, constructor, **kwargs)
        self.assert_category(
            ErrorCategory.UNSUPPORTED_OPERATION,
            constructor,
            path_count=1024,
            sampling="sobol",
            backend="cuda",
        )
        with self.assertRaises(TypeError):
            constructor(antithetic=1)

    def test_unrepresentable_prices_report_structured_core_errors(self):
        context = market(rate=-1000, dividend=-1000)
        for engine in engines():
            self.assert_category(
                ErrorCategory.INVALID_RESULT, engine.price, contract(), context
            )

    def test_implied_volatility_recovers_and_rejects_fixed_remaining_cashflows(self):
        context = market()
        option = contract(lower_barrier=50, upper_barrier=200)
        for engine in engines():
            quote = engine.price(option, context)
            actual = pricing.implied_volatility(
                engine, option, context, quote, lower_bound=0.15, upper_bound=0.25
            )
            self.assertAlmostEqual(actual, 0.2, delta=1e-5)
            fixed = contract(rebate=3, touch_state="touched")
            later = market(time=START + timedelta(days=1))
            self.assert_category(
                ErrorCategory.UNSUPPORTED_OPERATION,
                pricing.implied_volatility,
                engine,
                fixed,
                later,
                engine.price(fixed, later),
            )

    def test_cuda_prices_and_greeks_or_explicit_unavailability(self):
        engine = pricing.MonteCarloDoubleBarrierEngine(
            path_count=262_144, seed=42, backend="cuda", antithetic=True
        )
        for dates in (None, [FIRST, END], [FIRST]):
            option = contract(
                rebate=2,
                rebate_timing="at_hit",
                **(
                    {}
                    if dates is None
                    else {"observation_mode": "scheduled", "observation_dates": dates}
                ),
            )
            try:
                actual = engine.price_with_greeks(option, market(), ("delta", "gamma"))
            except KiyosiError as error:
                if os.environ.get("KIYOSI_TEST_CUDA") == "enabled":
                    raise
                self.assertEqual(error.category, ErrorCategory.BACKEND_UNAVAILABLE)
                return
            self.assertNotEqual(os.environ.get("KIYOSI_TEST_CUDA"), "disabled")
            reference = (
                pricing.AnalyticDoubleBarrierEngine()
                if dates is None
                else pricing.FiniteDifferenceDoubleBarrierEngine(
                    asset_step_count=1200, time_step_count=1200
                )
            )
            expected = reference.price_with_greeks(option, market(), ("delta", "gamma"))
            for name, tolerance in (("price", 0.12), ("delta", 0.05), ("gamma", 0.02)):
                self.assertAlmostEqual(
                    getattr(actual, name), getattr(expected, name), delta=tolerance
                )


if __name__ == "__main__":
    unittest.main()
