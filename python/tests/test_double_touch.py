"""Double-touch cash contracts and references independent of the PDE."""

import inspect
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta
from types import SimpleNamespace

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
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
    """Integrate log-Brownian survival and discounted exit probabilities.

    Parameters
    ----------
    spot : float, optional
        Underlying spot price.
    lower : float, optional
        Lower boundary of the surviving interval.
    upper : float, optional
        Upper boundary of the surviving interval.
    volatility : float, optional
        Annual volatility in decimal units.
    rate : float, optional
        Annual continuously compounded risk-free rate.
    dividend : float, optional
        Annual continuously compounded dividend yield.
    count : int, optional
        Integration panel count or series truncation budget.

    Returns
    -------
    tuple of float
        Expiry one-touch, at-hit one-touch and no-touch values, in that
        order.

    Notes
    -----
    The sine expansion solves the absorbing heat equation between log
    boundaries. An exponential tilt restores log-price drift.
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
    """Compute analytic spot derivatives of the killed log-Brownian solution.

    Parameters
    ----------
    spot : float
        Underlying spot price.
    volatility : float, optional
        Annual volatility in decimal units.
    rate : float, optional
        Annual continuously compounded risk-free rate.
    time : float, optional
        Remaining Actual/365 Fixed years.
    count : int, optional
        Integration panel count or series truncation budget.

    Returns
    -------
    list of types.SimpleNamespace
        Expiry one-touch, at-hit one-touch and no-touch prices with spot
        Greeks.
    """
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
    """Integrate survival through one or two independent normal increments.

    Parameters
    ----------
    dates : sequence of datetime.date
        One or two monitoring dates.
    count : int, optional
        Integration panel count or series truncation budget.

    Returns
    -------
    tuple of float
        Expiry one-touch, at-hit one-touch and no-touch values, in that
        order.
    """
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
    def assert_core_error(self, expected_error, operation):
        with self.assertRaises(expected_error) as caught:
            operation()
        self.assertIs(type(caught.exception), expected_error)

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
                        InvalidInputError,
                        lambda factory=factory, key=key, value=value: contract(
                            factory, **{key: value}
                        ),
                    )
            for changes, expected_error in (
                ({"lower_barrier": 120}, InvalidInputError),
                ({"lower_barrier": 121}, InvalidInputError),
                (
                    {"expiry_date": START - timedelta(days=1)},
                    InvalidInputError,
                ),
                ({"observation_mode": "scheduled"}, InvalidInputError),
                ({"observation_dates": [END]}, InvalidInputError),
                (
                    {
                        "observation_mode": "scheduled",
                        "observation_dates": [END, FIRST],
                    },
                    InvalidInputError,
                ),
            ):
                self.assert_core_error(
                    expected_error,
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
                    InvalidInputError,
                    lambda spot=spot, option=option: engine.price(
                        option, market(spot, time=later)
                    ),
                )
            known = option.replace(touch_state="touched")
            self.assert_core_error(
                InvalidInputError,
                lambda known=known: engine.price(known, market()),
            )
            expected = 0 if timing == "at_hit" else 3 * math.exp(-0.03 * 364 / 365)
            self.assertAlmostEqual(
                engine.price(known, market(time=later)), expected, delta=1e-12
            )
            self.assert_core_error(
                UnsupportedOperationError,
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
            result = engine.price_with_greeks(known, market(time=END), greeks="all")
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
            InvalidInputError,
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
            result = engine.price_with_greeks(known, context, greeks="all")
            self.assertAlmostEqual(result.price, expected, delta=1e-12)
            self.assertEqual(result.delta, 0)
            self.assertEqual(result.vega, 0)
            self.assertAlmostEqual(result.rho, -remaining * expected / 100, delta=1e-8)
            self.assert_core_error(
                UnsupportedOperationError,
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
            InvalidInputError,
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
            result = engine.price_with_greeks(option, market(), greeks="all")
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
                RootNotBracketedError,
                lambda option=option: pricing.implied_volatility(
                    engine, option, market(), 2, lower_bound=0.15, upper_bound=0.25
                ),
            )
            for spot in (80, 120):
                boundary = engine.price_with_greeks(option, market(spot), greeks="all")
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
            asset_step_count=19200, time_step_count=1600
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
                    rough = coarse.price_with_greeks(option, market(spot), greeks="all")
                    refined = fine.price_with_greeks(option, market(spot), greeks="all")
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
                    InvalidInputError,
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
            InvalidInputError,
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
            InvalidInputError, lambda: unstable.price(option, market())
        )
        with ThreadPoolExecutor(max_workers=2) as executor:
            values = list(
                executor.map(lambda _: engine.price(option, market()), range(4))
            )
        self.assertEqual(values, [engine.price(option, market())] * 4)


class AnalyticDoubleTouchTests(unittest.TestCase):
    def test_analytic_exit_density_series_and_discounted_peaks(self):
        engine = pricing.AnalyticDoubleTouchEngine()
        for sigma in (0.25, 0.6):
            for rate in (-0.03, 0.03):
                expected = continuous_reference(
                    volatility=sigma, rate=rate, count=8192
                )[1]
                actual = engine.price(
                    contract(settlement_timing="at_hit"),
                    market(rate=rate, volatility=sigma),
                )
                self.assertAlmostEqual(actual, expected, delta=1e-11)
        for payout in (1, 1e300):
            band, x = math.log(120 / 80), math.log(100 / 80)
            expected = math.exp(
                math.log(payout)
                + math.log(math.cosh(200 * (x - band / 2)))
                - math.log(math.cosh(100 * band))
            )
            actual = engine.price(
                contract(payout=payout, settlement_timing="at_hit"),
                market(rate=800, dividend=799.98),
            )
            self.assertAlmostEqual(actual / expected, 1, delta=1e-10)
        for rate in (-0.03, 0.03):
            k = math.sqrt(2 * abs(rate)) / 0.2
            half_band = math.log(100.5 / 99.5) / 2
            offset = math.log(100 / 99.5) - half_band
            transform = math.cosh if rate > 0 else math.cos
            expected = transform(k * offset) / transform(k * half_band)
            actual = engine.price(
                contract(
                    lower_barrier=99.5, upper_barrier=100.5, settlement_timing="at_hit"
                ),
                market(rate=rate, dividend=rate - 0.02),
            )
            self.assertAlmostEqual(actual, expected, delta=1e-11)
        for payout in (1, 1e300):
            option = contract(
                lower_barrier=70,
                upper_barrier=140,
                payout=payout,
                expiry_date=START + timedelta(days=1),
                settlement_timing="at_hit",
            )
            self.assertEqual(
                engine.price(
                    option, market(rate=1e-10, dividend=0.03, volatility=0.01)
                ),
                0,
            )

    def test_analytic_early_at_hit_payments_avoid_negative_rate_cancellation(self):
        engine = pricing.AnalyticDoubleTouchEngine()
        for up in (True, False):
            for sigma in (0.001, 1e-6, 1e-12):
                rate = -40
                dividend = rate - 0.03 if up else rate + 0.03 - sigma**2
                drift = rate - dividend - sigma**2 / 2
                towards = drift if up else -drift
                option = contract(
                    lower_barrier=80 if up else 100 / 1.003,
                    upper_barrier=100.3 if up else 125,
                    settlement_timing="at_hit",
                )
                # The far barrier and hits after expiry contribute below 1e-140.
                expected = math.exp(
                    -2
                    * rate
                    * math.log(1.003)
                    / (towards + math.sqrt(towards**2 + 2 * rate * sigma**2))
                )
                with self.subTest(up=up, sigma=sigma):
                    actual = engine.price(
                        option, market(rate=rate, dividend=dividend, volatility=sigma)
                    )
                    self.assertAlmostEqual(actual / expected, 1, delta=1e-9)

    def test_analytic_discount_and_payout_scaling_preserve_representable_prices(self):
        engine = pricing.AnalyticDoubleTouchEngine()
        # Independent driftless Fourier survival/exit series with cash in the exponent.
        for rate, payout, one, no, hit in (
            (
                -800,
                1e-300,
                1.6945880848984272e47,
                1.0317864872142041e47,
                1.5506138528255889e44,
            ),
            (800, 1e300, 2.2797808602039585e-48, 1.3880937239736419e-48, None),
        ):
            context = market(rate=rate, dividend=rate - 0.02)
            for factory, timing, expected in (
                (cash_double_touch, "at_expiry", one),
                (cash_double_no_touch, "at_expiry", no),
                (cash_double_touch, "at_hit", hit),
            ):
                if expected is None:
                    continue
                changes = (
                    {"settlement_timing": timing}
                    if factory is cash_double_touch
                    else {}
                )
                option = contract(factory, payout=payout, **changes)
                with self.subTest(rate=rate, factory=factory, timing=timing):
                    actual = engine.price(option, context)
                    self.assertAlmostEqual(
                        actual / expected,
                        1,
                        delta=1e-8 if timing == "at_hit" else 1e-11,
                    )
                    if rate < 0:
                        with self.assertRaises(ComputationFailureError):
                            engine.price(
                                contract(factory, payout=1, **changes), context
                            )

    def test_analytic_expiry_payments_match_live_quantlib(self):
        saved = ql.SavedSettings()
        try:
            start = ql.Date(1, 1, 2025)
            ql.Settings.instance().evaluationDate = start
            dc = ql.Actual365Fixed()
            for days, rate, dividend, volatility in (
                (365, 0.03, 0.01, 0.2),
                (365, -0.01, 0.02, 0.5),
                (7, 0.04, 0.0, 0.2),
            ):
                for spot in (80.02, 100, 119.98):
                    process = ql.BlackScholesMertonProcess(
                        ql.QuoteHandle(ql.SimpleQuote(spot)),
                        ql.YieldTermStructureHandle(
                            ql.FlatForward(start, dividend, dc)
                        ),
                        ql.YieldTermStructureHandle(ql.FlatForward(start, rate, dc)),
                        ql.BlackVolTermStructureHandle(
                            ql.BlackConstantVol(
                                start, ql.NullCalendar(), volatility, dc
                            )
                        ),
                    )
                    for factory, kind in (
                        (cash_double_touch, ql.DoubleBarrier.KnockIn),
                        (cash_double_no_touch, ql.DoubleBarrier.KnockOut),
                    ):
                        reference = ql.DoubleBarrierOption(
                            kind,
                            80,
                            120,
                            0,
                            ql.CashOrNothingPayoff(ql.Option.Call, 0, 7.5),
                            ql.EuropeanExercise(start + days),
                        )
                        reference.setPricingEngine(
                            ql.AnalyticDoubleBarrierBinaryEngine(process)
                        )
                        option = contract(
                            factory,
                            payout=7.5,
                            expiry_date=START + timedelta(days=days),
                        )
                        actual = pricing.AnalyticDoubleTouchEngine().price(
                            option,
                            market(
                                spot,
                                rate=rate,
                                dividend=dividend,
                                volatility=volatility,
                            ),
                        )
                        with self.subTest(
                            days=days, rate=rate, spot=spot, factory=factory
                        ):
                            self.assertAlmostEqual(actual, reference.NPV(), delta=1e-10)
        finally:
            del saved

    def test_analytic_three_cashflows_match_independent_series_and_scale(self):
        engine = pricing.AnalyticDoubleTouchEngine()
        for rate, dividend in ((0.03, 0.01), (0.04, 0), (-0.01, 0.02), (0, 0.01)):
            reference = continuous_reference(rate=rate, dividend=dividend, count=8192)
            for factory, timing, expected in zip(
                (cash_double_touch, cash_double_touch, cash_double_no_touch),
                ("at_expiry", "at_hit", "at_expiry"),
                reference,
                strict=True,
            ):
                changes = (
                    {"settlement_timing": timing}
                    if factory is cash_double_touch
                    else {}
                )
                for payout in (1, 7.5):
                    option = contract(factory, payout=payout, **changes)
                    with self.subTest(rate=rate, option=option):
                        self.assertAlmostEqual(
                            engine.price(option, market(rate=rate, dividend=dividend)),
                            payout * expected,
                            delta=payout * 1e-10,
                        )

    def test_analytic_history_current_boundaries_and_expiry(self):
        engine = pricing.AnalyticDoubleTouchEngine()
        for option in (
            contract(payout=3),
            contract(payout=3, settlement_timing="at_hit"),
            contract(cash_double_no_touch, payout=3),
        ):
            at_hit = option.settlement_timing == "at_hit"
            for spot in (79, 80, 120, 121):
                expected = (
                    (3 if at_hit else 3 * math.exp(-0.03)) if option.is_one_touch else 0
                )
                self.assertAlmostEqual(
                    engine.price(option, market(spot)), expected, delta=1e-12
                )
                with self.assertRaises(InvalidInputError):
                    engine.price(option, market(spot, time=START + timedelta(days=1)))
            prior = option.replace(touch_state="touched")
            with self.assertRaises(InvalidInputError):
                engine.price(prior, market())
            later = market(time=START + timedelta(days=1))
            expected = (
                0
                if at_hit or not option.is_one_touch
                else 3 * math.exp(-0.03 * 364 / 365)
            )
            self.assertAlmostEqual(engine.price(prior, later), expected, delta=1e-12)
            with self.assertRaises(UnsupportedOperationError):
                pricing.implied_volatility(engine, prior, later, expected)
            known = option.replace(touch_state="untouched")
            for spot in (80, 100, 120):
                terminal = engine.price_with_greeks(
                    known, market(spot, time=END), greeks="all"
                )
                self.assertEqual(
                    terminal.price, 3 if option.is_one_touch == (spot != 100) else 0
                )
                self.assertIsNone(terminal.delta)
                self.assertIsNone(terminal.vega)
            self.assertIsNone(option.touch_state)

    def test_analytic_schedules_validate_then_resolve_or_reject(self):
        engine = pricing.AnalyticDoubleTouchEngine()
        for option in (
            contract(observation_mode="scheduled", observation_dates=[FIRST]),
            contract(
                observation_mode="scheduled",
                observation_dates=[FIRST],
                settlement_timing="at_hit",
            ),
            contract(
                cash_double_no_touch,
                observation_mode="scheduled",
                observation_dates=[FIRST],
            ),
        ):
            with self.assertRaises(UnsupportedOperationError):
                engine.price(option, market(125))
            after = FIRST + timedelta(days=1)
            with self.assertRaises(InvalidInputError):
                engine.price(option, market(time=after))
            known = option.replace(touch_state="untouched")
            for spot in (80, 120):
                expected = (
                    0
                    if not option.is_one_touch
                    else (
                        1
                        if option.settlement_timing == "at_hit"
                        else math.exp(-0.03 * (END - FIRST).days / 365)
                    )
                )
                self.assertAlmostEqual(
                    engine.price(known, market(spot, time=FIRST)), expected, delta=1e-12
                )
            expected = (
                0 if option.is_one_touch else math.exp(-0.03 * (END - after).days / 365)
            )
            self.assertAlmostEqual(
                engine.price(known, market(125, time=after)), expected, delta=1e-12
            )
        weekend = contract(
            observation_mode="scheduled", observation_dates=[date(2025, 1, 4)]
        )
        with self.assertRaises(InvalidInputError):
            engine.price(weekend, market(calendar=weekdays_calendar()))

    def test_analytic_node_curves_and_numerical_failures(self):
        engine = pricing.AnalyticDoubleTouchEngine()
        curve = RateTermStructure(reference_time=START, times=[END], factors=[0.97])
        for rates in ({"rate": curve}, {"dividend": curve}):
            for spot in (100, 120):
                context = market(spot, **rates)
                option = contract()
                with self.assertRaises(UnsupportedOperationError):
                    engine.price(option, context)
                with self.assertRaises(UnsupportedOperationError):
                    engine.price_with_greeks(option, context, greeks="all")
                with self.assertRaises(UnsupportedOperationError):
                    pricing.implied_volatility(engine, option, context, math.nan)
        with self.assertRaises(ComputationFailureError):
            engine.price(contract(), market(volatility=1e308))
        for day in (START - timedelta(days=1), END + timedelta(days=1)):
            with self.assertRaises(InvalidInputError):
                engine.price(contract(), market(time=day))

    def test_analytic_greeks_iv_signatures_and_concurrent_pricing(self):
        engine = pricing.AnalyticDoubleTouchEngine()
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
        self.assertEqual(str(inspect.signature(type(engine))), "()")
        signature = inspect.signature(engine.price_with_greeks)
        for name in ("spot_shift", "volatility_shift", "rate_shift", "time_shift_days"):
            self.assertIs(
                signature.parameters[name].kind, inspect.Parameter.KEYWORD_ONLY
            )
            self.assertIs(signature.parameters[name].default, Ellipsis)
        options = (
            contract(touch_state="untouched"),
            contract(settlement_timing="at_hit", touch_state="untouched"),
            contract(cash_double_no_touch, touch_state="untouched"),
        )
        for spot in (80.02, 100, 119.98):
            for option, expected in zip(options, reference_greeks(spot), strict=True):
                result = engine.price_with_greeks(option, market(spot), greeks="all")
                for name, value in expected.items():
                    with self.subTest(spot=spot, option=option, greek=name):
                        if name == "speed" and spot != 100:
                            self.assertIsNone(result.speed)
                            continue
                        self.assertAlmostEqual(
                            getattr(result, name), value, delta=budgets[name]
                        )
        for option in options:
            price = engine.price(option, market())
            default = engine.price_with_greeks(option, market(), greeks="all")
            explicit = engine.price_with_greeks(
                option,
                market(),
                greeks="all",
                spot_shift=0.01,
                volatility_shift=0.0001,
                rate_shift=0.0001,
                time_shift_days=1,
            )
            for name in reference_greeks(100)[0]:
                self.assertEqual(getattr(default, name), getattr(explicit, name))
            self.assertAlmostEqual(
                pricing.implied_volatility(
                    engine, option, market(), price, lower_bound=0.15, upper_bound=0.25
                ),
                0.2,
                delta=1e-7,
            )
            for spot in (80, 120):
                result = engine.price_with_greeks(option, market(spot), greeks="all")
                self.assertIsNone(result.delta)
                self.assertIsNone(result.vega)
        with ThreadPoolExecutor(max_workers=2) as executor:
            values = list(
                executor.map(lambda _: engine.price(options[0], market()), range(4))
            )
        self.assertEqual(values, [values[0]] * 4)
        self.assertAlmostEqual(values[0], 0.6031839700294364, delta=1e-12)


if __name__ == "__main__":
    unittest.main()
