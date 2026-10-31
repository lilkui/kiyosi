"""CRR public contracts shared by the Rust implementation."""

import importlib
import math
import numbers
import sys
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticDigitalEngine,
    AnalyticEuropeanEngine,
    CoxRossRubinsteinDigitalEngine,
    CoxRossRubinsteinVanillaEngine,
    implied_volatility,
)

START, END = date(2025, 1, 1), date(2026, 1, 1)
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
VANILLA = (EuropeanOption, AmericanOption)
DIGITAL = (CashOrNothingOption, AssetOrNothingOption)


def contract(kind, direction="call", strike=100):
    terms = {
        "option_type": direction,
        "strike": strike,
        "effective_date": START,
        "expiry_date": END,
    }
    if kind is CashOrNothingOption:
        terms["payout"] = 10
    return kind(**terms)


def market(spot=100, rate=0.05, dividend=0.02, sigma=0.2, valuation=START):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=sigma
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def engine_for(kind, steps=256):
    factory = (
        CoxRossRubinsteinVanillaEngine
        if kind in VANILLA
        else CoxRossRubinsteinDigitalEngine
    )
    return factory(step_count=steps)


def rollback(
    kind, direction, steps, spot=100, rate=0.05, dividend=0.02, sigma=0.2, strike=100
):
    """Compute independent CRR prices and node Greeks by explicit rollback.

    Parameters
    ----------
    kind : type or str
        Contract class or barrier activation choice used by the test.
    direction : {'call', 'put'} or None
        Terminal payoff direction; touch contracts have no strike direction.
    steps : int
        Positive CRR time-step count.
    spot : float, optional
        Underlying spot price.
    rate : float, optional
        Annual continuously compounded risk-free rate.
    dividend : float, optional
        Annual continuously compounded dividend yield.
    sigma : float, optional
        Annual volatility in decimal units.
    strike : float, optional
        Exercise price.

    Returns
    -------
    tuple of float or None
        Price, node delta and node gamma; unavailable node Greeks are None.
    """
    up = math.exp(sigma / math.sqrt(steps))
    down = 1 / up
    probability = (math.exp((rate - dividend) / steps) - down) / (up - down)
    discount = math.exp(-rate / steps)
    sign = 1 if direction == "call" else -1
    values = []
    for node in range(steps + 1):
        terminal = spot * math.exp((2 * node - steps) * sigma / math.sqrt(steps))
        itm = sign * (terminal - strike) > 0
        values.append(
            (10 if kind is CashOrNothingOption else terminal) * itm
            if kind in DIGITAL
            else max(sign * (terminal - strike), 0)
        )
    first, second = [], []
    for level in range(steps, 0, -1):
        if level == 1:
            first = values[:2]
        if level == 2:
            second = values[:3]
        values = [
            discount * ((1 - probability) * values[n] + probability * values[n + 1])
            for n in range(level)
        ]
        if kind is AmericanOption:
            values = [
                max(
                    v,
                    sign
                    * (
                        spot * math.exp((2 * n - level + 1) * sigma / math.sqrt(steps))
                        - strike
                    ),
                )
                for n, v in enumerate(values)
            ]
    delta = (first[1] - first[0]) / (spot * (up - down))
    gamma = (
        None
        if steps == 1
        else (
            (second[2] - second[1]) / (spot * (up**2 - 1))
            - (second[1] - second[0]) / (spot * (1 - down**2))
        )
        / (spot * (up**2 - down**2) / 2)
    )
    return values[0], delta, gamma


class BinomialTests(unittest.TestCase):
    def test_price_and_greeks_release_the_gil(self):
        context = market()
        for kind in (*VANILLA, *DIGITAL):
            factory = (
                CoxRossRubinsteinVanillaEngine
                if kind in VANILLA
                else CoxRossRubinsteinDigitalEngine
            )
            engine = factory(step_count=20_000 if kind is AmericanOption else 1_000_000)
            option = contract(kind, "put")
            for method in ("price", "price_with_greeks"):
                with self.subTest(instrument=kind.__name__, method=method):
                    ready, start, observed = (
                        threading.Event(),
                        threading.Event(),
                        threading.Event(),
                    )

                    def python_worker(ready=ready, start=start, observed=observed):
                        ready.set()
                        start.wait()
                        observed.set()

                    worker = threading.Thread(target=python_worker)
                    worker.start()
                    ready.wait()
                    previous = sys.getswitchinterval()
                    try:
                        # Force the native call to provide the worker's scheduling opportunity.
                        sys.setswitchinterval(100)
                        start.set()
                        if method == "price":
                            engine.price(option, context)
                        else:
                            engine.price_with_greeks(option, context, "delta")
                        self.assertTrue(observed.is_set(), "binomial call held the GIL")
                    finally:
                        sys.setswitchinterval(previous)
                        start.set()
                        worker.join()

    def test_one_step_speed_omits_infeasible_scalar_stencil(self):
        engine = engine_for(EuropeanOption, 1)
        option = contract(EuropeanOption, strike=0.015)
        context = market(spot=0.015)
        result = engine.price_with_greeks(option, context, "speed")
        self.assertEqual(result.price, engine.price(option, context))
        self.assertIsNone(result.speed)

    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_default_steps_and_european_price(self):
        option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=date(2025, 1, 1),
        )
        engine = CoxRossRubinsteinVanillaEngine()
        self.assertEqual(engine.step_count, 256)
        self.assertEqual(
            engine.price(option, context),
            CoxRossRubinsteinVanillaEngine(step_count=256).price(option, context),
        )
        self.assertAlmostEqual(
            engine.price(option, context), 9.227005508154036, delta=0.01
        )

    def test_configuration_representation_and_domain_boundaries(self):
        @numbers.Integral.register
        class Index:
            def __index__(self):
                return 17

        for factory in (CoxRossRubinsteinVanillaEngine, CoxRossRubinsteinDigitalEngine):
            self.assertEqual(factory().step_count, 256)
            self.assertEqual(factory(step_count=Index()).step_count, 17)
            for count in (1, 1_000_000):
                engine = factory(step_count=count)
                self.assertEqual(engine.step_count, count)
                with self.assertRaises(AttributeError):
                    engine.step_count = 5
            for count in (0, -1, 1_000_001):
                self.assert_rejection(InvalidInputError, factory, step_count=count)
            for count in (True, False, 1.0, 1.5, "1", None):
                with self.assertRaises(TypeError):
                    factory(step_count=count)
            for count in (2**31, -(2**31) - 1):
                with self.assertRaises(OverflowError):
                    factory(step_count=count)
            with self.assertRaises(TypeError):
                factory(unknown=1)

    def test_node_greeks_and_prices_match_explicit_rollback(self):
        for steps in (1, 2, 3, 17, 255, 256, 257):
            for kind in (*VANILLA, *DIGITAL):
                for direction in ("call", "put"):
                    option = contract(kind, direction)
                    engine = engine_for(kind, steps)
                    expected = rollback(kind, direction, steps)
                    result = engine.price_with_greeks(
                        option, market(), ("delta", "gamma")
                    )
                    with self.subTest(steps=steps, kind=kind, direction=direction):
                        self.assertAlmostEqual(result.price, expected[0], delta=1e-10)
                        self.assertAlmostEqual(result.delta, expected[1], delta=1e-10)
                        if steps >= 2:
                            self.assertAlmostEqual(
                                result.gamma, expected[2], delta=1e-10
                            )
                            gamma_only = engine.price_with_greeks(
                                option, market(), "gamma"
                            )
                            self.assertEqual(result.gamma, gamma_only.gamma)
                            self.assertIsNone(gamma_only.delta)
                        elif kind in VANILLA:
                            self.assertIsNotNone(result.gamma)
                        else:
                            self.assertIsNone(result.gamma)
                        self.assertIsNone(result.vega)
                        self.assertEqual(result.price, engine.price(option, market()))

    def test_grid_refinement_and_negative_carry(self):
        for rate, dividend, sigma in (
            (0.05, 0.02, 0.2),
            (-0.03, 0.01, 0.45),
            (0.01, 0.07, 0.3),
            (-0.02, -0.05, 0.15),
        ):
            for spot in (95, 100, 105):
                context = market(spot, rate, dividend, sigma)
                for kind in (EuropeanOption, *DIGITAL):
                    analytic = (
                        AnalyticEuropeanEngine()
                        if kind is EuropeanOption
                        else AnalyticDigitalEngine()
                    )
                    for direction in ("call", "put"):
                        option = contract(kind, direction)
                        expected = analytic.price_with_greeks(
                            option, context, ("delta", "gamma")
                        )
                        for steps in (255, 256, 4095, 4096):
                            result = engine_for(kind, steps).price_with_greeks(
                                option, context, ("delta", "gamma")
                            )
                            budget = (
                                10 if kind is CashOrNothingOption else spot
                            ) / math.sqrt(steps)
                            self.assertAlmostEqual(
                                result.price, expected.price, delta=budget
                            )
                            self.assertAlmostEqual(
                                result.delta,
                                expected.delta,
                                delta=budget / spot / sigma,
                            )
                            self.assertAlmostEqual(
                                result.gamma,
                                expected.gamma,
                                delta=budget / spot**2 / sigma**2,
                            )

    def test_american_immediate_exercise_and_european_limit(self):
        for steps in (1, 2, 3, 17, 255, 256, 257):
            engine = engine_for(AmericanOption, steps)
            for direction, strike, spot, rate, dividend, delta in (
                ("put", 100, 88, 0.15, 0, -1),
                ("call", 88, 100, 0, 0.15, 1),
            ):
                result = engine.price_with_greeks(
                    contract(AmericanOption, direction, strike),
                    market(spot, rate, dividend),
                    all_greeks=True,
                )
                self.assertEqual(
                    (result.price, result.delta, result.gamma), (12, delta, 0)
                )
            for rate, dividend in ((0, 0), (0.04, 0), (0.04, -0.01)):
                context = market(rate=rate, dividend=dividend)
                american = engine.price_with_greeks(
                    contract(AmericanOption), context, all_greeks=True
                )
                european = engine.price_with_greeks(
                    contract(EuropeanOption), context, all_greeks=True
                )
                self.assertAlmostEqual(american.price, european.price, delta=1e-10)
                for name in GREEKS:
                    self.assertIsNotNone(getattr(american, name))
                    if name in ("delta", "gamma"):
                        self.assertAlmostEqual(
                            getattr(american, name),
                            getattr(european, name),
                            delta=1e-10,
                        )

    def test_expiry_strict_equality_and_lifetime_boundaries(self):
        for kind in (*VANILLA, *DIGITAL):
            engine = engine_for(kind)
            for direction in ("call", "put"):
                option = contract(kind, direction)
                for spot in (90, 100, 110):
                    context = market(spot, 100, -100, 1e-200, END)
                    intrinsic = max(
                        (1 if direction == "call" else -1) * (spot - 100), 0
                    )
                    expected = (
                        (10 if kind is CashOrNothingOption else spot) * (intrinsic > 0)
                        if kind in DIGITAL
                        else intrinsic
                    )
                    result = engine.price_with_greeks(option, context, all_greeks=True)
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
                for time in (
                    START - timedelta(days=1),
                    datetime(2026, 1, 1, tzinfo=UTC) + timedelta(microseconds=1),
                ):
                    self.assert_rejection(
                        InvalidInputError,
                        engine.price,
                        option,
                        market(valuation=time),
                    )

    def test_flat_rate_applicability_validation_order_and_failures(self):
        curve = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.05)]
        )
        for kind in (*VANILLA, *DIGITAL):
            engine, option = engine_for(kind), contract(kind)
            for valuation in (START, END):
                context = market(rate=curve, valuation=valuation)
                self.assert_rejection(
                    UnsupportedOperationError, engine.price, option, context
                )
                self.assert_rejection(
                    UnsupportedOperationError,
                    engine.price_with_greeks,
                    option,
                    context,
                    [],
                )
            for field in ("rate", "dividend"):
                context = market(**{field: curve})
                self.assert_rejection(
                    UnsupportedOperationError, engine.price, option, context
                )
            flat = RateTermStructure.flat(rate=0.05, reference_time=START)
            self.assertEqual(
                engine.price(option, market(rate=flat)), engine.price(option, market())
            )
            for sigma, rate, dividend in (
                (1e-200, 0.05, 0.02),
                (50, 0.05, 0.02),
                (0.2, -1000, -1000),
            ):
                self.assert_rejection(
                    ComputationFailureError,
                    engine.price,
                    option,
                    market(sigma=sigma, rate=rate, dividend=dividend),
                )
            self.assert_rejection(
                InvalidInputError,
                engine.price_with_greeks,
                option,
                market(),
                [],
            )
            for shifts in (
                {"spot_shift": 0},
                {"volatility_shift": math.nan},
                {"rate_shift": -1},
                {"time_shift_days": 0},
            ):
                self.assert_rejection(
                    InvalidInputError if kind in VANILLA else TypeError,
                    engine.price_with_greeks,
                    option,
                    market(),
                    "delta",
                    **shifts,
                )
            for shifts in (
                {"spot_shift": True},
                {"time_shift_days": 1.5},
                {"time_shift_days": True},
                {"unknown": 1},
            ):
                with self.assertRaises(TypeError):
                    engine.price_with_greeks(option, market(), "delta", **shifts)
            with self.assertRaises(OverflowError if kind in VANILLA else TypeError):
                engine.price_with_greeks(
                    option, market(), "delta", time_shift_days=2**31
                )
            mismatch = contract(
                CashOrNothingOption if kind in VANILLA else EuropeanOption
            )
            for operation, args in (
                (engine.price, (mismatch, market())),
                (engine.price_with_greeks, (mismatch, market(), "delta")),
                (implied_volatility, (engine, mismatch, market(), 10)),
            ):
                with self.assertRaises(UnsupportedOperationError):
                    operation(*args)

    def test_digital_greek_availability(self):
        for kind in DIGITAL:
            for steps in (1, 2, 256):
                engine, option = engine_for(kind, steps), contract(kind)
                expected = engine.price_with_greeks(option, market(), all_greeks=True)
                for name in GREEKS:
                    self.assertEqual(
                        getattr(expected, name) is not None,
                        name == "delta" or (name == "gamma" and steps >= 2),
                    )
                    selected = engine.price_with_greeks(
                        option,
                        market(),
                        name,
                    )
                    if getattr(expected, name) is not None:
                        self.assertAlmostEqual(
                            getattr(selected, name),
                            getattr(expected, name),
                            delta=1e-12,
                        )
                    self.assertEqual(selected.price, expected.price)

    def test_implied_volatility_tree_bounds_failures_and_preserved_state(self):
        for kind in (*VANILLA, *DIGITAL):
            for direction in ("call", "put"):
                engine = engine_for(kind, 17)
                option, context = (
                    contract(kind, direction),
                    market(rate=0.15, dividend=0),
                )
                quote = engine.price(option, context)
                self.assertAlmostEqual(
                    implied_volatility(engine, option, context, quote, upper_bound=0.4),
                    0.2,
                    delta=1e-6,
                )
                self.assert_rejection(
                    RootNotBracketedError,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    quote,
                    upper_bound=0.01,
                )
                self.assert_rejection(
                    RootNotBracketedError,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    -1,
                    upper_bound=0.4,
                )
                self.assert_rejection(
                    InvalidInputError,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    math.nan,
                )
                self.assert_rejection(
                    InvalidInputError,
                    implied_volatility,
                    engine,
                    option,
                    context,
                    quote,
                    max_iterations=0,
                )
                self.assertEqual(engine.price(option, context), quote)
                self.assertEqual(engine.step_count, 17)
                self.assertEqual(context.spot_price, 100)
                self.assertEqual(context.model_parameters.volatility, 0.2)

    def test_deterministic_endpoints_and_extreme_tail_regression(self):
        for rate in (-3.2, 3.2):
            for kind in DIGITAL:
                for direction in ("call", "put"):
                    pays = (direction == "call") == (rate > 0)
                    expected = (
                        (10 * math.exp(-rate) if kind is CashOrNothingOption else 100)
                        if pays
                        else 0
                    )
                    self.assertAlmostEqual(
                        engine_for(kind).price(
                            contract(kind, direction), market(rate=rate, dividend=0)
                        ),
                        expected,
                        delta=1e-9,
                    )
        context = market(spot=1e280, rate=0.04, dividend=0.01)
        option = contract(EuropeanOption, strike=1e280 * math.exp(8))
        expected = rollback(
            EuropeanOption,
            "call",
            2048,
            spot=1e280,
            rate=0.04,
            dividend=0.01,
            strike=option.strike,
        )[0]
        self.assertGreater(expected, 0)
        self.assertAlmostEqual(
            engine_for(EuropeanOption, 2048).price(option, context) / expected,
            1,
            delta=1e-9,
        )

    def test_shared_and_distinct_instances_support_concurrent_pricing(self):
        for kind in (*VANILLA, *DIGITAL):
            option, context, engine = contract(kind), market(), engine_for(kind)
            expected = engine.price(option, context)

            def price(index, engine=engine, kind=kind, option=option, context=context):
                selected = engine if index % 2 else engine_for(kind)
                quote = selected.price(option, context)
                sigma = implied_volatility(
                    selected, option, context, quote, lower_bound=0.1, upper_bound=0.22
                )
                return quote, sigma

            with ThreadPoolExecutor(max_workers=4) as pool:
                for quote, sigma in pool.map(price, range(8)):
                    self.assertEqual(quote, expected)
                    self.assertAlmostEqual(sigma, 0.2, delta=1e-6)

    def test_existing_quantlib_european_and_american_profiles(self):
        oracle = importlib.import_module("quantlib_reference.oracle")
        american = importlib.import_module("quantlib_reference.american")
        oracle.require_version()
        european_profile = next(
            p
            for p in oracle.load_fixture("numerical_engines.json")["profiles"]
            if p["engine"] == "CoxRossRubinsteinVanillaEngine"
        )
        american_config = oracle.load_fixture("american.json")
        american_profile = next(
            p
            for p in american_config["profiles"]
            if p["engine"] == "CoxRossRubinsteinVanillaEngine"
        )
        european_scenarios = oracle.load_fixture("scenarios.json")["scenarios"]
        for kind, rows, profile in (
            (EuropeanOption, european_scenarios, european_profile),
            (AmericanOption, american_config["scenarios"], american_profile),
        ):
            engine = CoxRossRubinsteinVanillaEngine(**profile["settings"])
            for scenario in rows:
                inputs = scenario["inputs"]
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
                if kind is EuropeanOption and oracle.exclusions(inputs):
                    # The numerical profile excludes wide-bump near-expiry stencils.
                    reference = {
                        name: value
                        for name, value in reference.items()
                        if name in ("price", "delta", "gamma")
                    }
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
