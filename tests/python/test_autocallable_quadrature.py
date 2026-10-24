import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)

START, END = date(2025, 1, 1), date(2026, 1, 1)


def context(spot=100, volatility=0.3, rate=0.04, valuation=START):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=0.01, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
        calendar=all_days_calendar(),
    )


def binary(coupon=0.1, history=None):
    return BinarySnowballOption(
        maturity_coupon_rate=0,
        barrier_state=history,
        effective_date=START,
        expiry_date=END,
        observations=autocallable_observations(
            [END], [100], knock_out_coupon_rates=[coupon]
        ),
    )


def reference(spot, volatility, rate, time, coupon=0.1):
    d2 = (math.log(spot / 100) + (rate - 0.01 - volatility**2 / 2) * time) / (
        volatility * math.sqrt(time)
    )
    return math.exp(-rate * time) * (1 + coupon * math.erfc(-d2 / math.sqrt(2)) / 2)


class AutocallableQuadratureTests(unittest.TestCase):
    def check_convergence(self, note, market):
        normal = pricing.QuadratureAutocallableEngine().price(note, market)
        refined = pricing.QuadratureAutocallableEngine(
            asset_step_count=1024, integration_standard_deviations=8
        ).price(note, market)
        expanded = pricing.QuadratureAutocallableEngine(
            asset_step_count=1024, integration_standard_deviations=10
        ).price(note, market)
        independent = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=2000, time_step_count=4000
        ).price(note, market)
        self.assertAlmostEqual(normal, refined, delta=1e-4)
        self.assertAlmostEqual(refined, expanded, delta=1e-4)
        # Fine FD error measured below 2e-6 here is inside the 1bp price budget.
        self.assertAlmostEqual(normal, independent, delta=1e-4)

    def test_multi_event_defaults_converge_for_four_products_and_both_modes(self):
        start, end = date(2025, 1, 1), date(2025, 4, 1)
        dates = [date(2025, 2, 1), date(2025, 3, 1), end]
        market = context(volatility=0.2, rate=0.03)
        for mode in ("at_expiry", "every_trading_day"):
            common = {
                "effective_date": start,
                "expiry_date": end,
                "barrier_state": "none",
            }
            knock_in = {"knock_in_level": 80, "knock_in_observation_mode": mode}
            strikes = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
            coupons = {
                "maturity_coupon_rate": 0.06,
                "observations": autocallable_observations(
                    dates, [110, 108, 105], knock_out_coupon_rates=[0.08] * 3
                ),
            }
            for note in (
                SnowballOption(**common, **knock_in, **strikes, **coupons),
                BinarySnowballOption(**common, **coupons),
                TernarySnowballOption(
                    **common, **knock_in, **coupons, minimum_coupon_rate=0.01
                ),
                PhoenixOption(
                    **common,
                    **knock_in,
                    **strikes,
                    coupon_rate=0.08,
                    observations=autocallable_observations(
                        dates, [110, 108, 105], coupon_barrier_levels=[95] * 3
                    ),
                ),
            ):
                with self.subTest(product=type(note).__name__, mode=mode):
                    self.check_convergence(note, market)

    def test_monthly_step_down_convergence_with_history_near_barriers(self):
        dates = [date(2025, month, 1) for month in range(2, 13)] + [END]
        levels = [110 - month for month in range(2, 13)] + [97]
        market = context(98.5, volatility=0.35, valuation=date(2025, 7, 15))
        common = {
            "initial_spot": 100,
            "knock_in_level": 95,
            "upper_strike": 100,
            "lower_strike": 60,
            "effective_date": START,
            "expiry_date": END,
        }
        notes = (
            SnowballOption(
                **common,
                maturity_coupon_rate=0.06,
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in",
                observations=autocallable_observations(
                    dates, levels, knock_out_coupon_rates=[0.08] * len(dates)
                ),
            ),
            PhoenixOption(
                **common,
                coupon_rate=0.08,
                knock_in_observation_mode="at_expiry",
                barrier_state="none",
                observations=autocallable_observations(
                    dates, levels, coupon_barrier_levels=[99] * len(dates)
                ),
            ),
        )
        for note in notes:
            with self.subTest(product=type(note).__name__):
                self.check_convergence(note, market)

    def test_signed_phoenix_coupons_discount_at_their_own_events(self):
        start, end = date(2025, 1, 1), date(2025, 4, 1)
        dates = [date(2025, 2, 1), date(2025, 3, 1), end]
        for coupon in (0.08, -0.2, 0):
            note = PhoenixOption(
                coupon_rate=coupon,
                initial_spot=100,
                knock_in_level=50,
                upper_strike=100,
                lower_strike=60,
                knock_in_observation_mode="at_expiry",
                barrier_state="none",
                effective_date=start,
                expiry_date=end,
                observations=autocallable_observations(dates, [200, 190, 180]),
            )
            expiry = datetime(2025, 4, 1, tzinfo=UTC)
            for valuation in (
                datetime(2025, 1, 1, tzinfo=UTC),
                datetime(2025, 2, 1, tzinfo=UTC),
                datetime(2025, 2, 1, tzinfo=UTC) + timedelta(microseconds=1),
            ):
                expected = math.exp(
                    -0.04 * (expiry - valuation).total_seconds() / (365 * 86400)
                )
                previous = start
                for event in dates:
                    payment = datetime.combine(event, datetime.min.time(), UTC)
                    if payment >= valuation:
                        expected += (
                            coupon
                            * (event - previous).days
                            / 365
                            * math.exp(
                                -0.04
                                * (payment - valuation).total_seconds()
                                / (365 * 86400)
                            )
                        )
                    previous = event
                actual = pricing.QuadratureAutocallableEngine().price(
                    note, context(volatility=1e-8, valuation=valuation)
                )
                self.assertAlmostEqual(actual, expected, delta=1e-9)

    def test_dense_terminal_pieces_preserve_deterministic_payoff_regions(self):
        start = date(2025, 1, 1)
        dates = [start + timedelta(days=index) for index in range(1, 33)]
        levels = [125 - 15 * index / 32 for index in range(1, 33)]
        engine = pricing.QuadratureAutocallableEngine(asset_step_count=128)
        for principal in (0.5, 1.0):
            note = SnowballOption(
                maturity_coupon_rate=0.06,
                initial_spot=100,
                knock_in_level=80,
                upper_strike=100,
                lower_strike=60,
                knock_in_observation_mode="at_expiry",
                barrier_state="none",
                principal_ratio=principal,
                effective_date=start,
                expiry_date=dates[-1],
                observations=autocallable_observations(
                    dates,
                    levels,
                    knock_out_coupon_rates=[
                        0.08 * 365 / index for index in range(1, 33)
                    ],
                ),
            )
            for spot in (50, 70, 90, 120, 140):
                with self.subTest(principal=principal, spot=spot):
                    market = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0, dividend_yield=0, volatility=1e-8
                        ),
                        spot_price=spot,
                        valuation_time=start,
                        calendar=all_days_calendar(),
                    )
                    expected = principal + 0.06 * 32 / 365
                    if spot <= 80:
                        expected = principal + (max(spot, 60) - 100) / 100
                    if spot >= 110:
                        expected = principal + 0.08
                    self.assertAlmostEqual(
                        engine.price(note, market), expected, delta=1e-8
                    )

    def test_core_defaults_and_representation_validation(self):
        engine = pricing.QuadratureAutocallableEngine()
        self.assertEqual(engine.asset_step_count, 512)
        self.assertEqual(engine.integration_standard_deviations, 8)
        for keyword, value, expected_error in (
            ("asset_step_count", True, TypeError),
            ("asset_step_count", 4.5, TypeError),
            ("asset_step_count", 2**100, OverflowError),
            ("integration_standard_deviations", True, TypeError),
        ):
            with (
                self.subTest(keyword=keyword, value=value),
                self.assertRaises(expected_error),
            ):
                pricing.QuadratureAutocallableEngine(**{keyword: value})
        for keyword, value in (
            ("asset_step_count", 3),
            ("asset_step_count", -1),
            ("integration_standard_deviations", 0),
            ("integration_standard_deviations", -1),
            ("integration_standard_deviations", math.nan),
            ("integration_standard_deviations", math.inf),
        ):
            with self.subTest(keyword=keyword, value=value):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    pricing.QuadratureAutocallableEngine(**{keyword: value})
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                )

    def test_zero_barrier_phoenix_coupons_include_finite_domain_tails(self):
        middle = date(2025, 7, 1)
        note = PhoenixOption(
            coupon_rate=-0.5,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=100,
            knock_in_observation_mode="at_expiry",
            principal_ratio=0.25,
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations([middle, END], [100, 100]),
        )
        time = (middle - START).days / 365
        survival = math.erfc((0.03 - 2) * math.sqrt(time) / (2 * math.sqrt(2))) / 2
        expected = (-0.5 * time + 0.25 * (1 - survival)) * math.exp(-0.04 * time) + (
            0.25 - 0.5 * (1 - time)
        ) * survival * math.exp(-0.04)
        for engine in (
            pricing.QuadratureAutocallableEngine(),
            pricing.QuadratureAutocallableEngine(
                asset_step_count=128, integration_standard_deviations=0.5
            ),
        ):
            self.assertAlmostEqual(
                engine.price(note, context(volatility=2)), expected, delta=1e-10
            )

    def test_four_terminal_portfolios_with_signed_coupons_in_both_modes(self):
        engine = pricing.QuadratureAutocallableEngine()
        market = context(volatility=0.8)

        def put(strike):
            return pricing.AnalyticVanillaEngine().price(
                EuropeanOption(
                    option_type="put",
                    strike=strike,
                    effective_date=START,
                    expiry_date=END,
                ),
                market,
            )

        for mode in ("at_expiry", "every_trading_day"):
            for coupon in (0.1, -4):
                cash = reference(100, 0.8, 0.04, 1, coupon)
                downside = cash - (put(100) - put(60)) / 100
                common = {
                    "knock_in_level": 100,
                    "knock_in_observation_mode": mode,
                    "effective_date": START,
                    "expiry_date": END,
                }
                strikes = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
                observations = autocallable_observations(
                    [END], [100], knock_out_coupon_rates=[coupon]
                )
                for note, expected in (
                    (binary(coupon), cash),
                    (
                        SnowballOption(
                            **common,
                            **strikes,
                            maturity_coupon_rate=0,
                            observations=observations,
                        ),
                        downside,
                    ),
                    (
                        TernarySnowballOption(
                            **common,
                            maturity_coupon_rate=0,
                            minimum_coupon_rate=0,
                            observations=observations,
                        ),
                        cash,
                    ),
                    (
                        PhoenixOption(
                            **common,
                            **strikes,
                            coupon_rate=coupon,
                            observations=autocallable_observations(
                                [END], [100], coupon_barrier_levels=[100]
                            ),
                        ),
                        downside,
                    ),
                ):
                    with self.subTest(
                        product=type(note).__name__, mode=mode, coupon=coupon
                    ):
                        self.assertAlmostEqual(
                            engine.price(note, market), expected, delta=1e-4
                        )

    def test_one_second_transition_matches_digital_formula(self):
        engine = pricing.QuadratureAutocallableEngine()
        valuation = datetime(2026, 1, 1, tzinfo=UTC) - timedelta(seconds=1)
        time = 1 / (365 * 86400)
        for volatility in (0.01, 0.2):
            for spot in (100, 100 + 50 * volatility * math.sqrt(time)):
                with self.subTest(volatility=volatility, spot=spot):
                    actual = engine.price(
                        binary(), context(spot, volatility, valuation=valuation)
                    )
                    self.assertAlmostEqual(
                        actual, reference(spot, volatility, 0.04, time), delta=1e-9
                    )

    def test_zero_lower_strike_affine_tail_and_nonunit_principal(self):
        market = context(20, volatility=2)
        put = pricing.AnalyticVanillaEngine().price(
            EuropeanOption(
                option_type="put", strike=100, effective_date=START, expiry_date=END
            ),
            market,
        )
        expected = 0.25 * math.exp(-0.04) - put / 100
        common = {
            "initial_spot": 100,
            "knock_in_level": 100,
            "upper_strike": 100,
            "lower_strike": 0,
            "knock_in_observation_mode": "at_expiry",
            "principal_ratio": 0.25,
            "effective_date": START,
            "expiry_date": END,
        }
        for note in (
            SnowballOption(
                **common,
                maturity_coupon_rate=0,
                observations=autocallable_observations(
                    [END], [100], knock_out_coupon_rates=[0]
                ),
            ),
            PhoenixOption(
                **common,
                coupon_rate=0,
                observations=autocallable_observations([END], [100]),
            ),
        ):
            with self.subTest(product=type(note).__name__):
                self.assertAlmostEqual(
                    pricing.QuadratureAutocallableEngine().price(note, market),
                    expected,
                    delta=1e-10,
                )

    def test_daily_knock_in_has_a_distinct_continuation_layer(self):
        end = date(2025, 4, 1)
        market = context(volatility=0.4)
        prices = []
        for mode in ("every_trading_day", "at_expiry"):
            note = TernarySnowballOption(
                maturity_coupon_rate=0.2,
                minimum_coupon_rate=0,
                knock_in_level=100,
                knock_in_observation_mode=mode,
                barrier_state="none",
                effective_date=START,
                expiry_date=end,
                observations=autocallable_observations(
                    [date(2025, 2, 1), date(2025, 3, 1), end],
                    [200, 190, 180],
                    knock_out_coupon_rates=[0.2] * 3,
                ),
            )
            actual = pricing.QuadratureAutocallableEngine().price(note, market)
            independent = pricing.FiniteDifferenceAutocallableEngine(
                asset_step_count=2000, time_step_count=4000
            ).price(note, market)
            self.assertAlmostEqual(actual, independent, delta=4e-4)
            prices.append(actual)
        self.assertLess(prices[0], prices[1] - 0.01)

    def test_all_greeks_match_independent_same_stencil_digital_formula(self):
        valuation = date(2025, 6, 1)
        time = (END - valuation).days / 365
        spot, vol, rate = 103, 0.3, -0.04
        h, v, r = 0.25, (vol + 0.002) - vol, (rate + 0.001) - rate
        value = lambda s=spot, sigma=vol, rf=rate, t=time: reference(s, sigma, rf, t)
        delta = lambda sigma, t: (
            (value(s=spot + h, sigma=sigma, t=t) - value(s=spot - h, sigma=sigma, t=t))
            / (2 * h)
        )
        gamma = lambda sigma, t: (
            (
                value(s=spot + h, sigma=sigma, t=t)
                - 2 * value(sigma=sigma, t=t)
                + value(s=spot - h, sigma=sigma, t=t)
            )
            / h**2
        )
        dt = 1 / 365
        expected = {
            "delta": delta(vol, time),
            "gamma": gamma(vol, time),
            "speed": (
                value(s=spot + 2 * h)
                - 2 * value(s=spot + h)
                + 2 * value(s=spot - h)
                - value(s=spot - 2 * h)
            )
            / (2 * h**3),
            "vega": (value(sigma=vol + v) - value(sigma=vol - v)) / (200 * v),
            "vanna": (delta(vol + v, time) - delta(vol - v, time)) / (200 * v),
            "zomma": (gamma(vol + v, time) - gamma(vol - v, time)) / (200 * v),
            "rho": (value(rf=rate + r) - value(rf=rate - r)) / (200 * r),
            "theta": (value(t=time - dt) - value(t=time + dt)) / 2,
            "charm": (delta(vol, time - dt) - delta(vol, time + dt)) / 2,
            "color": (gamma(vol, time - dt) - gamma(vol, time + dt)) / 2,
        }
        result = pricing.QuadratureAutocallableEngine().price_with_greeks(
            binary(history="none"),
            context(spot, vol, rate, valuation),
            all_greeks=True,
            spot_shift=h,
            volatility_shift=0.002,
            rate_shift=0.001,
            time_shift_days=1,
        )
        self.assertAlmostEqual(result.price, value(), delta=1e-10)
        for greek, target in expected.items():
            with self.subTest(greek=greek):
                self.assertAlmostEqual(
                    getattr(result, greek), target, delta=1e-9 + 1e-5 * abs(target)
                )

    def test_multi_event_greeks_are_stable_under_grid_and_bump_refinement(self):
        market = context(101.5, volatility=0.25, valuation=date(2025, 1, 15))
        dates = [date(2025, 6, 1), END]
        common = {
            "initial_spot": 100,
            "knock_in_level": 80,
            "upper_strike": 100,
            "lower_strike": 60,
            "knock_in_observation_mode": "at_expiry",
            "barrier_state": "none",
            "effective_date": START,
            "expiry_date": END,
        }
        notes = (
            SnowballOption(
                **common,
                maturity_coupon_rate=0.06,
                observations=autocallable_observations(
                    dates, [110, 105], knock_out_coupon_rates=[0.08, 0.08]
                ),
            ),
            PhoenixOption(
                **common,
                coupon_rate=0.08,
                observations=autocallable_observations(
                    dates, [110, 105], coupon_barrier_levels=[95, 95]
                ),
            ),
        )
        # Absolute units are price per spot, percentage point or calendar day.
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
        coarse_bumps = {
            "spot_shift": 0.5,
            "volatility_shift": 0.002,
            "rate_shift": 0.001,
        }
        fine_bumps = {
            "spot_shift": 0.25,
            "volatility_shift": 0.001,
            "rate_shift": 0.0005,
        }
        normal = pricing.QuadratureAutocallableEngine()
        refined = pricing.QuadratureAutocallableEngine(asset_step_count=1024)
        for note in notes:
            standard = normal.price_with_greeks(
                note, market, all_greeks=True, **coarse_bumps
            )
            finer_grid = refined.price_with_greeks(
                note, market, all_greeks=True, **coarse_bumps
            )
            smaller_bumps = refined.price_with_greeks(
                note, market, all_greeks=True, **fine_bumps
            )
            self.assertAlmostEqual(standard.price, finer_grid.price, delta=1e-4)
            for greek, (absolute, relative) in tolerances.items():
                with self.subTest(product=type(note).__name__, greek=greek):
                    reference_value = getattr(finer_grid, greek)
                    self.assertIsNotNone(reference_value)
                    self.assertIsNotNone(getattr(standard, greek))
                    self.assertIsNotNone(getattr(smaller_bumps, greek))
                    limit = absolute + relative * abs(reference_value)
                    self.assertAlmostEqual(
                        getattr(standard, greek), reference_value, delta=limit
                    )
                    self.assertAlmostEqual(
                        getattr(smaller_bumps, greek), reference_value, delta=limit
                    )

    def test_terminal_boundaries_and_unavailable_greeks(self):
        note = PhoenixOption(
            coupon_rate=0.08,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=60,
            knock_in_observation_mode="every_trading_day",
            barrier_state="none",
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations(
                [END], [100], coupon_barrier_levels=[100]
            ),
        )
        for spot, expected in (
            (100, 1.08),
            (math.nextafter(100, 0), 1),
            (80, 1),
            (math.nextafter(80, 0), math.nextafter(80, 0) / 100),
            (60, 0.6),
        ):
            with self.subTest(spot=spot):
                result = pricing.QuadratureAutocallableEngine().price_with_greeks(
                    note, context(spot, valuation=END), all_greeks=True
                )
                self.assertEqual(result.price, expected)
                for greek in (
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
                ):
                    self.assertIsNone(getattr(result, greek))

    def test_history_validation_and_terminated_note(self):
        engine = pricing.QuadratureAutocallableEngine()
        valuation = date(2025, 7, 1)

        def historical_note(history):
            return BinarySnowballOption(
                maturity_coupon_rate=0,
                barrier_state=history,
                effective_date=START,
                expiry_date=END,
                observations=autocallable_observations(
                    [date(2025, 3, 1), END],
                    [110, 100],
                    knock_out_coupon_rates=[0.08, 0.1],
                ),
            )

        for operation in (
            lambda: engine.price(historical_note(None), context(valuation=valuation)),
            lambda: engine.price_with_greeks(
                historical_note(None), context(valuation=valuation), "delta"
            ),
            lambda: pricing.implied_volatility(
                engine, historical_note(None), context(valuation=valuation), 1
            ),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                operation()
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )
        result = engine.price_with_greeks(
            historical_note("knocked_out"),
            context(valuation=valuation),
            all_greeks=True,
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
        self.assertGreater(
            engine.price(historical_note("none"), context(valuation=valuation)), 0
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(
                binary(history="none"), context(valuation=END + timedelta(days=1))
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE
        )

    def test_volatility_recovery_with_dense_elapsed_observations(self):
        dates = [START + timedelta(days=i) for i in range(1, 366)]
        market = context(110.25, valuation=datetime(2025, 12, 31, 12, tzinfo=UTC))
        engine = pricing.QuadratureAutocallableEngine(asset_step_count=16)
        notes = [
            BinarySnowballOption(
                maturity_coupon_rate=0,
                barrier_state="none",
                effective_date=START,
                expiry_date=END,
                observations=autocallable_observations(
                    schedule,
                    [110] * len(schedule),
                    knock_out_coupon_rates=[0.08] * len(schedule),
                ),
            )
            for schedule in ([END], dates)
        ]
        quote = engine.price(notes[0], market)
        for note in notes:
            self.assertEqual(engine.price(note, market), quote)
            solved = pricing.implied_volatility(
                engine,
                note,
                market,
                quote,
                lower_bound=0.1,
                upper_bound=0.5,
                price_tolerance=1e-11,
            )
            self.assertAlmostEqual(solved, 0.3, delta=1e-7)

    def test_volatility_and_coupon_recovery_and_impossible_target(self):
        engine = pricing.QuadratureAutocallableEngine()
        market = context(spot=103)
        quote = reference(103, 0.3, 0.04, 1)
        solved = pricing.implied_volatility(
            engine,
            binary(),
            market,
            quote,
            lower_bound=0.1,
            upper_bound=0.5,
            price_tolerance=1e-11,
        )
        self.assertAlmostEqual(solved, 0.3, delta=1e-7)
        multiple_events = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations(
                [date(2025, 7, 1), END], [100, 100], knock_out_coupon_rates=[0.1, 0.1]
            ),
        )
        multiple_quote = engine.price(multiple_events, market)
        default_volatility = pricing.implied_volatility(
            engine, multiple_events, market, multiple_quote
        )
        self.assertAlmostEqual(default_volatility, 0.3, delta=1e-5)
        self.assertAlmostEqual(
            engine.price(multiple_events, context(103, volatility=default_volatility)),
            multiple_quote,
            delta=1e-8,
        )
        for convention in ("shift_maturity_coupon", "preserve_maturity_coupon"):
            solved = pricing.implied_coupon(
                engine,
                binary(),
                market,
                quote,
                quote_convention=convention,
                lower_bound=-1,
                upper_bound=1,
                price_tolerance=1e-11,
                max_iterations=1,
            )
            self.assertAlmostEqual(solved, 0.1, delta=1e-9)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            pricing.implied_volatility(
                engine, binary(), market, 10, lower_bound=0.1, upper_bound=0.5
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.UNBRACKETED_VOLATILITY
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            pricing.implied_coupon(
                engine,
                binary(),
                market,
                10,
                quote_convention="preserve_maturity_coupon",
                lower_bound=-0.2,
                upper_bound=0.2,
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.UNBRACKETED_COUPON
        )

    def test_coupon_workspace_recovers_four_products_with_changing_knock_in_exposure(
        self,
    ):
        start, end = date(2025, 1, 1), date(2025, 4, 1)
        dates = [date(2025, 2, 1), date(2025, 3, 1), end]
        common = {"effective_date": start, "expiry_date": end}
        coupons = {
            "maturity_coupon_rate": 0.08,
            "observations": autocallable_observations(
                dates, [110, 108, 105], knock_out_coupon_rates=[0.08] * 3
            ),
        }
        notes = (
            SnowballOption(
                **common,
                **coupons,
                initial_spot=100,
                knock_in_level=80,
                upper_strike=100,
                lower_strike=100,
                knock_in_observation_mode="every_trading_day",
            ),
            BinarySnowballOption(**common, **coupons),
            TernarySnowballOption(
                **common,
                **coupons,
                minimum_coupon_rate=0,
                knock_in_level=80,
                knock_in_observation_mode="every_trading_day",
            ),
            PhoenixOption(
                **common,
                coupon_rate=0.08,
                initial_spot=100,
                knock_in_level=80,
                upper_strike=100,
                lower_strike=60,
                observations=autocallable_observations(
                    dates, [110, 108, 105], coupon_barrier_levels=[95] * 3
                ),
                knock_in_observation_mode="every_trading_day",
            ),
        )
        engine = pricing.QuadratureAutocallableEngine(asset_step_count=64)
        market = context(volatility=0.2, rate=0.03)
        for note in notes:
            quote = engine.price(note, market)
            for convention in ("preserve_maturity_coupon", "shift_maturity_coupon"):
                kwargs = (
                    {}
                    if isinstance(note, PhoenixOption)
                    else {"quote_convention": convention}
                )
                with self.subTest(product=type(note).__name__, convention=convention):
                    solved = pricing.implied_coupon(
                        engine,
                        note,
                        market,
                        quote,
                        lower_bound=0,
                        upper_bound=0.2,
                        price_tolerance=1e-10,
                        **kwargs,
                    )
                    self.assertAlmostEqual(solved, 0.08, delta=1e-8)

    def test_current_knock_out_and_historical_knock_in(self):
        engine = pricing.QuadratureAutocallableEngine()
        middle = date(2025, 7, 1)
        note = BinarySnowballOption(
            maturity_coupon_rate=0,
            barrier_state="none",
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations(
                [middle, END], [100, 100], knock_out_coupon_rates=[0.1, 0.1]
            ),
        )
        for spot in (100, math.nextafter(100, 0)):
            result = engine.price_with_greeks(
                note,
                context(spot, valuation=middle),
                ["delta", "gamma", "theta", "vega", "rho"],
            )
            expected = (
                1 + 0.1 * (middle - START).days / 365
                if spot == 100
                else reference(spot, 0.3, 0.04, (END - middle).days / 365)
            )
            self.assertAlmostEqual(result.price, expected, delta=1e-10)
            self.assertIsNone(result.delta)
            self.assertIsNone(result.gamma)
            self.assertIsNone(result.theta)
            self.assertIsNotNone(result.vega)
            self.assertIsNotNone(result.rho)
            if spot == 100:
                self.assertEqual(result.vega, 0)
                self.assertEqual(result.rho, 0)
        for mode in ("at_expiry", "every_trading_day"):
            for history in ("none", "knocked_in"):
                common = {
                    "maturity_coupon_rate": 0.05,
                    "knock_in_level": 80,
                    "knock_in_observation_mode": mode,
                    "barrier_state": history,
                    "effective_date": START,
                    "expiry_date": END,
                    "observations": autocallable_observations(
                        [END], [110], knock_out_coupon_rates=[0.08]
                    ),
                }
                for product, expected in (
                    (
                        SnowballOption(
                            **common,
                            initial_spot=100,
                            upper_strike=100,
                            lower_strike=60,
                        ),
                        0.9 if history == "knocked_in" else 1.05,
                    ),
                    (
                        TernarySnowballOption(**common, minimum_coupon_rate=0.02),
                        1.02 if history == "knocked_in" else 1.05,
                    ),
                ):
                    with self.subTest(
                        mode=mode, history=history, product=type(product).__name__
                    ):
                        if mode == "at_expiry" and history == "knocked_in":
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                engine.price(product, context(90, valuation=END))
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.INVALID_OPTION,
                            )
                            continue
                        self.assertEqual(
                            engine.price(product, context(90, valuation=END)), expected
                        )

    def test_reused_spot_and_volatility_curves_match_scalar_prices(self):
        engine = pricing.QuadratureAutocallableEngine(asset_step_count=128)
        end = date(2025, 1, 20)
        note = SnowballOption(
            maturity_coupon_rate=0.06,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=60,
            knock_in_observation_mode="every_trading_day",
            barrier_state="none",
            effective_date=START,
            expiry_date=end,
            observations=autocallable_observations(
                [date(2025, 1, 8), end], [110, 105], knock_out_coupon_rates=[0.08, 0.08]
            ),
        )
        spot, h = 100.25, 0.25
        volatilities = (0.25, 0.3)

        def values(vol):
            return [
                engine.price(
                    note,
                    context(spot + offset * h, vol, -0.02, date(2025, 1, 4)),
                )
                for offset in (0, 1, -1, 2, -2)
            ]

        expected = []
        for volatility in volatilities:
            v = (volatility + 0.002) - volatility
            center, up, down, up2, down2 = values(volatility)
            high, high_up, high_down, _, _ = values(volatility + v)
            low, low_up, low_down, _, _ = values(volatility - v)
            expected.append(
                (
                    center,
                    {
                        "delta": (up - down) / (2 * h),
                        "gamma": (up - 2 * center + down) / h**2,
                        "speed": (up2 - 2 * up + 2 * down - down2) / (2 * h**3),
                        "vanna": (high_up - high_down - low_up + low_down)
                        / (400 * h * v),
                        "zomma": (
                            (high_up - 2 * high + high_down)
                            - (low_up - 2 * low + low_down)
                        )
                        / (200 * h**2 * v),
                    },
                )
            )

        def evaluate(volatility):
            return engine.price_with_greeks(
                note,
                context(spot, volatility, -0.02, date(2025, 1, 4)),
                ["delta", "gamma", "speed", "vanna", "zomma"],
                spot_shift=h,
                volatility_shift=0.002,
            )

        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(evaluate, volatilities))
        for result, (center, greeks) in zip(results, expected, strict=True):
            self.assertEqual(result.price, center)
            for greek, reference in greeks.items():
                with self.subTest(greek=greek):
                    self.assertAlmostEqual(
                        getattr(result, greek), reference, delta=1e-12
                    )

    def test_shared_engine_concurrent_requests_match_independent_formula(self):
        engine = pricing.QuadratureAutocallableEngine()
        note = binary()
        spots = (90, 100, 110)
        with ThreadPoolExecutor(max_workers=3) as executor:
            prices = list(
                executor.map(lambda spot: engine.price(note, context(spot)), spots)
            )
        for spot, actual in zip(spots, prices, strict=True):
            self.assertAlmostEqual(actual, reference(spot, 0.3, 0.04, 1), delta=1e-10)
