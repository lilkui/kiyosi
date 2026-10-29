"""Single-barrier CRR public contracts, migrated events and independent references."""

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
from kiyosi.instruments import BarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
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
    return (pricing.CoxRossRubinsteinBarrierEngine(step_count=720),)


class BarrierBinomialTests(unittest.TestCase):
    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_crr_requires_exact_future_observation_alignment(self):
        contract = option(
            observation_mode="scheduled",
            observation_dates=[START + timedelta(days=91), END],
        )
        self.assert_rejection(
            UnsupportedOperationError,
            pricing.CoxRossRubinsteinBarrierEngine(step_count=256).price,
            contract,
            market(),
        )
        aligned = pricing.CoxRossRubinsteinBarrierEngine(step_count=360)
        self.assertGreater(aligned.price(contract, market()), 0)
        intraday = datetime(2025, 1, 1, 12, tzinfo=UTC)
        self.assert_rejection(
            UnsupportedOperationError,
            aligned.price,
            contract,
            market(valuation=intraday),
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

    def test_expiry_only_observation_matches_one_step_payment(self):
        contract = option(
            "up_and_out",
            expiry_date=START + timedelta(days=2),
            barrier_level=101,
            rebate=2,
            observation_mode="scheduled",
            observation_dates=[START + timedelta(days=2)],
        )
        context = market(rate=0, dividend=0)
        up = math.exp(0.2 * math.sqrt(2 / 365))
        self.assertAlmostEqual(
            pricing.CoxRossRubinsteinBarrierEngine(step_count=1).price(
                contract, context
            ),
            2 / (1 + up),
            delta=1e-12,
        )

    def test_defaults_strict_settings_and_owned_values(self):
        factory = pricing.CoxRossRubinsteinBarrierEngine
        self.assertEqual(factory().step_count, 256)
        self.assertEqual(
            repr(factory()), "CoxRossRubinsteinBarrierEngine(step_count=256)"
        )

        class Index:
            def __index__(self):
                return 17

        numbers.Integral.register(Index)
        self.assertEqual(factory(step_count=Index()).step_count, 17)
        for steps in (1, 1_000_000):
            engine = factory(step_count=steps)
            self.assertEqual(engine.step_count, steps)
            with self.assertRaises(AttributeError):
                engine.step_count = 2
        for value in (0, -1, 1_000_001):
            self.assert_rejection(InvalidInputError, factory, step_count=value)
        for value in (True, False, 1.0, 1.5, "1", None):
            with self.assertRaises(TypeError):
                factory(step_count=value)
        for value in (2**31, -(2**31) - 1):
            with self.assertRaises(OverflowError):
                factory(step_count=value)
        with self.assertRaises(TypeError):
            factory(unknown=1)
        with self.assertRaises(TypeError):
            factory(17)

    def test_only_requested_node_greeks_are_returned_and_shifts_are_unused(self):
        for steps in (1, 2, 256):
            engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=steps)
            contract, context = option(), market()
            result = engine.price_with_greeks(contract, context, all_greeks=True)
            self.assertIsNotNone(result.delta)
            self.assertEqual(result.gamma is not None, steps >= 2)
            for name in OTHER_GREEKS:
                self.assertIsNone(getattr(result, name))
            for name in ("delta", "gamma", *OTHER_GREEKS):
                selected = engine.price_with_greeks(
                    contract,
                    context,
                    name,
                    spot_shift=1e9,
                    volatility_shift=1e9,
                    rate_shift=1e9,
                    time_shift_days=1000,
                )
                self.assertEqual(getattr(selected, name), getattr(result, name))
                self.assertEqual(selected.price, result.price)

    def test_current_hit_paid_history_and_resolved_vanilla_precede_grid(self):
        engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=3)
        for kind in KINDS:
            level = 120 if kind.startswith("up") else 80
            for timing in (
                ("at_hit", "at_expiry") if kind.endswith("out") else ("at_expiry",)
            ):
                contract = option(
                    kind,
                    rebate=7,
                    rebate_timing=timing,
                    observation_mode="scheduled",
                    observation_dates=[START, START + timedelta(days=1), END],
                )
                context = market(spot=level)
                expected = pricing.AnalyticBarrierEngine().price(contract, context)
                self.assertEqual(engine.price(contract, context), expected)
                result = engine.price_with_greeks(contract, context, all_greeks=True)
                for name in ("delta", "gamma", *OTHER_GREEKS):
                    self.assertIsNone(getattr(result, name))
                touched = contract.replace(touch_state="touched")
                context = market(valuation=START + timedelta(days=2))
                expected = pricing.AnalyticBarrierEngine().price(touched, context)
                result = engine.price_with_greeks(touched, context, all_greeks=True)
                self.assertEqual(result.price, expected)
                self.assertIsNotNone(result.delta)
                self.assertIsNotNone(result.gamma)
                if kind.endswith("out"):
                    self.assertEqual(result.delta, 0)
                self.assertIsNone(contract.touch_state)
                self.assert_rejection(
                    InvalidInputError, engine.price, touched, market()
                )

    def test_microsecond_alignment_and_inclusive_noncentral_node_hits(self):
        end = START + timedelta(days=4)
        contract = option(
            "up_and_out",
            expiry_date=end,
            observation_mode="scheduled",
            observation_dates=[START + timedelta(days=2)],
        )
        engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=7)
        time = datetime(2025, 1, 1, 12, tzinfo=UTC)
        context = market(valuation=time)
        self.assertGreater(engine.price(contract, context), 0)
        self.assert_rejection(
            UnsupportedOperationError,
            engine.price,
            contract,
            market(valuation=time + timedelta(microseconds=1)),
        )
        for sigma in (0.1, 0.2, 0.3, 0.4):
            for up in (False, True):
                jump = sigma * math.sqrt(1 / 365)
                contract = option(
                    "up_and_out" if up else "down_and_out",
                    "call" if up else "put",
                    expiry_date=START + timedelta(days=2),
                    barrier_level=100 * math.exp(jump if up else -jump),
                    observation_mode="scheduled",
                    observation_dates=[START + timedelta(days=1)],
                )
                # The sole ITM path is knocked out at its first move, including equality.
                self.assertAlmostEqual(
                    pricing.CoxRossRubinsteinBarrierEngine(step_count=2).price(
                        contract, market(volatility=sigma)
                    ),
                    0,
                    delta=1e-12,
                )

    def test_flat_curve_applicability_calendar_and_validation_order(self):
        engine = pricing.CoxRossRubinsteinBarrierEngine()
        curve = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.03 * 360 / 365)]
        )
        for time in (START, END):
            for changes in ({"rate": curve}, {"dividend": curve}):
                context = market(valuation=time, **changes)
                self.assert_rejection(
                    UnsupportedOperationError,
                    engine.price,
                    option(),
                    context,
                )
                self.assert_rejection(
                    UnsupportedOperationError,
                    engine.price_with_greeks,
                    option(),
                    context,
                    [],
                )
                self.assert_rejection(
                    UnsupportedOperationError,
                    pricing.implied_volatility,
                    engine,
                    option(),
                    context,
                    math.nan,
                )
        invalid = option(
            observation_mode="scheduled", observation_dates=[date(2025, 1, 4)]
        )
        weekdays = PricingContext(
            model_parameters=market().model_parameters,
            spot_price=100,
            valuation_time=START,
        )
        self.assert_rejection(InvalidInputError, engine.price, invalid, weekdays)
        for sigma, rate in ((1e-200, 0.03), (50, 0.03), (0.2, -1000)):
            self.assert_rejection(
                ComputationFailureError,
                engine.price,
                option(),
                market(volatility=sigma, rate=rate),
            )
        for kwargs in (
            {"spot_shift": True},
            {"time_shift_days": 1.5},
            {"time_shift_days": True},
            {"unknown": 1},
        ):
            with self.assertRaises(TypeError):
                engine.price_with_greeks(option(), market(), "delta", **kwargs)
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

    def test_solver_tree_domain_and_failures_preserve_inputs(self):
        engine = pricing.CoxRossRubinsteinBarrierEngine()
        contract, context = option(barrier_level=60), market()
        quote = engine.price(contract, context)
        solved = pricing.implied_volatility(
            engine, contract, context, quote, lower_bound=1e-8, upper_bound=0.5
        )
        self.assertAlmostEqual(
            engine.price(contract, market(volatility=solved)), quote, delta=2e-7
        )
        for price, controls, expected_error in (
            (
                quote,
                {"upper_bound": 0.0002},
                RootNotBracketedError,
            ),
            (-1, {"upper_bound": 0.5}, RootNotBracketedError),
            (math.nan, {}, InvalidInputError),
            (quote, {"max_iterations": 0}, InvalidInputError),
        ):
            self.assert_rejection(
                expected_error,
                pricing.implied_volatility,
                engine,
                contract,
                context,
                price,
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
        self.assertEqual(engine.step_count, 256)

    def test_shared_and_distinct_instances_allow_concurrent_pricing_and_solving(self):
        engine = pricing.CoxRossRubinsteinBarrierEngine()
        contract, context = option(barrier_level=60), market()
        quote = engine.price(contract, context)

        def evaluate(index):
            selected = engine if index % 2 else pricing.CoxRossRubinsteinBarrierEngine()
            result = selected.price_with_greeks(contract, context, ("delta", "gamma"))
            sigma = pricing.implied_volatility(
                selected,
                contract,
                context,
                result.price,
                lower_bound=0.15,
                upper_bound=0.25,
            )
            return result.price, sigma

        with ThreadPoolExecutor(max_workers=4) as pool:
            for price, sigma in pool.map(evaluate, range(8)):
                self.assertEqual(price, quote)
                self.assertAlmostEqual(sigma, 0.2, delta=0.0002)

    def test_continuous_prices_and_greeks_against_live_quantlib(self):

        oracle = importlib.import_module("quantlib_reference.barrier")
        oracle.g.require_version()
        engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=2048)
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
                            ("price", 0.25),
                            ("delta", 0.07),
                            ("gamma", 0.01),
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
