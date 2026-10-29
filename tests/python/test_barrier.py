"""Single-barrier public API, event history and independent QuantLib comparisons."""

import math
import unittest
from datetime import UTC, date, datetime, timedelta

import QuantLib as ql
from kiyosi import InvalidInputError, RootNotBracketedError, UnsupportedOperationError
from kiyosi.instruments import BarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
)
from kiyosi.pricing import (
    AnalyticBarrierEngine,
    AnalyticEuropeanEngine,
    implied_volatility,
)


class BarrierTests(unittest.TestCase):
    def terms(self, **changes):
        return {
            "option_type": "call",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
            "barrier_level": 120,
            "barrier_type": "up_and_out",
        } | changes

    def context(self, spot=100, time=date(2025, 1, 1), **changes):
        return PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=spot,
            valuation_time=time,
            **changes,
        )

    def test_history_is_prior_and_paid_hit_rebate_does_not_recur(self):
        engine = AnalyticBarrierEngine()
        option = BarrierOption(**self.terms(rebate=7, rebate_timing="at_hit"))
        self.assertEqual(option.touch_state, None)
        self.assertEqual(option.observation_mode, "continuous")
        self.assertEqual(engine.price(option, self.context(120)), 7)
        with self.assertRaises(InvalidInputError) as error:
            engine.price(option, self.context(time=date(2025, 1, 2)))
        self.assertIs(type(error.exception), InvalidInputError)
        touched = option.replace(touch_state="touched")
        self.assertEqual(engine.price(touched, self.context(time=date(2025, 1, 2))), 0)
        self.assertIsNone(option.touch_state)
        self.assertEqual(touched.replace(touch_state=None), option)

    def assert_rejection(self, expected_error, action):
        with self.assertRaises(expected_error) as error:
            action()
        self.assertIs(type(error.exception), expected_error)

    def test_construction_representation_and_replacement_failures_preserve_owner(self):
        option = BarrierOption(**self.terms())
        for field in ("strike", "barrier_level", "rebate"):
            for value in (True, "100", None, complex(1, 0)):
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(TypeError),
                ):
                    BarrierOption(**self.terms(**{field: value}))
        for field in (
            "option_type",
            "barrier_type",
            "rebate_timing",
            "observation_mode",
            "touch_state",
        ):
            with self.subTest(field=field), self.assertRaises(ValueError):
                BarrierOption(**self.terms(**{field: "unknown"}))
            with self.subTest(field=field), self.assertRaises(TypeError):
                BarrierOption(**self.terms(**{field: True}))
        for values, expected_error in (
            ({"barrier_level": 0}, InvalidInputError),
            ({"barrier_level": float("nan"), "strike": -1}, InvalidInputError),
            ({"strike": 0}, InvalidInputError),
            ({"rebate": -1}, InvalidInputError),
            ({"rebate": float("inf")}, InvalidInputError),
            ({"expiry_date": date(2024, 1, 1)}, InvalidInputError),
            (
                {"barrier_type": "up_and_in", "rebate_timing": "at_hit"},
                InvalidInputError,
            ),
            ({"observation_mode": "scheduled"}, InvalidInputError),
            ({"observation_dates": [date(2025, 1, 2)]}, InvalidInputError),
            (
                {
                    "observation_mode": "scheduled",
                    "observation_dates": [date(2026, 1, 2)],
                },
                InvalidInputError,
            ),
            (
                {
                    "observation_mode": "scheduled",
                    "observation_dates": [date(2025, 1, 2)] * 2,
                },
                InvalidInputError,
            ),
        ):
            with self.subTest(values=values):
                self.assert_rejection(
                    expected_error,
                    lambda values=values: BarrierOption(**self.terms(**values)),
                )
        for fields, error in (
            ({"touch_state": "bad"}, ValueError),
            ({"touch_state": 1}, TypeError),
            ({"strike": 7}, TypeError),
        ):
            with self.assertRaises(error):
                option.replace(**fields)
            self.assertEqual(option, BarrierOption(**self.terms()))
        with self.assertRaises(AttributeError):
            option.touch_state = "touched"
        self.assertEqual(option.replace(), option)

    def test_expiry_and_prior_settlement_matrix(self):
        engine = AnalyticBarrierEngine()
        for kind in ("up_and_in", "up_and_out", "down_and_in", "down_and_out"):
            for state in ("touched", "untouched"):
                for level in (90, 100, 120):
                    for timing in (
                        ("at_expiry",)
                        if kind.endswith("_in")
                        else ("at_hit", "at_expiry")
                    ):
                        option = BarrierOption(
                            **self.terms(
                                strike=80,
                                barrier_type=kind,
                                barrier_level=level,
                                touch_state=state,
                                rebate=7,
                                rebate_timing=timing,
                            )
                        )
                        hit = state == "touched" or (
                            100 >= level if kind.startswith("up") else 100 <= level
                        )
                        rebate = 0 if state == "touched" and timing == "at_hit" else 7
                        expected = (
                            (20 if hit else 7)
                            if kind.endswith("_in")
                            else (rebate if hit else 20)
                        )
                        context = self.context(time=option.expiry_date)
                        with self.subTest(
                            kind=kind, state=state, level=level, timing=timing
                        ):
                            self.assertEqual(engine.price(option, context), expected)
                            result = engine.price_with_greeks(
                                option, context, all_greeks=True
                            )
                            self.assertTrue(
                                all(getattr(result, g) is None for g in GREEKS)
                            )
                            self.assert_rejection(
                                UnsupportedOperationError,
                                lambda option=option, context=context, expected=expected: (
                                    implied_volatility(
                                        engine, option, context, expected
                                    )
                                ),
                            )
        for timing in ("at_hit", "at_expiry"):
            option = BarrierOption(
                **self.terms(rebate=7, rebate_timing=timing, touch_state="touched")
            )
            context = self.context(time=date(2025, 1, 2))
            expected = 0 if timing == "at_hit" else 7 * math.exp(-0.05 * 364 / 365)
            self.assertAlmostEqual(engine.price(option, context), expected, delta=1e-12)
            self.assert_rejection(
                UnsupportedOperationError,
                lambda option=option, context=context, expected=expected: (
                    implied_volatility(engine, option, context, expected)
                ),
            )

    def test_scheduled_fixing_occurs_only_at_midnight_and_history_is_strict(self):
        engine = AnalyticBarrierEngine()
        fixing = date(2025, 1, 2)
        option = BarrierOption(
            **self.terms(
                observation_mode="scheduled", observation_dates=[fixing], rebate=7
            )
        )
        self.assert_rejection(
            UnsupportedOperationError, lambda: engine.price(option, self.context())
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: engine.price(
                option.replace(touch_state="touched"), self.context(time=fixing)
            ),
        )
        self.assertAlmostEqual(
            engine.price(option, self.context(120, fixing)),
            7 * math.exp(-0.05 * 364 / 365),
            delta=1e-12,
        )
        noon = datetime(2025, 1, 2, 12, tzinfo=UTC)
        self.assert_rejection(
            InvalidInputError, lambda: engine.price(option, self.context(150, noon))
        )
        untouched = option.replace(touch_state="untouched")
        vanilla = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=option.effective_date,
            expiry_date=option.expiry_date,
        )
        self.assertEqual(
            engine.price(untouched, self.context(150, noon)),
            AnalyticEuropeanEngine().price(vanilla, self.context(150, noon)),
        )
        dates = untouched.observation_dates
        dates.clear()
        self.assertEqual(untouched.observation_dates, [fixing])
        self.assertEqual(untouched.observation_schedule.dates, [fixing])
        self.assertAlmostEqual(untouched.mean_observation_year_fraction, 1 / 365)
        weekend = BarrierOption(
            **self.terms(
                observation_mode="scheduled",
                observation_dates=[date(2025, 1, 4)],
                touch_state="touched",
            )
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: engine.price(weekend, self.context(time=date(2025, 1, 5))),
        )

    def test_scheduled_bgk_domain_and_event_sensitive_greeks(self):
        engine = AnalyticBarrierEngine()
        for up in (False, True):
            level = 120 if up else 80
            kind = "up_and_out" if up else "down_and_out"
            scheduled = BarrierOption(
                **self.terms(
                    barrier_type=kind,
                    barrier_level=level,
                    observation_mode="scheduled",
                    observation_dates=[date(2026, 1, 1)],
                    touch_state="untouched",
                )
            )
            shift = (1 if up else -1) * 0.5825971579390107 * 0.2
            shifted = BarrierOption(
                **self.terms(
                    barrier_type=kind,
                    barrier_level=level * math.exp(shift),
                    touch_state="untouched",
                )
            )
            noon = datetime(2025, 1, 1, 12, tzinfo=UTC)
            context = self.context(time=noon)
            self.assertAlmostEqual(
                engine.price(scheduled, context),
                engine.price(shifted, context),
                delta=1e-11,
            )
            self.assert_rejection(
                UnsupportedOperationError,
                lambda scheduled=scheduled, up=up, noon=noon: engine.price(
                    scheduled, self.context(150 if up else 50, noon)
                ),
            )
            dense = BarrierOption(
                **self.terms(
                    barrier_type=kind,
                    barrier_level=level,
                    observation_mode="scheduled",
                    observation_dates=[
                        date(2025, 1, 1) + timedelta(days=i) for i in range(1, 366)
                    ],
                    touch_state="untouched",
                )
            )
            result = engine.price_with_greeks(
                dense,
                self.context(time=noon, calendar=all_days_calendar()),
                all_greeks=True,
            )
            for greek in ("theta", "charm", "color"):
                self.assertIsNone(getattr(result, greek))
            self.assertIsNotNone(result.delta)
        option = BarrierOption(**self.terms(touch_state="untouched", rebate=7))
        result = engine.price_with_greeks(option, self.context(120), all_greeks=True)
        self.assertTrue(all(getattr(result, g) is None for g in GREEKS))
        near = engine.price_with_greeks(option, self.context(119.995), all_greeks=True)
        self.assertIsNone(near.delta)
        self.assertIsNone(near.speed)
        self.assertIsNotNone(near.vega)

    def test_quantlib_price_matrix_and_independent_greeks(self):
        engine = AnalyticBarrierEngine()
        for kind in KINDS:
            for direction in ("call", "put"):
                for strike in (70, 100, 130):
                    for timing in (
                        ("at_expiry",)
                        if kind.endswith("_in")
                        else ("at_hit", "at_expiry")
                    ):
                        for rate, dividend, sigma in (
                            (0.05, 0.02, 0.2),
                            (-0.02, 0.07, 0.45),
                        ):
                            level = 120 if kind.startswith("up") else 80
                            terms = self.terms(
                                option_type=direction,
                                strike=strike,
                                barrier_type=kind,
                                barrier_level=level,
                                rebate=7,
                                rebate_timing=timing,
                                touch_state="untouched",
                            )
                            context = self.context().replace(
                                model_parameters=BlackScholesMertonParameters(
                                    risk_free_rate=rate,
                                    dividend_yield=dividend,
                                    volatility=sigma,
                                )
                            )
                            expected = reference(
                                terms, rate=rate, dividend=dividend, sigma=sigma
                            )
                            with self.subTest(
                                kind=kind,
                                direction=direction,
                                strike=strike,
                                timing=timing,
                                rate=rate,
                            ):
                                self.assertAlmostEqual(
                                    engine.price(BarrierOption(**terms), context),
                                    expected,
                                    delta=1e-8,
                                )
        terms = self.terms(
            barrier_type="down_and_out",
            barrier_level=80,
            rebate=7,
            touch_state="untouched",
        )
        option = BarrierOption(**terms)
        result = engine.price_with_greeks(option, self.context(), all_greeks=True)

        def f(spot=100, sigma=0.2, rate=0.05, days=0):
            return reference(terms, spot=spot, sigma=sigma, rate=rate, days=days)

        h, v, r = 0.03, 0.0001, 0.0001

        def delta(sigma=0.2, days=0):
            return (f(100 + h, sigma, days=days) - f(100 - h, sigma, days=days)) / (
                2 * h
            )

        def gamma(sigma=0.2, days=0):
            return (
                f(100 + h, sigma, days=days)
                - 2 * f(sigma=sigma, days=days)
                + f(100 - h, sigma, days=days)
            ) / h**2

        expected = {
            "delta": delta(),
            "gamma": gamma(),
            "speed": (f(100 + 2 * h) - 2 * f(100 + h) + 2 * f(100 - h) - f(100 - 2 * h))
            / (2 * h**3),
            "vega": (f(sigma=0.2 + v) - f(sigma=0.2 - v)) / (200 * v),
            "vanna": (delta(0.2 + v) - delta(0.2 - v)) / (200 * v),
            "zomma": (gamma(0.2 + v) - gamma(0.2 - v)) / (200 * v),
            "rho": (f(rate=0.05 + r) - f(rate=0.05 - r)) / (200 * r),
            "theta": f(days=1) - f(),
            "charm": delta(days=1) - delta(),
            "color": gamma(days=1) - gamma(),
        }
        for greek, value in expected.items():
            with self.subTest(greek=greek):
                self.assertAlmostEqual(
                    getattr(result, greek), value, delta=GREEK_TOLERANCES[greek]
                )
        sigma = implied_volatility(
            engine,
            option,
            self.context(),
            result.price,
            lower_bound=0.15,
            upper_bound=0.25,
        )
        self.assertAlmostEqual(sigma, 0.2, delta=1e-7)

    def test_tiny_volatility_and_extreme_monetary_scales(self):
        engine = AnalyticBarrierEngine()
        for kind in KINDS:
            up = kind.startswith("up")
            knock_in = kind.endswith("_in")
            for direction in ("call", "put"):
                for spot, sigma in ((1e16, 1e-20), (1e150, 1e-200)):
                    option = BarrierOption(
                        **self.terms(
                            option_type=direction,
                            strike=spot,
                            barrier_type=kind,
                            barrier_level=(1.2 if up else 0.8) * spot,
                        )
                    )
                    context = self.context(spot).replace(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0, dividend_yield=0, volatility=sigma
                        )
                    )
                    expected = 0 if knock_in else spot * sigma / math.sqrt(2 * math.pi)
                    actual = engine.price(option, context)
                    self.assertAlmostEqual(
                        actual / expected if expected else actual,
                        1 if expected else 0,
                        delta=1e-12,
                    )
                terms = self.terms(
                    option_type=direction,
                    barrier_type=kind,
                    barrier_level=120 if up else 80,
                )
                unit = engine.price(BarrierOption(**terms), self.context())
                for scale in (1e-300, 1e300):
                    scaled = terms | {
                        "strike": 100 * scale,
                        "barrier_level": terms["barrier_level"] * scale,
                    }
                    actual = engine.price(
                        BarrierOption(**scaled), self.context(100 * scale)
                    )
                    self.assertAlmostEqual(actual / scale, unit, delta=1e-9)

    def test_analysis_representation_errors_validation_and_concurrency(self):
        from concurrent.futures import ThreadPoolExecutor

        engine = AnalyticBarrierEngine()
        option = BarrierOption(**self.terms(touch_state="untouched"))
        context = self.context()
        for field, value, error in (
            ("spot_shift", True, TypeError),
            ("time_shift_days", 1.5, TypeError),
            ("time_shift_days", 2**40, OverflowError),
        ):
            with self.assertRaises(error):
                engine.price_with_greeks(option, context, "delta", **{field: value})
        for fields in (
            {"spot_shift": 0},
            {"volatility_shift": float("nan")},
            {"time_shift_days": 0},
        ):
            self.assert_rejection(
                InvalidInputError,
                lambda fields=fields: engine.price_with_greeks(
                    option, context, "delta", **fields
                ),
            )
        self.assert_rejection(
            InvalidInputError, lambda: engine.price_with_greeks(option, context, [])
        )
        for changes in (
            {"lower_bound": 0},
            {"max_iterations": 0},
            {"price_tolerance": -1},
        ):
            self.assert_rejection(
                InvalidInputError,
                lambda changes=changes: implied_volatility(
                    engine, option, context, 1, **changes
                ),
            )
        self.assert_rejection(
            RootNotBracketedError,
            lambda: implied_volatility(
                engine, option, context, -1, lower_bound=0.15, upper_bound=0.25
            ),
        )
        curve = RateTermStructure(
            reference_time=date(2025, 1, 1), times=[date(2026, 1, 1)], factors=[0.95]
        )
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=curve)
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: engine.price_with_greeks(option, curved, []),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_volatility(engine, option, curved, 1, lower_bound=0),
        )
        with ThreadPoolExecutor(4) as pool:
            prices = list(pool.map(lambda _: engine.price(option, context), range(16)))
        self.assertEqual(prices, [engine.price(option, context)] * 16)
        self.assertEqual(option, BarrierOption(**self.terms(touch_state="untouched")))


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
GREEK_TOLERANCES = {
    "delta": 1e-6,
    "gamma": 1e-7,
    "speed": 2e-7,
    "theta": 1e-4,
    "charm": 1e-5,
    "color": 3e-6,
    "vega": 1e-6,
    "vanna": 1e-7,
    "zomma": 2e-8,
    "rho": 1e-6,
}
KINDS = {
    "up_and_in": ql.Barrier.UpIn,
    "up_and_out": ql.Barrier.UpOut,
    "down_and_in": ql.Barrier.DownIn,
    "down_and_out": ql.Barrier.DownOut,
}


def reference(terms, spot=100, sigma=0.2, rate=0.05, dividend=0.02, days=0):
    with ql.SavedSettings():
        valuation = terms["effective_date"] + timedelta(days=days)
        today = ql.Date(valuation.day, valuation.month, valuation.year)
        end = terms["expiry_date"]
        expiry = ql.Date(end.day, end.month, end.year)
        ql.Settings.instance().evaluationDate = today
        process = ql.BlackScholesMertonProcess(
            ql.QuoteHandle(ql.SimpleQuote(spot)),
            ql.YieldTermStructureHandle(
                ql.FlatForward(today, dividend, ql.Actual365Fixed())
            ),
            ql.YieldTermStructureHandle(
                ql.FlatForward(today, rate, ql.Actual365Fixed())
            ),
            ql.BlackVolTermStructureHandle(
                ql.BlackConstantVol(
                    today, ql.NullCalendar(), sigma, ql.Actual365Fixed()
                )
            ),
        )
        payoff = ql.PlainVanillaPayoff(
            ql.Option.Call if terms["option_type"] == "call" else ql.Option.Put,
            terms["strike"],
        )

        def value(kind, rebate):
            contract = ql.BarrierOption(
                KINDS[kind],
                terms["barrier_level"],
                rebate,
                payoff,
                ql.EuropeanExercise(expiry),
            )
            contract.setPricingEngine(ql.AnalyticBarrierEngine(process))
            return contract.NPV()

        kind = terms["barrier_type"]
        rebate = terms.get("rebate", 0)
        if kind.endswith("_in") or terms.get("rebate_timing", "at_expiry") == "at_hit":
            result = value(kind, rebate)
        else:
            opposite = kind.replace("_out", "_in")
            result = (
                value(kind, 0)
                + rebate * math.exp(-rate * (end - valuation).days / 365)
                - (value(opposite, rebate) - value(opposite, 0))
            )
        return result
