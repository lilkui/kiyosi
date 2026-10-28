"""Migrated FD contracts and numerical budgets from the legacy autocallable suite."""

import math
import unittest
from datetime import UTC, date, datetime, timedelta

from kiyosi import pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    CashOrNothingOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)


class AutocallableAccuracyTests(unittest.TestCase):
    def test_nearby_barriers_preserve_prices_and_greeks(self):
        start, middle, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        engine = pricing.FiniteDifferenceAutocallableEngine()

        def price(barrier):
            option = BinarySnowballOption(
                maturity_coupon_rate=0.02,
                effective_date=start,
                expiry_date=end,
                observations=autocallable_observations(
                    [middle, end], [100, barrier], knock_out_coupon_rates=[0.08, 0.08]
                ),
            )
            return engine.price_with_greeks(option, context, ["delta", "gamma"])

        expected = price(100)
        for barrier in (math.nextafter(100, 200), 100 + 1e-12, 100 + 1e-8, 100 + 1e-4):
            with self.subTest(barrier=barrier):
                actual = price(barrier)
                self.assertLessEqual(actual.price, 1.08 * math.exp(-0.04))
                for measure in ("price", "delta", "gamma"):
                    self.assertAlmostEqual(
                        getattr(actual, measure), getattr(expected, measure), delta=1e-7
                    )

    def test_near_expiry_greeks_match_an_analytic_digital(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [100], knock_out_coupon_rates=[0.1]
            ),
        )
        digital = CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=0.1,
            effective_date=start,
            expiry_date=end,
        )
        engine = pricing.FiniteDifferenceAutocallableEngine()
        for volatility in (0.01, 0.2):
            for seconds in (3600, 600, 60, 1):
                valuation = datetime(2026, 1, 1, tzinfo=UTC) - timedelta(
                    seconds=seconds
                )
                time = seconds / (365 * 86400)
                width = 100 * volatility * math.sqrt(time)
                for spot in (100, 100 + 0.5 * width):
                    with self.subTest(
                        volatility=volatility, seconds=seconds, spot=spot
                    ):
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0.04,
                                dividend_yield=0.01,
                                volatility=volatility,
                            ),
                            spot_price=spot,
                            valuation_time=valuation,
                            calendar=all_days_calendar(),
                        )
                        actual = engine.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        expected = pricing.AnalyticDigitalEngine().price_with_greeks(
                            digital, context, ["delta", "gamma"]
                        )
                        self.assertAlmostEqual(
                            actual.price,
                            math.exp(-0.04 * time) + expected.price,
                            delta=1e-5,
                        )
                        for measure, absolute in (("delta", 1e-6), ("gamma", 1e-7)):
                            reference = getattr(expected, measure)
                            self.assertAlmostEqual(
                                getattr(actual, measure),
                                reference,
                                delta=absolute + 0.005 * abs(reference),
                            )

    def test_snowball_greeks_preserve_smooth_price_maximum_between_grid_nodes(self):
        start, end = date(2025, 10, 23), date(2026, 10, 23)
        calendar = sse_calendar()
        dates = list(
            monthly_schedule(
                start=start, end=end, first_observation_month=1, calendar=calendar
            ).dates
        )
        if dates[-1] != end:
            dates.append(end)
        note = SnowballOption(
            maturity_coupon_rate=0.2,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            knock_in_observation_mode="every_trading_day",
            barrier_state="none",
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                dates, [103] * len(dates), knock_out_coupon_rates=[0.2] * len(dates)
            ),
        )
        model = BlackScholesMertonParameters(
            risk_free_rate=0.02, dividend_yield=0, volatility=0.2
        )
        engine = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=1600, time_step_count=1600, asset_upper_boundary=300
        )

        def context(spot):
            return PricingContext(
                model_parameters=model,
                spot_price=spot,
                valuation_time=start,
                calendar=calendar,
            )

        # Dealer-normalized Gamma references from an independent quadrature solve.
        for spot, expected, tolerance in (
            (82.0, 2.61633362, 0.03),
            (107.49, 0.93813253, 0.01),
            (107.5, 0.93813253, 0.01),
            (107.51, 0.93813253, 0.01),
        ):
            with self.subTest(spot=spot):
                result = engine.price_with_greeks(
                    note, context(spot), ["delta", "gamma"]
                )
                self.assertAlmostEqual(-10000 * result.gamma, expected, delta=tolerance)
                bump = 0.005
                low = engine.price(note, context(spot - bump))
                high = engine.price(note, context(spot + bump))
                self.assertAlmostEqual(
                    100 * result.delta, 100 * (high - low) / (2 * bump), delta=1e-5
                )
                self.assertAlmostEqual(
                    10000 * result.gamma,
                    10000 * (high - 2 * result.price + low) / bump**2,
                    delta=0.002,
                )

    def test_coarse_grids_preserve_positive_payoff_bounds_and_flat_greeks(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [100], knock_out_coupon_rates=[0.1]
            ),
        )
        for seconds in (1, 3600):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=0.2
                ),
                spot_price=350,
                valuation_time=datetime(2026, 1, 1, tzinfo=UTC)
                - timedelta(seconds=seconds),
                calendar=all_days_calendar(),
            )
            for count in (3, 4, 5, 10, 20, 40):
                with self.subTest(seconds=seconds, count=count):
                    engine = pricing.FiniteDifferenceAutocallableEngine(
                        asset_step_count=count, time_step_count=2000
                    )
                    result = engine.price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    self.assertGreaterEqual(result.price, 1 - 1e-10)
                    self.assertLessEqual(result.price, 1.1 + 1e-10)
                    self.assertAlmostEqual(result.price, 1.1, delta=1e-6)
                    self.assertEqual(engine.price(option, context), result.price)
                    self.assertAlmostEqual(result.delta, 0, delta=1e-8)
                    self.assertAlmostEqual(result.gamma, 0, delta=1e-8)

    def test_terminal_digital_separates_space_time_domain_and_barrier_placement_errors(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )

        def error(assets, times, upper, barrier=100):
            option = BinarySnowballOption(
                maturity_coupon_rate=0,
                effective_date=start,
                expiry_date=end,
                observations=autocallable_observations(
                    [end], [barrier], knock_out_coupon_rates=[0.1]
                ),
            )
            digital = CashOrNothingOption(
                option_type="call",
                strike=barrier,
                payout=0.1,
                effective_date=start,
                expiry_date=end,
            )
            engine = pricing.FiniteDifferenceAutocallableEngine(
                asset_step_count=assets,
                time_step_count=times,
                asset_upper_boundary=upper,
            )
            expected = math.exp(-0.05) + pricing.AnalyticDigitalEngine().price(
                digital, context
            )
            return abs(engine.price(option, context) - expected)

        coarse = error(200, 2000, 400)
        for assets in (400, 800, 1600):
            with self.subTest(axis="space", assets=assets):
                refined = error(assets, 2000, 400)
                self.assertLessEqual(refined, coarse + 1e-7)
                if assets == 1600:
                    self.assertLessEqual(refined, 1e-5)
        for times in (100, 400, 1600):
            with self.subTest(axis="time", times=times):
                self.assertLessEqual(error(1600, times, 400), 1e-5)
        for upper in (500, 700, 900):
            with self.subTest(axis="domain", upper=upper):
                self.assertLessEqual(error(1600, 2000, upper), 1e-5)
        for barrier in (99.75, 99.875, 100, 100.125, 100.25):
            with self.subTest(axis="barrier_placement", barrier=barrier):
                self.assertLessEqual(error(800, 2000, 400, barrier), 1e-5)

    def test_projection_preserves_exact_current_event_barrier_comparisons(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = PhoenixOption(
            coupon_rate=0.08,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=60,
            barrier_state="none",
            knock_in_observation_mode="every_trading_day",
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [100], coupon_barrier_levels=[100]
            ),
        )
        engine = pricing.FiniteDifferenceAutocallableEngine()
        for spot, expected in (
            (100, 1.08),
            (math.nextafter(100, 0), 1),
            (80, 1),
            (math.nextafter(80, 0), math.nextafter(80, 0) / 100),
            (math.nextafter(100, 200), 1.08),
        ):
            with self.subTest(spot=spot):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                    ),
                    spot_price=spot,
                    valuation_time=end,
                    calendar=all_days_calendar(),
                )
                result = engine.price_with_greeks(option, context, ["delta", "gamma"])
                self.assertEqual(result.price, expected)
                self.assertIsNone(result.delta)
                self.assertIsNone(result.gamma)

    def test_daily_calendars_preserve_cash_carry_portfolio_across_event_restarts(self):
        start, end = date(2024, 1, 1), date(2025, 1, 1)
        dates = [date(2024, 4, 1), date(2024, 7, 1), date(2024, 10, 1), end]

        def carry_rate(day):
            time = (day - start).days / 365
            return math.expm1(0.04 * time) / time

        option = TernarySnowballOption(
            maturity_coupon_rate=carry_rate(end),
            minimum_coupon_rate=carry_rate(end),
            knock_in_level=75,
            knock_in_observation_mode="every_trading_day",
            barrier_state="none",
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                dates,
                [110] * 4,
                knock_out_coupon_rates=[carry_rate(day) for day in dates],
            ),
        )
        for calendar in (all_days_calendar(), weekdays_calendar()):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                ),
                spot_price=100.25,
                valuation_time=start,
                calendar=calendar,
            )
            for scheme in ("crank_nicolson", "implicit_euler", "explicit_euler"):
                with self.subTest(calendar=calendar, scheme=scheme):
                    engine = pricing.FiniteDifferenceAutocallableEngine(
                        asset_step_count=80,
                        time_step_count=2000,
                        scheme=scheme,
                        asset_upper_boundary=400,
                    )
                    result = engine.price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    self.assertAlmostEqual(result.price, 1, delta=1e-5)
                    self.assertAlmostEqual(result.delta, 0, delta=1e-6)
                    self.assertAlmostEqual(result.gamma, 0, delta=1e-7)
                    self.assertEqual(engine.price(option, context), result.price)
