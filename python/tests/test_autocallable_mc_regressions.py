"""Autocallable Monte Carlo shared-path and event-grid regressions."""

import math
import unittest
from datetime import UTC, date, datetime

from kiyosi import ComputationFailureError, pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)


class AutocallableMonteCarloRegressionTests(unittest.TestCase):
    def test_scalar_price_survives_confidence_interval_overflow(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = BinarySnowballOption(
            maturity_coupon_rate=0,
            principal_ratio=0,
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [100], knock_out_coupon_rates=[1.5e308]
            ),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        engine = pricing.MonteCarloAutocallableEngine(
            path_count=2, seed=0, thread_count=1
        )
        self.assertAlmostEqual(engine.price(note, context) / 7.5e307, 1, delta=1e-12)
        with self.assertRaises(ComputationFailureError):
            engine.price_with_statistics(note, context)

    def test_shared_spot_simulations_match_separate_seeded_prices(self):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        observation_dates = [date(2025, 1, 3), date(2025, 1, 8), end]
        knock_out_levels = [101, 100, 99]
        terms = {"barrier_state": "none", "effective_date": start, "expiry_date": end}
        cases = [
            (
                BinarySnowballOption(
                    maturity_coupon_rate=0.05,
                    **terms,
                    observations=autocallable_observations(
                        observation_dates,
                        knock_out_levels,
                        knock_out_coupon_rates=[0.08] * 3,
                    ),
                ),
                pricing.MonteCarloAutocallableEngine,
            ),
        ]
        for mode in ("every_trading_day", "at_expiry"):
            common = {**terms, "knock_in_level": 99, "knock_in_observation_mode": mode}
            cases.extend(
                [
                    (
                        SnowballOption(
                            maturity_coupon_rate=0.06,
                            initial_spot=100,
                            upper_strike=100,
                            lower_strike=60,
                            **common,
                            observations=autocallable_observations(
                                observation_dates,
                                knock_out_levels,
                                knock_out_coupon_rates=[0.08] * 3,
                            ),
                        ),
                        pricing.MonteCarloAutocallableEngine,
                    ),
                    (
                        PhoenixOption(
                            coupon_rate=0.08,
                            initial_spot=100,
                            upper_strike=100,
                            lower_strike=60,
                            **common,
                            observations=autocallable_observations(
                                observation_dates,
                                knock_out_levels,
                                coupon_barrier_levels=[100, 99, 98],
                            ),
                        ),
                        pricing.MonteCarloAutocallableEngine,
                    ),
                    (
                        TernarySnowballOption(
                            maturity_coupon_rate=0.06,
                            minimum_coupon_rate=0.02,
                            **common,
                            observations=autocallable_observations(
                                observation_dates,
                                knock_out_levels,
                                knock_out_coupon_rates=[0.08] * 3,
                            ),
                        ),
                        pricing.MonteCarloAutocallableEngine,
                    ),
                ]
            )
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.8
        )
        spot, shift = 100.25, 0.2
        h = (spot + shift) - spot
        contexts = [
            PricingContext(
                model_parameters=parameters,
                spot_price=spot + offset * h,
                valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
                calendar=all_days_calendar(),
            )
            for offset in (0, 1, -1, 2, -2)
        ]
        for note, engine_type in cases:
            for paths in (1, 31, 32):
                for seed in (42, 2**64 - 1):
                    with self.subTest(note=type(note).__name__, paths=paths, seed=seed):
                        engine = engine_type(path_count=paths, seed=seed)
                        p, up, down, up2, down2 = [
                            engine.price(note, ctx) for ctx in contexts
                        ]
                        result = engine.price_with_greeks(
                            note,
                            contexts[0],
                            ["delta", "gamma", "speed"],
                            spot_shift=shift,
                        )
                        self.assertEqual(result.price, p)
                        self.assertEqual(result.delta, (up - down) / (2 * h))
                        self.assertEqual(result.gamma, (up - 2 * p + down) / h / h)
                        self.assertEqual(
                            result.speed,
                            (up2 - 2 * up + 2 * down - down2) / h / h / (2 * h),
                        )
                        numerical = engine.price_with_greeks(
                            note, contexts[0], greeks="all", spot_shift=shift
                        )
                        self.assertEqual(
                            (result.price, result.delta, result.gamma, result.speed),
                            (
                                numerical.price,
                                numerical.delta,
                                numerical.gamma,
                                numerical.speed,
                            ),
                        )
                        self.assertIsNone(result.vega)

    def test_irrelevant_knock_in_events_preserve_spot_and_time_greeks(self):
        import math

        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = SnowballOption(
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=75,
            upper_strike=100,
            lower_strike=100,
            barrier_state="none",
            knock_in_observation_mode="every_trading_day",
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [110], knock_out_coupon_rates=[0]
            ),
        )
        for engine in (pricing.MonteCarloAutocallableEngine(path_count=64, seed=42),):
            for time in (
                datetime(2025, 1, 1, tzinfo=UTC),
                datetime(2025, 1, 2, 12, tzinfo=UTC),
            ):
                with self.subTest(engine=type(engine).__name__, time=time):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                        ),
                        spot_price=75,
                        valuation_time=time,
                    )
                    result = engine.price_with_greeks(
                        note, context, ["delta", "gamma", "theta", "charm", "color"]
                    )
                    years = (
                        datetime(2026, 1, 1, tzinfo=UTC) - time
                    ).total_seconds() / (365 * 86400)
                    self.assertAlmostEqual(
                        result.price, math.exp(-0.04 * years), delta=1e-12
                    )
                    self.assertEqual(
                        (result.delta, result.gamma, result.charm, result.color),
                        (0, 0, 0, 0),
                    )
                    self.assertIsNotNone(result.theta)
                    self.assertAlmostEqual(
                        result.theta, 0.04 / 365 * result.price, delta=1e-8
                    )

    def test_completed_knock_in_preserves_time_greeks_without_hiding_live_events(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=75,
            valuation_time=datetime(2025, 1, 2, 12, tzinfo=UTC),
        )
        for engine in (pricing.MonteCarloAutocallableEngine(path_count=64, seed=42),):
            for state in ("none", "knocked_in"):
                with self.subTest(engine=type(engine).__name__, state=state):
                    note = SnowballOption(
                        maturity_coupon_rate=0.06,
                        initial_spot=100,
                        knock_in_level=75,
                        upper_strike=100,
                        lower_strike=60,
                        barrier_state=state,
                        effective_date=start,
                        expiry_date=end,
                        knock_in_observation_mode="every_trading_day",
                        observations=autocallable_observations(
                            [end], [110], knock_out_coupon_rates=[0.08]
                        ),
                    )
                    result = engine.price_with_greeks(
                        note, context, ["theta", "charm", "color"]
                    )
                    for value in (result.theta, result.charm, result.color):
                        self.assertEqual(value is not None, state == "knocked_in")

    def test_fixed_implied_volatility_preserves_non_finite_pricing_errors(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = BinarySnowballOption(
            maturity_coupon_rate=1e308,
            principal_ratio=1e308,
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [110], knock_out_coupon_rates=[1e308]
            ),
        )
        for valuation in (start, end):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=0.2
                ),
                spot_price=100,
                valuation_time=valuation,
            )
            with self.subTest(valuation=valuation):
                with self.assertRaises(ComputationFailureError) as error:
                    pricing.implied_volatility(
                        pricing.MonteCarloAutocallableEngine(path_count=2, seed=1),
                        note,
                        context,
                        1,
                    )
                self.assertIs(type(error.exception), ComputationFailureError)

    def test_event_only_monte_carlo_ignores_unobserved_trading_days(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0, volatility=0.3
        )
        for observations in ([date(2025, 7, 1)], [date(2025, 7, 1), end]):
            terms = {
                "maturity_coupon_rate": 0.04,
                "effective_date": start,
                "expiry_date": end,
                "observations": autocallable_observations(
                    observations,
                    [110] * len(observations),
                    knock_out_coupon_rates=[0.08] * len(observations),
                ),
            }
            for mode in ("at_expiry", "every_trading_day"):
                snowball = SnowballOption(
                    **terms,
                    initial_spot=100,
                    knock_in_level=90,
                    upper_strike=100,
                    lower_strike=60,
                    knock_in_observation_mode=mode,
                )
                for note, engine in (
                    (
                        BinarySnowballOption(**terms),
                        pricing.MonteCarloAutocallableEngine(path_count=2000, seed=42),
                    ),
                    (
                        snowball,
                        pricing.MonteCarloAutocallableEngine(path_count=2000, seed=42),
                    ),
                ):
                    with self.subTest(
                        observations=observations, note=type(note).__name__, mode=mode
                    ):
                        prices = [
                            engine.price(
                                note,
                                PricingContext(
                                    model_parameters=parameters,
                                    spot_price=100,
                                    valuation_time=start,
                                    calendar=calendar,
                                ),
                            )
                            for calendar in (weekdays_calendar(), all_days_calendar())
                        ]
                        if (
                            isinstance(note, SnowballOption)
                            and mode == "every_trading_day"
                        ):
                            self.assertNotEqual(*prices)
                        else:
                            self.assertEqual(*prices)

    def check_autocallable(self, note, context, expected):
        for engine in (pricing.MonteCarloAutocallableEngine(path_count=32, seed=7),):
            with self.subTest(
                engine=type(engine).__name__, product=type(note).__name__
            ):
                self.assertAlmostEqual(
                    engine.price(note, context), expected, delta=1e-12
                )

    def test_current_knockouts_pay_only_contractual_coupon(self):
        common = {"effective_date": START, "expiry_date": END}
        observations = autocallable_observations(
            [FIRST, MIDDLE, END],
            [100, 100, 100],
            knock_out_coupon_rates=[-0.365, 100, 100],
        )
        strikes = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
        notes = (
            BinarySnowballOption(
                **common,
                maturity_coupon_rate=5,
                barrier_state="none",
                observations=observations,
            ),
            SnowballOption(
                **common,
                **strikes,
                maturity_coupon_rate=5,
                knock_in_level=80,
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in",
                observations=observations,
            ),
            TernarySnowballOption(
                **common,
                maturity_coupon_rate=5,
                minimum_coupon_rate=-1,
                knock_in_level=80,
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in",
                observations=observations,
            ),
        )
        for note in notes:
            self.check_autocallable(note, cashflow_market(100, FIRST), 0.99)
        for barrier, expected in ((100, 0.99), (math.nextafter(100, 200), 1)):
            note = PhoenixOption(
                **common,
                **strikes,
                coupon_rate=-0.365,
                knock_in_level=80,
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in",
                observations=autocallable_observations(
                    [FIRST, END], [100, 100], coupon_barrier_levels=[barrier, 0]
                ),
            )
            self.check_autocallable(note, cashflow_market(100, FIRST), expected)

    def test_historical_knockin_and_terminal_caps(self):
        common = {
            "initial_spot": 100,
            "knock_in_level": 80,
            "knock_in_observation_mode": "every_trading_day",
            "upper_strike": 100,
            "lower_strike": 60,
            "barrier_state": "knocked_in",
            "principal_ratio": 1.25,
            "effective_date": START,
            "expiry_date": END,
        }
        snowball = SnowballOption(
            **common,
            maturity_coupon_rate=5,
            observations=autocallable_observations(
                [END], [110], knock_out_coupon_rates=[100]
            ),
        )
        phoenix = PhoenixOption(
            **common,
            coupon_rate=-0.365,
            observations=autocallable_observations(
                [FIRST, END], [110, 110], coupon_barrier_levels=[90, 90]
            ),
        )
        for spot, expected in ((50, 0.85), (90, 1.15), (105, 1.25)):
            self.check_autocallable(snowball, cashflow_market(spot, END), expected)
            self.check_autocallable(
                phoenix,
                cashflow_market(spot, END),
                expected - (0.02 if spot >= 90 else 0),
            )


START, FIRST, MIDDLE, END = (
    date(2025, 1, 1),
    date(2025, 1, 11),
    date(2025, 1, 21),
    date(2025, 1, 31),
)


def cashflow_market(spot, valuation):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=spot,
        valuation_time=valuation,
        calendar=all_days_calendar(),
    )
