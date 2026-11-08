"""Asian public contracts and independent geometric/Levy approximation references."""

import importlib
import math
import unittest
from datetime import datetime, timedelta
from fractions import Fraction
from functools import partial

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    ArithmeticAveragePriceOption,
    EuropeanOption,
    GeometricAveragePriceOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    AnalyticGeometricAveragePriceEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
    implied_volatility,
    numerical_greeks,
)
from support.history import dated as dated_history
from support.history import history_fields
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
PAIRS = (
    (GeometricAveragePriceOption, AnalyticGeometricAveragePriceEngine),
    (ArithmeticAveragePriceOption, TurnbullWakemanArithmeticAveragePriceEngine),
)
GREEKS = [
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
]


def terms(**changes):
    return {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "averaging_start_time": START,
        "expiry_time": END,
    } | changes


def market(at=START, rate=0.04, dividend=0.01, volatility=0.2, spot=100):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=at,
    )


class AsianTests(unittest.TestCase):
    def assert_rejection(self, expected, call, *args, **kwargs):
        with self.assertRaises(expected) as error:
            call(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def test_owned_history_replacement_and_native_representation_errors(self):
        for kind, engine_type in PAIRS:
            option = kind(**history_fields(**terms()))
            self.assertIsNone(option.realized_average)
            self.assertEqual(option.replace_history(), option)
            updated = option.replace_history(
                realized_average=Fraction(211, 2),
                history_time=(option.history_time or option.effective_time)
                if Fraction(211, 2) is not None
                else None,
            )
            self.assertEqual(updated.realized_average, 105.5)
            for name in terms():
                self.assertEqual(getattr(option, name), getattr(updated, name))
            self.assert_rejection(
                InvalidInputError, option.replace_history, realized_average=-1
            )
            self.assertIsNone(option.realized_average)
            for name in (*terms(), "realized_average"):
                with self.assertRaises(AttributeError):
                    setattr(option, name, 1)
            for name in ("strike", "realized_average"):
                for invalid in (True, False, "100", None, 1 + 0j, object()):
                    if name == "realized_average" and invalid is None:
                        self.assertIsNone(
                            kind(
                                **history_fields(**terms(realized_average=None))
                            ).realized_average
                        )
                        continue
                    with (
                        self.subTest(kind=kind, name=name, value=invalid),
                        self.assertRaises(TypeError),
                    ):
                        kind(**history_fields(**terms(**{name: invalid})))
                with self.assertRaises(OverflowError):
                    kind(**history_fields(**terms(**{name: 10**400})))
            for name in ("effective_time", "averaging_start_time", "expiry_time"):
                for invalid in (True, "2025-01-01", datetime(2025, 1, 1).date()):
                    with self.assertRaises(TypeError):
                        kind(**history_fields(**terms(**{name: invalid})))
            with self.assertRaises(ValueError):
                kind(**history_fields(**terms(option_type="CALL")))
            with self.assertRaises(TypeError):
                kind("call", 100, START, START, END)
            for invalid in (True, "100"):
                with self.assertRaises(TypeError):
                    option.replace_history(
                        realized_average=invalid,
                        history_time=(option.history_time or option.effective_time)
                        if invalid is not None
                        else None,
                    )
            with self.assertRaises(TypeError):
                option.replace_history(strike=101)
            other = PAIRS[1 if kind is GeometricAveragePriceOption else 0][0](**terms())
            engine = engine_type()
            for wrong in (
                other,
                EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_time=START,
                    expiry_time=END,
                ),
            ):
                with self.assertRaises(UnsupportedOperationError):
                    engine.price(dated_history(wrong, market()), market())
                with self.assertRaises(UnsupportedOperationError):
                    implied_volatility(
                        engine,
                        dated_history(wrong, market()),
                        market(),
                        5,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )

    def test_validation_schedule_history_life_and_single_fixing(self):
        averaging = datetime(2025, 2, 1)
        for kind, engine_type in PAIRS:
            for strike in (0, -1, math.nan, math.inf):
                self.assert_rejection(InvalidInputError, kind, **terms(strike=strike))
            for strike, average, expected in (
                (0, -1, InvalidInputError),
                (100, -1, InvalidInputError),
                (100, 0, InvalidInputError),
            ):
                self.assert_rejection(
                    expected,
                    kind,
                    **terms(
                        strike=strike,
                        realized_average=average,
                        effective_time=END,
                        expiry_time=START,
                    ),
                )
            for average in (-1, math.nan, math.inf):
                self.assert_rejection(
                    InvalidInputError,
                    kind,
                    **terms(realized_average=average),
                )
            for invalid in (START - timedelta(days=1), END + timedelta(days=1)):
                self.assert_rejection(
                    InvalidInputError,
                    kind,
                    **terms(averaging_start_time=invalid),
                )
            original = kind(**history_fields(**terms(averaging_start_time=averaging)))
            seasoned = original.replace_history(
                realized_average=110,
                history_time=original.history_time or original.effective_time,
            )
            engine = engine_type()
            for at in (START, averaging):
                self.assertGreater(
                    engine.price(dated_history(original, market(at)), market(at)), 0
                )
                self.assert_rejection(
                    InvalidInputError,
                    engine.price,
                    dated_history(seasoned, market(at)),
                    market(at),
                )
            for at in (averaging + timedelta(days=1), END):
                self.assertGreater(
                    engine.price(dated_history(seasoned, market(at)), market(at)), 0
                )
                self.assert_rejection(
                    MissingHistoryError,
                    engine.price,
                    dated_history(original, market(at)),
                    market(at),
                )
            for at in (
                START - timedelta(days=1),
                datetime(2026, 1, 1, microsecond=1),
            ):
                self.assert_rejection(
                    InvalidInputError,
                    engine.price,
                    dated_history(seasoned, market(at)),
                    market(at),
                )
            self.assertEqual(
                engine.price(dated_history(seasoned, market(END)), market(END)), 10
            )
            result = engine.price_with_greeks(
                dated_history(seasoned, market(END)), market(END), greeks="all"
            )
            assert_terminal_greeks(self, result, 0)
            self.assert_rejection(
                UnsupportedOperationError,
                implied_volatility,
                engine,
                dated_history(seasoned, market(END)),
                market(END),
                10,
                lower_bound=0.0,
                upper_bound=4.0,
            )
            for direction in ("call", "put"):
                single = kind(
                    **history_fields(
                        **terms(option_type=direction, averaging_start_time=END)
                    )
                )
                vanilla = EuropeanOption(
                    option_type=direction,
                    strike=100,
                    effective_time=START,
                    expiry_time=END,
                )
                for at in (START, END - timedelta(days=1), END):
                    self.assertEqual(
                        engine.price(dated_history(single, market(at)), market(at)),
                        AnalyticEuropeanEngine().price(
                            dated_history(vanilla, market(at)), market(at)
                        ),
                    )
                    self.assert_rejection(
                        InvalidInputError,
                        engine.price,
                        dated_history(
                            single.replace_history(
                                realized_average=110,
                                history_time=single.history_time
                                or single.effective_time,
                            ),
                            market(at),
                        ),
                        market(at),
                    )

    def test_greek_requests_stencils_and_solver_controls(self):
        for kind, engine_type in PAIRS:
            engine, option, context = (
                engine_type(),
                kind(**history_fields(**terms())),
                market(),
            )
            full = engine.price_with_greeks(
                dated_history(option, context), context, greeks="all"
            )
            for g in ("theta", "charm", "color"):
                self.assertIsNone(getattr(full, g))
            for request in (
                "delta",
                ["delta", "delta"],
                (g for g in ("delta", "vega")),
            ):
                selected = engine.price_with_greeks(
                    dated_history(option, context), context, request
                )
                self.assertEqual(selected.delta, full.delta)
                self.assertIsNone(selected.gamma)
            for settings, exception in (
                ({}, TypeError),
                ({"greeks": "Delta"}, ValueError),
                ({"greeks": True}, TypeError),
                ({"greeks": "delta", "spot_shift": True}, TypeError),
                ({"greeks": "delta", "time_shift": 1.5}, TypeError),
                ({"greeks": "delta", "time_shift": 2**40}, TypeError),
            ):
                with self.assertRaises(exception):
                    numerical_greeks(
                        engine, dated_history(option, context), context, **settings
                    )
            self.assert_rejection(
                InvalidInputError,
                partial(numerical_greeks, engine),
                option,
                context,
                "delta",
                spot_shift=0,
            )
            collapsed = numerical_greeks(
                engine,
                dated_history(option, context),
                context,
                "delta",
                spot_shift=math.ulp(0.0),
            )
            self.assertIsNone(collapsed.delta)
            seasoned = option.replace_history(
                realized_average=100,
                history_time=option.history_time or option.effective_time,
            )
            active = market(datetime(2025, 7, 1))
            result = engine.price_with_greeks(
                dated_history(seasoned, active), active, greeks="all"
            )
            self.assertTrue(all(getattr(result, g) is not None for g in GREEKS))
            self.assertAlmostEqual(
                implied_volatility(
                    engine,
                    dated_history(seasoned, active),
                    active,
                    result.price,
                    lower_bound=0.0,
                    upper_bound=4.0,
                ),
                0.2,
                delta=1e-7,
            )
            self.assert_rejection(
                RootNotBracketedError,
                implied_volatility,
                engine,
                dated_history(option, context),
                context,
                -1,
                lower_bound=0.0,
                upper_bound=4.0,
            )
            self.assert_rejection(
                InvalidInputError,
                implied_volatility,
                engine,
                dated_history(option, context),
                context,
                math.nan,
                lower_bound=0.0,
                upper_bound=4.0,
            )
            for field, value, exception in (
                ("max_iterations", True, TypeError),
                ("max_iterations", 2.5, TypeError),
                ("max_iterations", 2**40, OverflowError),
                ("lower_bound", True, TypeError),
            ):
                with self.assertRaises(exception):
                    implied_volatility(
                        engine,
                        dated_history(option, context),
                        context,
                        full.price,
                        **{"lower_bound": 0.0, "upper_bound": 4.0, field: value},
                    )
            self.assertIsNone(option.realized_average)
            self.assertEqual(context.model_parameters.volatility, 0.2)

    def test_arithmetic_solver_prices_only_the_requested_bracket(self):
        option = ArithmeticAveragePriceOption(**history_fields(**terms()))
        engine = TurnbullWakemanArithmeticAveragePriceEngine()
        quote = engine.price(dated_history(option, market()), market())
        self.assertAlmostEqual(
            implied_volatility(
                engine,
                dated_history(option, market(volatility=100)),
                market(volatility=100),
                quote,
                lower_bound=0.05,
                upper_bound=0.4,
            ),
            0.2,
            delta=1e-7,
        )

    def test_single_fixing_preserves_independent_asset_discount_at_extreme_rates(self):
        context = market(rate=1e16, dividend=0.02)
        expected = 100 * math.exp(-0.02)
        vanilla = EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        self.assertAlmostEqual(
            AnalyticEuropeanEngine().price(dated_history(vanilla, context), context),
            expected,
            delta=1e-12,
        )
        for kind, engine_type in PAIRS:
            option = kind(**history_fields(**terms(averaging_start_time=END)))
            self.assertAlmostEqual(
                engine_type().price(dated_history(option, context), context),
                expected,
                delta=1e-12,
            )

    def test_non_finite_small_volatility_moments_preserve_invalid_result(self):
        option = ArithmeticAveragePriceOption(**history_fields(**terms()))
        for sigma in (1e-6, 0.001):
            self.assert_rejection(
                ComputationFailureError,
                TurnbullWakemanArithmeticAveragePriceEngine().price,
                dated_history(option, market(rate=400, dividend=0, volatility=sigma)),
                market(rate=400, dividend=0, volatility=sigma),
            )

    def test_curve_applicability_coverage_and_expiry_order(self):
        middle = START + timedelta(days=182)
        curve = RateTermStructure(
            reference_time=START,
            times=[middle, END],
            factors=[math.exp(-0.02 * 182 / 365), math.exp(-0.05)],
        )
        context = market(rate=curve, dividend=0)
        option = GeometricAveragePriceOption(**history_fields(**terms()))
        h = 182 / 365
        second = (0.05 - 0.02 * h) / (1 - h)
        drift = 0.02 * (h - h * h / 2) + second * (1 - h) ** 2 / 2 - 0.04 / 4
        width = math.sqrt(0.04 / 3)

        def cdf(x):
            return math.erfc(-x / math.sqrt(2)) / 2

        expected = (
            math.exp(-0.05)
            * 100
            * (
                math.exp(drift + width * width / 2) * cdf(drift / width + width)
                - cdf(drift / width)
            )
        )
        self.assertAlmostEqual(
            AnalyticGeometricAveragePriceEngine().price(
                dated_history(option, context), context
            ),
            expected,
            delta=1e-11,
        )
        engine = TurnbullWakemanArithmeticAveragePriceEngine()
        arithmetic = ArithmeticAveragePriceOption(**history_fields(**terms()))
        self.assert_rejection(
            UnsupportedOperationError,
            engine.price,
            dated_history(arithmetic, context),
            context,
        )
        self.assert_rejection(
            UnsupportedOperationError,
            engine.price_with_greeks,
            dated_history(arithmetic, context),
            context,
            [],
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            dated_history(
                arithmetic.replace_history(
                    realized_average=110,
                    history_time=arithmetic.history_time or arithmetic.effective_time,
                ),
                context,
            ),
            context,
        )
        self.assertEqual(
            engine.price(
                dated_history(
                    arithmetic.replace_history(
                        realized_average=110,
                        history_time=arithmetic.history_time
                        or arithmetic.effective_time,
                    ),
                    context.replace(valuation_time=END),
                ),
                context.replace(valuation_time=END),
            ),
            10,
        )
        short = RateTermStructure(reference_time=START, times=[middle], factors=[0.99])
        for kind, engine_type in PAIRS:
            self.assert_rejection(
                InvalidInputError,
                engine_type().price,
                dated_history(kind(**history_fields(**terms())), market(rate=short)),
                market(rate=short),
            )
        dated_flat = RateTermStructure.flat(rate=0.04, reference_time=START)
        self.assertEqual(
            engine.price(
                dated_history(arithmetic, market(rate=dated_flat)),
                market(rate=dated_flat),
            ),
            engine.price(dated_history(arithmetic, market()), market()),
        )

    def test_live_quantlib_geometric_and_same_levy_approximation(self):

        asian = importlib.import_module("quantlib_reference.asian")
        oracle = importlib.import_module("quantlib_reference.oracle")
        differences = importlib.import_module("price_differences").price_differences
        oracle.require_version()
        with ql.SavedSettings():
            asian.check_bindings()
            rows = list(asian.rows())
        self.assertEqual(len(rows), 24)
        for row in rows:
            inputs = row["inputs"]
            kind, engine_type = PAIRS[0 if inputs["averaging"] == "geometric" else 1]
            option = kind(
                **history_fields(
                    **terms(
                        option_type=inputs["option"],
                        strike=inputs["strike"],
                        effective_time=datetime.fromisoformat(inputs["effective_time"]),
                        averaging_start_time=datetime.fromisoformat(
                            inputs["averaging_start_time"]
                        ),
                        expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
                        realized_average=inputs["realized_average"] or None,
                    )
                )
            )
            context = market(
                datetime.fromisoformat(inputs["valuation"]),
                inputs["rate"],
                inputs["dividend"],
                inputs["volatility"],
                inputs["spot"],
            )
            engine = engine_type()
            with self.subTest(case=row["case_id"]):
                self.assertAlmostEqual(
                    engine.price(dated_history(option, context), context),
                    row["outputs"]["price"],
                    delta=row["tolerances"]["price"],
                )
                requested = sorted(set(row["outputs"]) - {"price"})
                if requested:
                    joint = numerical_greeks(
                        engine,
                        dated_history(option, context),
                        context,
                        requested,
                        spot_shift=inputs["spot_shift"],
                    )
                    numeric = differences(
                        engine,
                        option,
                        context,
                        requested,
                        spot_shift=inputs["spot_shift"],
                    )
                    for name in requested:
                        budget = (
                            inputs[f"numerical_tolerance_{name}"]
                            + inputs[f"uncertainty_{name}"]
                        )
                        self.assertAlmostEqual(
                            getattr(joint, name), row["outputs"][name], delta=budget
                        )
                        self.assertAlmostEqual(
                            getattr(numeric, name), row["outputs"][name], delta=budget
                        )
