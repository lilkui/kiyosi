"""Terminal quadrature contracts through the public Python API."""

import importlib
import math
import sys
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime
from pathlib import Path

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import BlackScholesMertonParameters, RateTermStructure
from kiyosi.pricing import (
    AnalyticDigitalEngine,
    AnalyticEuropeanEngine,
    QuadratureDigitalEngine,
    QuadratureVanillaEngine,
    implied_volatility,
)
from test_binomial import END, GREEKS, START, market

KINDS = (EuropeanOption, CashOrNothingOption, AssetOrNothingOption)


def contract(kind, direction="call", strike=100, end=END, payout=10):
    terms = {
        "option_type": direction,
        "strike": strike,
        "effective_date": START,
        "expiry_date": end,
    }
    if kind is CashOrNothingOption:
        terms["payout"] = payout
    return kind(**terms)


def engine_for(kind):
    return (
        QuadratureVanillaEngine()
        if kind is EuropeanOption
        else QuadratureDigitalEngine()
    )


class QuadratureTests(unittest.TestCase):
    def assert_rejection(self, expected, operation, *args, **kwargs):
        with self.assertRaises(expected) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def test_strict_expiry_and_lifetime_boundaries(self):
        for kind in KINDS:
            engine = engine_for(kind)
            for direction, sign in (("call", 1), ("put", -1)):
                option = contract(kind, direction)
                for spot in (math.nextafter(100, 0), 100, math.nextafter(100, 101)):
                    context = market(spot=spot, valuation=END)
                    pays = sign * (spot - 100) > 0
                    expected = (
                        max(sign * (spot - 100), 0)
                        if kind is EuropeanOption
                        else (10 if kind is CashOrNothingOption else spot)
                        if pays
                        else 0
                    )
                    result = engine.price_with_greeks(option, context, all_greeks=True)
                    self.assertEqual(engine.price(option, context), expected)
                    self.assertEqual(result.price, expected)
                    for name in GREEKS:
                        self.assertIsNone(getattr(result, name))
                    self.assert_rejection(
                        UnsupportedOperationError,
                        implied_volatility,
                        engine,
                        option,
                        context,
                        expected,
                    )
                for valuation in (
                    datetime(2024, 12, 31, 23, 59, 59, 999999, tzinfo=UTC),
                    datetime(2026, 1, 1, 0, 0, 0, 1, tzinfo=UTC),
                ):
                    context = market(valuation=valuation)
                    self.assert_rejection(
                        InvalidInputError, engine.price, option, context
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
                        implied_volatility,
                        engine,
                        option,
                        context,
                        1,
                    )

    def test_numerical_greeks_converge_and_results_own_their_values(self):
        for kind in KINDS:
            engine, option = engine_for(kind), contract(kind)
            context = market(valuation=date(2025, 6, 1))
            analytic = (
                AnalyticEuropeanEngine()
                if kind is EuropeanOption
                else AnalyticDigitalEngine()
            )
            expected = analytic.price_with_greeks(option, context, all_greeks=True)
            result = engine.price_with_greeks(option, context, all_greeks=True)
            for name in GREEKS:
                budget = (
                    3e-5
                    if name == "theta"
                    else 1e-6
                    if name in ("delta", "vega", "rho")
                    else 1e-7
                )
                self.assertAlmostEqual(
                    getattr(result, name), getattr(expected, name), delta=budget
                )
                selected = engine.price_with_greeks(option, context, name)
                self.assertEqual(getattr(selected, name), getattr(result, name))
                self.assertIsNone(
                    getattr(selected, "gamma" if name == "delta" else "delta")
                )
            errors = [
                abs(
                    engine.price_with_greeks(
                        option, context, "delta", spot_shift=h
                    ).delta
                    - expected.delta
                )
                for h in (0.2, 0.1, 0.05)
            ]
            self.assertGreater(errors[0], errors[1])
            self.assertGreater(errors[1], errors[2])
            collapsed = engine.price_with_greeks(
                option, context, "delta", spot_shift=math.ulp(0.0)
            )
            self.assertIsNone(collapsed.delta)
            self.assert_rejection(UnsupportedOperationError, collapsed.require, "delta")
            saved_delta = result.delta
            del engine, option, context
            self.assertEqual(result.delta, saved_delta)
            self.assertGreater(result.price, 0)
            with self.assertRaises(AttributeError):
                result.price = 0

    def test_applicability_representation_validation_and_failure_state(self):
        curve = RateTermStructure(reference_time=START, times=[END], factors=[0.95])
        for kind in KINDS:
            engine, option, context = engine_for(kind), contract(kind), market()
            dated = context.replace(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=RateTermStructure.flat(0.05, reference_time=START),
                    dividend_yield=RateTermStructure.flat(0.02, reference_time=START),
                    volatility=0.2,
                )
            )
            self.assertEqual(engine.price(option, dated), engine.price(option, context))
            for field in ("risk_free_rate", "dividend_yield"):
                parameters = context.model_parameters.replace(**{field: curve})
                curved = context.replace(
                    model_parameters=parameters, valuation_time=END
                )
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
                    implied_volatility,
                    engine,
                    option,
                    curved,
                    1,
                )
                with self.assertRaises(TypeError):
                    engine.price_with_greeks(option, curved, "delta", spot_shift=True)
            quote = engine.price(option, context)
            for shifts in (
                {"spot_shift": 0},
                {"volatility_shift": math.nan},
                {"rate_shift": math.inf},
                {"time_shift_days": 0},
            ):
                self.assert_rejection(
                    InvalidInputError,
                    engine.price_with_greeks,
                    option,
                    context,
                    "delta",
                    **shifts,
                )
            for shifts in (
                {"spot_shift": True},
                {"rate_shift": "1"},
                {"volatility_shift": None},
                {"time_shift_days": 1.5},
                {"time_shift_days": True},
                {"unknown": 1},
            ):
                with self.assertRaises(TypeError):
                    engine.price_with_greeks(option, context, "delta", **shifts)
            with self.assertRaises(OverflowError):
                engine.price_with_greeks(
                    option, context, "delta", time_shift_days=2**31
                )
            for bad in (
                contract(AmericanOption),
                contract(
                    CashOrNothingOption if kind is EuropeanOption else EuropeanOption
                ),
            ):
                with self.assertRaises(UnsupportedOperationError):
                    engine.price(bad, context)
                with self.assertRaises(UnsupportedOperationError):
                    implied_volatility(engine, bad, context, 1)
            with self.assertRaises(TypeError):
                type(engine)(asset_step_count=512)
            self.assert_rejection(
                ComputationFailureError,
                engine.price,
                option,
                market(rate=-1000, dividend=-1000),
            )
            self.assert_rejection(
                ComputationFailureError,
                engine.price,
                contract(kind, end=date(2035, 1, 1)),
                market(sigma=sys.float_info.max),
            )
            self.assertEqual(engine.price(option, context), quote)
            self.assertEqual(context.model_parameters.volatility, 0.2)

    def test_implied_volatility_recovery_and_solver_failures(self):
        for kind in KINDS:
            for direction in ("call", "put"):
                engine, option, context = (
                    engine_for(kind),
                    contract(kind, direction),
                    market(),
                )
                quote = engine.price(option, context)
                self.assertAlmostEqual(
                    implied_volatility(
                        engine,
                        option,
                        context,
                        quote,
                        lower_bound=0.1,
                        upper_bound=0.22,
                    ),
                    0.2,
                    delta=1e-7,
                )
                self.assert_rejection(
                    RootNotBracketedError,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    -1,
                )
                for controls in (
                    {"max_iterations": 0},
                    {"lower_bound": 0},
                    {"price_tolerance": math.nan},
                ):
                    self.assert_rejection(
                        InvalidInputError,
                        implied_volatility,
                        engine,
                        option,
                        context,
                        quote,
                        **controls,
                    )
                for controls in (
                    {"max_iterations": True},
                    {"max_iterations": 1.5},
                    {"lower_bound": True},
                ):
                    with self.assertRaises(TypeError):
                        implied_volatility(engine, option, context, quote, **controls)
                with self.assertRaises(OverflowError):
                    implied_volatility(
                        engine, option, context, quote, max_iterations=2**31
                    )
                self.assert_rejection(
                    NonConvergenceError,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    quote,
                    lower_bound=0.11,
                    upper_bound=0.23,
                    max_iterations=1,
                    price_tolerance=1e-15,
                    parameter_tolerance=1e-15,
                )
                self.assertEqual(engine.price(option, context), quote)
                self.assertEqual(context.model_parameters.volatility, 0.2)

    def test_existing_quantlib_vanilla_profiles(self):
        root = Path(__file__).resolve().parent

        scenarios = importlib.import_module("quantlib_reference.scenarios")
        oracle = importlib.import_module("quantlib_reference.oracle")
        oracle.require_version()
        profile = next(
            p
            for p in scenarios.numerical_profiles(
                root / "quantlib_reference/numerical_engines.json"
            )
            if p["engine"] == "QuadratureVanillaEngine"
        )
        for scenario in scenarios.load_scenarios(
            root / "quantlib_reference/scenarios.json"
        ):
            inputs = scenario["inputs"]
            if oracle.exclusions(inputs):
                continue
            option = EuropeanOption(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_date=date.fromisoformat(inputs["effective_date"]),
                expiry_date=date.fromisoformat(inputs["expiry_date"]),
            )
            context = market(
                inputs["spot"],
                inputs["rate"],
                inputs["dividend"],
                inputs["volatility"],
                date.fromisoformat(inputs["valuation"]),
            )
            with ql.SavedSettings():
                reference, uncertainty = oracle.reference(inputs)
            result = QuadratureVanillaEngine().price_with_greeks(
                option,
                context,
                sorted(reference.keys() - {"price"}),
                **profile["shifts"],
            )
            for name, expected in reference.items():
                with self.subTest(case=scenario["case_id"], measure=name):
                    self.assertAlmostEqual(
                        getattr(result, name),
                        expected,
                        delta=profile["numerical_tolerances"][name]
                        + uncertainty[f"uncertainty_{name}"],
                    )

    def test_shared_and_distinct_engines_support_concurrent_pricing(self):
        for kind in KINDS:
            engine, option, context = engine_for(kind), contract(kind), market()
            expected = engine.price_with_greeks(option, context, "delta")

            def compute(
                index, engine=engine, kind=kind, option=option, context=context
            ):
                selected = engine if index % 2 else engine_for(kind)
                result = selected.price_with_greeks(option, context, "delta")
                return result.price, result.delta

            with ThreadPoolExecutor(max_workers=4) as pool:
                self.assertEqual(
                    list(pool.map(compute, range(12))),
                    [(expected.price, expected.delta)] * 12,
                )

    def test_existing_quantlib_digital_profiles(self):

        digital = importlib.import_module("quantlib_reference.digital")
        oracle = importlib.import_module("quantlib_reference.oracle")
        oracle.require_version()
        config = digital.configuration()
        profile = next(
            p for p in config["profiles"] if p["engine"] == "QuadratureDigitalEngine"
        )
        for identifier, inputs in digital.scenarios(config):
            kind = (
                CashOrNothingOption
                if inputs["payoff"] == "cash"
                else AssetOrNothingOption
            )
            option = kind(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_date=date.fromisoformat(inputs["effective_date"]),
                expiry_date=date.fromisoformat(inputs["expiry_date"]),
                **({"payout": inputs["payout"]} if kind is CashOrNothingOption else {}),
            )
            context = market(
                inputs["spot"],
                inputs["rate"],
                inputs["dividend"],
                inputs["volatility"],
                date.fromisoformat(inputs["valuation"]),
            )
            with ql.SavedSettings():
                reference, uncertainty = oracle.reference(inputs, digital.STABILITY)
            if (option.expiry_date - context.valuation_time.date()).days <= 2:
                # The existing quadrature profile checks only price at this boundary.
                self.assertAlmostEqual(
                    QuadratureDigitalEngine().price(option, context),
                    reference["price"],
                    delta=profile["tolerances"]["price"],
                )
                continue
            names = sorted(reference.keys() - {"price"})
            result = QuadratureDigitalEngine().price_with_greeks(
                option, context, names, **profile["shifts"]
            )
            for name in ["price", *names]:
                with self.subTest(case=identifier, measure=name):
                    self.assertAlmostEqual(
                        getattr(result, name),
                        reference[name],
                        delta=profile["numerical_tolerances"][name]
                        + uncertainty[f"uncertainty_{name}"],
                    )

    def test_extreme_forwards_and_compensated_greeks(self):
        for rate, dividend in ((1e308, 1e307), (1e307, 1e308), (1e308, 1e308)):
            for kind in KINDS:
                for direction in ("call", "put"):
                    option = contract(kind, direction, end=date(2035, 1, 1))
                    for sigma in (0.2, 1e-10, math.ulp(0.0)):
                        context = market(rate=rate, dividend=dividend, sigma=sigma)
                        self.assertEqual(engine_for(kind).price(option, context), 0)
        option = contract(EuropeanOption, strike=1e300)
        context = market(1e308, 0, 0, valuation=date(2025, 6, 1))
        for name in ("gamma", "speed", "zomma", "color"):
            result = QuadratureVanillaEngine().price_with_greeks(
                option, context, ["delta", name], spot_shift=1e306
            )
            self.assertEqual(result.price, 1e308 - 1e300)
            self.assertEqual(result.delta, 1)
            self.assertEqual(getattr(result, name), 0)

    def test_feasible_bumped_failures_propagate_without_changing_inputs(self):
        for kind in (EuropeanOption, AssetOrNothingOption):
            engine = engine_for(kind)
            option = contract(kind, strike=1e300)
            context = market(1e308, 0, -0.4)
            quote = engine.price(option, context)
            self.assert_rejection(
                ComputationFailureError,
                engine.price_with_greeks,
                option,
                context,
                "delta",
                spot_shift=3e307,
            )
            self.assertEqual(engine.price(option, context), quote)
            self.assertEqual(context.spot_price, 1e308)
        engine = QuadratureDigitalEngine()
        option = contract(CashOrNothingOption, "put", payout=1e308)
        context = market(rate=-0.4, dividend=-0.4)
        quote = engine.price(option, context)
        self.assert_rejection(
            ComputationFailureError,
            engine.price_with_greeks,
            option,
            context,
            "rho",
            rate_shift=0.5,
        )
        self.assertEqual(engine.price(option, context), quote)
        self.assertEqual(context.model_parameters.risk_free_rate, -0.4)
