"""Single-barrier quadrature through the public Python API."""

import importlib
import math
import numbers
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import PricingContext, RateTermStructure
from test_barrier_binomial import END, KINDS, OTHER_GREEKS, START, market, option


def engines():
    return (pricing.QuadratureBarrierEngine(),)


class BarrierQuadratureTests(unittest.TestCase):
    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_early_observation_rebate_discount(self):
        engine = pricing.QuadratureBarrierEngine()
        observation = date(2025, 4, 1)
        for rate in (-0.02, 0.04):
            for timing in ("at_hit", "at_expiry"):
                contract = option(
                    "up_and_out",
                    strike=1e9,
                    rebate=7,
                    rebate_timing=timing,
                    observation_mode="scheduled",
                    observation_dates=[observation],
                )
                time = 90 / 365
                horizon = 360 / 365
                z = (math.log(1.2) - (rate - 0.03) * time) / (0.2 * math.sqrt(time))
                expected = (
                    7
                    * math.exp(-rate * (time if timing == "at_hit" else horizon))
                    * 0.5
                    * math.erfc(z / math.sqrt(2))
                )
                self.assertAlmostEqual(
                    engine.price(contract, market(rate=rate)), expected, delta=2e-11
                )

    def test_history_is_required_and_failure_does_not_change_engine(self):
        for engine in engines():
            contract = option()
            expected = engine.price(contract, market())
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                contract,
                market(valuation=START + timedelta(days=1)),
            )
            self.assertEqual(engine.price(contract, market()), expected)
            self.assert_rejection(
                InvalidInputError,
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
                    else pricing.AnalyticEuropeanEngine().price(vanilla, at_observation)
                )
                later = (
                    pricing.AnalyticEuropeanEngine().price(vanilla, after_observation)
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
        value = pricing.AnalyticEuropeanEngine().price(vanilla, context)
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

    def test_valuation_and_greek_validation_preserve_core_errors(self):
        for engine in engines():
            for valuation in (START - timedelta(days=1), END + timedelta(days=1)):
                self.assert_rejection(
                    InvalidInputError,
                    engine.price,
                    option(),
                    market(valuation=valuation),
                )
            self.assert_rejection(
                InvalidInputError,
                engine.price_with_greeks,
                option(),
                market(),
                [],
            )
            self.assert_rejection(
                InvalidInputError,
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
            self.assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                fixed,
                later,
                engine.price(fixed, later),
            )

    def test_continuous_prices_and_greeks_against_live_quantlib(self):

        oracle = importlib.import_module("quantlib_reference.barrier")
        oracle.g.require_version()
        engine = pricing.QuadratureBarrierEngine(
            asset_step_count=2048, integration_standard_deviations=10
        )
        for kind in KINDS:
            for direction in ("call", "put"):
                for timing in (
                    ("at_hit", "at_expiry") if kind.endswith("out") else ("at_expiry",)
                ):
                    for rate, dividend in ((0.03, 0.01), (-0.02, 0.01)):
                        contract = option(
                            kind, direction, rebate=7, rebate_timing=timing
                        )
                        inputs = {
                            "BarrierType": kind,
                            "settlement": timing,
                            "monitoring": "continuous",
                            "valuation": START.isoformat(),
                            "effective_date": START.isoformat(),
                            "expiry_date": END.isoformat(),
                            "option": direction,
                            "strike": 100,
                            "spot": 100,
                            "barrier": contract.barrier_level,
                            "rebate": 7,
                            "rate": rate,
                            "dividend": dividend,
                            "volatility": 0.2,
                        }

                        def value(spot, inputs=inputs):
                            return oracle.price(
                                tuple(sorted((inputs | {"spot": spot}).items()))
                            )

                        with ql.SavedSettings():
                            center, high, low = value(100), value(100.01), value(99.99)
                        expected = {
                            "price": center,
                            "delta": (high - low) / 0.02,
                            "gamma": (high - 2 * center + low) / 0.01**2,
                        }
                        result = engine.price_with_greeks(
                            contract,
                            market(rate=rate, dividend=dividend),
                            ("delta", "gamma"),
                        )
                        for name, budget in (
                            ("price", 3e-8),
                            ("delta", 2e-7),
                            ("gamma", 2e-7),
                        ):
                            with self.subTest(
                                kind=kind,
                                direction=direction,
                                timing=timing,
                                rate=rate,
                                measure=name,
                            ):
                                self.assertAlmostEqual(
                                    getattr(result, name), expected[name], delta=budget
                                )

    def test_early_ending_schedules_match_independent_bivariate_normal_payoffs(self):
        engine = pricing.QuadratureBarrierEngine(
            asset_step_count=1024, integration_standard_deviations=10
        )
        observation = START + timedelta(days=91)
        time, horizon = 91 / 365, 360 / 365
        sigma, rate, dividend = 0.2, 0.03, 0.01
        normal = ql.CumulativeNormalDistribution()
        for kind in KINDS:
            hit_sign = 1 if kind.startswith("up") else -1
            level = 120 if hit_sign == 1 else 80
            a = (math.log(level / 100) - (rate - dividend - sigma**2 / 2) * time) / (
                sigma * math.sqrt(time)
            )
            for direction in ("call", "put"):
                payoff_sign = 1 if direction == "call" else -1
                b = -(rate - dividend - sigma**2 / 2) * math.sqrt(horizon) / sigma
                joint = ql.BivariateCumulativeNormalDistributionWe04DP(
                    hit_sign * payoff_sign * math.sqrt(time / horizon)
                )
                hit_payoff = payoff_sign * (
                    100
                    * math.exp(-dividend * horizon)
                    * joint(
                        -hit_sign * (a - sigma * math.sqrt(time)),
                        -payoff_sign * (b - sigma * math.sqrt(horizon)),
                    )
                    - 100
                    * math.exp(-rate * horizon)
                    * joint(-hit_sign * a, -payoff_sign * b)
                )
                vanilla = payoff_sign * (
                    100
                    * math.exp(-dividend * horizon)
                    * normal(-payoff_sign * (b - sigma * math.sqrt(horizon)))
                    - 100 * math.exp(-rate * horizon) * normal(-payoff_sign * b)
                )
                hit_probability = normal(-hit_sign * a)
                for timing in (
                    ("at_expiry",) if kind.endswith("in") else ("at_hit", "at_expiry")
                ):
                    if kind.endswith("in"):
                        expected = hit_payoff + 3 * math.exp(-rate * horizon) * (
                            1 - hit_probability
                        )
                    else:
                        expected = (
                            vanilla
                            - hit_payoff
                            + 3
                            * math.exp(
                                -rate * (time if timing == "at_hit" else horizon)
                            )
                            * hit_probability
                        )
                    contract = option(
                        kind,
                        direction,
                        rebate=3,
                        rebate_timing=timing,
                        observation_mode="scheduled",
                        observation_dates=[observation],
                    )
                    with self.subTest(kind=kind, direction=direction, timing=timing):
                        self.assertAlmostEqual(
                            engine.price(contract, market()), expected, delta=2e-6
                        )

    def test_two_observations_match_independent_joint_survival_probability(self):
        engine = pricing.QuadratureBarrierEngine(
            asset_step_count=2048, integration_standard_deviations=10
        )
        first, last = 91 / 365, 273 / 365
        for kind in ("up_and_out", "down_and_out"):
            sign = 1 if kind.startswith("up") else -1
            level = 120 if sign == 1 else 80
            # An unreachable call strike isolates the rebate; the joint normal gives no-hit probability.
            threshold = lambda time, level=level: (
                (math.log(level / 100) - (0.03 - 0.01 - 0.02) * time)
                / (0.2 * math.sqrt(time))
            )
            joint = ql.BivariateCumulativeNormalDistributionWe04DP(
                math.sqrt(first / last)
            )
            survival = joint(sign * threshold(first), sign * threshold(last))
            contract = option(
                kind,
                strike=1e9,
                rebate=7,
                observation_mode="scheduled",
                observation_dates=[
                    START + timedelta(days=91),
                    START + timedelta(days=273),
                ],
            )
            expected = 7 * math.exp(-0.03 * 360 / 365) * (1 - survival)
            self.assertAlmostEqual(
                engine.price(contract, market()), expected, delta=2e-6
            )

    def test_settings_and_representation_errors_keep_native_exceptions(self):
        constructor = pricing.QuadratureBarrierEngine
        engine = constructor()
        self.assertEqual(
            (engine.asset_step_count, engine.integration_standard_deviations), (512, 8)
        )
        self.assertIn("asset_step_count=512", repr(engine))

        class Index:
            def __index__(self):
                return 513

        with self.assertRaises(TypeError):
            constructor(asset_step_count=Index())
        numbers.Integral.register(Index)
        self.assertEqual(constructor(asset_step_count=Index()).asset_step_count, 513)
        with self.assertRaises(AttributeError):
            engine.asset_step_count = 4
        for value in (True, 4.5, "4", None):
            with self.assertRaises(TypeError):
                constructor(asset_step_count=value)
        with self.assertRaises(OverflowError):
            constructor(asset_step_count=2**31)
        for value in (True, "8", None, 1j):
            with self.assertRaises(TypeError):
                constructor(integration_standard_deviations=value)
        for changes in (
            {"asset_step_count": 3},
            {"integration_standard_deviations": 0},
            {"integration_standard_deviations": math.inf},
            {"integration_standard_deviations": math.nan},
        ):
            self.assert_rejection(InvalidInputError, constructor, **changes)
        with self.assertRaises(TypeError):
            constructor(512)
        with self.assertRaises(TypeError):
            constructor(unknown=8)
        for changes in (
            {"spot_shift": True},
            {"time_shift_days": 1.5},
            {"time_shift_days": True},
            {"unknown": 1},
        ):
            with self.assertRaises(TypeError):
                engine.price_with_greeks(option(), market(), "delta", **changes)
        with self.assertRaises(OverflowError):
            engine.price_with_greeks(option(), market(), "delta", time_shift_days=2**31)
        mismatch = EuropeanOption(
            option_type="call", strike=100, effective_date=START, expiry_date=END
        )
        for operation, args in (
            (engine.price, (mismatch, market())),
            (engine.price_with_greeks, (mismatch, market(), "delta")),
            (pricing.implied_volatility, (engine, mismatch, market(), 10)),
        ):
            with self.assertRaises(TypeError):
                operation(*args)

    def test_applicability_precedes_request_solver_and_expiry_validation(self):
        engine = pricing.QuadratureBarrierEngine()
        curve = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.03 * 360 / 365)]
        )
        for context in (market(rate=curve), market(rate=curve, valuation=END)):
            for operation, args in (
                (engine.price, (option(), context)),
                (engine.price_with_greeks, (option(), context, [])),
                (pricing.implied_volatility, (engine, option(), context, math.nan)),
            ):
                self.assert_rejection(UnsupportedOperationError, operation, *args)
        bad = option(
            observation_mode="scheduled",
            observation_dates=[date(2025, 1, 4)],
            touch_state="touched",
        )
        weekdays = PricingContext(
            model_parameters=market().model_parameters,
            spot_price=120,
            valuation_time=START + timedelta(days=30),
        )
        self.assert_rejection(InvalidInputError, engine.price, bad, weekdays)
        for sigma in (1e200, 1e308):
            self.assert_rejection(
                ComputationFailureError,
                engine.price,
                option(),
                market(volatility=sigma),
            )
        huge_grid = pricing.QuadratureBarrierEngine(
            integration_standard_deviations=1e308
        )
        scheduled = option(
            observation_mode="scheduled", observation_dates=[START + timedelta(days=91)]
        )
        self.assert_rejection(
            ComputationFailureError,
            huge_grid.price,
            scheduled,
            market(volatility=100),
        )

    def test_scheduled_greeks_solver_and_concurrency(self):
        engine = pricing.QuadratureBarrierEngine(asset_step_count=256)
        contract = option(
            observation_mode="scheduled",
            observation_dates=[START + timedelta(days=91), START + timedelta(days=273)],
        )
        context = market()
        quote = engine.price(contract, context)
        result = engine.price_with_greeks(contract, context, all_greeks=True)
        self.assertEqual(result.price, quote)
        for name in ("delta", "gamma", *OTHER_GREEKS):
            self.assertTrue(math.isfinite(getattr(result, name)), name)

        def evaluate(index):
            selected = (
                engine
                if index % 2
                else pricing.QuadratureBarrierEngine(asset_step_count=256)
            )
            sigma = pricing.implied_volatility(
                selected, contract, context, quote, lower_bound=0.15, upper_bound=0.25
            )
            return selected.price(contract, context), sigma

        with ThreadPoolExecutor(max_workers=4) as pool:
            for price, sigma in pool.map(evaluate, range(4)):
                self.assertEqual(price, quote)
                self.assertAlmostEqual(sigma, 0.2, delta=2e-7)
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
                lower_bound=0.15,
                upper_bound=0.25,
                **controls,
            )
        with self.assertRaises(TypeError):
            pricing.implied_volatility(engine, contract, context, True)
        with self.assertRaises(TypeError):
            pricing.implied_volatility(
                engine, contract, context, quote, max_iterations=1.5
            )
        self.assertEqual(engine.price(contract, context), quote)
        self.assertEqual(context.model_parameters.volatility, 0.2)
