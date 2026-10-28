"""Finite-difference public contracts and migrated independent reference profiles."""

import importlib
import json
import math
import numbers
import sys
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import get_args

import QuantLib as ql
from kiyosi import ErrorCategory, KiyosiError
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    AnalyticDigitalEngine,
    AnalyticVanillaEngine,
    FiniteDifferenceBarrierEngine,
    FiniteDifferenceDigitalEngine,
    FiniteDifferenceScheme,
    FiniteDifferenceVanillaEngine,
    implied_volatility,
)
from test_binomial import DIGITAL, END, GREEKS, START, VANILLA, contract, market

KINDS = (*VANILLA, *DIGITAL)
FACTORIES = (
    FiniteDifferenceVanillaEngine,
    FiniteDifferenceDigitalEngine,
    FiniteDifferenceBarrierEngine,
)
SCHEMES = ("explicit_euler", "implicit_euler", "crank_nicolson")


def engine_for(kind, **settings):
    return (
        FiniteDifferenceVanillaEngine
        if kind in VANILLA
        else FiniteDifferenceDigitalEngine
    )(**settings)


class FiniteDifferenceTests(unittest.TestCase):
    def category(self, category, operation, *args, **kwargs):
        with self.assertRaises(KiyosiError) as error:
            operation(*args, **kwargs)
        self.assertEqual(error.exception.category, category)

    def test_settings_defaults_representation_validation_and_immutability(self):
        @numbers.Integral.register
        class Index:
            def __index__(self):
                return 17

        self.assertEqual(get_args(FiniteDifferenceScheme), SCHEMES)
        for factory in FACTORIES:
            engine = factory()
            self.assertEqual(
                (
                    engine.asset_step_count,
                    engine.time_step_count,
                    engine.scheme,
                    engine.asset_upper_boundary,
                ),
                (800, 200, "crank_nicolson", None),
            )
            self.assertEqual(factory(asset_step_count=Index()).asset_step_count, 17)
            for count in (3, 10_001, 2**31 - 1):
                self.assertEqual(
                    factory(asset_step_count=count).asset_step_count, count
                )
            for count in (1, 100_001, 2**31 - 1):
                self.assertEqual(factory(time_step_count=count).time_step_count, count)
            for name, minimum in (("asset_step_count", 3), ("time_step_count", 1)):
                for bad in (-1, 0, minimum - 1):
                    self.category(
                        ErrorCategory.INVALID_PARAMETER, factory, **{name: bad}
                    )
                for bad in (True, False, 1.0, 1.5, "3", None):
                    with self.assertRaises(TypeError):
                        factory(**{name: bad})
                for bad in (2**31, -(2**31) - 1):
                    with self.assertRaises(OverflowError):
                        factory(**{name: bad})
            for bad in (0, -1, math.nan, math.inf):
                self.category(
                    ErrorCategory.INVALID_PARAMETER, factory, asset_upper_boundary=bad
                )
            for bad in (True, "400", object()):
                with self.assertRaises(TypeError):
                    factory(asset_upper_boundary=bad)
            for bad in (None, 0, True):
                with self.assertRaises(TypeError):
                    factory(scheme=bad)
            with self.assertRaises(ValueError):
                factory(scheme="CrankNicolson")
            for choice in SCHEMES:
                self.assertEqual(factory(scheme=choice).scheme, choice)
            with self.assertRaises(TypeError):
                factory(unknown=1)
            with self.assertRaises(TypeError):
                factory(400)
            with self.assertRaises(TypeError):
                factory(asset_step_count=0, asset_upper_boundary=True)
            for name in (
                "asset_step_count",
                "time_step_count",
                "scheme",
                "asset_upper_boundary",
            ):
                with self.assertRaises(AttributeError):
                    setattr(engine, name, 1)

    def test_expiry_life_and_clipped_grid_validation(self):
        for kind in KINDS:
            clipped = engine_for(
                kind, time_step_count=2**31 - 1, asset_upper_boundary=1
            )
            self.category(
                ErrorCategory.INVALID_PARAMETER, clipped.price, contract(kind), market()
            )
            self.category(
                ErrorCategory.INVALID_PARAMETER,
                clipped.price_with_greeks,
                contract(kind),
                market(),
                "delta",
            )
            engine = engine_for(kind, asset_upper_boundary=1)
            for direction, sign in (("call", 1), ("put", -1)):
                option = contract(kind, direction)
                self.category(
                    ErrorCategory.INVALID_PARAMETER, engine.price, option, market()
                )
                for spot in (math.nextafter(100, 0), 100, math.nextafter(100, 101)):
                    context = market(spot=spot, valuation=END)
                    pays = sign * (spot - 100) > 0
                    expected = (
                        max(sign * (spot - 100), 0)
                        if kind in VANILLA
                        else (10 if kind is CashOrNothingOption else spot)
                        if pays
                        else 0
                    )
                    result = engine.price_with_greeks(option, context, all_greeks=True)
                    self.assertEqual(result.price, expected)
                    self.assertEqual(engine.price(option, context), expected)
                    self.assertTrue(
                        all(getattr(result, name) is None for name in GREEKS)
                    )
                    self.category(
                        ErrorCategory.UNSUPPORTED_OPERATION,
                        implied_volatility,
                        engine,
                        option,
                        context,
                        expected,
                    )
                for valuation in (
                    START - timedelta(days=1),
                    datetime(2026, 1, 1, microsecond=1, tzinfo=UTC),
                ):
                    self.category(
                        ErrorCategory.INVALID_TIME_RANGE,
                        engine.price,
                        option,
                        market(valuation=valuation),
                    )
            if kind is not AmericanOption:
                tiny = math.ulp(0.0)
                self.category(
                    ErrorCategory.INVALID_PARAMETER,
                    engine_for(kind).price,
                    contract(kind, strike=tiny),
                    market(spot=tiny),
                )

    def test_analysis_requests_shifts_failures_and_input_ownership(self):
        for kind in KINDS:
            engine, option, context = engine_for(kind), contract(kind), market()
            baseline = engine.price(option, context)
            for settings in (
                {"spot_shift": 0},
                {"volatility_shift": math.nan},
                {"rate_shift": math.inf},
                {"time_shift_days": 0},
            ):
                self.category(
                    ErrorCategory.INVALID_PARAMETER,
                    engine.price_with_greeks,
                    option,
                    context,
                    "delta",
                    **settings,
                )
            self.category(
                ErrorCategory.INVALID_PARAMETER,
                engine.price_with_greeks,
                option,
                context,
                [],
            )
            for settings in (
                {"spot_shift": True},
                {"volatility_shift": None},
                {"rate_shift": "1"},
                {"time_shift_days": True},
                {"time_shift_days": 1.5},
                {"unknown": 1},
            ):
                with self.assertRaises(TypeError):
                    engine.price_with_greeks(option, context, "delta", **settings)
            with self.assertRaises(OverflowError):
                engine.price_with_greeks(
                    option, context, "delta", time_shift_days=2**31
                )
            with self.assertRaises(TypeError):
                engine.price_with_greeks(option, context)
            with self.assertRaises(TypeError):
                engine.price_with_greeks(option, context, "delta", all_greeks=True)
            with self.assertRaises(ValueError):
                engine.price_with_greeks(option, context, "unknown")
            other = contract(CashOrNothingOption if kind in VANILLA else EuropeanOption)
            with self.assertRaises(TypeError):
                engine.price(other, context)
            with self.assertRaises(TypeError):
                implied_volatility(engine, other, context, baseline)
            for shift in (1e-300, 100):
                self.assertIsNone(
                    engine.price_with_greeks(
                        option, context, "speed", spot_shift=shift
                    ).speed
                )
            self.assertEqual(engine.price(option, context), baseline)
            self.assertEqual(
                (
                    context.spot_price,
                    context.model_parameters.volatility,
                    engine.asset_upper_boundary,
                ),
                (100, 0.2, None),
            )
            result = engine.price_with_greeks(option, context, "delta")
            del engine, option, context
            self.assertTrue(math.isfinite(result.price) and math.isfinite(result.delta))

    def test_curve_support_and_validation_order(self):
        middle = date(2025, 7, 1)
        risk = RateTermStructure(
            reference_time=START, times=[middle, END], factors=[math.exp(0.1), 1]
        )
        context = market(rate=risk, dividend=0, spot=0.01)
        option = contract(AmericanOption, "put")
        engine = FiniteDifferenceVanillaEngine()
        for valuation in (START, date(2025, 4, 1), middle, date(2025, 10, 1)):
            shifted = context.replace(valuation_time=valuation)
            expected = 100 * risk.factor(valuation, max(valuation, middle)) - 0.01
            self.assertAlmostEqual(engine.price(option, shifted), expected, delta=1e-5)
        for kind in DIGITAL:
            engine, option = engine_for(kind), contract(kind)
            for valuation in (START, END):
                shifted = context.replace(valuation_time=valuation)
                self.category(
                    ErrorCategory.UNSUPPORTED_OPERATION, engine.price, option, shifted
                )
                self.category(
                    ErrorCategory.UNSUPPORTED_OPERATION,
                    engine.price_with_greeks,
                    option,
                    shifted,
                    [],
                )
                with self.assertRaises(TypeError):
                    engine.price_with_greeks(option, shifted, [], spot_shift=True)
        short = RateTermStructure(reference_time=START, times=[middle], factors=[0.99])
        self.category(
            ErrorCategory.INVALID_TIME_RANGE,
            FiniteDifferenceVanillaEngine().price,
            contract(EuropeanOption),
            market(rate=short),
        )

    def test_implied_volatility_and_solver_failures(self):
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
                self.category(
                    ErrorCategory.UNBRACKETED_VOLATILITY,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    -1,
                    lower_bound=0.1,
                    upper_bound=0.22,
                )
                self.category(
                    ErrorCategory.INVALID_PARAMETER,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    math.nan,
                )
                self.category(
                    ErrorCategory.SOLVER_NON_CONVERGENCE,
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
        engine = FiniteDifferenceVanillaEngine(
            scheme="explicit_euler", asset_step_count=80, time_step_count=500
        )
        option, context = contract(EuropeanOption), market()
        self.category(
            ErrorCategory.INVALID_PARAMETER,
            implied_volatility,
            engine,
            option,
            context,
            engine.price(option, context),
        )

    def test_grid_refinement_schemes_interpolation_and_long_expiry(self):
        for kind in (EuropeanOption, *DIGITAL):
            analytic = (
                AnalyticVanillaEngine() if kind in VANILLA else AnalyticDigitalEngine()
            )
            for direction in ("call", "put"):
                option, context = contract(kind, direction), market(spot=100.25)
                expected = analytic.price(option, context)
                previous = math.inf
                for count in (100, 200, 400, 800):
                    actual = engine_for(
                        kind,
                        asset_step_count=count,
                        time_step_count=count,
                        asset_upper_boundary=400,
                    ).price(option, context)
                    self.assertLess(abs(actual - expected), previous)
                    previous = abs(actual - expected)
                self.assertLess(previous, 0.01)
                for scheme in SCHEMES:
                    engine = engine_for(
                        kind,
                        asset_step_count=200,
                        time_step_count=2000,
                        scheme=scheme,
                        asset_upper_boundary=400,
                    )
                    result = engine.price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    self.assertAlmostEqual(result.price, expected, delta=0.05)
                    h = 0.01
                    high = engine.price(option, context.replace(spot_price=100.25 + h))
                    low = engine.price(option, context.replace(spot_price=100.25 - h))
                    self.assertAlmostEqual(
                        result.delta, (high - low) / (2 * h), delta=1e-6
                    )
                    self.assertAlmostEqual(
                        result.gamma, (high - 2 * result.price + low) / h**2, delta=1e-6
                    )
                option = type(option)(
                    option_type=direction,
                    strike=100,
                    effective_date=START,
                    expiry_date=date(2035, 1, 1),
                    **({"payout": 10} if kind is CashOrNothingOption else {}),
                )
                context = market(rate=0.03, dividend=0.02, sigma=0.6)
                self.assertAlmostEqual(
                    engine_for(kind, time_step_count=1000).price(option, context),
                    analytic.price(option, context),
                    delta=0.01,
                )

    def test_shared_and_distinct_engines_support_concurrent_analysis(self):
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
                    list(pool.map(compute, range(8))),
                    [(expected.price, expected.delta)] * 8,
                )

    def test_existing_quantlib_european_and_american_profiles(self):
        root = Path(__file__).resolve().parents[3] / "tests/python"
        sys.path.insert(0, str(root))
        oracle = importlib.import_module("quantlib_reference.oracle")
        american = importlib.import_module("quantlib_reference.american")
        scenarios = importlib.import_module("quantlib_reference.scenarios")
        oracle.require_version()
        european_profile = next(
            p
            for p in scenarios.numerical_profiles(
                root / "quantlib_reference/numerical_engines.json"
            )
            if p["engine"] == "FiniteDifferenceVanillaEngine"
        )
        american_config = json.loads(
            (root / "quantlib_reference/american.json").read_text()
        )
        american_profile = next(
            p
            for p in oracle.expand_profiles(american_config)
            if p["engine"] == "FiniteDifferenceVanillaEngine"
        )
        european_scenarios = scenarios.load_scenarios(
            root / "quantlib_reference/scenarios.json"
        )
        for kind, rows, profile in (
            (EuropeanOption, european_scenarios, european_profile),
            (AmericanOption, american_config["scenarios"], american_profile),
        ):
            engine = FiniteDifferenceVanillaEngine(**profile["settings"])
            for scenario in rows:
                inputs = scenario["inputs"]
                if kind is EuropeanOption and oracle.exclusions(inputs):
                    continue
                option = kind(
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
                    reference, metadata = (
                        oracle.reference(inputs)
                        if kind is EuropeanOption
                        else american.reference(inputs)
                    )
                names = sorted(reference.keys() - {"price"})
                result = engine.price_with_greeks(
                    option, context, names, **profile["shifts"]
                )
                for name, expected in reference.items():
                    budget = (
                        profile["tolerances"]
                        if name in ("price", "delta", "gamma")
                        else profile["numerical_tolerances"]
                    )
                    with self.subTest(
                        kind=kind, case=scenario["case_id"], measure=name
                    ):
                        self.assertAlmostEqual(
                            getattr(result, name),
                            expected,
                            delta=budget[name] + metadata[f"uncertainty_{name}"],
                        )

    def test_existing_quantlib_digital_profiles(self):
        root = Path(__file__).resolve().parents[3] / "tests/python"
        sys.path.insert(0, str(root))
        digital = importlib.import_module("quantlib_reference.digital")
        oracle = importlib.import_module("quantlib_reference.oracle")
        oracle.require_version()
        config = digital.configuration()
        profile = next(
            p
            for p in config["profiles"]
            if p["engine"] == "FiniteDifferenceDigitalEngine"
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
            boundary = (option.expiry_date - context.valuation_time.date()).days <= 2
            settings = profile["settings"] | (
                profile["boundary_settings"] if boundary else {}
            )
            # Preserve the legacy native-only cases and wide-bump acceptance scenarios.
            wrapped = (
                not boundary
                and inputs["spot"] == 100
                and inputs["expiry_date"] == "2026-01-06"
            )
            names = (
                sorted(reference.keys() - {"price"}) if wrapped else ["delta", "gamma"]
            )
            result = FiniteDifferenceDigitalEngine(**settings).price_with_greeks(
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

    def test_refined_one_day_vanilla_prices_and_native_greeks(self):
        end = START + timedelta(days=1)
        engine = FiniteDifferenceVanillaEngine(
            asset_step_count=6400, time_step_count=1600, asset_upper_boundary=400
        )
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction, strike=100, effective_date=START, expiry_date=end
            )
            context = market(sigma=0.3)
            expected = AnalyticVanillaEngine().price_with_greeks(
                option, context, ["delta", "gamma"]
            )
            actual = engine.price_with_greeks(option, context, ["delta", "gamma"])
            for name, tolerance in (
                ("price", 0.001),
                ("delta", 0.0001),
                ("gamma", 0.0003),
            ):
                self.assertAlmostEqual(
                    getattr(actual, name), getattr(expected, name), delta=tolerance
                )

    def test_american_long_expiry_domain_matches_live_quantlib(self):
        root = Path(__file__).resolve().parents[3] / "tests/python"
        sys.path.insert(0, str(root))
        american = importlib.import_module("quantlib_reference.american")
        inputs = {
            "option": "call",
            "strike": 100,
            "spot": 100,
            "rate": 0.03,
            "dividend": 0.05,
            "volatility": 1.0,
            "effective_date": "2025-01-01",
            "valuation": "2025-01-01",
            "expiry_date": "2035-01-01",
        }
        with ql.SavedSettings():
            reference = american.native(tuple(sorted(inputs.items())), (3200, 3200))[
                "price"
            ]
        option = AmericanOption(
            option_type="call",
            strike=100,
            effective_date=START,
            expiry_date=date(2035, 1, 1),
        )
        context = market(rate=0.03, dividend=0.05, sigma=1)
        coarse = FiniteDifferenceVanillaEngine().price(option, context)
        engine = FiniteDifferenceVanillaEngine(time_step_count=1600)
        refined = engine.price(option, context)
        self.assertAlmostEqual(coarse, reference, delta=0.2)
        self.assertAlmostEqual(refined, reference, delta=0.03)
        self.assertLess(abs(refined - reference), abs(coarse - reference))
        self.assertEqual(
            engine.price_with_greeks(option, context, "vega").price, refined
        )
