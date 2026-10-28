"""Double-touch cash contracts and references independent of the PDE."""

import math
import sys
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace

from kiyosi import ErrorCategory, KiyosiError, pricing
from kiyosi.instruments import (
    DoubleTouchOption,
    cash_double_no_touch,
    cash_double_touch,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
    weekdays_calendar,
)

sys.path.append(str(Path(__file__).resolve().parents[3] / "tests/python"))
from price_differences import price_differences

START, END = date(2025, 1, 1), date(2026, 1, 1)
FIRST = START + timedelta(days=180)


def contract(factory=cash_double_touch, **changes):
    return factory(
        **(
            {
                "effective_date": START,
                "expiry_date": END,
                "lower_barrier": 80,
                "upper_barrier": 120,
                "payout": 1,
            }
            | changes
        )
    )


def market(
    spot=100, *, time=START, volatility=0.2, rate=0.03, dividend=0.01, calendar=None
):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=time,
        calendar=calendar if calendar is not None else all_days_calendar(),
    )


def continuous_reference(
    spot=100,
    *,
    lower=80,
    upper=120,
    volatility=0.2,
    rate=0.03,
    dividend=0.01,
    count=1024,
):
    """Integrate the absorbing Brownian density in log spot and its discounted exit.

    The sine expansion solves the absorbing heat equation on (log L, log U).
    The exponential tilt restores log-price drift; no finite-difference code is used.
    """
    width, x = math.log(upper / lower), math.log(spot / lower)
    drift = rate - dividend - volatility**2 / 2
    alpha = drift / volatility**2
    survival_terms, integral_terms = [], []
    for n in range(1, count + 1):
        frequency = n * math.pi / width
        coefficient = (
            2
            / width
            * math.exp(-alpha * x)
            * math.sin(frequency * x)
            * frequency
            * (1 - (-1) ** n * math.exp(alpha * width))
            / (alpha**2 + frequency**2)
        )
        decay = (volatility * frequency) ** 2 / 2 + drift**2 / (2 * volatility**2)
        survival_terms.append(coefficient * math.exp(-decay))
        integral_terms.append(
            coefficient * -math.expm1(-(rate + decay)) / (rate + decay)
        )
    untouched = math.exp(-rate) * math.fsum(survival_terms)
    return (
        math.exp(-rate) - untouched,
        1 - untouched - rate * math.fsum(integral_terms),
        untouched,
    )


def cash_reference(spot, *, volatility=0.2, rate=0.03, time=1, count=64):
    """Analytic spot derivatives of the killed log-Brownian sine solution."""
    width, x = math.log(120 / 80), math.log(spot / 80)
    alpha = (rate - 0.01 - volatility**2 / 2) / volatility**2
    beta = math.sqrt(alpha**2 + 2 * rate / volatility**2)
    plus, minus = -alpha + beta, -alpha - beta
    weight = -math.expm1(minus * width) / (
        math.exp(plus * width) - math.exp(minus * width)
    )
    survival, transient = [0.0] * 4, [0.0] * 4
    for n in range(1, count + 1):
        k = n * math.pi / width
        coefficient = 2 * k / width * (1 - (-1) ** n * math.exp(alpha * width))
        decay = volatility**2 / 2 * (k * k + alpha * alpha) + rate
        factor = coefficient * math.exp(-alpha * x - decay * time)
        sine, cosine = math.sin(k * x), math.cos(k * x)
        derivatives = (
            sine,
            k * cosine - alpha * sine,
            (alpha * alpha - k * k) * sine - 2 * alpha * k * cosine,
            (3 * alpha * k * k - alpha**3) * sine
            + (3 * alpha * alpha * k - k**3) * cosine,
        )
        for order, derivative in enumerate(derivatives):
            survival[order] += factor * derivative / (k * k + alpha * alpha)
            transient[order] += factor * derivative / (k * k + beta * beta)
    # Subtract the transient from the stationary exit transform so third derivatives converge.
    at_hit = [
        weight * plus**order * math.exp(plus * x)
        + (1 - weight) * minus**order * math.exp(minus * x)
        - transient[order]
        for order in range(4)
    ]
    at_expiry = [math.exp(-rate * time) - survival[0]] + [
        -value for value in survival[1:]
    ]
    return [
        SimpleNamespace(
            price=value[0],
            delta=value[1] / spot,
            gamma=(value[2] - value[1]) / spot**2,
            speed=(value[3] - 3 * value[2] + 2 * value[1]) / spot**3,
        )
        for value in (at_expiry, at_hit, survival)
    ]


def reference_greeks(spot):
    shift = 0.0001
    base = cash_reference(spot)
    vol_up, vol_down = (
        cash_reference(spot, volatility=0.2 + shift),
        cash_reference(spot, volatility=0.2 - shift),
    )
    rate_up, rate_down = (
        cash_reference(spot, rate=0.03 + shift),
        cash_reference(spot, rate=0.03 - shift),
    )
    after = cash_reference(spot, time=1 - 1 / 365)
    return [
        {
            "delta": b.delta,
            "gamma": b.gamma,
            "speed": b.speed,
            "theta": t.price - b.price,
            "charm": t.delta - b.delta,
            "color": t.gamma - b.gamma,
            "vega": (u.price - d.price) / (200 * shift),
            "vanna": (u.delta - d.delta) / (200 * shift),
            "zomma": (u.gamma - d.gamma) / (200 * shift),
            "rho": (r.price - s.price) / (200 * shift),
        }
        for b, u, d, r, s, t in zip(
            base, vol_up, vol_down, rate_up, rate_down, after, strict=True
        )
    ]


def normal(x):
    return math.erfc(-x / math.sqrt(2)) / 2


def survival_at_fixing(spot, time):
    deviation = 0.2 * math.sqrt(time)
    return normal(math.log(120.125 / spot) / deviation) - normal(
        math.log(80.125 / spot) / deviation
    )


def scheduled_reference(dates, count=1024):
    """Integrate survival through one or two independent normal increments."""
    time = (dates[0] - START).days / 365
    survived_first = survival_at_fixing(100.25, time)
    survival = survived_first
    if len(dates) == 2:
        deviation = 0.2 * math.sqrt(time)
        left, right = (
            math.log(80.125 / 100.25) / deviation,
            math.log(120.125 / 100.25) / deviation,
        )
        step = (right - left) / count

        def integrand(z):
            return (
                survival_at_fixing(100.25 * math.exp(deviation * z), 1 - time)
                * math.exp(-z * z / 2)
                / math.sqrt(2 * math.pi)
            )

        total = integrand(left) + integrand(right)
        total += math.fsum(
            (4 if i % 2 else 2) * integrand(left + i * step) for i in range(1, count)
        )
        survival = step * total / 3
    return (
        math.exp(-0.03) * (1 - survival),
        math.exp(-0.03 * time) * (1 - survived_first)
        + math.exp(-0.03 * (1 if len(dates) == 2 else time))
        * (survived_first - survival),
        math.exp(-0.03) * survival,
    )


class DoubleTouchTests(unittest.TestCase):
    def assert_core_error(self, category, operation):
        with self.assertRaises(KiyosiError) as caught:
            operation()
        self.assertEqual(caught.exception.category, category)

    def test_factories_defaults_owning_schedules_and_history(self):
        option = contract()
        self.assertIsInstance(option, DoubleTouchOption)
        self.assertTrue(option.is_one_touch)
        self.assertEqual(option.settlement_timing, "at_expiry")
        self.assertEqual(option.observation_mode, "continuous")
        self.assertIsNone(option.touch_state)
        self.assertEqual(option.observation_dates, [])
        self.assertEqual(option.payout, 1)
        replaced = option.replace(touch_state="untouched")
        self.assertEqual(replaced.touch_state, "untouched")
        self.assertIsNone(option.touch_state)
        self.assertEqual(replaced.replace(touch_state=None), option)
        dates = [FIRST, END]
        scheduled = contract(
            cash_double_no_touch, observation_mode="scheduled", observation_dates=dates
        )
        dates.clear()
        copy = scheduled.observation_dates
        copy.clear()
        self.assertEqual(scheduled.observation_dates, [FIRST, END])
        self.assertFalse(scheduled.is_one_touch)
        self.assertEqual(scheduled.settlement_timing, "at_expiry")
        self.assertIn("DoubleTouchOption", repr(scheduled))
        with self.assertRaises(AttributeError):
            option.payout = 2

    def test_protocols_domain_errors_and_failed_replace_preserve_inputs(self):
        for factory in (cash_double_touch, cash_double_no_touch):
            for key in ("payout", "lower_barrier", "upper_barrier"):
                for value in (True, "100", object()):
                    with (
                        self.subTest(factory=factory, key=key, value=value),
                        self.assertRaises(TypeError),
                    ):
                        contract(factory, **{key: value})
                for value in (0, -1, math.inf, math.nan):
                    self.assert_core_error(
                        ErrorCategory.INVALID_PARAMETER,
                        lambda factory=factory, key=key, value=value: contract(
                            factory, **{key: value}
                        ),
                    )
            for changes, category in (
                ({"lower_barrier": 120}, ErrorCategory.INVALID_PARAMETER),
                ({"lower_barrier": 121}, ErrorCategory.INVALID_PARAMETER),
                (
                    {"expiry_date": START - timedelta(days=1)},
                    ErrorCategory.INVALID_TIME_RANGE,
                ),
                ({"observation_mode": "scheduled"}, ErrorCategory.INVALID_SCHEDULE),
                ({"observation_dates": [END]}, ErrorCategory.INVALID_SCHEDULE),
                (
                    {
                        "observation_mode": "scheduled",
                        "observation_dates": [END, FIRST],
                    },
                    ErrorCategory.INVALID_SCHEDULE,
                ),
            ):
                self.assert_core_error(
                    category,
                    lambda factory=factory, changes=changes: contract(
                        factory, **changes
                    ),
                )
        with self.assertRaises(TypeError):
            contract(cash_double_no_touch, settlement_timing="at_hit")
        with self.assertRaises(ValueError):
            contract(settlement_timing="later")
        option = contract()
        with self.assertRaises(TypeError):
            option.replace(touch_state=True)
        self.assertEqual(option, contract())

    def test_continuous_prices_match_independent_series_and_converge(self):
        options = (
            contract(),
            contract(settlement_timing="at_hit"),
            contract(cash_double_no_touch),
        )
        reference = continuous_reference()
        for left, right in zip(
            reference, continuous_reference(count=2048), strict=True
        ):
            self.assertAlmostEqual(left, right, delta=1e-9)
        coarse = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=80, time_step_count=100
        )
        fine = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=800, time_step_count=1600
        )
        for option, expected in zip(options, reference, strict=True):
            actual = fine.price(option, market())
            self.assertLess(
                abs(actual - expected), abs(coarse.price(option, market()) - expected)
            )
            self.assertAlmostEqual(actual, expected, delta=1e-5)
        scaled = (
            contract(payout=7.5),
            contract(payout=7.5, settlement_timing="at_hit"),
            contract(cash_double_no_touch, payout=7.5),
        )
        for option, expected in zip(scaled, reference, strict=True):
            self.assertAlmostEqual(
                fine.price(option, market()), 7.5 * expected, delta=7.5e-5
            )
        self.assertAlmostEqual(
            fine.price(options[0], market()) + fine.price(options[2], market()),
            math.exp(-0.03),
            delta=1e-8,
        )
        # Nonzero log drift and a negative rate exercise the tilted series separately.
        for rate, dividend in ((0.04, 0.0), (-0.01, 0.02)):
            expected = continuous_reference(rate=rate, dividend=dividend)
            for option, value in zip(options, expected, strict=True):
                self.assertAlmostEqual(
                    fine.price(option, market(rate=rate, dividend=dividend)),
                    value,
                    delta=1e-5,
                )

    def test_scheduled_one_two_fixings_early_window_and_outside_continuation(self):
        engine = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
        )
        for dates in ([END], [FIRST, END], [FIRST]):
            changes = {
                "lower_barrier": 80.125,
                "upper_barrier": 120.125,
                "observation_mode": "scheduled",
                "observation_dates": dates,
            }
            options = (
                contract(**changes),
                contract(**changes, settlement_timing="at_hit"),
                contract(cash_double_no_touch, **changes),
            )
            expected = scheduled_reference(dates)
            for a, b in zip(expected, scheduled_reference(dates, 2048), strict=True):
                self.assertAlmostEqual(a, b, delta=1e-10)
            for option, value in zip(options, expected, strict=True):
                self.assertAlmostEqual(
                    engine.price(option, market(100.25)), value, delta=3e-5
                )
        future = contract(
            cash_double_no_touch,
            lower_barrier=80.125,
            upper_barrier=120.125,
            observation_mode="scheduled",
            observation_dates=[END],
        )
        self.assertAlmostEqual(
            engine.price(future, market(125)),
            math.exp(-0.03) * survival_at_fixing(125, 1),
            delta=3e-5,
        )

    def test_history_current_touch_past_payments_and_expiry(self):
        engine = pricing.FiniteDifferenceDoubleTouchEngine()
        later = START + timedelta(days=1)
        for timing in ("at_hit", "at_expiry"):
            option = contract(payout=3, settlement_timing=timing)
            for spot in (79, 80, 120, 121):
                self.assertAlmostEqual(
                    engine.price(option, market(spot)),
                    3 if timing == "at_hit" else 3 * math.exp(-0.03),
                    delta=1e-12,
                )
                self.assert_core_error(
                    ErrorCategory.INVALID_PARAMETER,
                    lambda spot=spot, option=option: engine.price(
                        option, market(spot, time=later)
                    ),
                )
            known = option.replace(touch_state="touched")
            self.assert_core_error(
                ErrorCategory.INVALID_OPTION,
                lambda known=known: engine.price(known, market()),
            )
            expected = 0 if timing == "at_hit" else 3 * math.exp(-0.03 * 364 / 365)
            self.assertAlmostEqual(
                engine.price(known, market(time=later)), expected, delta=1e-12
            )
            self.assert_core_error(
                ErrorCategory.UNSUPPORTED_OPERATION,
                lambda known=known, expected=expected: pricing.implied_volatility(
                    engine, known, market(time=later), expected
                ),
            )
        no_touch = contract(cash_double_no_touch, payout=3)
        for spot in (80, 120):
            self.assertEqual(engine.price(no_touch, market(spot)), 0)
        for option in (
            no_touch,
            contract(payout=3),
            contract(payout=3, settlement_timing="at_hit"),
        ):
            known = option.replace(touch_state="untouched")
            result = engine.price_with_greeks(known, market(time=END), all_greeks=True)
            self.assertEqual(result.price, 0 if known.is_one_touch else 3)
            self.assertIsNone(result.delta)
            self.assertIsNone(result.vega)
            for spot in (80, 120):
                self.assertEqual(
                    engine.price(known, market(spot, time=END)),
                    3 if known.is_one_touch else 0,
                )

    def test_early_window_utc_fixing_calendar_and_fixed_cash_greeks(self):
        engine = pricing.FiniteDifferenceDoubleTouchEngine()
        option = contract(
            cash_double_no_touch,
            payout=2,
            observation_mode="scheduled",
            observation_dates=[FIRST],
        )
        event = datetime.combine(FIRST, datetime.min.time(), UTC)
        self.assertEqual(engine.price(option, market(120, time=event)), 0)
        after = event + timedelta(seconds=1)
        self.assert_core_error(
            ErrorCategory.INVALID_PARAMETER,
            lambda: engine.price(option, market(130, time=after)),
        )
        known = option.replace(touch_state="untouched")
        for offset in (timedelta(seconds=1), timedelta(hours=12)):
            time = event + offset
            remaining = (
                datetime.combine(END, datetime.min.time(), UTC) - time
            ).total_seconds() / (86400 * 365)
            expected = 2 * math.exp(-0.03 * remaining)
            context = market(130, time=time)
            result = engine.price_with_greeks(known, context, all_greeks=True)
            self.assertAlmostEqual(result.price, expected, delta=1e-12)
            self.assertEqual(result.delta, 0)
            self.assertEqual(result.vega, 0)
            self.assertAlmostEqual(result.rho, -remaining * expected / 100, delta=1e-8)
            self.assert_core_error(
                ErrorCategory.UNSUPPORTED_OPERATION,
                lambda context=context, expected=expected: pricing.implied_volatility(
                    engine, known, context, expected
                ),
            )
        weekend = contract(
            cash_double_no_touch,
            observation_mode="scheduled",
            observation_dates=[date(2025, 1, 4)],
        )
        self.assert_core_error(
            ErrorCategory.INVALID_DATE,
            lambda: engine.price(weekend, market(130, calendar=weekdays_calendar())),
        )

    def test_greeks_iv_boundary_and_time_stencils(self):
        engine = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=800, time_step_count=1600
        )
        for option in (
            contract(touch_state="untouched"),
            contract(settlement_timing="at_hit", touch_state="untouched"),
            contract(cash_double_no_touch, touch_state="untouched"),
        ):
            result = engine.price_with_greeks(option, market(), all_greeks=True)
            bumped = price_differences(engine, option, market())
            for name in (
                "delta",
                "gamma",
                "vega",
                "rho",
                "theta",
                "charm",
                "color",
                "speed",
                "vanna",
                "zomma",
            ):
                self.assertAlmostEqual(
                    getattr(result, name), getattr(bumped, name), delta=1e-5
                )
            solved = pricing.implied_volatility(
                engine,
                option,
                market(),
                result.price,
                lower_bound=0.15,
                upper_bound=0.25,
            )
            self.assertAlmostEqual(solved, 0.2, delta=1e-6)
            self.assert_core_error(
                ErrorCategory.UNBRACKETED_VOLATILITY,
                lambda option=option: pricing.implied_volatility(
                    engine, option, market(), 2, lower_bound=0.15, upper_bound=0.25
                ),
            )
            for spot in (80, 120):
                boundary = engine.price_with_greeks(
                    option, market(spot), all_greeks=True
                )
                for name in ("delta", "gamma", "theta", "vega", "rho"):
                    self.assertIsNone(getattr(boundary, name))
        scheduled = contract(
            cash_double_no_touch,
            observation_mode="scheduled",
            observation_dates=[FIRST, END],
            touch_state="untouched",
        )
        result = engine.price_with_greeks(
            scheduled, market(time=FIRST), ["theta", "charm", "color"]
        )
        self.assertIsNone(result.theta)
        self.assertIsNone(result.charm)
        self.assertIsNone(result.color)

    def test_near_barrier_greeks_converge_to_independent_series(self):
        options = (
            contract(touch_state="untouched"),
            contract(settlement_timing="at_hit", touch_state="untouched"),
            contract(cash_double_no_touch, touch_state="untouched"),
        )
        # Absolute budgets use the library's per-unit, per-percent and per-day Greek units.
        budgets = {
            "delta": 1e-7,
            "gamma": 1e-7,
            "speed": 1e-5,
            "theta": 1e-8,
            "charm": 1e-8,
            "color": 1e-8,
            "vega": 1e-8,
            "vanna": 1e-7,
            "zomma": 1e-6,
            "rho": 1e-8,
        }
        coarse = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=800, time_step_count=200
        )
        fine = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=25600, time_step_count=1600
        )
        for spot in (80.02, 119.98):
            expected = reference_greeks(spot)
            more_terms = cash_reference(spot, count=128)
            integrated = continuous_reference(spot, count=16384)
            for index, option in enumerate(options):
                with self.subTest(spot=spot, option=option):
                    reference = expected[index]
                    price = cash_reference(spot)[index].price
                    self.assertAlmostEqual(price, integrated[index], delta=1e-9)
                    for name in ("gamma", "speed"):
                        self.assertAlmostEqual(
                            reference[name],
                            getattr(more_terms[index], name),
                            delta=1e-14,
                        )
                    rough = coarse.price_with_greeks(
                        option, market(spot), all_greeks=True
                    )
                    refined = fine.price_with_greeks(
                        option, market(spot), all_greeks=True
                    )
                    self.assertAlmostEqual(refined.price, price, delta=1e-7)
                    for name, budget in budgets.items():
                        with self.subTest(greek=name, budget=budget):
                            rough_error = abs(getattr(rough, name) - reference[name])
                            refined_error = abs(
                                getattr(refined, name) - reference[name]
                            )
                            self.assertLess(refined_error, rough_error)
                            self.assertLess(refined_error, budget)
                            if name in ("gamma", "speed"):
                                self.assertGreater(rough_error, budget)

    def test_node_curves_and_early_at_hit_curve_horizon(self):
        from kiyosi.instruments import CashOrNothingOption

        engine = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
        )
        for early in (False, True):
            horizon = FIRST if early else END
            rate = RateTermStructure(
                reference_time=START,
                times=[FIRST] if early else [FIRST, END],
                factors=[0.97] if early else [0.97, 0.99],
            )
            context = market(125, rate=rate)
            fields = {
                "lower_barrier": 80.125,
                "upper_barrier": 120.125,
                "observation_mode": "scheduled",
                "observation_dates": [horizon],
            }
            touched = sum(
                pricing.AnalyticDigitalEngine().price(
                    CashOrNothingOption(
                        option_type=direction,
                        strike=level,
                        payout=1,
                        effective_date=START,
                        expiry_date=horizon,
                    ),
                    context,
                )
                for direction, level in (("put", 80.125), ("call", 120.125))
            )
            option = contract(settlement_timing="at_hit", **fields)
            self.assertAlmostEqual(engine.price(option, context), touched, delta=3e-5)
            if early:
                self.assert_core_error(
                    ErrorCategory.INVALID_TIME_RANGE,
                    lambda fields=fields, context=context: engine.price(
                        contract(**fields), context
                    ),
                )
            else:
                no = contract(cash_double_no_touch, **fields)
                self.assertAlmostEqual(
                    engine.price(no, context), 0.99 - touched, delta=3e-5
                )

    def test_narrow_band_tiny_volatility_settings_and_concurrency(self):
        engine = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=160, time_step_count=800
        )
        option = contract(cash_double_no_touch, lower_barrier=99.5, upper_barrier=100.5)
        self.assertLess(engine.price(option, market()), 1e-8)
        self.assertAlmostEqual(
            engine.price(option, market(volatility=1e-12, dividend=0.03)),
            math.exp(-0.03),
            delta=1e-8,
        )
        self.assert_core_error(
            ErrorCategory.INVALID_PARAMETER,
            lambda: pricing.FiniteDifferenceDoubleTouchEngine(asset_step_count=2),
        )
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceDoubleTouchEngine(asset_step_count=True)
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceDoubleTouchEngine(time_step_count=1.5)
        unstable = pricing.FiniteDifferenceDoubleTouchEngine(
            asset_step_count=400, time_step_count=1, scheme="explicit_euler"
        )
        self.assert_core_error(
            ErrorCategory.INVALID_PARAMETER, lambda: unstable.price(option, market())
        )
        with ThreadPoolExecutor(max_workers=2) as executor:
            values = list(
                executor.map(lambda _: engine.price(option, market()), range(4))
            )
        self.assertEqual(values, [engine.price(option, market())] * 4)


if __name__ == "__main__":
    unittest.main()
