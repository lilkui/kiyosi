"""Digital contracts shared by the Rust implementation."""

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
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticDigitalEngine,
    AnalyticEuropeanEngine,
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


class DigitalTests(unittest.TestCase):
    def setUp(self):
        self.terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        self.context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=self.terms["effective_date"],
        )
        self.engine = AnalyticDigitalEngine()

    def options(self, **changes):
        terms = dict(self.terms, **changes)
        return (CashOrNothingOption(**terms, payout=10), AssetOrNothingOption(**terms))

    def assert_rejection(self, expected, call, *args, **kwargs):
        with self.assertRaises(expected) as error:
            call(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def test_prices_greeks_and_volatility_recovery(self):
        for cls, expected, extra in (
            (CashOrNothingOption, 4.945810910532236, {"payout": 10}),
            (AssetOrNothingOption, 58.6851146134764, {}),
        ):
            option = cls(**self.terms, **extra)
            result = self.engine.price_with_greeks(
                option, self.context, all_greeks=True
            )
            self.assertAlmostEqual(result.price, expected, delta=1e-12)
            self.assertAlmostEqual(
                implied_volatility(
                    self.engine,
                    option,
                    self.context,
                    result.price,
                    lower_bound=0.1,
                    upper_bound=0.22,
                ),
                0.2,
                delta=1e-7,
            )
            for name in GREEKS:
                selected = self.engine.price_with_greeks(option, self.context, name)
                self.assertEqual(getattr(selected, name), getattr(result, name))
                self.assertEqual(selected.require(name), getattr(result, name))
                self.assertIsNone(
                    getattr(selected, "gamma" if name == "delta" else "delta")
                )
            self.assertEqual(option.option_type, "call")
            self.assertEqual(option.strike, 100)
            self.assertEqual(option.effective_date, self.terms["effective_date"])
            self.assertEqual(option.expiry_date, self.terms["expiry_date"])
            self.assertEqual(option, cls(**self.terms, **extra))
            with self.assertRaises(AttributeError):
                option.strike = 105

    def test_live_quantlib_prices_greeks_and_scalar_differences(self):

        digital = importlib.import_module("quantlib_reference.digital")
        oracle = importlib.import_module("quantlib_reference.oracle")
        differences = importlib.import_module("price_differences").price_differences
        oracle.require_version()
        config = digital.configuration()
        for identifier, inputs in digital.scenarios(config):
            cls = (
                CashOrNothingOption
                if inputs["payoff"] == "cash"
                else AssetOrNothingOption
            )
            option = cls(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_date=date.fromisoformat(inputs["effective_date"]),
                expiry_date=date.fromisoformat(inputs["expiry_date"]),
                **({"payout": inputs["payout"]} if inputs["payoff"] == "cash" else {}),
            )
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=inputs["rate"],
                    dividend_yield=inputs["dividend"],
                    volatility=inputs["volatility"],
                ),
                spot_price=inputs["spot"],
                valuation_time=date.fromisoformat(inputs["valuation"]),
            )
            with ql.SavedSettings():
                reference, uncertainty = oracle.reference(inputs, digital.STABILITY)
            # Existing references omit time Greeks when a whole-day stencil reaches expiry.
            smooth = (option.expiry_date - context.valuation_time.date()).days > 2
            names = (
                tuple(reference.keys() - {"price"}) if smooth else ("delta", "gamma")
            )
            result = self.engine.price_with_greeks(option, context, names)
            for name, expected in reference.items():
                if name != "price" and name not in names:
                    continue
                with self.subTest(case=identifier, measure=name):
                    self.assertAlmostEqual(
                        getattr(result, name),
                        expected,
                        delta=config["profile_defaults"]["tolerances"][name]
                        + uncertainty[f"uncertainty_{name}"],
                    )
            if smooth:
                for bump in (0.01, 0.02):
                    numeric = differences(
                        self.engine, option, context, names, spot_shift=bump
                    )
                    for name in names:
                        with self.subTest(case=identifier, measure=name, bump=bump):
                            self.assertAlmostEqual(
                                getattr(numeric, name),
                                reference[name],
                                delta=config["profile_defaults"][
                                    "numerical_tolerances"
                                ][name]
                                + uncertainty[f"uncertainty_{name}"],
                            )

    def test_strict_expiry_payoffs_and_valuation_boundaries(self):
        end = self.terms["expiry_date"]
        for direction in ("call", "put"):
            for spot in (
                99,
                math.nextafter(100, 0),
                100,
                math.nextafter(100, 101),
                101,
            ):
                context = self.context.replace(valuation_time=end, spot_price=spot)
                for option in self.options(option_type=direction):
                    pays = spot > 100 if direction == "call" else spot < 100
                    expected = (
                        (spot if isinstance(option, AssetOrNothingOption) else 10)
                        if pays
                        else 0
                    )
                    result = self.engine.price_with_greeks(
                        option, context, all_greeks=True
                    )
                    self.assertEqual(self.engine.price(option, context), expected)
                    self.assertEqual(result.price, expected)
                    for name in GREEKS:
                        self.assertIsNone(getattr(result, name))
                    self.assert_rejection(
                        UnsupportedOperationError,
                        implied_volatility,
                        self.engine,
                        option,
                        context,
                        expected,
                    )
        for option in self.options():
            for time in (
                datetime(2024, 12, 31, 23, 59, 59, 999999, tzinfo=UTC),
                datetime(2026, 1, 1, 0, 0, 0, 1, tzinfo=UTC),
            ):
                outside = self.context.replace(valuation_time=time)
                self.assert_rejection(
                    InvalidInputError, self.engine.price, option, outside
                )
                self.assert_rejection(
                    InvalidInputError,
                    self.engine.price_with_greeks,
                    option,
                    outside,
                    [],
                )
                self.assert_rejection(
                    InvalidInputError,
                    implied_volatility,
                    self.engine,
                    option,
                    outside,
                    0,
                )

    def test_constructor_domains_and_native_representation_errors(self):
        for cls in (CashOrNothingOption, AssetOrNothingOption):
            extra = {"payout": 10} if cls is CashOrNothingOption else {}
            for strike in (0, -1, math.nan, math.inf):
                self.assert_rejection(
                    InvalidInputError,
                    cls,
                    **dict(self.terms, strike=strike),
                    **extra,
                )
            self.assert_rejection(
                InvalidInputError,
                cls,
                **dict(self.terms, expiry_date=date(2024, 1, 1)),
                **extra,
            )
            for field in ("strike", *extra):
                for value in (True, "1", None, 1j):
                    with (
                        self.subTest(field=field, value=value),
                        self.assertRaises(TypeError),
                    ):
                        cls(**(self.terms | extra | {field: value}))
                with self.assertRaises(OverflowError):
                    cls(**(self.terms | extra | {field: 10**1000}))
            for value in (True, 1, None):
                with self.assertRaises(TypeError):
                    cls(**dict(self.terms, option_type=value), **extra)
            with self.assertRaises(ValueError):
                cls(**dict(self.terms, option_type="Call"), **extra)
            for field in ("effective_date", "expiry_date"):
                for value in ("2025-01-01", None, datetime(2025, 1, 1)):
                    with self.assertRaises(TypeError):
                        cls(**dict(self.terms, **{field: value}), **extra)
            with self.assertRaises(TypeError):
                cls("call", 100, date(2025, 1, 1), date(2026, 1, 1))
        for payout in (0, -1, math.nan, math.inf):
            self.assert_rejection(
                InvalidInputError,
                CashOrNothingOption,
                **self.terms,
                payout=payout,
            )
        # All representation checks precede the core's payout-first domain validation.
        with self.assertRaises(TypeError):
            CashOrNothingOption(**dict(self.terms, strike=True), payout=0)
        self.assert_rejection(
            InvalidInputError,
            CashOrNothingOption,
            **dict(self.terms, strike=0),
            payout=0,
        )
        valid = CashOrNothingOption(
            **dict(self.terms, strike=Fraction(201, 2)), payout=Fraction(3, 2)
        )
        self.assertEqual(valid.strike, 100.5)
        self.assertEqual(valid.payout, 1.5)

    def test_requests_settings_solver_errors_and_failure_state(self):
        for option in self.options():
            quote = self.engine.price(option, self.context)
            for changes in (
                {},
                {"all_greeks": 1},
                {"greeks": True},
                {"greeks": [1]},
                {"greeks": "delta", "all_greeks": True},
            ):
                with self.assertRaises(TypeError):
                    self.engine.price_with_greeks(option, self.context, **changes)
            with self.assertRaises(ValueError):
                self.engine.price_with_greeks(option, self.context, "Delta")
            self.assert_rejection(
                InvalidInputError,
                self.engine.price_with_greeks,
                option,
                self.context,
                [],
            )
            for field in (
                "spot_shift",
                "volatility_shift",
                "rate_shift",
                "time_shift_days",
            ):
                for value in (True, "1", None):
                    with self.assertRaises(TypeError):
                        self.engine.price_with_greeks(
                            option, self.context, "delta", **{field: value}
                        )
                for value in (0, -1):
                    self.assert_rejection(
                        InvalidInputError,
                        self.engine.price_with_greeks,
                        option,
                        self.context,
                        "delta",
                        **{field: value},
                    )
            with self.assertRaises(TypeError):
                self.engine.price_with_greeks(
                    option, self.context, "delta", time_shift_days=1.5
                )
            with self.assertRaises(OverflowError):
                self.engine.price_with_greeks(
                    option, self.context, "delta", time_shift_days=2**40
                )
            for settings in (
                {"lower_bound": 0},
                {"lower_bound": 4},
                {"upper_bound": math.inf},
                {"max_iterations": 0},
                {"price_tolerance": 0},
                {"parameter_tolerance": math.nan},
            ):
                self.assert_rejection(
                    InvalidInputError,
                    implied_volatility,
                    self.engine,
                    option,
                    self.context,
                    quote,
                    **settings,
                )
            for value, expected_error in (
                (math.nan, InvalidInputError),
                (-1, RootNotBracketedError),
                (1000, RootNotBracketedError),
            ):
                self.assert_rejection(
                    expected_error,
                    implied_volatility,
                    self.engine,
                    option,
                    self.context,
                    value,
                )
            for settings in ({"lower_bound": True}, {"max_iterations": 1.0}):
                with self.assertRaises(TypeError):
                    implied_volatility(
                        self.engine, option, self.context, quote, **settings
                    )
            self.assertEqual(self.engine.price(option, self.context), quote)
            self.assertEqual(self.context.model_parameters.volatility, 0.2)
            with self.assertRaises(TypeError):
                AnalyticEuropeanEngine().price(option, self.context)
        with self.assertRaises(TypeError):
            self.engine.price(EuropeanOption(**self.terms), self.context)

    def test_curves_coverage_time_knots_and_concurrency(self):
        start, end = self.terms["effective_date"], self.terms["expiry_date"]
        knot = start + timedelta(days=180)
        curve = RateTermStructure(
            reference_time=start,
            times=[knot, end],
            factors=[
                math.exp(-0.02 * 180 / 365),
                math.exp(-0.02 * 180 / 365 - 0.08 * 185 / 365),
            ],
        )
        model = self.context.model_parameters.replace(risk_free_rate=curve)
        context = self.context.replace(model_parameters=model, valuation_time=knot)
        flat = context.replace(
            model_parameters=model.replace(
                risk_free_rate=RateTermStructure.flat(0.08, reference_time=knot)
            )
        )
        short = RateTermStructure(reference_time=start, times=[knot], factors=[0.99])
        uncovered = self.context.replace(
            model_parameters=model.replace(dividend_yield=short)
        )
        for option in self.options():
            actual = self.engine.price_with_greeks(option, context, all_greeks=True)
            expected = self.engine.price_with_greeks(option, flat, all_greeks=True)
            for name in ("price", *GREEKS):
                self.assertAlmostEqual(
                    getattr(actual, name), getattr(expected, name), delta=1e-10
                )
            self.assert_rejection(
                InvalidInputError, self.engine.price, option, uncovered
            )
            self.assert_rejection(
                InvalidInputError,
                implied_volatility,
                self.engine,
                option,
                uncovered,
                0,
            )
            with ThreadPoolExecutor(max_workers=4) as pool:
                prices = list(
                    pool.map(
                        lambda _, option=option: self.engine.price(option, context),
                        range(16),
                    )
                )
            self.assertEqual(prices, [actual.price] * 16)

    def test_scaled_discounts_and_overflowing_settlements(self):
        for rate in (710, 740, 750, -710, -750):
            amount = 1e300 if rate > 0 else 1e-300
            context = self.context.replace(
                spot_price=amount,
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=rate, volatility=0.2
                ),
            )
            discounted = math.exp(math.log(amount) - rate)
            for direction, sign in (("call", 1), ("put", -1)):
                terms = dict(self.terms, option_type=direction, strike=amount)
                for option, argument in (
                    (CashOrNothingOption(**terms, payout=amount), sign * 0.1),
                    (AssetOrNothingOption(**terms), -sign * 0.1),
                ):
                    expected = discounted * 0.5 * math.erfc(argument / math.sqrt(2))
                    self.assertTrue(
                        math.isclose(
                            self.engine.price(option, context), expected, rel_tol=2e-12
                        )
                    )
        for direction in ("call", "put"):
            for asset in (False, True):
                for overflow in (False, True):
                    rate = -1 if overflow else -0.05
                    amount = 1.75e308
                    spot = amount if asset else 100
                    context = self.context.replace(
                        spot_price=spot,
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate + 0.02 if asset else rate,
                            dividend_yield=rate if asset else rate - 0.06,
                            volatility=0.2,
                        ),
                    )
                    terms = dict(self.terms, option_type=direction, strike=spot)
                    option = (
                        AssetOrNothingOption(**terms)
                        if asset
                        else CashOrNothingOption(**terms, payout=amount)
                    )
                    if overflow:
                        self.assert_rejection(
                            ComputationFailureError,
                            self.engine.price,
                            option,
                            context,
                        )
                    else:
                        probability = 0.5 * math.erfc(
                            (-0.2 if direction == "call" else 0.2) / math.sqrt(2)
                        )
                        self.assertTrue(
                            math.isclose(
                                self.engine.price(option, context),
                                amount * (math.exp(0.05) * probability),
                                rel_tol=1e-10,
                            )
                        )
        for direction in ("call", "put"):
            context = self.context.replace(
                spot_price=1e-300,
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=-710, dividend_yield=-710, volatility=0.2
                ),
            )
            terms = dict(
                self.terms,
                option_type=direction,
                strike=1e-304 if direction == "call" else 1e-296,
            )
            for option in (
                CashOrNothingOption(**terms, payout=1e-300),
                AssetOrNothingOption(**terms),
            ):
                self.assertTrue(
                    math.isclose(
                        self.engine.price(option, context),
                        math.exp(math.log(1e-300) + 710),
                        rel_tol=1e-12,
                    )
                )

    def test_selected_overflow_tiny_volatility_and_scaled_tail_greeks(self):
        option = CashOrNothingOption(**dict(self.terms, strike=1e-200), payout=1e100)
        context = self.context.replace(
            spot_price=1e-200,
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
        )
        result = self.engine.price_with_greeks(option, context, "delta")
        self.assertTrue(
            math.isclose(result.delta, 1.984762737385059e300, rel_tol=1e-12)
        )
        self.assertIsNone(result.gamma)
        self.assert_rejection(
            ComputationFailureError,
            self.engine.price_with_greeks,
            option,
            context,
            "gamma",
        )
        for sigma in (1e-200, math.ulp(0.0)):
            context = self.context.replace(
                valuation_time=datetime(2026, 1, 1, tzinfo=UTC)
                - timedelta(microseconds=1),
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=sigma
                ),
            )
            for direction in ("call", "put"):
                for option in self.options(option_type=direction):
                    self.assertEqual(
                        self.engine.price(option, context),
                        50 if isinstance(option, AssetOrNothingOption) else 5,
                    )
                    if sigma == 1e-200:
                        result = self.engine.price_with_greeks(option, context, "gamma")
                        expected_sign = (
                            1 if isinstance(option, AssetOrNothingOption) else -1
                        ) * (1 if direction == "call" else -1)
                        self.assertTrue(math.isfinite(result.gamma))
                        self.assertEqual(math.copysign(1, result.gamma), expected_sign)
                        self.assertGreater(abs(result.gamma), 1e200)
                    else:
                        self.assert_rejection(
                            ComputationFailureError,
                            self.engine.price_with_greeks,
                            option,
                            context,
                            "gamma",
                        )
        # Existing independent high-precision tail regressions, r = q = -710.
        for direction, log_strike, expected in (
            ("call", 8, 8.923870780328184e-38),
            ("put", -8, -2.9638565030696756e-41),
        ):
            for rate in (-710, -700):
                for spot in (1, 1e-200):
                    option = AssetOrNothingOption(
                        **dict(
                            self.terms,
                            option_type=direction,
                            strike=spot * math.exp(log_strike),
                        )
                    )
                    context = self.context.replace(
                        spot_price=spot,
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate, dividend_yield=rate, volatility=0.2
                        ),
                    )
                    result = self.engine.price_with_greeks(option, context, "delta")
                    self.assertTrue(
                        math.isclose(
                            result.delta,
                            expected * math.exp(-rate - 710),
                            rel_tol=1e-10,
                        )
                    )

    def test_native_speed_large_scales_and_single_bump_availability(self):
        context = self.context.replace(spot_price=100000)
        for option, expected in (
            (
                CashOrNothingOption(**dict(self.terms, strike=100000), payout=1e8),
                -4.204659661267558e-6,
            ),
            (
                AssetOrNothingOption(**dict(self.terms, strike=100000)),
                -4.631047683255255e-9,
            ),
        ):
            result = self.engine.price_with_greeks(option, context, "speed")
            self.assertTrue(math.isclose(result.speed, expected, rel_tol=1e-7))
            self.assertIsNone(result.gamma)
        option = CashOrNothingOption(**dict(self.terms, strike=1.5), payout=1)
        result = self.engine.price_with_greeks(
            option, self.context.replace(spot_price=1.5), "speed", spot_shift=1
        )
        self.assertAlmostEqual(result.speed, -0.07172406990163582, delta=1e-14)

    def test_expiry_reaching_time_stencils_use_scalar_derivatives_on_both_sides(self):

        differences = importlib.import_module("price_differences").price_differences
        context = self.context.replace(spot_price=1, valuation_time=date(2025, 12, 31))
        for direction in ("call", "put"):
            for option in self.options(option_type=direction, strike=1):
                actual = self.engine.price_with_greeks(
                    option, context, ["charm", "color"]
                )
                expected = differences(self.engine, option, context, ["charm", "color"])
                self.assertAlmostEqual(actual.charm, expected.charm, delta=1e-9)
                self.assertAlmostEqual(actual.color, expected.color, delta=1e-7)
