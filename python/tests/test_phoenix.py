"""Phoenix contracts and quadrature through the installed Rust public API."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

from kiyosi import (
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    AutocallableObservation,
    PhoenixOption,
    autocallable_observations,
)
from kiyosi.market import RateTermStructure, all_days_calendar, custom_calendar
from kiyosi.pricing import QuadratureAutocallableEngine, implied_volatility
from test_snowball import market, structured

GREEKS = (
    "delta",
    "gamma",
    "speed",
    "vega",
    "vanna",
    "zomma",
    "rho",
    "theta",
    "charm",
    "color",
)

START, MIDDLE, END = date(2025, 1, 1), date(2025, 2, 3), date(2025, 4, 1)


def contract(**changes):
    fields = {
        "coupon_rate": 0.365,
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 60,
        "knock_in_observation_mode": "at_expiry",
        "effective_date": START,
        "expiry_date": END,
        "barrier_state": "none",
        "observations": autocallable_observations(
            [MIDDLE, END], [110, 110], coupon_barrier_levels=[90, 90]
        ),
    }
    fields.update(changes)
    return PhoenixOption(**fields)


class PhoenixTests(unittest.TestCase):
    def test_current_conditional_coupon_and_settlement_boundaries(self):
        engine = QuadratureAutocallableEngine()
        for coupon in (0.365, -0.365):
            note = contract(coupon_rate=coupon, principal_ratio=0.25)
            for spot, base, paid in (
                (60, -0.15, False),
                (70, -0.05, False),
                (80, 0.25, False),
                (90, 0.25, True),
                (110, 0.25, True),
            ):
                self.assertAlmostEqual(
                    engine.price(note, market(spot, END)),
                    base + (coupon * 57 / 365 if paid else 0),
                )
            self.assertAlmostEqual(
                engine.price(note, market(110, MIDDLE)), 0.25 + coupon * 33 / 365
            )

    def assert_rejection(self, expected, operation, *args, **kwargs):
        with self.assertRaises(expected) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def test_owned_defaults_replacement_and_strict_representations(self):
        note = contract(barrier_state=None)
        self.assertEqual(note.principal_ratio, 1)
        self.assertIsNone(note.barrier_state)
        self.assertEqual(note.coupon_barrier_levels, [90, 90])
        records = note.observations
        records.clear()
        self.assertEqual(note.observation_dates, [MIDDLE, END])
        self.assertEqual(
            note.replace(barrier_state="none").replace(barrier_state=None), note
        )
        self.assertIn("PhoenixOption(coupon_rate=", repr(note))
        for fields in (
            {"coupon_rate": True},
            {"coupon_rate": "0.1"},
            {"principal_ratio": None},
            {"initial_spot": True},
            {"observations": [(END, 110)]},
            {"effective_date": datetime(2025, 1, 1)},
            {"knock_in_observation_mode": 1},
            {"barrier_state": True},
        ):
            with self.subTest(fields=fields), self.assertRaises(TypeError):
                contract(**fields)
        for fields in (
            {"knock_in_observation_mode": "daily"},
            {"barrier_state": "unknown"},
        ):
            with self.assertRaises(ValueError):
                contract(**fields)
        for fields in ({}, {"observations": []}, {"barrier_state": True}):
            with self.assertRaises(TypeError):
                note.replace(**fields)
        with self.assertRaises(AttributeError):
            note.coupon_rate = 1
        engine = QuadratureAutocallableEngine()
        self.assertEqual(
            (engine.asset_step_count, engine.integration_standard_deviations), (512, 8)
        )
        with self.assertRaises(TypeError):
            engine.price(object(), market())
        with self.assertRaises(TypeError):
            implied_volatility(engine, object(), market(), 1)

    def test_core_validation_order_errors_and_history(self):
        for fields, expected_error in (
            ({"coupon_rate": math.nan}, InvalidInputError),
            ({"initial_spot": 0}, InvalidInputError),
            ({"lower_strike": 101}, InvalidInputError),
            ({"principal_ratio": -1}, InvalidInputError),
            ({"observations": []}, InvalidInputError),
            (
                {"observations": [AutocallableObservation(END, 110)] * 2},
                InvalidInputError,
            ),
            ({"expiry_date": date(2024, 1, 1)}, InvalidInputError),
        ):
            self.assert_rejection(expected_error, contract, **fields)
        for value in (-1, math.nan, math.inf):
            self.assert_rejection(
                InvalidInputError,
                contract,
                observations=[
                    AutocallableObservation(END, 110, coupon_barrier_level=value)
                ],
            )
        self.assert_rejection(
            InvalidInputError,
            contract,
            observations=[AutocallableObservation(END, 110, coupon_rate=0.1)],
        )
        engine = QuadratureAutocallableEngine()
        unknown = contract(
            knock_in_observation_mode="every_trading_day", barrier_state=None
        )
        self.assert_rejection(
            InvalidInputError, engine.price, unknown, market(valuation=MIDDLE)
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            contract(barrier_state="knocked_in"),
            market(valuation=END),
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            contract(barrier_state="knocked_out"),
            market(),
        )
        nontrading = contract(
            expiry_date=date(2025, 4, 5),
            observations=[AutocallableObservation(date(2025, 2, 1), 110)],
        )
        self.assert_rejection(InvalidInputError, engine.price, nontrading, market())
        nontrading = contract(expiry_date=date(2025, 4, 5), barrier_state=None)
        self.assert_rejection(
            InvalidInputError, engine.price, nontrading, market(valuation=END)
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            nontrading.replace(barrier_state="none"),
            market(valuation=END),
        )
        for valuation in (date(2024, 12, 31), datetime(2025, 4, 1, 0, 0, 0, 1, UTC)):
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                contract(),
                market(valuation=valuation),
            )
        curve = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.04 * 90 / 365)]
        )
        self.assert_rejection(
            UnsupportedOperationError,
            engine.price_with_greeks,
            contract(),
            market(rate=curve),
            [],
        )
        self.assert_rejection(
            InvalidInputError, engine.price_with_greeks, contract(), market(), []
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            contract(),
            market(),
            "delta",
            spot_shift=0,
        )
        self.assertIsNone(unknown.barrier_state)

    def test_independent_quantlib_terminal_portfolios(self):
        engine = QuadratureAutocallableEngine()
        for name, kind, inputs in structured.scenarios():
            if kind != "PhoenixOption":
                continue
            with self.subTest(name=name):
                start, end, valuation = (
                    date.fromisoformat(inputs[k])
                    for k in ("effective_date", "expiry_date", "valuation")
                )
                note = contract(
                    effective_date=start,
                    expiry_date=end,
                    coupon_rate=inputs["coupon_rate"],
                    principal_ratio=inputs["principal_ratio"],
                    barrier_state=inputs["history"],
                    knock_in_level=inputs["knock_in_level"],
                    knock_in_observation_mode=inputs["knock_in_mode"],
                    observations=autocallable_observations(
                        [end],
                        [110],
                        coupon_barrier_levels=[inputs["coupon_barrier_level"]],
                    ),
                )
                context = market(
                    inputs["spot"], valuation, inputs["volatility"], inputs["rate"]
                )
                self.assertAlmostEqual(
                    engine.price(note, context),
                    structured.price(kind, inputs),
                    delta=1e-10,
                )

    def test_signed_coupons_are_discounted_on_their_own_dates_and_past_coupons_excluded(
        self,
    ):
        engine = QuadratureAutocallableEngine()
        for coupon in (0.08, -0.2, 0):
            note = contract(
                coupon_rate=coupon,
                observations=autocallable_observations([MIDDLE, END], [200, 190]),
            )
            for valuation in (START, MIDDLE, datetime(2025, 2, 3, 0, 0, 0, 1, UTC)):
                v = (
                    valuation
                    if isinstance(valuation, datetime)
                    else datetime.combine(valuation, datetime.min.time(), UTC)
                )
                t = lambda d, v=v: (
                    (datetime.combine(d, datetime.min.time(), UTC) - v).total_seconds()
                    / (365 * 86400)
                )
                expected = math.exp(-0.04 * t(END))
                for previous, current in ((START, MIDDLE), (MIDDLE, END)):
                    if t(current) >= 0:
                        expected += (
                            coupon
                            * (current - previous).days
                            / 365
                            * math.exp(-0.04 * t(current))
                        )
                self.assertAlmostEqual(
                    engine.price(note, market(valuation=valuation, volatility=1e-8)),
                    expected,
                    delta=1e-9,
                )

    def test_unconditional_coupon_tails_and_fixed_cashflow_identifiability(self):
        middle, end = date(2025, 7, 1), date(2026, 1, 1)
        note = contract(
            coupon_rate=-0.5,
            lower_strike=100,
            principal_ratio=0.25,
            expiry_date=end,
            observations=autocallable_observations([middle, end], [100, 100]),
        )
        time = 181 / 365
        survival = math.erfc((0.03 - 2) * math.sqrt(time) / (2 * math.sqrt(2))) / 2
        expected = (-0.5 * time + 0.25 * (1 - survival)) * math.exp(-0.04 * time) + (
            0.25 - 0.5 * (1 - time)
        ) * survival * math.exp(-0.04)
        for settings in (
            {},
            {"asset_step_count": 128, "integration_standard_deviations": 0.5},
        ):
            self.assertAlmostEqual(
                QuadratureAutocallableEngine(**settings).price(
                    note, market(volatility=2)
                ),
                expected,
                delta=1e-10,
            )
        # Only a final unconditional coupon remains, so volatility cannot affect cashflows.
        fixed = contract(
            lower_strike=100, observations=autocallable_observations([END], [110])
        )
        engine = QuadratureAutocallableEngine()
        quote = engine.price(fixed, market())
        self.assertAlmostEqual(
            quote, (1 + 0.365 * 90 / 365) * math.exp(-0.04 * 90 / 365)
        )
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            fixed,
            market(),
            quote,
        )

    def test_event_greeks_settled_history_and_solver_failures(self):
        note = contract()
        engine = QuadratureAutocallableEngine()
        result = engine.price_with_greeks(note, market(90, MIDDLE), all_greeks=True)
        self.assertIsNone(result.delta)
        self.assertIsNone(result.gamma)
        self.assertIsNone(result.theta)
        self.assertIsNotNone(result.vega)
        self.assertIsNotNone(result.rho)
        result = engine.price_with_greeks(note, market(110, MIDDLE), all_greeks=True)
        self.assertEqual((result.vega, result.rho), (0, 0))
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            note,
            market(110, MIDDLE),
            result.price,
        )
        settled = note.replace(barrier_state="knocked_out")
        self.assertEqual(engine.price(settled, market(valuation=END)), 0)
        result = engine.price_with_greeks(note, market(90, END), all_greeks=True)
        self.assertTrue(all(getattr(result, g) is None for g in GREEKS))
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            note,
            market(90, END),
            result.price,
        )
        ctx = market(103)
        quote = engine.price(note, ctx)
        solved = implied_volatility(
            engine, note, ctx, quote, lower_bound=0.2, upper_bound=0.4
        )
        self.assertAlmostEqual(solved, 0.3, delta=1e-6)
        self.assert_rejection(
            RootNotBracketedError,
            implied_volatility,
            engine,
            note,
            ctx,
            10,
            lower_bound=0.2,
            upper_bound=0.4,
        )
        self.assert_rejection(
            NonConvergenceError,
            implied_volatility,
            engine,
            note,
            ctx,
            quote,
            lower_bound=0.2,
            upper_bound=0.4,
            max_iterations=1,
            price_tolerance=1e-15,
            parameter_tolerance=1e-15,
        )
        self.assertEqual(ctx.model_parameters.volatility, 0.3)
        self.assertEqual(note.barrier_state, "none")

    def test_two_event_prices_match_independent_conditional_payoff_integral(self):
        calendar = custom_calendar(
            base=all_days_calendar(),
            closed_dates=[
                START + timedelta(days=i)
                for i in range(91)
                if START + timedelta(days=i) not in (START, MIDDLE, END)
            ],
        )
        first, remaining = 33 / 365, 57 / 365
        sigma, rate, dividend = 0.3, 0.04, 0.01
        drift = rate - dividend - sigma**2 / 2
        width = sigma * math.sqrt(first)
        normal = lambda z: math.erfc(-z / math.sqrt(2)) / 2
        z_for = lambda level: (math.log(level / 100) - drift * first) / width

        def terminal_expectation(spot, previously_in, coupon):
            threshold = 100 if previously_in else 95
            # Principal and capped loss are a piecewise affine terminal portfolio.
            pieces = [
                (0, 60, 0.6, 0),
                (60, threshold, 0, 0.01),
                (threshold, math.inf, 1, 0),
            ]
            deviation = sigma * math.sqrt(remaining)
            mean = math.log(spot) + drift * remaining
            value = coupon * remaining * normal((mean - math.log(90)) / deviation)
            for lower, upper, cash, asset in pieces:
                low = (math.log(lower) - mean) / deviation if lower else -math.inf
                high = (math.log(upper) - mean) / deviation
                value += cash * (normal(high) - normal(low))
                value += (
                    asset
                    * spot
                    * math.exp((rate - dividend) * remaining)
                    * (normal(high - deviation) - normal(low - deviation))
                )
            return value * math.exp(-rate * remaining)

        for mode in ("at_expiry", "every_trading_day"):
            for coupon in (0.08, -0.2):

                def reference(panels, mode=mode, coupon=coupon):
                    value = (1 + coupon * first) * (1 - normal(z_for(110)))
                    for left, right in zip(
                        [-12, z_for(90), z_for(95)],
                        [z_for(90), z_for(95), z_for(110)],
                        strict=True,
                    ):
                        h = (right - left) / panels
                        points = (
                            [math.nextafter(left, right)]
                            + [left + h * i for i in range(1, panels)]
                            + [math.nextafter(right, left)]
                        )

                        def integrand(z, right=right):
                            spot = 100 * math.exp(drift * first + width * z)
                            payoff = terminal_expectation(
                                spot,
                                mode == "every_trading_day" and right <= z_for(95),
                                coupon,
                            )
                            payoff += coupon * first if right > z_for(90) else 0
                            return (
                                payoff * math.exp(-z * z / 2) / math.sqrt(2 * math.pi)
                            )

                        value += (
                            h
                            / 3
                            * math.fsum(
                                integrand(z)
                                * (1 if i in (0, panels) else 4 if i % 2 else 2)
                                for i, z in enumerate(points)
                            )
                        )
                    return value * math.exp(-rate * first)

                coarse, fine = reference(1024), reference(2048)
                self.assertAlmostEqual(coarse, fine, delta=1e-9)
                note = contract(
                    coupon_rate=coupon,
                    knock_in_level=95,
                    knock_in_observation_mode=mode,
                )
                self.assertAlmostEqual(
                    QuadratureAutocallableEngine().price(
                        note, market(calendar=calendar)
                    ),
                    fine,
                    delta=1e-6,
                )

    def test_grid_domain_and_bump_convergence(self):
        values = []
        for mode in ("at_expiry", "every_trading_day"):
            note = contract(
                knock_in_level=95,
                knock_in_observation_mode=mode,
                observations=autocallable_observations(
                    [MIDDLE, date(2025, 3, 3), END],
                    [110, 108, 105],
                    coupon_barrier_levels=[95] * 3,
                ),
            )
            prices = [
                QuadratureAutocallableEngine(
                    asset_step_count=n, integration_standard_deviations=e
                ).price(note, market())
                for n, e in ((512, 8), (1024, 8), (1024, 10))
            ]
            self.assertAlmostEqual(prices[0], prices[1], delta=1e-4)
            self.assertAlmostEqual(prices[1], prices[2], delta=1e-4)
            values.append(prices[0])
        self.assertLess(values[1], values[0] - 0.001)
        end = date(2026, 1, 1)
        note = contract(
            expiry_date=end,
            observations=autocallable_observations(
                [date(2025, 6, 2), end], [110, 105], coupon_barrier_levels=[95, 95]
            ),
        )
        context = market(101.5, date(2025, 1, 15), volatility=0.25)
        coarse = {"spot_shift": 0.5, "volatility_shift": 0.002, "rate_shift": 0.001}
        fine = {"spot_shift": 0.25, "volatility_shift": 0.001, "rate_shift": 0.0005}
        standard = QuadratureAutocallableEngine().price_with_greeks(
            note, context, all_greeks=True, **coarse
        )
        engine = QuadratureAutocallableEngine(asset_step_count=1024)
        refined = engine.price_with_greeks(note, context, all_greeks=True, **coarse)
        smaller = engine.price_with_greeks(note, context, all_greeks=True, **fine)
        tolerances = {
            "delta": (3e-6, 0.005),
            "gamma": (3e-7, 0.01),
            "speed": (1e-7, 0.05),
            "vega": (3e-6, 0.005),
            "vanna": (1e-7, 0.05),
            "zomma": (1e-7, 0.05),
            "rho": (3e-6, 0.005),
            "theta": (1e-7, 0.02),
            "charm": (1e-7, 0.05),
            "color": (1e-7, 0.05),
        }
        self.assertAlmostEqual(standard.price, refined.price, delta=1e-4)
        for greek, (absolute, relative) in tolerances.items():
            target = getattr(refined, greek)
            self.assertIsNotNone(target)
            for actual in (getattr(standard, greek), getattr(smaller, greek)):
                self.assertAlmostEqual(
                    actual, target, delta=absolute + relative * abs(target)
                )

    def test_all_greeks_and_volatility_recovery_against_digital_formula(self):
        # Equal terminal strikes remove KI dependence, leaving cash plus a digital.
        end = date(2026, 1, 1)
        note = contract(
            expiry_date=end,
            lower_strike=100,
            coupon_rate=0.1,
            observations=autocallable_observations(
                [end], [100], coupon_barrier_levels=[100]
            ),
        )
        engine = QuadratureAutocallableEngine()
        spot, vol, rate = 103, 0.3, -0.04

        def value(s=spot, sigma=vol, rf=rate, t=1):
            d2 = (math.log(s / 100) + (rf - 0.01 - sigma**2 / 2) * t) / (
                sigma * math.sqrt(t)
            )
            return math.exp(-rf * t) * (1 + 0.1 * math.erfc(-d2 / math.sqrt(2)) / 2)

        h, v, r = 0.25, (vol + 0.002) - vol, (rate + 0.001) - rate

        def delta(sigma=vol, t=1):
            return (value(spot + h, sigma, t=t) - value(spot - h, sigma, t=t)) / (2 * h)

        def gamma(sigma=vol, t=1):
            return (
                value(spot + h, sigma, t=t)
                - 2 * value(sigma=sigma, t=t)
                + value(spot - h, sigma, t=t)
            ) / h**2

        dt = 1 / 365
        # At the effective date the shared contract uses a forward time stencil.
        expected = {
            "delta": delta(),
            "gamma": gamma(),
            "speed": (
                value(spot + 2 * h)
                - 2 * value(spot + h)
                + 2 * value(spot - h)
                - value(spot - 2 * h)
            )
            / (2 * h**3),
            "vega": (value(sigma=vol + v) - value(sigma=vol - v)) / (200 * v),
            "vanna": (delta(vol + v) - delta(vol - v)) / (200 * v),
            "zomma": (gamma(vol + v) - gamma(vol - v)) / (200 * v),
            "rho": (value(rf=rate + r) - value(rf=rate - r)) / (200 * r),
            "theta": value(t=1 - dt) - value(),
            "charm": delta(t=1 - dt) - delta(),
            "color": gamma(t=1 - dt) - gamma(),
        }
        context = market(spot, volatility=vol, rate=rate)
        result = engine.price_with_greeks(
            note,
            context,
            all_greeks=True,
            spot_shift=h,
            volatility_shift=0.002,
            rate_shift=0.001,
        )
        for greek, target in expected.items():
            with self.subTest(greek=greek):
                self.assertAlmostEqual(
                    getattr(result, greek), target, delta=1e-9 + 1e-5 * abs(target)
                )
        quote = value()
        solved = implied_volatility(
            engine,
            note,
            context,
            quote,
            lower_bound=0.25,
            upper_bound=0.4,
            price_tolerance=1e-11,
        )
        self.assertAlmostEqual(solved, vol, delta=1e-7)
        self.assert_rejection(
            RootNotBracketedError,
            implied_volatility,
            engine,
            note,
            context,
            10,
            lower_bound=0.1,
            upper_bound=0.5,
        )
        self.assertEqual(context.model_parameters.volatility, vol)
        with ThreadPoolExecutor(max_workers=3) as pool:
            prices = list(
                pool.map(
                    lambda s: engine.price(note, market(s, volatility=vol, rate=rate)),
                    [90, 100, 110],
                )
            )
        for s, actual in zip([90, 100, 110], prices, strict=True):
            self.assertAlmostEqual(actual, value(s=s), delta=1e-12)

    def test_coupon_above_knock_out_zero_coupon_and_expiry_without_observation(self):
        engine = QuadratureAutocallableEngine()
        note = contract(
            observations=autocallable_observations(
                [MIDDLE, END], [110, 110], coupon_barrier_levels=[120, 120]
            )
        )
        self.assertAlmostEqual(engine.price(note, market(110, MIDDLE)), 1)
        self.assertAlmostEqual(
            engine.price(note, market(120, MIDDLE)), 1 + 0.365 * 33 / 365
        )
        zero = contract(coupon_rate=0)
        result = engine.price_with_greeks(zero, market(90, MIDDLE), "delta")
        self.assertIsNotNone(result.delta)
        no_expiry_event = contract(
            observations=autocallable_observations(
                [MIDDLE], [110], coupon_barrier_levels=[90]
            )
        )
        self.assertEqual(engine.price(no_expiry_event, market(100, END)), 1)
        # An effective-date observation has a zero-length coupon period.
        immediate = contract(
            observations=autocallable_observations(
                [START, END], [110, 110], coupon_barrier_levels=[90, 90]
            )
        )
        result = engine.price_with_greeks(immediate, market(90), "delta")
        self.assertIsNotNone(result.delta)

    def test_dense_deterministic_ladder_and_short_terminal_transition(self):
        dates = [START + timedelta(days=i) for i in range(1, 33)]
        note = contract(
            expiry_date=dates[-1],
            principal_ratio=0.5,
            observations=autocallable_observations(
                dates,
                [125 - 15 * i / 32 for i in range(1, 33)],
                coupon_barrier_levels=[90] * 32,
            ),
        )
        engine = QuadratureAutocallableEngine(asset_step_count=1024)
        for spot, expected in (
            (50, 0.1),
            (70, 0.2),
            (95, 0.532),
            (120, 0.511),
            (140, 0.501),
        ):
            self.assertAlmostEqual(
                engine.price(
                    note,
                    market(
                        spot,
                        volatility=1e-8,
                        rate=0,
                        dividend=0,
                        calendar=all_days_calendar(),
                    ),
                ),
                expected,
                delta=1e-8,
            )
        valuation = datetime(2025, 4, 1, tzinfo=UTC) - timedelta(seconds=1)
        note = contract(
            lower_strike=100,
            observations=autocallable_observations(
                [END], [110], coupon_barrier_levels=[100]
            ),
        )
        for sigma in (0.01, 0.2):
            t = 1 / (365 * 86400)
            d2 = (0.03 - sigma * sigma / 2) * math.sqrt(t) / sigma
            expected = math.exp(-0.04 * t) * (
                1 + 0.365 * 90 / 365 * math.erfc(-d2 / math.sqrt(2)) / 2
            )
            self.assertAlmostEqual(
                engine.price(note, market(100, valuation, volatility=sigma)),
                expected,
                delta=1e-9,
            )
