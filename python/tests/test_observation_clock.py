"""Observation clocks are explicit and independent of maturity settlement."""

import math
import pickle
import unittest
from datetime import UTC, date, datetime, time

from kiyosi import InvalidInputError, UnsupportedOperationError
from kiyosi.instruments import (
    Accumulator,
    AutocallableObservation,
    RangeAccrualOption,
    SnowballOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    fixed_interval_schedule,
    monthly_schedule,
)
from kiyosi.pricing import (
    AnalyticRangeAccrualEngine,
    FiniteDifferenceAccumulatorEngine,
    FiniteDifferenceAutocallableEngine,
    MonteCarloAccumulatorEngine,
    MonteCarloAutocallableEngine,
    QuadratureAccumulatorEngine,
    QuadratureAutocallableEngine,
    implied_volatility,
)
from support.history import dated as dated_history
from support.history import history_fields

START = datetime(2025, 1, 6, 9)
END = datetime(2025, 1, 6, 18)
CLOCK = time(15, 0, 0, 123456)


def range_terms():
    return {
        "notional": 1000,
        "annual_coupon_rate": 0.12,
        "lower_bound": 90,
        "upper_bound": 110,
        "effective_time": START,
        "expiry_time": END,
    }


def snowball_terms():
    return {
        "maturity_coupon_rate": 0.12,
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 0,
        "effective_time": START,
        "expiry_time": END,
        "observations": [AutocallableObservation.snowball(END, 1000, coupon_rate=0)],
    }


def context(at, spot=100, rate=0):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=0, volatility=0
        ),
        spot_price=spot,
        valuation_time=at,
    )


class ObservationClockTests(unittest.TestCase):
    def test_clock_requires_naive_time_and_preserves_microseconds(self):
        class HiddenTimezone(time):
            tzinfo = property(lambda self: None)

        class NanosecondTime(time):
            nanosecond = 1

        for value in (
            date(2025, 1, 6),
            START,
            "15:00",
            15,
            True,
            None,
            time(15, tzinfo=UTC),
            HiddenTimezone(15, tzinfo=UTC),
            NanosecondTime(15),
        ):
            with self.subTest(value=value), self.assertRaises(TypeError):
                RangeAccrualOption(
                    **history_fields(observation_clock_time=value, **range_terms())
                )
        with self.assertRaises(TypeError):
            RangeAccrualOption(**history_fields(**range_terms()))
        note = RangeAccrualOption(
            **history_fields(observation_clock_time=CLOCK, **range_terms())
        )
        self.assertEqual(note.observation_clock_time, CLOCK)
        self.assertEqual(pickle.loads(pickle.dumps(note)), note)
        replacement = note.replace(observation_clock_time=time(14, 30, 1, 999999))
        self.assertEqual(replacement.observation_clock_time, time(14, 30, 1, 999999))
        self.assertEqual(note.observation_clock_time, CLOCK)

    def test_generated_sequences_use_explicit_clock_and_complete_upper_bound(self):
        schedule = monthly_schedule(
            start_time=datetime(2025, 1, 31, 9),
            end_time=datetime(2025, 3, 31, 14),
            first_observation_month=1,
            observation_clock_time=CLOCK,
        )
        self.assertEqual(list(schedule), [datetime(2025, 2, 28, 15, 0, 0, 123456)])
        empty = fixed_interval_schedule(
            start_time=datetime(2025, 1, 3, 16),
            end_time=datetime(2025, 1, 6, 14),
            interval_days=1,
            observation_clock_time=CLOCK,
        )
        self.assertEqual(list(empty), [])

    def test_range_accrual_rejects_empty_schedule_atomically(self):
        with self.assertRaises(InvalidInputError):
            RangeAccrualOption(
                **history_fields(
                    observation_clock_time=time(15),
                    **(range_terms() | {"effective_time": datetime(2025, 1, 6, 16)}),
                )
            )
        note = RangeAccrualOption(
            **history_fields(observation_clock_time=CLOCK, **range_terms())
        )
        with self.assertRaises(InvalidInputError):
            note.replace(observation_clock_time=time(8))
        self.assertEqual(note.observation_clock_time, CLOCK)

    def test_completed_range_observations_discount_coupon_until_maturity(self):
        engine = AnalyticRangeAccrualEngine()
        for hour, hits in ((15, None), (16, 1), (18, 1)):
            with self.subTest(hour=hour):
                note = RangeAccrualOption(
                    **history_fields(
                        observation_clock_time=time(15),
                        realized_hit_count=hits,
                        **range_terms(),
                    )
                )
                market = context(datetime(2025, 1, 6, hour), rate=0.5).replace(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.5, dividend_yield=0, volatility=0.2
                    )
                )
                expected = (
                    120 * 9 / (365 * 24) * math.exp(-0.5 * (18 - hour) / (365 * 24))
                )
                actual = engine.price(dated_history(note, market), market)
                self.assertAlmostEqual(actual, expected, delta=1e-14)
                with self.assertRaises(UnsupportedOperationError):
                    implied_volatility(
                        engine,
                        dated_history(note, market),
                        market,
                        actual,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )

    def test_accumulator_allows_no_daily_events_and_does_not_add_units_at_expiry(self):
        terms = {
            "strike": 100,
            "knock_out_level": 110,
            "daily_quantity": 1,
            "acceleration_factor": 1,
            "effective_time": datetime(2025, 1, 6, 16),
            "expiry_time": END,
            "observation_clock_time": time(15),
        }
        empty = Accumulator(**history_fields(**terms))
        engine = QuadratureAccumulatorEngine()
        self.assertEqual(
            engine.price(
                dated_history(empty, context(terms["effective_time"], 105)),
                context(terms["effective_time"], 105),
            ),
            0,
        )
        observed = empty.replace(
            effective_time=START,
            accumulated_quantity=3,
            knock_out_settled=False,
            history_time=empty.history_time or empty.expiry_time,
        )
        self.assertEqual(
            engine.price(dated_history(observed, context(END, 105)), context(END, 105)),
            15,
        )
        self.assertEqual(pickle.loads(pickle.dumps(observed)), observed)

    def test_daily_knock_in_does_not_observe_off_clock_maturity(self):
        note = SnowballOption(
            **history_fields(
                observation_clock_time=time(15),
                barrier_state="no_event",
                **snowball_terms(),
            )
        )
        expected = 1 + 0.12 * 9 / (365 * 24)
        for engine in (
            QuadratureAutocallableEngine(),
            FiniteDifferenceAutocallableEngine(),
            MonteCarloAutocallableEngine(path_count=32, seed=42),
        ):
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(
                        dated_history(note, context(END, 70)), context(END, 70)
                    ),
                    expected,
                )
        empty = note.replace(
            effective_time=datetime(2025, 1, 6, 16), barrier_state=None
        )
        self.assertAlmostEqual(
            QuadratureAutocallableEngine().price(
                dated_history(empty, context(END, 70)), context(END, 70)
            ),
            1 + 0.12 * 2 / (365 * 24),
        )

    def test_current_daily_observation_uses_the_exact_microsecond(self):
        option = Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=1,
            effective_time=datetime(2025, 1, 6, 15, 0, 0, 123455),
            expiry_time=END,
            observation_clock_time=CLOCK,
        )
        engine = QuadratureAccumulatorEngine()
        observed = datetime(2025, 1, 6, 15, 0, 0, 123456)
        self.assertEqual(
            engine.price(
                dated_history(option, context(observed, 105)), context(observed, 105)
            ),
            5,
        )
        with self.assertRaises(InvalidInputError):
            engine.price(
                dated_history(
                    option, context(observed.replace(microsecond=123457), 105)
                ),
                context(observed.replace(microsecond=123457), 105),
            )

    def test_future_accumulation_occurs_once_before_off_clock_maturity(self):
        at = datetime(2025, 1, 6, 15, 0, 0, 123455)
        option = Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=1,
            effective_time=at,
            expiry_time=END,
            observation_clock_time=CLOCK,
        )
        market = context(at, 105).replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.000001
            )
        )
        for engine in (
            QuadratureAccumulatorEngine(),
            FiniteDifferenceAccumulatorEngine(),
            MonteCarloAccumulatorEngine(path_count=32, seed=42),
        ):
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(dated_history(option, market), market), 5, delta=1e-4
                )

    def test_future_pricing_after_last_observation_does_not_add_maturity_knock_in(self):
        note = SnowballOption(
            **history_fields(
                observation_clock_time=time(15),
                barrier_state="no_event",
                **snowball_terms(),
            )
        )
        at = datetime(2025, 1, 6, 15, 0, 0, 1)
        market = context(at, spot=81, rate=-150).replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=-150, dividend_yield=0, volatility=0.000001
            )
        )
        remaining = (END - at).total_seconds() / (365 * 86400)
        expected = (1 + 0.12 * 9 / (365 * 24)) * math.exp(150 * remaining)
        for engine in (
            QuadratureAutocallableEngine(),
            FiniteDifferenceAutocallableEngine(),
            MonteCarloAutocallableEngine(path_count=32, seed=42),
        ):
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(dated_history(note, market), market),
                    expected,
                    delta=1e-6,
                )

    def test_monitoring_replacement_requires_explicit_clock_changes(self):
        expiry = SnowballOption(
            **history_fields(
                observation_clock_time=None,
                knock_in_observation_mode="at_expiry",
                **snowball_terms(),
            )
        )
        self.assertIsNone(expiry.observation_clock_time)
        with self.assertRaises(InvalidInputError):
            expiry.replace(knock_in_observation_mode="every_trading_day")
        daily = expiry.replace(
            knock_in_observation_mode="every_trading_day", observation_clock_time=CLOCK
        )
        self.assertEqual(daily.observation_clock_time, CLOCK)
        self.assertEqual(pickle.loads(pickle.dumps(daily)), daily)
        with self.assertRaises(InvalidInputError):
            daily.replace(knock_in_observation_mode="at_expiry")
        self.assertIsNone(
            daily.replace(
                knock_in_observation_mode="at_expiry", observation_clock_time=None
            ).observation_clock_time
        )
        with self.assertRaises(InvalidInputError):
            daily.replace(
                knock_in_observation_mode="at_expiry", observation_clock_time=CLOCK
            )
        with self.assertRaises(InvalidInputError):
            SnowballOption(
                **history_fields(
                    knock_in_observation_mode="at_expiry",
                    observation_clock_time=CLOCK,
                    **snowball_terms(),
                )
            )
        self.assertEqual(daily.observation_clock_time, CLOCK)

    def test_completed_daily_monitoring_has_no_implied_volatility_or_expiry_greeks_jump(
        self,
    ):
        daily = SnowballOption(
            **history_fields(
                observation_clock_time=time(15),
                barrier_state="no_event",
                **(
                    snowball_terms()
                    | {
                        "observations": [
                            AutocallableObservation.snowball(
                                datetime(2025, 1, 6, 10), 1000, coupon_rate=0
                            )
                        ]
                    }
                ),
            )
        )
        expiry = daily.replace(
            knock_in_observation_mode="at_expiry",
            observation_clock_time=None,
            barrier_state=daily.barrier_state,
            history_time=(daily.history_time or daily.effective_time)
            if daily.barrier_state is not None
            else None,
        )
        expected = 1 + 0.12 * 9 / (365 * 24)
        for engine in (
            MonteCarloAutocallableEngine(path_count=128, seed=42),
            FiniteDifferenceAutocallableEngine(),
            QuadratureAutocallableEngine(),
        ):
            for hour in (16, 18):
                for volatility in (0, 0.0001):
                    with self.subTest(
                        engine=type(engine).__name__, hour=hour, volatility=volatility
                    ):
                        market = context(datetime(2025, 1, 6, hour), 70).replace(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0,
                                dividend_yield=0,
                                volatility=volatility,
                            )
                        )
                        actual = engine.price(dated_history(daily, market), market)
                        self.assertAlmostEqual(actual, expected, delta=1e-8)
                        with self.assertRaises(UnsupportedOperationError):
                            implied_volatility(
                                engine,
                                dated_history(daily, market),
                                market,
                                actual,
                                lower_bound=0.0,
                                upper_bound=4.0,
                            )
                        self.assertAlmostEqual(
                            engine.price(dated_history(expiry, market), market),
                            0.7,
                            delta=1e-5,
                        )
            result = engine.price_with_greeks(
                dated_history(daily, context(END, 80)),
                context(END, 80),
                ["delta", "gamma"],
            )
            self.assertEqual(result.delta, 0)
            self.assertEqual(result.gamma, 0)
        self.assertIsNone(
            QuadratureAutocallableEngine()
            .price_with_greeks(
                dated_history(expiry, context(END, 80)), context(END, 80), "delta"
            )
            .delta
        )
