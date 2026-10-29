"""Basic snowball contracts through the installed Rust extension."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

from kiyosi import (
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    AutocallableObservation,
    SnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import QuadratureAutocallableEngine, implied_volatility
from quantlib_reference import structured

START, END = date(2025, 1, 1), date(2025, 4, 1)


def contract(**changes):
    fields = {
        "maturity_coupon_rate": 0.06,
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 60,
        "effective_date": START,
        "expiry_date": END,
        "knock_in_observation_mode": "at_expiry",
        "observations": autocallable_observations(
            [END], [110], knock_out_coupon_rates=[0.08]
        ),
    }
    fields.update(changes)
    return SnowballOption(**fields)


def market(
    spot=100, valuation=START, volatility=0.3, rate=0.04, dividend=0.01, calendar=None
):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
        calendar=calendar or weekdays_calendar(),
    )


class SnowballTests(unittest.TestCase):
    def test_current_settlement(self):
        engine = QuadratureAutocallableEngine()
        note = contract()
        for spot, expected in (
            (70, 0.7),
            (80, 1 + 0.06 * 90 / 365),
            (110, 1 + 0.08 * 90 / 365),
        ):
            self.assertAlmostEqual(engine.price(note, market(spot, END)), expected)

    def assert_rejection(self, expected, operation, *args, **kwargs):
        with self.assertRaises(expected) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def test_core_defaults_ownership_and_native_protocol_errors(self):
        engine = QuadratureAutocallableEngine()
        self.assertEqual(
            (engine.asset_step_count, engine.integration_standard_deviations), (512, 8)
        )
        self.assertEqual(contract().principal_ratio, 1)
        self.assertIsNone(contract().barrier_state)
        self.assertEqual(AutocallableObservation(END, 110).coupon_rate, 0)
        observations = autocallable_observations(iter([END]), iter([110]))
        note = contract(observations=iter(observations))
        observations.clear()
        copy = note.observations
        copy.clear()
        self.assertEqual(note.observation_dates, [END])
        self.assertEqual(note.knock_out_levels, [110])
        self.assertEqual(note.knock_out_coupon_rates, [0])
        known = note.replace(barrier_state="none")
        self.assertEqual(known.observations, note.observations)
        self.assertIsNone(note.barrier_state)
        self.assertEqual(known.replace(barrier_state=None), note)
        for changes in (
            {"barrier_state": True},
            {"barrier_state": "unknown"},
            {},
            {"observations": []},
        ):
            with self.assertRaises((TypeError, ValueError)):
                note.replace(**changes)
        self.assertEqual(
            note, contract(observations=[AutocallableObservation(END, 110)])
        )
        for fields in (
            {"initial_spot": True},
            {"initial_spot": "100"},
            {"principal_ratio": None},
            {"observations": [(END, 110)]},
            {"effective_date": datetime(2025, 1, 1)},
            {"knock_in_observation_mode": 1},
        ):
            with self.subTest(fields=fields), self.assertRaises(TypeError):
                contract(**fields)
        with self.assertRaises(ValueError):
            contract(knock_in_observation_mode="daily")
        for fields in (
            {"coupon_rate": None},
            {"coupon_rate": True},
            {"coupon_barrier_level": "0"},
        ):
            with self.assertRaises(TypeError):
                AutocallableObservation(END, 110, **fields)
        for fields, error in (
            ({"asset_step_count": True}, TypeError),
            ({"asset_step_count": 4.5}, TypeError),
            ({"asset_step_count": 2**100}, OverflowError),
            ({"integration_standard_deviations": None}, TypeError),
        ):
            with self.assertRaises(error):
                QuadratureAutocallableEngine(**fields)
        with self.assertRaises(AttributeError):
            note.initial_spot = 5

    def test_domain_validation_timing_and_failure_state(self):
        self.assert_rejection(InvalidInputError, autocallable_observations, [END], [])
        for fields, expected_error in (
            ({"observations": []}, InvalidInputError),
            (
                {"observations": [AutocallableObservation(END, 110)] * 2},
                InvalidInputError,
            ),
            ({"lower_strike": 101}, InvalidInputError),
            ({"initial_spot": 0}, InvalidInputError),
            ({"maturity_coupon_rate": math.inf}, InvalidInputError),
            (
                {
                    "observations": [
                        AutocallableObservation(END, 110, coupon_barrier_level=1)
                    ]
                },
                InvalidInputError,
            ),
            ({"expiry_date": date(2024, 1, 1)}, InvalidInputError),
        ):
            self.assert_rejection(expected_error, contract, **fields)
        # Records retain input values until the authoritative note constructor validates them.
        invalid = AutocallableObservation(END, math.nan)
        self.assert_rejection(InvalidInputError, contract, observations=[invalid])
        engine = QuadratureAutocallableEngine()
        unknown = contract(knock_in_observation_mode="every_trading_day")
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            unknown,
            market(valuation=date(2025, 1, 2)),
        )
        impossible = contract(barrier_state="knocked_in")
        self.assert_rejection(
            InvalidInputError, engine.price, impossible, market(valuation=END)
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            unknown.replace(barrier_state="knocked_out"),
            market(),
        )
        # Non-trading observations precede history checks; missing history precedes expiry checks.
        nontrading = contract(
            expiry_date=date(2025, 4, 5),
            observations=[AutocallableObservation(date(2025, 2, 1), 110)],
        )
        self.assert_rejection(InvalidInputError, engine.price, nontrading, market())
        nontrading = contract(
            expiry_date=date(2025, 4, 5),
            observations=[AutocallableObservation(date(2025, 2, 3), 110)],
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            nontrading,
            market(valuation=date(2025, 3, 3)),
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            nontrading.replace(barrier_state="none"),
            market(valuation=date(2025, 3, 3)),
        )
        for valuation in (date(2024, 12, 31), datetime(2025, 4, 1, 0, 0, 0, 1, UTC)):
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                contract(),
                market(valuation=valuation),
            )
        curve = RateTermStructure(
            reference_time=START,
            times=[END],
            factors=[math.exp(-0.04 * 90 / 365)],
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
            if kind != "SnowballOption":
                continue
            with self.subTest(name=name):
                start, end, valuation = (
                    date.fromisoformat(inputs[k])
                    for k in ("effective_date", "expiry_date", "valuation")
                )
                note = contract(
                    effective_date=start,
                    expiry_date=end,
                    maturity_coupon_rate=inputs["maturity_coupon_rate"],
                    principal_ratio=inputs["principal_ratio"],
                    barrier_state=inputs["history"],
                    knock_in_level=inputs["knock_in_level"],
                    knock_in_observation_mode=inputs["knock_in_mode"],
                    observations=autocallable_observations(
                        [end], [110], knock_out_coupon_rates=[inputs["coupon_rate"]]
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

    def test_zero_lower_strike_affine_tail_and_negative_price(self):
        # Migrates the legacy high-volatility affine-tail portfolio regression.
        import QuantLib as ql

        end = date(2026, 1, 1)
        context = market(20, volatility=2)
        discount = math.exp(-0.04)
        forward = 20 * math.exp(0.03)
        put = ql.BlackCalculator(
            ql.PlainVanillaPayoff(ql.Option.Put, 100), forward, 2, discount
        ).value()
        note = contract(
            expiry_date=end,
            principal_ratio=0.25,
            lower_strike=0,
            knock_in_level=100,
            maturity_coupon_rate=0,
            observations=autocallable_observations([end], [100]),
        )
        price = QuadratureAutocallableEngine().price(note, context)
        self.assertLess(price, 0)
        self.assertAlmostEqual(price, 0.25 * discount - put / 100, delta=1e-10)

    def test_daily_layers_and_domain_convergence(self):
        dates = [date(2025, 2, 1), date(2025, 3, 1), END]
        context = market(volatility=0.3, calendar=all_days_calendar())
        prices = []
        for mode in ("at_expiry", "every_trading_day"):
            note = contract(
                knock_in_level=95,
                knock_in_observation_mode=mode,
                observations=autocallable_observations(
                    dates, [110, 108, 105], knock_out_coupon_rates=[0.08] * 3
                ),
            )
            values = [
                QuadratureAutocallableEngine(
                    asset_step_count=n, integration_standard_deviations=e
                ).price(note, context)
                for n, e in ((512, 8), (1024, 8), (1024, 10))
            ]
            self.assertAlmostEqual(values[0], values[1], delta=1e-4)
            self.assertAlmostEqual(values[1], values[2], delta=1e-4)
            prices.append(values[0])
        self.assertLess(prices[1], prices[0] - 0.001)

    def test_current_events_history_and_unavailable_sensitivities(self):
        middle = date(2025, 2, 3)
        note = contract(
            barrier_state="none",
            observations=autocallable_observations(
                [middle, END], [110, 110], knock_out_coupon_rates=[0.08, 0.08]
            ),
        )
        engine = QuadratureAutocallableEngine()
        current = market(110, middle)
        result = engine.price_with_greeks(note, current, all_greeks=True)
        self.assertAlmostEqual(result.price, 1 + 0.08 * 33 / 365)
        self.assertIsNone(result.delta)
        self.assertIsNone(result.gamma)
        self.assertIsNone(result.theta)
        self.assertEqual((result.vega, result.rho), (0, 0))
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            note,
            current,
            result.price,
        )
        past = market(110, datetime(2025, 2, 3, 0, 0, 0, 1, UTC))
        self.assert_rejection(
            InvalidInputError, engine.price, note.replace(barrier_state=None), past
        )
        settled = note.replace(barrier_state="knocked_out")
        result = engine.price_with_greeks(
            settled, market(valuation=date(2025, 3, 3)), all_greeks=True
        )
        self.assertEqual(result.price, 0)
        for greek in (
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
        ):
            self.assertEqual(getattr(result, greek), 0)
        expiry = engine.price_with_greeks(note, market(80, END), all_greeks=True)
        self.assertIsNone(expiry.delta)
        daily = contract(
            knock_in_observation_mode="every_trading_day", barrier_state="knocked_in"
        )
        self.assertEqual(engine.price(daily, market(90, END)), 0.9)
        # Midnight KI is strict; intraday spot does not create a historical breach.
        daily = daily.replace(barrier_state="none")
        for when, has_theta in ((datetime(2025, 1, 2, 12, tzinfo=UTC), False),):
            result = engine.price_with_greeks(daily, market(80, when), "theta")
            self.assertEqual(result.theta is not None, has_theta)

    def test_settings_errors_and_fixed_cashflows(self):
        for fields in (
            {"asset_step_count": 3},
            {"integration_standard_deviations": 0},
            {"integration_standard_deviations": math.nan},
        ):
            self.assert_rejection(
                InvalidInputError, QuadratureAutocallableEngine, **fields
            )
        note = contract(
            lower_strike=100,
            maturity_coupon_rate=0,
            barrier_state="none",
            observations=autocallable_observations([END], [110]),
        )
        engine = QuadratureAutocallableEngine()
        context = market(80)
        result = engine.price_with_greeks(note, context, all_greeks=True)
        self.assertAlmostEqual(result.price, math.exp(-0.04 * 90 / 365), delta=1e-15)
        self.assertEqual(result.delta, 0)
        self.assertGreater(result.rho * -1, 0)
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            note,
            context,
            result.price,
        )

    def test_all_greeks_and_volatility_recovery_against_digital_formula(self):
        # Equal terminal strikes remove KI dependence, leaving cash plus a digital.
        end = date(2026, 1, 1)
        note = contract(
            expiry_date=end,
            lower_strike=100,
            maturity_coupon_rate=0,
            observations=autocallable_observations(
                [end], [100], knock_out_coupon_rates=[0.1]
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

    def test_two_event_prices_match_independent_conditional_payoff_integral(self):
        middle = date(2025, 2, 3)
        calendar = custom_calendar(
            base=all_days_calendar(),
            closed_dates=[
                START + timedelta(days=i)
                for i in range(91)
                if START + timedelta(days=i) not in (START, middle, END)
            ],
        )
        first, remaining = 33 / 365, 57 / 365
        sigma, rate, dividend = 0.3, 0.04, 0.01
        drift = rate - dividend - sigma**2 / 2
        width = sigma * math.sqrt(first)
        normal = lambda z: math.erfc(-z / math.sqrt(2)) / 2
        knock_out = (math.log(110 / 100) - drift * first) / width
        knock_in = (math.log(95 / 100) - drift * first) / width

        def terminal_expectation(spot, previously_in):
            threshold = 100 if previously_in else 95
            intact = 1 if previously_in else 1 + 0.06 * 90 / 365
            pieces = [
                (0, 60, 0.6, 0),
                (60, threshold, 0, 0.01),
                (threshold, 110, intact, 0),
                (110, math.inf, 1 + 0.08 * 90 / 365, 0),
            ]
            deviation = sigma * math.sqrt(remaining)
            mean = math.log(spot) + drift * remaining
            value = 0
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

            def integrand(z, previously_in):
                spot = 100 * math.exp(drift * first + width * z)
                payoff = terminal_expectation(spot, previously_in)
                return payoff * math.exp(-z * z / 2) / math.sqrt(2 * math.pi)

            def reference(panels, mode=mode):
                value = (1 + 0.08 * first) * (1 - normal(knock_out))
                # Splitting at the KI boundary preserves the jump in continuation value.
                for left, right in ((-12, knock_in), (knock_in, knock_out)):
                    h = (right - left) / panels
                    # Use interior endpoint limits at the discontinuity.
                    points = (
                        [math.nextafter(left, right)]
                        + [left + h * i for i in range(1, panels)]
                        + [math.nextafter(right, left)]
                    )
                    value += (
                        h
                        / 3
                        * math.fsum(
                            integrand(
                                z, mode == "every_trading_day" and right <= knock_in
                            )
                            * (1 if i in (0, panels) else 4 if i % 2 else 2)
                            for i, z in enumerate(points)
                        )
                    )
                return value * math.exp(-rate * first)

            coarse, fine = reference(2048), reference(4096)
            self.assertAlmostEqual(coarse, fine, delta=1e-9)
            note = contract(
                knock_in_level=95,
                knock_in_observation_mode=mode,
                observations=autocallable_observations(
                    [middle, END], [110, 110], knock_out_coupon_rates=[0.08, 0.08]
                ),
            )
            self.assertAlmostEqual(
                QuadratureAutocallableEngine().price(note, market(calendar=calendar)),
                fine,
                delta=1e-6,
            )

    def test_dense_deterministic_ladders_and_short_terminal_transition(self):
        dates = [START + timedelta(days=i) for i in range(1, 33)]
        note = contract(
            expiry_date=dates[-1],
            barrier_state="none",
            principal_ratio=0.5,
            observations=autocallable_observations(
                dates,
                [125 - 15 * i / 32 for i in range(1, 33)],
                knock_out_coupon_rates=[0.08 * 365 / i for i in range(1, 33)],
            ),
        )
        engine = QuadratureAutocallableEngine(asset_step_count=128)
        for spot, payoff in (
            (50, 0.1),
            (70, 0.2),
            (90, 0.5 + 0.06 * 32 / 365),
            (120, 0.58),
            (140, 0.58),
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
                payoff,
                delta=1e-8,
            )
        valuation = datetime(2025, 4, 1, tzinfo=UTC) - timedelta(seconds=1)
        note = contract(
            lower_strike=100,
            maturity_coupon_rate=0,
            observations=autocallable_observations(
                [END], [100], knock_out_coupon_rates=[0.1]
            ),
        )
        for sigma in (0.01, 0.2):
            t = 1 / (365 * 86400)
            d2 = (0.03 - sigma**2 / 2) * math.sqrt(t) / sigma
            expected = math.exp(-0.04 * t) * (
                1 + 0.1 * 90 / 365 * math.erfc(-d2 / math.sqrt(2)) / 2
            )
            self.assertAlmostEqual(
                engine.price(note, market(100, valuation, volatility=sigma)),
                expected,
                delta=1e-9,
            )

    def test_multi_event_greeks_stay_stable_across_grid_and_bump_refinement(self):
        end = date(2026, 1, 1)
        note = contract(
            expiry_date=end,
            barrier_state="none",
            observations=autocallable_observations(
                [date(2025, 6, 2), end], [110, 105], knock_out_coupon_rates=[0.08, 0.08]
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
            with self.subTest(greek=greek):
                self.assertIsNotNone(target)
                for actual in (getattr(standard, greek), getattr(smaller, greek)):
                    self.assertAlmostEqual(
                        actual, target, delta=absolute + relative * abs(target)
                    )
