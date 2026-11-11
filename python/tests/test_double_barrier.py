"""Double-barrier contracts and independent analytic references."""

import math
import unittest
from datetime import datetime
from itertools import product

import QuantLib as ql
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    UnsupportedOperationError,
)
from kiyosi.instruments import DoubleBarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
)
from kiyosi.pricing import (
    AnalyticDoubleBarrierEngine,
    AnalyticEuropeanEngine,
    numerical_greeks,
)
from support.history import dated as dated_history
from support.history import history_fields
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


def contract(**changes):
    return DoubleBarrierOption(
        **history_fields(
            **(
                {
                    "option_type": "call",
                    "strike": 100,
                    "effective_time": START,
                    "expiry_time": END,
                    "lower_barrier": 80,
                    "upper_barrier": 120,
                    "barrier_type": "knock_out",
                }
                | changes
            )
        )
    )


def market(spot=100, time=START, rate=0.04, dividend=0.01, volatility=0.2):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=time,
    )


def reference(direction, kind, spot=100, volatility=0.2, series=20):
    saved = ql.SavedSettings()
    try:
        start, end = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
        ql.Settings.instance().evaluationDate = start
        dc = ql.Actual365Fixed()
        process = ql.BlackScholesMertonProcess(
            ql.QuoteHandle(ql.SimpleQuote(spot)),
            ql.YieldTermStructureHandle(ql.FlatForward(start, 0.01, dc)),
            ql.YieldTermStructureHandle(ql.FlatForward(start, 0.04, dc)),
            ql.BlackVolTermStructureHandle(
                ql.BlackConstantVol(start, ql.NullCalendar(), volatility, dc)
            ),
        )
        option = ql.DoubleBarrierOption(
            ql.DoubleBarrier.KnockIn
            if kind == "knock_in"
            else ql.DoubleBarrier.KnockOut,
            80,
            120,
            0,
            ql.PlainVanillaPayoff(
                ql.Option.Call if direction == "call" else ql.Option.Put, 100
            ),
            ql.EuropeanExercise(end),
        )
        option.setPricingEngine(ql.AnalyticDoubleBarrierEngine(process, series))
        return option.NPV()
    finally:
        del saved


class DoubleBarrierTests(unittest.TestCase):
    def test_prices_greeks_and_parity_match_live_quantlib(self):
        engine = AnalyticDoubleBarrierEngine()
        for direction, kind in product(("call", "put"), ("knock_in", "knock_out")):
            option = contract(option_type=direction, barrier_type=kind)
            expected = reference(dated_history(direction, kind), kind)
            self.assertAlmostEqual(
                expected,
                reference(dated_history(direction, kind), kind, series=10),
                delta=1e-10,
            )
            result = engine.price_with_greeks(
                dated_history(option, market()), market(), ["delta", "gamma"]
            )
            self.assertAlmostEqual(result.price, expected, delta=1e-9)
            low, high = [
                reference(dated_history(direction, kind), kind, spot=s)
                for s in (99.99, 100.01)
            ]
            self.assertAlmostEqual(result.delta, (high - low) / 0.02, delta=1e-7)
            self.assertAlmostEqual(
                result.gamma, (high - 2 * expected + low) / 0.01**2, delta=1e-6
            )
            self.assertIsNone(result.vega)
            other = contract(
                option_type=direction,
                barrier_type="knock_in" if kind == "knock_out" else "knock_out",
            )
            vanilla = EuropeanOption(
                option_type=direction, strike=100, effective_time=START, expiry_time=END
            )
            self.assertAlmostEqual(
                result.price + engine.price(dated_history(other, market()), market()),
                AnalyticEuropeanEngine().price(
                    dated_history(vanilla, market()), market()
                ),
                delta=1e-9,
            )

    def assert_rejection(self, expected_error, action):
        with self.assertRaises(expected_error) as caught:
            action()
        self.assertIs(type(caught.exception), expected_error)

    def test_defaults_representations_core_validation_and_owned_replacement(self):
        from kiyosi.instruments import BarrierOption

        option = contract()
        single = BarrierOption(
            option_type="call",
            strike=100,
            effective_time=START,
            expiry_time=END,
            barrier_level=80,
            barrier_type="down_and_out",
        )
        for field in (
            "rebate",
            "rebate_timing",
            "observation_times",
            "touch_state",
        ):
            self.assertEqual(getattr(option, field), getattr(single, field))
        self.assertEqual(
            (option.lower_barrier, option.upper_barrier, option.barrier_type),
            (80, 120, "knock_out"),
        )
        self.assertEqual(
            (
                option.option_type,
                option.strike,
                option.effective_time,
                option.expiry_time,
            ),
            ("call", 100, START, END),
        )
        for field in ("strike", "lower_barrier", "upper_barrier", "rebate"):
            for value in (True, "100", None, 1 + 0j):
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(TypeError),
                ):
                    contract(**{field: value})
            with self.assertRaises(OverflowError):
                contract(**{field: 10**1000})
        for field in (
            "option_type",
            "barrier_type",
            "rebate_timing",
            "touch_state",
        ):
            with self.subTest(field=field), self.assertRaises(ValueError):
                contract(**{field: "unknown"})
            with self.assertRaises(TypeError):
                contract(**{field: True})
        for field in ("effective_time", "expiry_time"):
            with self.assertRaises(TypeError):
                contract(**{field: "2025-01-01"})
        for values, expected_error in [
            ({"lower_barrier": 0}, InvalidInputError),
            ({"lower_barrier": math.nan}, InvalidInputError),
            ({"upper_barrier": math.inf}, InvalidInputError),
            ({"upper_barrier": 80}, InvalidInputError),
            ({"lower_barrier": 130}, InvalidInputError),
            ({"strike": 0}, InvalidInputError),
            ({"expiry_time": datetime(2024, 1, 1)}, InvalidInputError),
            ({"rebate": -1}, InvalidInputError),
            ({"rebate": math.nan}, InvalidInputError),
            (
                {"barrier_type": "knock_in", "rebate_timing": "at_hit"},
                InvalidInputError,
            ),
            ({"observation_times": []}, InvalidInputError),
            (
                {"observation_times": [END, END]},
                InvalidInputError,
            ),
            (
                {"observation_times": [END, START]},
                InvalidInputError,
            ),
            (
                {
                    "observation_times": [datetime(2026, 1, 2)],
                },
                InvalidInputError,
            ),
        ]:
            with self.subTest(values=values):
                self.assert_rejection(
                    expected_error, lambda values=values: contract(**values)
                )
        for changes, error in [
            ({"touch_state": "unknown"}, ValueError),
            ({"touch_state": False}, TypeError),
            ({"strike": 90}, TypeError),
        ]:
            with self.assertRaises(error):
                option.replace_history(**changes)
            self.assertEqual(option, contract())
        known = option.replace_history(
            touch_state="untouched",
            history_time=option.history_time or option.expiry_time,
        )
        self.assertEqual(known.replace_history(), known)
        self.assertEqual(known.replace_history(touch_state=None), option)
        self.assertIsNone(option.touch_state)
        scheduled = contract(observation_times=(d for d in [START, END]))
        dates = list(scheduled.observation_times)
        dates.clear()
        self.assertEqual(scheduled.observation_times, (START, END))
        self.assertEqual(scheduled.observation_schedule.times, (START, END))
        with self.assertRaises(AttributeError):
            option.upper_barrier = 130

    def test_current_touch_history_rebates_expiry_and_native_time_boundaries(self):
        from datetime import datetime, timedelta

        engine = AnalyticDoubleBarrierEngine()
        later = market(time=START + timedelta(days=1))
        for spot, timing in product((79, 80, 120, 121), ("at_hit", "at_expiry")):
            option = contract(rebate=3, rebate_timing=timing)
            expected = 3 if timing == "at_hit" else 3 * math.exp(-0.04)
            self.assertAlmostEqual(
                engine.price(dated_history(option, market(spot)), market(spot)),
                expected,
                delta=1e-12,
            )
            past = option.replace_history(
                touch_state="touched",
                history_time=option.expiry_time,
            )
            expected = 0 if timing == "at_hit" else 3 * math.exp(-0.04 * 364 / 365)
            self.assertAlmostEqual(
                engine.price(dated_history(past, later), later), expected, delta=1e-12
            )
            self.assert_rejection(
                InvalidInputError,
                lambda past=past: engine.price(dated_history(past, market()), market()),
            )
            if spot in (80, 120):
                result = engine.price_with_greeks(
                    dated_history(option, market(spot)), market(spot), greeks="all"
                )
                for name in (
                    "delta",
                    "gamma",
                    "speed",
                    "charm",
                    "color",
                    "vanna",
                    "zomma",
                ):
                    self.assertIsNone(getattr(result, name))
        self.assert_rejection(
            MissingHistoryError,
            lambda: engine.price(dated_history(contract(), later), later),
        )
        microsecond = market(time=datetime(2025, 1, 1, microsecond=1))
        self.assert_rejection(
            MissingHistoryError,
            lambda: engine.price(dated_history(contract(), microsecond), microsecond),
        )
        for kind, spot in product(("knock_in", "knock_out"), (80, 110, 120)):
            option = contract(barrier_type=kind, rebate=2, touch_state="untouched")
            touched = spot != 110
            expected = max(spot - 100, 0) if ((kind == "knock_in") == touched) else 2
            result = engine.price_with_greeks(
                dated_history(option, market(spot, time=END)),
                market(spot, time=END),
                greeks="all",
            )
            self.assertEqual(result.price, expected)
            assert_terminal_greeks(
                self, result, None if touched else int(kind == "knock_out")
            )
        option = contract(touch_state="untouched")
        after = market(time=datetime(2026, 1, 1, microsecond=1))
        self.assert_rejection(
            InvalidInputError, lambda: engine.price(dated_history(option, after), after)
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: engine.price(
                dated_history(option, market(time=datetime(2024, 12, 31))),
                market(time=datetime(2024, 12, 31)),
            ),
        )
        inside = contract(barrier_type="knock_in", touch_state="touched")
        vanilla = EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        self.assertEqual(
            engine.price(dated_history(inside, later), later),
            AnalyticEuropeanEngine().price(dated_history(vanilla, later), later),
        )

    def test_schedules_validate_then_resolve_before_analytic_restrictions(self):
        from datetime import datetime

        engine = AnalyticDoubleBarrierEngine()
        first = datetime(2025, 6, 30)
        for dates in ([first], [first, END]):
            option = contract(observation_times=dates)
            self.assert_rejection(
                UnsupportedOperationError,
                lambda option=option: engine.price(
                    dated_history(option, market()), market()
                ),
            )
            touched = option.replace_history(
                touch_state="touched",
                history_time=option.expiry_time,
            )
            self.assert_rejection(
                InvalidInputError,
                lambda touched=touched: engine.price(
                    dated_history(touched, market()), market()
                ),
            )
        for kind in ("knock_in", "knock_out"):
            option = contract(
                barrier_type=kind,
                rebate=3,
                observation_times=[first],
            )
            self.assertAlmostEqual(
                engine.price(
                    dated_history(option, market(120, time=first)),
                    market(120, time=first),
                ),
                AnalyticEuropeanEngine().price(
                    dated_history(
                        EuropeanOption(
                            option_type="call",
                            strike=100,
                            effective_time=START,
                            expiry_time=END,
                        ),
                        market(120, time=first),
                    ),
                    market(120, time=first),
                )
                if kind == "knock_in"
                else 3 * math.exp(-0.04 * 185 / 365),
                delta=1e-12,
            )
            intraday = market(130, time=datetime(2025, 6, 30, 12))
            self.assert_rejection(
                MissingHistoryError,
                lambda option=option, intraday=intraday: engine.price(
                    dated_history(option, intraday), intraday
                ),
            )
            known = option.replace_history(
                touch_state="untouched",
                history_time=option.history_time or option.expiry_time,
            )
            expected = (
                3 * math.exp(-0.04 * 184.5 / 365)
                if kind == "knock_in"
                else AnalyticEuropeanEngine().price(
                    dated_history(
                        EuropeanOption(
                            option_type="call",
                            strike=100,
                            effective_time=START,
                            expiry_time=END,
                        ),
                        intraday,
                    ),
                    intraday,
                )
            )
            self.assertAlmostEqual(
                engine.price(dated_history(known, intraday), intraday),
                expected,
                delta=1e-12,
            )
        with self.assertRaises(InvalidInputError):
            contract(observation_times=[datetime(2025, 7, 5)])

    def test_image_spectral_switch_boundary_limits_and_tiny_volatility(self):
        engine = AnalyticDoubleBarrierEngine()
        for sigma in (0.20273255202675663, 0.20273255608140772, 0.3040988310811233):
            self.assertAlmostEqual(
                engine.price(
                    dated_history(contract(), market(volatility=sigma)),
                    market(volatility=sigma),
                ),
                reference(
                    dated_history("call", "knock_out"), "knock_out", volatility=sigma
                ),
                delta=1e-9,
            )
        for spot in (
            80.001,
            119.999,
            math.nextafter(80, math.inf),
            math.nextafter(120, 0),
        ):
            actual = engine.price(dated_history(contract(), market(spot)), market(spot))
            self.assertGreaterEqual(actual, 0)
            self.assertAlmostEqual(
                actual,
                reference(dated_history("call", "knock_out"), "knock_out", spot=spot),
                delta=1e-9,
            )
        vanilla = EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        self.assertAlmostEqual(
            engine.price(
                dated_history(
                    contract(lower_barrier=0.01, upper_barrier=10000), market()
                ),
                market(),
            ),
            AnalyticEuropeanEngine().price(dated_history(vanilla, market()), market()),
            delta=1e-9,
        )
        self.assertEqual(
            engine.price(
                dated_history(
                    contract(lower_barrier=99.99, upper_barrier=100.01), market()
                ),
                market(),
            ),
            0,
        )
        for direction, strike in (("call", 125), ("put", 75)):
            self.assertEqual(
                engine.price(
                    dated_history(
                        contract(option_type=direction, strike=strike), market()
                    ),
                    market(),
                ),
                0,
            )
        for up, timing in product((False, True), ("at_hit", "at_expiry")):
            hit = math.log(102 / 100 if up else 100 / 98) / 0.03
            expected = 3 * math.exp(-0.04 * (hit if timing == "at_hit" else 1))
            option = contract(
                lower_barrier=80 if up else 98,
                upper_barrier=102 if up else 120,
                rebate=3,
                rebate_timing=timing,
            )
            self.assertAlmostEqual(
                engine.price(
                    dated_history(
                        option, market(volatility=1e-12, dividend=0.01 if up else 0.07)
                    ),
                    market(volatility=1e-12, dividend=0.01 if up else 0.07),
                ),
                expected,
                delta=1e-7,
            )
        self.assertAlmostEqual(
            engine.price(
                dated_history(
                    contract(lower_barrier=99.5, upper_barrier=100.5, strike=90),
                    market(volatility=1e-12, dividend=0.04),
                ),
                market(volatility=1e-12, dividend=0.04),
            ),
            10 * math.exp(-0.04),
            delta=1e-10,
        )

    def test_rebates_match_independent_exit_series_including_negative_rates(self):
        def rebate_reference(rate, count):
            width = math.log(125 / 80)
            survival, integrated = [], []
            for index in range(count):
                frequency = 2 * index + 1
                coefficient = 4 * (-1 if index % 2 else 1) / (frequency * math.pi)
                decay = 0.5 * 0.2**2 * (frequency * math.pi / width) ** 2
                survival.append(coefficient * math.exp(-decay))
                integrated.append(
                    coefficient * -math.expm1(-(rate + decay)) / (rate + decay)
                )
            discounted = math.exp(-rate) * math.fsum(survival)
            return [
                3 * (math.exp(-rate) - discounted),
                3 * (1 - discounted - rate * math.fsum(integrated)),
                3 * discounted,
            ]

        engine = AnalyticDoubleBarrierEngine()
        for rate in (-0.02, 0, 0.04):
            refs, refined = rebate_reference(rate, 4097), rebate_reference(rate, 8193)
            for a, b in zip(refs, refined, strict=True):
                self.assertAlmostEqual(a, b, delta=1e-11)
            context = market(rate=rate, dividend=rate - 0.02)
            for index, kind, timing in (
                (0, "knock_out", "at_expiry"),
                (1, "knock_out", "at_hit"),
                (2, "knock_in", "at_expiry"),
            ):
                terms = {
                    "upper_barrier": 125,
                    "barrier_type": kind,
                    "rebate_timing": timing,
                }
                actual = engine.price(
                    dated_history(contract(**terms, rebate=3), context), context
                ) - engine.price(dated_history(contract(**terms), context), context)
                self.assertAlmostEqual(actual, refs[index], delta=1e-9)

    def test_shared_analysis_solving_errors_and_concurrent_reads_preserve_inputs(self):
        from concurrent.futures import ThreadPoolExecutor
        from datetime import timedelta

        from kiyosi.market import RateTermStructure
        from kiyosi.pricing import implied_volatility

        option = contract(lower_barrier=50, upper_barrier=200)
        context = market()
        engine = AnalyticDoubleBarrierEngine()
        quote = engine.price(dated_history(option, context), context)
        solved = implied_volatility(
            engine,
            dated_history(option, context),
            context,
            quote,
            lower_bound=0.15,
            upper_bound=0.25,
        )
        self.assertAlmostEqual(solved, 0.2, delta=1e-7)
        known = option.replace_history(
            touch_state="untouched",
            history_time=option.history_time or option.expiry_time,
        )
        later = market(time=START + timedelta(days=1))
        result = engine.price_with_greeks(
            dated_history(known, later), later, greeks="all"
        )
        for name in (
            "delta",
            "gamma",
            "vega",
            "rho",
            "theta",
            "speed",
            "charm",
            "color",
            "vanna",
            "zomma",
        ):
            self.assertTrue(math.isfinite(getattr(result, name)))
        for field in (
            "spot_shift",
            "volatility_shift",
            "rate_shift",
            "time_shift",
        ):
            self.assert_rejection(
                InvalidInputError,
                lambda field=field: numerical_greeks(
                    engine,
                    dated_history(option, context),
                    context,
                    ["delta"],
                    **{field: timedelta() if field == "time_shift" else 0},
                ),
            )
            with self.assertRaises(TypeError):
                numerical_greeks(
                    engine,
                    dated_history(option, context),
                    context,
                    ["delta"],
                    **{field: True},
                )
        with self.assertRaises(TypeError):
            numerical_greeks(
                engine,
                dated_history(option, context),
                context,
                ["delta"],
                time_shift=1.5,
            )
        with self.assertRaises(OverflowError):
            implied_volatility(
                engine,
                dated_history(option, context),
                context,
                quote,
                max_iterations=2**80,
                lower_bound=0.0,
                upper_bound=4.0,
            )
        fixed = contract(rebate=3, touch_state="touched")
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_volatility(
                engine,
                dated_history(fixed, later),
                later,
                engine.price(dated_history(fixed, later), later),
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
        node = RateTermStructure(
            reference_time=START,
            times=[END, END + (END - START)],
            factors=[math.exp(-0.04), (math.exp(-0.04)) * 0.5],
        )
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=node)
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: engine.price(dated_history(option, curved), curved),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: engine.price_with_greeks(dated_history(option, curved), curved, []),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_volatility(
                engine,
                dated_history(option, curved),
                curved,
                math.nan,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
        near = market(80.005)
        guarded = engine.price_with_greeks(
            dated_history(contract(), near), near, ["delta", "gamma", "vega"]
        )
        self.assertIsNone(guarded.delta)
        self.assertIsNone(guarded.gamma)
        self.assertIsNotNone(guarded.vega)
        with ThreadPoolExecutor(max_workers=2) as pool:
            self.assertEqual(
                list(
                    pool.map(
                        lambda _: engine.price(dated_history(option, context), context),
                        range(4),
                    )
                ),
                [quote] * 4,
            )
        self.assertEqual(option, contract(lower_barrier=50, upper_barrier=200))
        self.assertEqual(context, market())

    def test_settled_zero_rebates_do_not_require_representable_discount(self):
        engine = AnalyticDoubleBarrierEngine()
        outside = contract(expiry_time=datetime(2027, 1, 1))
        self.assertEqual(
            engine.price(
                dated_history(outside, market(120, rate=1e308, dividend=0)),
                market(120, rate=1e308, dividend=0),
            ),
            0,
        )
        inside = contract(
            expiry_time=datetime(2027, 1, 1),
            barrier_type="knock_in",
            observation_times=[START],
            touch_state="untouched",
        )
        self.assertEqual(
            engine.price(
                dated_history(
                    inside, market(time=datetime(2025, 1, 2), rate=1e308, dividend=0)
                ),
                market(time=datetime(2025, 1, 2), rate=1e308, dividend=0),
            ),
            0,
        )
