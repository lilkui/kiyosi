"""American approximation contracts shared by the Rust implementation."""

import importlib
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta
from fractions import Fraction

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import AmericanOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    BjerksundStenslandVanillaEngine,
    implied_volatility,
)

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


class AmericanTests(unittest.TestCase):
    def setUp(self):
        self.terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        self.option = AmericanOption(**self.terms)
        self.context = self.market()
        self.engine = BjerksundStenslandVanillaEngine()

    def market(self, spot=100, rate=0.05, dividend=0.03, volatility=0.2, at=None):
        return PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
            ),
            spot_price=spot,
            valuation_time=at or self.terms["effective_date"],
        )

    def assert_rejection(self, expected, call, *args, **kwargs):
        with self.assertRaises(expected) as error:
            call(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def test_maturity_regressions(self):
        for days, call, put in (
            (1, 0.4203293406666333, 0.41497263683106667),
            (7, 1.1233367180705613, 1.0862318403299156),
            (30, 2.362502879143129, 2.205771411369753),
            (91, 4.1945462483931095, 3.7306674960129627),
            (180, 5.986257668860937, 5.094433820244056),
            (1000, 14.425584357882556, 10.310794114806157),
            (3650, 25.33817727859211, 15.052781075279043),
        ):
            for direction, expected in (("call", call), ("put", put)):
                with self.subTest(days=days, direction=direction):
                    option = AmericanOption(
                        **dict(
                            self.terms,
                            option_type=direction,
                            expiry_date=self.terms["effective_date"]
                            + timedelta(days=days),
                        )
                    )
                    self.assertAlmostEqual(
                        self.engine.price(option, self.context), expected, delta=1e-11
                    )

    def test_early_exercise_and_european_limits(self):
        for direction, spot, rate, dividend, expected in (
            ("call", 200, 0.03, 0.1, 100),
            ("put", 50, 0.1, 0.03, 50),
        ):
            option = AmericanOption(**dict(self.terms, option_type=direction))
            self.assertEqual(
                self.engine.price(option, self.market(spot, rate, dividend)), expected
            )
        for direction, rate, dividend in (
            ("call", 0.05, 0),
            ("call", 0.05, -0.01),
            ("put", 0, 0.05),
        ):
            terms = dict(self.terms, option_type=direction)
            context = self.market(rate=rate, dividend=dividend)
            self.assertEqual(
                self.engine.price(AmericanOption(**terms), context),
                AnalyticEuropeanEngine().price(EuropeanOption(**terms), context),
            )

    def test_expiry_and_microsecond_exercise_boundaries(self):
        end = self.terms["expiry_date"]
        for direction in ("call", "put"):
            option = AmericanOption(**dict(self.terms, option_type=direction))
            for spot in (90, 100, 110):
                context = self.market(spot=spot, rate=-0.05, dividend=-0.03, at=end)
                expected = max(spot - 100 if direction == "call" else 100 - spot, 0)
                result = self.engine.price_with_greeks(option, context, greeks="all")
                self.assertEqual(result.price, expected)
                self.assertTrue(all(getattr(result, name) is None for name in GREEKS))
                self.assert_rejection(
                    UnsupportedOperationError,
                    implied_volatility,
                    self.engine,
                    option,
                    context,
                    expected,
                )
            for at in (
                datetime(2024, 12, 31, 23, 59, 59, 999999, UTC),
                datetime(2026, 1, 1, 0, 0, 0, 1, UTC),
            ):
                self.assert_rejection(
                    InvalidInputError,
                    self.engine.price,
                    option,
                    self.market(at=at),
                )
            self.assertGreaterEqual(
                self.engine.price(
                    option,
                    self.market(at=datetime(2025, 12, 31, 23, 59, 59, 999999, UTC)),
                ),
                0,
            )
        same_day = AmericanOption(**dict(self.terms, effective_date=end))
        self.assertEqual(self.engine.price(same_day, self.market(spot=110, at=end)), 10)

    def test_construction_protocols_validation_order_and_ownership(self):
        for strike in (0, -1, float("nan"), float("inf")):
            self.assert_rejection(
                InvalidInputError,
                AmericanOption,
                **dict(self.terms, strike=strike, expiry_date=date(2024, 1, 1)),
            )
        self.assert_rejection(
            InvalidInputError,
            AmericanOption,
            **dict(self.terms, expiry_date=date(2024, 1, 1)),
        )
        for fields in (
            {"strike": True},
            {"strike": "100"},
            {"strike": 1 + 0j},
            {"strike": None},
            {"effective_date": datetime(2025, 1, 1)},
            {"expiry_date": "2026-01-01"},
            {"option_type": 1},
            {"unknown": 1},
        ):
            with self.assertRaises(TypeError):
                AmericanOption(**dict(self.terms, **fields))
        with self.assertRaises(ValueError):
            AmericanOption(**dict(self.terms, option_type="CALL"))
        with self.assertRaises(OverflowError):
            AmericanOption(**dict(self.terms, strike=10**1000))
        self.assertEqual(
            AmericanOption(**dict(self.terms, strike=Fraction(201, 2))).strike, 100.5
        )
        self.assertEqual(self.option, AmericanOption(**self.terms))
        self.assertEqual(self.option.effective_date, self.terms["effective_date"])
        self.assertEqual(self.option.expiry_date, self.terms["expiry_date"])
        self.assertEqual(self.option.option_type, "call")
        with self.assertRaises(AttributeError):
            self.option.strike = 105

    def test_applicability_and_error_order(self):
        for direction in ("call", "put"):
            option = AmericanOption(**dict(self.terms, option_type=direction))
            for r, q, sigma, expected_error in (
                (-0.01, 0.02, 0.2, UnsupportedOperationError),
                (0.05, 0.1, 0.02, UnsupportedOperationError),
                (0, 0.02, 0.01, UnsupportedOperationError),
                (0, 0.1, 0.05, UnsupportedOperationError),
                (0.05, 0.03, 1e200, ComputationFailureError),
                (0.1, 0.05, 1e-200, ComputationFailureError),
            ):
                context = self.market(
                    rate=r if direction == "call" else q,
                    dividend=q if direction == "call" else r,
                    volatility=sigma,
                )
                self.assert_rejection(
                    expected_error, self.engine.price, option, context
                )
                self.assert_rejection(
                    expected_error,
                    self.engine.price_with_greeks,
                    option,
                    context,
                    "delta",
                )
            curve = RateTermStructure(
                reference_time=self.terms["effective_date"],
                times=[self.terms["expiry_date"]],
                factors=[math.exp(-0.05)],
            )
            for context in (
                self.market(rate=curve),
                self.market(dividend=curve),
                self.market(rate=curve, at=self.terms["expiry_date"]),
            ):
                self.assert_rejection(
                    UnsupportedOperationError,
                    self.engine.price,
                    option,
                    context,
                )
                self.assert_rejection(
                    UnsupportedOperationError,
                    self.engine.price_with_greeks,
                    option,
                    context,
                    [],
                )
                self.assert_rejection(
                    UnsupportedOperationError,
                    implied_volatility,
                    self.engine,
                    option,
                    context,
                    10,
                )
        explicit = self.market(
            rate=RateTermStructure.flat(
                0.05, reference_time=self.terms["effective_date"]
            ),
            dividend=RateTermStructure.flat(0.03),
        )
        self.assertEqual(
            self.engine.price(self.option, explicit),
            self.engine.price(self.option, self.context),
        )
        context = self.market(rate=0, dividend=0.02)
        self.assertGreater(self.engine.price(self.option, context), 0)
        self.assert_rejection(
            UnsupportedOperationError,
            self.engine.price_with_greeks,
            self.option,
            context,
            "rho",
        )

    def test_greeks_defaults_scalar_differences_and_shared_solving(self):

        differences = importlib.import_module("price_differences").price_differences
        for direction in ("call", "put"):
            option = AmericanOption(**dict(self.terms, option_type=direction))
            result = self.engine.price_with_greeks(option, self.context, greeks="all")
            numerical = differences(self.engine, option, self.context)
            for name in GREEKS:
                with self.subTest(direction=direction, name=name):
                    self.assertAlmostEqual(
                        getattr(result, name), getattr(numerical, name), delta=1e-7
                    )
                    selected = self.engine.price_with_greeks(option, self.context, name)
                    self.assertEqual(selected.require(name), getattr(result, name))
                    self.assertIsNone(
                        getattr(selected, "gamma" if name == "delta" else "delta")
                    )
            # Fixed bracket avoids the put's unsupported low-volatility exercise boundary.
            if direction == "put":
                self.assert_rejection(
                    UnsupportedOperationError,
                    implied_volatility,
                    self.engine,
                    option,
                    self.context,
                    result.price,
                )
            self.assertAlmostEqual(
                implied_volatility(
                    self.engine,
                    option,
                    self.context,
                    result.price,
                    lower_bound=0.1,
                    upper_bound=0.4,
                ),
                0.2,
                delta=1e-6,
            )
        quote = self.engine.price(self.option, self.context)
        with ThreadPoolExecutor(max_workers=4) as pool:
            values = list(
                pool.map(
                    lambda _: implied_volatility(
                        self.engine, self.option, self.context, quote
                    ),
                    range(8),
                )
            )
        for value in values:
            self.assertAlmostEqual(value, 0.2, delta=1e-6)
        self.assert_rejection(
            RootNotBracketedError,
            implied_volatility,
            self.engine,
            self.option,
            self.context,
            -1,
        )
        self.assert_rejection(
            NonConvergenceError,
            implied_volatility,
            self.engine,
            self.option,
            self.context,
            quote,
            max_iterations=1,
            price_tolerance=1e-16,
            parameter_tolerance=1e-16,
        )
        self.assertEqual(self.context.spot_price, 100)
        self.assertEqual(self.context.model_parameters.volatility, 0.2)
        self.assertEqual(self.engine.price(self.option, self.context), quote)

    def test_analysis_rejects_protocols_and_unsupported_pairs(self):
        self.assert_rejection(
            InvalidInputError,
            self.engine.price_with_greeks,
            self.option,
            self.context,
            [],
        )
        for settings in (
            {"spot_shift": 0},
            {"volatility_shift": float("nan")},
            {"rate_shift": -1},
            {"time_shift_days": 0},
        ):
            self.assert_rejection(
                InvalidInputError,
                self.engine.price_with_greeks,
                self.option,
                self.context,
                "delta",
                **settings,
            )
        for settings in (
            {"spot_shift": True},
            {"volatility_shift": "0.01"},
            {"time_shift_days": 1.5},
            {"time_shift_days": True},
            {"unknown": 1},
        ):
            with self.assertRaises(TypeError):
                self.engine.price_with_greeks(
                    self.option, self.context, "delta", **settings
                )
        with self.assertRaises(OverflowError):
            self.engine.price_with_greeks(
                self.option, self.context, "delta", time_shift_days=2**31
            )
        for settings in (
            {"max_iterations": True},
            {"max_iterations": 1.5},
            {"lower_bound": True},
            {"unknown": 1},
        ):
            with self.assertRaises(TypeError):
                implied_volatility(
                    self.engine, self.option, self.context, 10, **settings
                )
        with self.assertRaises(OverflowError):
            implied_volatility(
                self.engine, self.option, self.context, 10, max_iterations=2**31
            )
        with self.assertRaises(TypeError):
            implied_volatility(self.engine, self.option, self.context, True)
        for kwargs in (
            {"greeks": None},
            {"greeks": 1},
            {"greeks": "delta", "all_greeks": True},
        ):
            with self.assertRaises(TypeError):
                self.engine.price_with_greeks(self.option, self.context, **kwargs)
        for instrument in (
            EuropeanOption(**self.terms),
            CashOrNothingOption(**self.terms, payout=10),
        ):
            with self.assertRaises(UnsupportedOperationError):
                self.engine.price(instrument, self.context)
            with self.assertRaises(UnsupportedOperationError):
                self.engine.price_with_greeks(instrument, self.context, "delta")
            with self.assertRaises(UnsupportedOperationError):
                implied_volatility(self.engine, instrument, self.context, 10)
        with self.assertRaises(UnsupportedOperationError):
            AnalyticEuropeanEngine().price(self.option, self.context)
        with self.assertRaises(UnsupportedOperationError):
            implied_volatility(AnalyticEuropeanEngine(), self.option, self.context, 10)

    def test_live_quantlib_refinement_and_existing_error_budgets(self):
        american = importlib.import_module("quantlib_reference.american")
        oracle = importlib.import_module("quantlib_reference.oracle")
        oracle.require_version()
        config = oracle.load_fixture("american.json")
        profile = next(
            p
            for p in config["profiles"]
            if p["engine"] == "BjerksundStenslandVanillaEngine"
        )
        for scenario in config["scenarios"]:
            inputs = scenario["inputs"]
            option = AmericanOption(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_date=date.fromisoformat(inputs["effective_date"]),
                expiry_date=date.fromisoformat(inputs["expiry_date"]),
            )
            context = self.market(
                inputs["spot"],
                inputs["rate"],
                inputs["dividend"],
                inputs["volatility"],
                date.fromisoformat(inputs["valuation"]),
            )
            with ql.SavedSettings():
                reference, uncertainty = american.reference(inputs)
            names = sorted(reference.keys() - {"price"})
            result = self.engine.price_with_greeks(
                option, context, names, **profile["shifts"]
            )
            self.assertAlmostEqual(
                result.price, reference["price"], delta=profile["tolerances"]["price"]
            )
            for name in names:
                with self.subTest(case=scenario["case_id"], measure=name):
                    self.assertAlmostEqual(
                        getattr(result, name),
                        reference[name],
                        delta=profile["numerical_tolerances"][name]
                        + uncertainty[f"uncertainty_{name}"],
                    )
