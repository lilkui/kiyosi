"""Double barriers: public contracts and independent pricing references."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta
from itertools import product

import QuantLib as ql
from kiyosi import ErrorCategory, KiyosiError, pricing
from kiyosi.instruments import DoubleBarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)
from price_differences import price_differences

START, END = date(2025, 1, 1), date(2026, 1, 1)
FIRST = START + timedelta(days=180)


def contract(**changes):
    return DoubleBarrierOption(
        **(
            {
                "option_type": "call",
                "strike": 100,
                "effective_date": START,
                "expiry_date": END,
                "lower_barrier": 80,
                "upper_barrier": 120,
                "barrier_type": "knock_out",
            }
            | changes
        )
    )


def market(spot=100, *, time=START, volatility=0.2, dividend=0.01, calendar=None):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=time,
        calendar=all_days_calendar() if calendar is None else calendar,
    )


def normal(value):
    return 0.5 * math.erfc(-value / math.sqrt(2))


def terminal_reference(spot, direction, lower, upper, time, rebate=2):
    """Closed-form lognormal cash/asset moments of the surviving interval."""
    deviation = 0.2 * math.sqrt(time)

    def moments(left, right):
        d1_left = (math.log(spot / left) + 0.05 * time) / deviation
        d1_right = (math.log(spot / right) + 0.05 * time) / deviation
        return (
            spot * math.exp(-0.01 * time) * (normal(d1_left) - normal(d1_right)),
            math.exp(-0.04 * time)
            * (normal(d1_left - deviation) - normal(d1_right - deviation)),
        )

    left, right = (
        (max(lower, 100), upper) if direction == "call" else (lower, min(upper, 100))
    )
    asset, cash = moments(left, right)
    _, survival = moments(lower, upper)
    sign = 1 if direction == "call" else -1
    return sign * (asset - 100 * cash) + rebate * (math.exp(-0.04 * time) - survival)


def vanilla_reference(spot, direction, time, *, strike=100):
    sign = 1 if direction == "call" else -1
    deviation = 0.2 * math.sqrt(time)
    d1 = (math.log(spot / strike) + 0.05 * time) / deviation
    return sign * (
        spot * math.exp(-0.01 * time) * normal(sign * d1)
        - strike * math.exp(-0.04 * time) * normal(sign * (d1 - deviation))
    )


def first_fixing_reference(spot, direction, timing, *, final_fixing=True, count=1024):
    """Integrate continuation over states surviving the first fixing."""
    first = 180 / 365
    deviation = 0.2 * math.sqrt(first)
    lower = (math.log(80.125 / spot) - 0.01 * first) / deviation
    upper = (math.log(120.125 / spot) - 0.01 * first) / deviation
    width = (upper - lower) / count

    def integrand(z):
        asset = spot * math.exp(0.01 * first + deviation * z)
        continuation = (
            terminal_reference(asset, direction, 80.125, 120.125, 1 - first)
            if final_fixing
            else vanilla_reference(asset, direction, 1 - first)
        )
        return continuation * math.exp(-0.5 * z * z) / math.sqrt(2 * math.pi)

    integral = integrand(lower) + integrand(upper)
    for index in range(1, count):
        integral += (4 if index % 2 else 2) * integrand(lower + index * width)
    touched = normal(lower) + normal(-upper)
    return (
        math.exp(-0.04 * first) * width * integral / 3
        + 2 * math.exp(-0.04 * (first if timing == "at_hit" else 1)) * touched
    )


def continuous_reference(direction, kind, *, spot=100, series=10):
    """Only zero-rebate cases: QuantLib's analytic double engine ignores rebate."""
    with ql.SavedSettings():
        beginning, expiry = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
        ql.Settings.instance().evaluationDate = beginning
        day_count = ql.Actual365Fixed()
        process = ql.BlackScholesMertonProcess(
            ql.QuoteHandle(ql.SimpleQuote(spot)),
            ql.YieldTermStructureHandle(ql.FlatForward(beginning, 0.01, day_count)),
            ql.YieldTermStructureHandle(ql.FlatForward(beginning, 0.04, day_count)),
            ql.BlackVolTermStructureHandle(
                ql.BlackConstantVol(beginning, ql.NullCalendar(), 0.2, day_count)
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
            ql.EuropeanExercise(expiry),
        )
        option.setPricingEngine(ql.AnalyticDoubleBarrierEngine(process, series))
        return option.NPV()


def continuous_rebate_references(count):
    # Integrate the killed-Brownian kernel in y, then discount -d(survival).
    # Source: https://www.stat.uchicago.edu/~lalley/Courses/312/BrownianMotion312.pdf, exercise 6.
    width = math.log(125 / 80)
    survival_terms, integral_terms = [], []
    for index in range(count):
        frequency = 2 * index + 1
        coefficient = 4 * (-1 if index % 2 else 1) / (frequency * math.pi)
        decay = 0.5 * 0.2**2 * (frequency * math.pi / width) ** 2
        survival_terms.append(coefficient * math.exp(-decay))
        integral_terms.append(
            coefficient * -math.expm1(-(0.04 + decay)) / (0.04 + decay)
        )
    discounted_survival = math.exp(-0.04) * math.fsum(survival_terms)
    return {
        "at_expiry": 3 * (math.exp(-0.04) - discounted_survival),
        "at_hit": 3 * (1 - discounted_survival - 0.04 * math.fsum(integral_terms)),
        "untouched": 3 * discounted_survival,
    }


class DoubleBarrierTests(unittest.TestCase):
    def assert_core_error(self, category, operation):
        with self.assertRaises(KiyosiError) as caught:
            operation()
        self.assertEqual(caught.exception.category, category)

    def test_defaults_ownership_and_history_replacement(self):
        option = contract()
        self.assertEqual(option.rebate, 0)
        self.assertEqual(option.rebate_timing, "at_expiry")
        self.assertEqual(option.observation_mode, "continuous")
        self.assertIsNone(option.touch_state)
        self.assertEqual(option.observation_dates, [])
        dates = [FIRST, END]
        option = contract(
            observation_mode="scheduled", observation_dates=dates, rebate=3
        )
        dates.clear()
        copy = option.observation_dates
        copy.clear()
        self.assertEqual(option.observation_dates, [FIRST, END])
        changed = option.replace(touch_state="touched")
        self.assertEqual(changed.touch_state, "touched")
        self.assertIsNone(option.touch_state)
        for field in (
            "option_type",
            "strike",
            "effective_date",
            "expiry_date",
            "lower_barrier",
            "upper_barrier",
            "barrier_type",
            "rebate",
            "rebate_timing",
            "observation_dates",
        ):
            self.assertEqual(getattr(option, field), getattr(changed, field))
        self.assertEqual(changed.replace(touch_state=None), option)
        with self.assertRaises(AttributeError):
            option.lower_barrier = 70
        with self.assertRaises(ValueError):
            option.replace(touch_state="unknown")
        self.assertIsNone(option.touch_state)

    def test_conversion_errors_remain_python_native(self):
        for field in ("strike", "lower_barrier", "upper_barrier", "rebate"):
            for value in (True, "100", complex(100, 0)):
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(TypeError),
                ):
                    contract(**{field: value})
        with self.assertRaises(OverflowError):
            contract(upper_barrier=10**1000)
        for value in (True, 1.5):
            with self.assertRaises(TypeError):
                pricing.FiniteDifferenceDoubleBarrierEngine(asset_step_count=value)
        with self.assertRaises(OverflowError):
            pricing.FiniteDifferenceDoubleBarrierEngine(asset_step_count=2**40)
        with self.assertRaises(ValueError):
            contract(barrier_type="up_and_out")
        with self.assertRaises(TypeError):
            contract(
                observation_mode="scheduled",
                observation_dates=[datetime(2025, 7, 1)],  # noqa: DTZ001 - reject naive datetimes
            )

    def test_core_rejections_preserve_stable_categories(self):
        for fields, category in (
            ({"lower_barrier": 0}, ErrorCategory.INVALID_PARAMETER),
            ({"upper_barrier": math.inf}, ErrorCategory.INVALID_PARAMETER),
            ({"upper_barrier": 80}, ErrorCategory.INVALID_PARAMETER),
            ({"lower_barrier": math.nan}, ErrorCategory.INVALID_PARAMETER),
            ({"rebate": -1}, ErrorCategory.INVALID_PARAMETER),
            ({"strike": 0}, ErrorCategory.INVALID_STRIKE),
            (
                {"barrier_type": "knock_in", "rebate_timing": "at_hit"},
                ErrorCategory.INVALID_OPTION,
            ),
            ({"observation_mode": "scheduled"}, ErrorCategory.INVALID_SCHEDULE),
            ({"observation_dates": [END]}, ErrorCategory.INVALID_SCHEDULE),
        ):
            with self.subTest(fields=fields):
                self.assert_core_error(
                    category, lambda fields=fields: contract(**fields)
                )

    def test_continuous_prices_and_greeks_match_stable_independent_references(self):
        for direction, kind in product(("call", "put"), ("knock_in", "knock_out")):
            expected = continuous_reference(direction, kind)
            self.assertAlmostEqual(
                expected, continuous_reference(direction, kind, series=20), delta=1e-11
            )
            option = contract(option_type=direction, barrier_type=kind)
            errors = []
            for count in (80, 320):
                actual = pricing.FiniteDifferenceDoubleBarrierEngine(
                    asset_step_count=count, time_step_count=count
                ).price(option, market())
                errors.append(abs(actual - expected))
            with self.subTest(direction=direction, kind=kind):
                self.assertLess(errors[1], errors[0])
                self.assertLess(errors[1], 0.01)
            engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                asset_step_count=640, time_step_count=800
            )
            actual = engine.price_with_greeks(option, market(), ["delta", "gamma"])
            h = 0.025
            derivatives = []
            for shift in (h, h / 2):
                high = continuous_reference(direction, kind, spot=100 + shift)
                low = continuous_reference(direction, kind, spot=100 - shift)
                derivatives.append(
                    ((high - low) / (2 * shift), (high - 2 * expected + low) / shift**2)
                )
            self.assertAlmostEqual(derivatives[0][0], derivatives[1][0], delta=1e-6)
            self.assertAlmostEqual(derivatives[0][1], derivatives[1][1], delta=1e-6)
            self.assertAlmostEqual(actual.delta, derivatives[1][0], delta=0.003)
            self.assertAlmostEqual(actual.gamma, derivatives[1][1], delta=0.0005)

    def test_current_and_past_touches_preserve_rebate_timing(self):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine()
        for spot, timing in product((79, 80, 120, 121), ("at_hit", "at_expiry")):
            option = contract(rebate=3, rebate_timing=timing)
            expected = 3 if timing == "at_hit" else 3 * math.exp(-0.04)
            self.assertAlmostEqual(
                engine.price(option, market(spot)), expected, delta=1e-12
            )
            past = option.replace(touch_state="touched")
            expected = 0 if timing == "at_hit" else 3 * math.exp(-0.04 * 364 / 365)
            self.assertAlmostEqual(
                engine.price(past, market(time=START + timedelta(days=1))),
                expected,
                delta=1e-12,
            )
        self.assert_core_error(
            ErrorCategory.INVALID_PARAMETER,
            lambda: engine.price(
                contract(), market(time=datetime(2025, 1, 1, 0, 0, 1, tzinfo=UTC))
            ),
        )
        self.assert_core_error(
            ErrorCategory.INVALID_OPTION,
            lambda: engine.price(contract(touch_state="touched"), market()),
        )
        for bound, inside in ((80, math.inf), (120, 0)):
            observed = contract(
                observation_mode="scheduled", observation_dates=[START, END], rebate=3
            )
            actual = engine.price_with_greeks(
                observed, market(bound), ["delta", "gamma", "vega"]
            )
            self.assertAlmostEqual(actual.price, 3 * math.exp(-0.04), delta=1e-12)
            self.assertIsNone(actual.delta)
            self.assertIsNone(actual.gamma)
            self.assertIsNone(actual.vega)
            future = contract(
                observation_mode="scheduled", observation_dates=[END], rebate=3
            )
            just_inside = market(math.nextafter(bound, inside))
            self.assertEqual(
                engine.price(observed, just_inside), engine.price(future, just_inside)
            )

    def test_discrete_observation_is_only_at_midnight_and_history_can_end_early(self):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine()
        time = datetime.combine(FIRST, datetime.min.time(), UTC) + timedelta(hours=12)
        for kind in ("knock_in", "knock_out"):
            option = contract(
                barrier_type=kind,
                rebate=3,
                observation_mode="scheduled",
                observation_dates=[FIRST],
                touch_state="untouched",
            )
            context = market(130, time=time)
            expected = (
                3 * math.exp(-0.04 * 184.5 / 365)
                if kind == "knock_in"
                else pricing.AnalyticVanillaEngine().price(
                    EuropeanOption(
                        option_type="call",
                        strike=100,
                        effective_date=START,
                        expiry_date=END,
                    ),
                    context,
                )
            )
            self.assertAlmostEqual(engine.price(option, context), expected, delta=1e-12)
            if kind == "knock_in":
                self.assert_core_error(
                    ErrorCategory.UNSUPPORTED_OPERATION,
                    lambda option=option, context=context, expected=expected: (
                        pricing.implied_volatility(engine, option, context, expected)
                    ),
                )
        future = contract(observation_mode="scheduled", observation_dates=[END])
        self.assertAlmostEqual(
            engine.price(future, market(125)),
            terminal_reference(125, "call", 80, 120, 1, rebate=0),
            delta=0.002,
        )

    def test_one_and_two_fixings_and_early_windows_match_independent_integration(self):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=800, time_step_count=2000, asset_upper_boundary=400
        )
        for direction, timing in product(("call", "put"), ("at_hit", "at_expiry")):
            for dates in ([END], [FIRST, END], [FIRST]):
                option = contract(
                    option_type=direction,
                    lower_barrier=80.125,
                    upper_barrier=120.125,
                    rebate=2,
                    rebate_timing=timing,
                    observation_mode="scheduled",
                    observation_dates=dates,
                )
                if dates == [END]:
                    expected = terminal_reference(100.25, direction, 80.125, 120.125, 1)
                else:
                    expected = first_fixing_reference(
                        100.25, direction, timing, final_fixing=dates[-1] == END
                    )
                    self.assertAlmostEqual(
                        expected,
                        first_fixing_reference(
                            100.25,
                            direction,
                            timing,
                            final_fixing=dates[-1] == END,
                            count=2048,
                        ),
                        delta=1e-9,
                    )
                with self.subTest(direction=direction, timing=timing, dates=dates):
                    self.assertAlmostEqual(
                        engine.price(option, market(100.25)), expected, delta=0.002
                    )

    def test_in_out_parity_preserves_expiry_rebates_and_native_greeks(self):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=800, time_step_count=800, asset_upper_boundary=400
        )
        for mode, direction in product(("continuous", "scheduled"), ("call", "put")):
            terms = {
                "option_type": direction,
                "rebate": 2,
                "observation_mode": mode,
                "observation_dates": [FIRST, END] if mode == "scheduled" else [],
            }
            values = [
                engine.price_with_greeks(
                    contract(**terms, barrier_type=kind), market(), ["delta", "gamma"]
                )
                for kind in ("knock_in", "knock_out")
            ]
            vanilla = pricing.AnalyticVanillaEngine().price_with_greeks(
                EuropeanOption(
                    option_type=direction,
                    strike=100,
                    effective_date=START,
                    expiry_date=END,
                ),
                market(),
                ["delta", "gamma"],
            )
            for measure, budget in (
                ("price", 0.01),
                ("delta", 0.003),
                ("gamma", 0.0005),
            ):
                expected = getattr(vanilla, measure) + (
                    2 * math.exp(-0.04) if measure == "price" else 0
                )
                self.assertAlmostEqual(
                    sum(getattr(value, measure) for value in values),
                    expected,
                    delta=budget,
                )

    def test_greek_stencils_and_implied_volatility_follow_existing_contracts(self):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=400, time_step_count=800
        )
        option = contract()
        native = engine.price_with_greeks(
            option, market(), ["delta", "gamma", "vega", "rho"]
        )
        bumped = price_differences(
            engine, option, market(), greeks=("delta", "gamma", "vega", "rho")
        )
        self.assertEqual(native.price, engine.price(option, market()))
        for measure, budget in (
            ("delta", 1e-4),
            ("gamma", 2e-4),
            ("vega", 1e-7),
            ("rho", 1e-7),
        ):
            self.assertAlmostEqual(
                getattr(native, measure), getattr(bumped, measure), delta=budget
            )
        solved = pricing.implied_volatility(
            engine, option, market(), native.price, lower_bound=0.15, upper_bound=0.25
        )
        self.assertAlmostEqual(solved, 0.2, delta=1e-6)
        self.assert_core_error(
            ErrorCategory.UNSUPPORTED_OPERATION,
            lambda: pricing.implied_volatility(
                engine,
                option.replace(touch_state="touched"),
                market(time=START + timedelta(days=1)),
                0,
            ),
        )
        scheduled = contract(
            observation_mode="scheduled",
            observation_dates=[FIRST, END],
            touch_state="untouched",
        )
        event = engine.price_with_greeks(
            scheduled, market(time=FIRST), ["theta", "charm", "color"]
        )
        self.assertIsNone(event.theta)
        self.assertIsNone(event.charm)
        self.assertIsNone(event.color)
        at_expiry = engine.price_with_greeks(
            option.replace(touch_state="untouched"),
            market(110, time=END),
            all_greeks=True,
        )
        self.assertEqual(at_expiry.price, 10)
        self.assertIsNone(at_expiry.delta)
        self.assertIsNone(at_expiry.vega)

    def test_narrow_band_and_tiny_volatility_do_not_lose_surviving_payoff(self):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=160, time_step_count=800
        )
        option = contract(lower_barrier=99.5, upper_barrier=100.5, strike=90)
        self.assertLess(engine.price(option, market()), 1e-8)
        actual = engine.price(option, market(volatility=1e-12, dividend=0.04))
        self.assertAlmostEqual(actual, 10 * math.exp(-0.04), delta=1e-6)
        for direction, strike in (("call", 140), ("put", 60)):
            excluded = contract(option_type=direction, strike=strike)
            self.assertEqual(engine.price(excluded, market()), 0)
            activated = contract(
                option_type=direction, strike=strike, barrier_type="knock_in"
            )
            expected = pricing.AnalyticVanillaEngine().price(
                EuropeanOption(
                    option_type=direction,
                    strike=strike,
                    effective_date=START,
                    expiry_date=END,
                ),
                market(),
            )
            self.assertEqual(engine.price(activated, market()), expected)

    def test_edge_grid_refinement_preserves_independent_reference_limits(self):
        for spot in (80.125, 119.875):
            expected = continuous_reference("call", "knock_out", spot=spot, series=20)
            self.assertAlmostEqual(
                expected,
                continuous_reference("call", "knock_out", spot=spot, series=10),
                delta=1e-10,
            )
            errors = []
            for count in (80, 160, 320):
                engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                    asset_step_count=count, time_step_count=count
                )
                errors.append(abs(engine.price(contract(), market(spot)) - expected))
            self.assertLess(errors[-1], errors[0])
            self.assertLess(errors[-1], 0.0001)
        option = contract(lower_barrier=99.5, upper_barrier=100.5, strike=90)
        errors = []
        for count in (80, 160, 320):
            engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                asset_step_count=count, time_step_count=5 * count
            )
            self.assertLess(engine.price(option, market()), 1e-8)
            errors.append(
                abs(
                    engine.price(option, market(volatility=1e-12, dividend=0.04))
                    - 10 * math.exp(-0.04)
                )
            )
        self.assertLess(errors[-1], errors[0])
        self.assertLess(errors[-1], 1e-6)

    def test_grid_validation_calendar_and_shared_instance_concurrency(self):
        unstable = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=400, time_step_count=1, scheme="explicit_euler"
        )
        self.assert_core_error(
            ErrorCategory.INVALID_PARAMETER,
            lambda: unstable.price(contract(), market()),
        )
        clipped = pricing.FiniteDifferenceDoubleBarrierEngine(asset_upper_boundary=110)
        self.assert_core_error(
            ErrorCategory.INVALID_PARAMETER, lambda: clipped.price(contract(), market())
        )
        weekend = date(2025, 7, 5)
        option = contract(observation_mode="scheduled", observation_dates=[weekend])
        engine = pricing.FiniteDifferenceDoubleBarrierEngine()
        self.assert_core_error(
            ErrorCategory.INVALID_DATE,
            lambda: engine.price(option, market(calendar=weekdays_calendar())),
        )
        option, context = contract(), market()
        expected = engine.price(option, context)
        with ThreadPoolExecutor(max_workers=2) as pool:
            actual = list(pool.map(lambda _: engine.price(option, context), range(4)))
        self.assertEqual(actual, [expected] * 4)

    def test_continuous_future_hits_discount_rebates_at_the_correct_time(self):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=800, time_step_count=2000
        )
        for lower, upper, dividend in ((80, 102, 0), (98, 120, 0.08)):
            hit_time = (
                math.log(upper / 100) / 0.04
                if dividend == 0
                else math.log(100 / lower) / 0.04
            )
            for timing in ("at_hit", "at_expiry"):
                option = contract(
                    lower_barrier=lower,
                    upper_barrier=upper,
                    rebate=3,
                    rebate_timing=timing,
                )
                expected = 3 * math.exp(-0.04 * (hit_time if timing == "at_hit" else 1))
                actual = engine.price(
                    option, market(volatility=1e-12, dividend=dividend)
                )
                with self.subTest(lower=lower, upper=upper, timing=timing):
                    self.assertAlmostEqual(actual, expected, delta=1e-4)

    def test_implicit_and_explicit_schemes_price_unresolved_double_barriers(self):
        expected = continuous_reference("call", "knock_out")
        for scheme, count in (("implicit_euler", 320), ("explicit_euler", 40)):
            engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                asset_step_count=count, time_step_count=2000, scheme=scheme
            )
            with self.subTest(scheme=scheme):
                self.assertAlmostEqual(
                    engine.price(contract(), market()), expected, delta=0.01
                )

    def test_overlapping_event_cells_preserve_surviving_payoff(self):
        lower, upper = 80.05, 80.125
        option = contract(
            option_type="put",
            lower_barrier=lower,
            upper_barrier=upper,
            rebate=2,
            observation_mode="scheduled",
            observation_dates=[END],
        )
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=800, time_step_count=2000, asset_upper_boundary=400
        )
        expected = terminal_reference(100.25, "put", lower, upper, 1)
        self.assertAlmostEqual(
            engine.price(option, market(100.25)), expected, delta=0.0002
        )

    def test_interim_observations_preserve_nonnegative_continuation(self):
        strike = 80
        first = (FIRST - START).days / 365
        deviation = 0.2 * math.sqrt(first)

        def reference(spot, lower, upper, count):
            left = (math.log(lower / spot) - 0.01 * first) / deviation
            right = (math.log(upper / spot) - 0.01 * first) / deviation
            width = (right - left) / count

            def integrand(z):
                asset = spot * math.exp(0.01 * first + deviation * z)
                continuation = vanilla_reference(
                    asset, "call", 1 - first, strike=strike
                )
                return continuation * math.exp(-0.5 * z * z) / math.sqrt(2 * math.pi)

            total = integrand(left) + integrand(right)
            for index in range(1, count):
                total += (4 if index % 2 else 2) * integrand(left + index * width)
            return math.exp(-0.04 * first) * width * total / 3

        worst = [0.0] * 3
        for lower, upper in (
            (80.05, 80.0501),
            (80.24995, 80.25005),
            (80.74995, 80.75005),
        ):
            expected = reference(100.25, lower, upper, 128)
            self.assertGreater(expected, 0)
            self.assertAlmostEqual(
                expected, reference(100.25, lower, upper, 64), delta=1e-14
            )
            option = contract(
                strike=strike,
                lower_barrier=lower,
                upper_barrier=upper,
                observation_mode="scheduled",
                observation_dates=[FIRST],
            )
            for scheme in ("crank_nicolson", "implicit_euler"):
                for index, count in enumerate((800, 1600, 3200)):
                    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                        asset_step_count=count,
                        time_step_count=2000,
                        asset_upper_boundary=400,
                        scheme=scheme,
                    )
                    actual = engine.price(option, market(100.25))
                    with self.subTest(
                        lower=lower, upper=upper, scheme=scheme, count=count
                    ):
                        self.assertGreater(actual, 0)
                        self.assertAlmostEqual(actual, expected, delta=expected * 0.02)
                    worst[index] = max(worst[index], abs(actual - expected) / expected)
        self.assertLess(worst[-1], worst[0])
        self.assertLess(worst[-1], 0.01)

        option = contract(
            strike=strike,
            lower_barrier=80.24995,
            upper_barrier=120,
            observation_mode="scheduled",
            observation_dates=[FIRST],
        )
        before = datetime.combine(FIRST, datetime.min.time(), UTC) - timedelta(
            seconds=1
        )
        for scheme in ("crank_nicolson", "implicit_euler"):
            engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                asset_step_count=800,
                time_step_count=2000,
                asset_upper_boundary=400,
                scheme=scheme,
            )
            self.assertGreaterEqual(engine.price(option, market(80, time=before)), 0)

    def test_continuous_stochastic_rebates_match_independent_exit_time_series(self):
        references = continuous_rebate_references(129)
        refined = continuous_rebate_references(257)
        for timing, value in references.items():
            self.assertAlmostEqual(value, refined[timing], delta=2e-8)
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=640, time_step_count=800
        )
        # q = r - sigma^2/2 makes log spot driftless and S the log-interval midpoint.
        context = market(dividend=0.02)
        for kind, timing in (
            ("knock_out", "at_hit"),
            ("knock_out", "at_expiry"),
            ("knock_in", "at_expiry"),
        ):
            terms = {
                "upper_barrier": 125,
                "barrier_type": kind,
                "rebate_timing": timing,
            }
            base = engine.price(contract(**terms), context)
            actual = engine.price(contract(**terms, rebate=3), context) - base
            expected = references["untouched" if kind == "knock_in" else timing]
            with self.subTest(kind=kind, timing=timing):
                self.assertAlmostEqual(actual, expected, delta=0.001)
