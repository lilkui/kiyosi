"""Intraday contracts use the same timestamp and event semantics as Rust."""

import inspect
import math
import pickle
import unittest
from datetime import UTC, date, datetime, timedelta
from datetime import time as clock_time

from kiyosi import ComputationFailureError, InvalidInputError
from kiyosi.instruments import (
    Accumulator,
    BarrierOption,
    EuropeanOption,
    PhoenixOption,
    RangeAccrualOption,
    SnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    fixed_interval_schedule,
    monthly_schedule,
    weekdays_calendar,
)
from kiyosi.pricing import (
    AnalyticBarrierEngine,
    AnalyticEuropeanEngine,
    AnalyticRangeAccrualEngine,
    FiniteDifferenceAccumulatorEngine,
    FiniteDifferenceAutocallableEngine,
    MonteCarloAccumulatorEngine,
    MonteCarloAutocallableEngine,
    QuadratureAccumulatorEngine,
    QuadratureAutocallableEngine,
    numerical_greeks,
)


def time(day, hour, microsecond=0):
    return datetime(2025, 1, day, hour, microsecond=microsecond)


def market(at, spot=105):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.0001
        ),
        spot_price=spot,
        valuation_time=at,
    )


def accumulator(**changes):
    fields = {
        "strike": 100,
        "knock_out_level": 110,
        "daily_quantity": 1,
        "acceleration_factor": 1,
        "effective_time": time(6, 9),
        "expiry_time": time(7, 15),
    }
    fields.update(changes)
    return Accumulator(observation_clock_time=clock_time(15), **fields)


def accumulator_engines():
    return (
        QuadratureAccumulatorEngine(),
        FiniteDifferenceAccumulatorEngine(
            asset_step_count=600, time_step_count=100, asset_upper_boundary=160
        ),
        MonteCarloAccumulatorEngine(path_count=128, seed=42),
    )


class TimestampTests(unittest.TestCase):
    def test_subday_time_bumps_converge_to_intraday_theta(self):
        option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_time=time(2, 11),
            expiry_time=time(2, 13),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0,
                dividend_yield=0,
                volatility=0.2,
            ),
            spot_price=100,
            valuation_time=time(2, 12),
        )
        engine = AnalyticEuropeanEngine()
        exact = engine.price_with_greeks(option, context, "theta").require("theta")
        errors = [
            abs(
                numerical_greeks(
                    engine,
                    option,
                    context,
                    "theta",
                    time_shift=timedelta(seconds=seconds),
                ).require("theta")
                - exact
            )
            for seconds in (60, 1)
        ]
        self.assertLess(errors[1], errors[0])
        self.assertLess(errors[1], abs(exact) * 1e-7)
        for shift in (timedelta(), timedelta(microseconds=-1)):
            with self.subTest(shift=shift), self.assertRaises(InvalidInputError):
                numerical_greeks(engine, option, context, "theta", time_shift=shift)
        for shift in (True, 1, 0.001, None):
            with self.subTest(shift=shift), self.assertRaises(TypeError):
                numerical_greeks(engine, option, context, "theta", time_shift=shift)

    def test_time_inputs_reject_dates_timezones_and_lost_precision(self):
        class NanosecondTime(datetime):
            nanosecond = 1

        class HiddenTimezone(datetime):
            tzinfo = property(lambda self: None)

        for value in (
            date(2025, 1, 6),
            time(6, 9).replace(tzinfo=UTC),
            HiddenTimezone(2025, 1, 6, tzinfo=UTC),
            NanosecondTime(2025, 1, 6),
            True,
            1,
            "2025-01-06",
        ):
            for operation in (
                lambda value=value: market(value),
                lambda value=value: accumulator(effective_time=value),
                lambda value=value: RateTermStructure.flat(0.02, reference_time=value),
                lambda value=value: autocallable_observations([value], [110]),
                lambda value=value: fixed_interval_schedule(
                    observation_clock_time=clock_time(),
                    start_time=value,
                    end_time=time(7, 15),
                    interval_days=1,
                ),
            ):
                with self.subTest(value=value), self.assertRaises(TypeError):
                    operation()
        self.assertTrue(weekdays_calendar().is_trading_day(date(2025, 1, 6)))
        with self.assertRaises(TypeError):
            weekdays_calendar().is_trading_day(time(6, 9))

    def test_intraday_expiry_and_accrual_preserve_microseconds(self):
        start, expiry = time(6, 10, 123456), time(6, 15, 654321)
        context = market(start, 100).replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            )
        )
        call = EuropeanOption(
            option_type="call", strike=100, effective_time=start, expiry_time=expiry
        )
        put = call.replace(option_type="put")
        years = (18000 + 0.530865) / (365 * 86400)
        engine = AnalyticEuropeanEngine()
        self.assertAlmostEqual(
            engine.price(call, context) - engine.price(put, context),
            100 * (math.exp(-0.02 * years) - math.exp(-0.05 * years)),
            delta=1e-12,
        )
        self.assertAlmostEqual(
            context.model_parameters.risk_free_curve.factor(start, expiry),
            math.exp(-0.05 * years),
            delta=1e-15,
        )
        self.assertEqual(engine.price(call, market(expiry, 110)), 10)
        for at in (time(6, 15, 654323), time(6, 15, 654322)):
            with self.subTest(at=at), self.assertRaises(InvalidInputError):
                engine.price(call, market(at))
        self.assertEqual(pickle.loads(pickle.dumps(call)), call)
        self.assertIsNone(call.expiry_time.tzinfo)
        self.assertEqual(call.expiry_time, expiry)
        self.assertIn("effective_time", inspect.signature(EuropeanOption).parameters)
        with self.assertRaises(TypeError):
            call.replace(expiry_date=expiry)
        with self.assertRaises(InvalidInputError):
            call.replace(expiry_time=time(6, 9))
        self.assertEqual(call.expiry_time, expiry)

    def test_observation_schedules_preserve_clock_and_full_timestamp_order(self):
        def clock(month, day, hour):
            return datetime(2025, month, day, hour, 11, 12, 123456)

        schedule = monthly_schedule(
            observation_clock_time=clock_time(10, 11, 12, 123456),
            start_time=clock(1, 31, 10),
            end_time=clock(3, 31, 15),
            first_observation_month=1,
        )
        self.assertEqual(schedule.times, [clock(2, 28, 10), clock(3, 31, 10)])
        self.assertEqual(pickle.loads(pickle.dumps(schedule)), schedule)
        fixed = fixed_interval_schedule(
            observation_clock_time=clock_time(10, 11, 12, 123456),
            start_time=clock(1, 3, 10),
            end_time=clock(1, 7, 9),
            interval_days=1,
        )
        self.assertEqual(list(fixed), [clock(1, 6, 10)])
        option = BarrierOption(
            option_type="call",
            strike=100,
            effective_time=time(6, 9),
            expiry_time=time(6, 16),
            barrier_level=110,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_times=[time(6, 10), time(6, 10, 1), time(6, 15)],
        )
        self.assertEqual(
            option.observation_times, [time(6, 10), time(6, 10, 1), time(6, 15)]
        )
        for invalid in ([time(6, 10)] * 2, [time(6, 15), time(6, 10)]):
            with self.subTest(invalid=invalid), self.assertRaises(InvalidInputError):
                option.replace(observation_times=invalid)

    def test_scheduled_barriers_use_exact_current_observations_and_strict_history(self):
        option = BarrierOption(
            option_type="call",
            strike=100,
            effective_time=time(6, 9),
            expiry_time=time(6, 16),
            barrier_level=110,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_times=[time(6, 10), time(6, 15)],
            rebate=7,
            rebate_timing="at_hit",
        )
        engine = AnalyticBarrierEngine()
        self.assertEqual(engine.price(option, market(time(6, 10), 110)), 7)
        with self.assertRaises(InvalidInputError):
            engine.price(option, market(time(6, 10, 1), 100))
        touched = option.replace_history(touch_state="touched")
        with self.assertRaises(InvalidInputError):
            engine.price(touched, market(time(6, 9), 110))
        self.assertEqual(engine.price(touched, market(time(6, 10, 1), 100)), 0)

    def test_range_accrual_observes_same_day_and_excludes_only_the_effective_instant(
        self,
    ):
        option = RangeAccrualOption(
            observation_clock_time=clock_time(15),
            notional=1000,
            annual_coupon_rate=0.12,
            lower_bound=1,
            upper_bound=1000,
            effective_time=time(6, 9),
            expiry_time=time(6, 15),
        )
        engine = AnalyticRangeAccrualEngine()
        self.assertAlmostEqual(
            engine.price(option, market(time(6, 15), 100)),
            120 * 6 / (24 * 365),
            delta=1e-14,
        )
        option = option.replace(effective_time=time(6, 15), expiry_time=time(7, 15))
        self.assertAlmostEqual(
            engine.price(option, market(time(6, 15, 1), 100)), 120 / 365, delta=1e-14
        )
        with self.assertRaises(InvalidInputError):
            engine.price(
                option.replace_history(realized_hit_count=1),
                market(time(6, 15, 1), 100),
            )

    def test_daily_history_uses_observation_clock_and_inclusive_start(self):
        option = accumulator()
        for engine in accumulator_engines():
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(option, market(time(6, 10))), 10, delta=1e-4
                )
                with self.assertRaises(InvalidInputError):
                    engine.price(option, market(time(6, 15, 1)))
                single = option.replace(effective_time=time(7, 15))
                self.assertEqual(engine.price(single, market(time(7, 15))), 5)
        note = SnowballOption(
            observation_clock_time=clock_time(15),
            maturity_coupon_rate=0.06,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=time(6, 9),
            expiry_time=time(7, 15),
            observations=autocallable_observations([time(7, 15)], [110]),
        )
        engine = QuadratureAutocallableEngine()
        self.assertAlmostEqual(
            engine.price(note, market(time(6, 10), 100)),
            1 + 0.06 * 30 / (365 * 24),
            delta=1e-12,
        )
        with self.assertRaises(InvalidInputError):
            engine.price(note, market(time(6, 15, 1), 100))

    def test_explicit_accumulator_knock_outs_do_not_add_daily_quantity(self):
        for hour, barrier, expected in (
            (10, 110, 10),
            (10, 100, 0),
            (15, 100, 0),
            (16, 100, 5),
        ):
            option = accumulator(
                knock_out_level=barrier, knock_out_observation_times=[time(6, hour)]
            )
            for engine in accumulator_engines():
                with self.subTest(
                    hour=hour, barrier=barrier, engine=type(engine).__name__
                ):
                    self.assertAlmostEqual(
                        engine.price(option, market(time(6, 9))), expected, delta=1e-4
                    )

    def test_autocallable_explicit_events_do_not_trigger_daily_knock_in(self):
        note = SnowballOption(
            observation_clock_time=clock_time(15),
            maturity_coupon_rate=0.12,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=time(6, 9),
            expiry_time=time(6, 15),
            observations=autocallable_observations([time(6, 10)], [1000]),
        )
        context = market(time(6, 9), 70).replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=-1000, volatility=0.0001
            )
        )
        engines = (
            MonteCarloAutocallableEngine(path_count=128, seed=42),
            FiniteDifferenceAutocallableEngine(),
            QuadratureAutocallableEngine(),
        )
        for engine in engines:
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(note, context), 1 + 0.12 * 6 / (365 * 24), delta=1e-6
                )
        note = note.replace(
            maturity_coupon_rate=0,
            observations=autocallable_observations(
                [time(6, 10)], [110], knock_out_coupon_rates=[0.12]
            ),
        )
        for engine in engines:
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(note, market(time(6, 9), 120)),
                    1 + 0.12 / (365 * 24),
                    delta=1e-6,
                )

    def test_nontrading_dates_do_not_trigger_daily_knock_out_or_suppress_vega(self):
        option = accumulator(
            effective_time=time(3, 9), expiry_time=time(6, 15), accumulated_quantity=3
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=110,
            valuation_time=time(4, 15),
        )
        result = QuadratureAccumulatorEngine().price_with_greeks(
            option, context, "vega"
        )
        root_time = math.sqrt(2 / 365)
        expected = (
            -105
            * root_time
            * math.exp(-0.5 * (0.1 * root_time) ** 2)
            / math.sqrt(2 * math.pi)
        )
        self.assertAlmostEqual(result.vega, expected, delta=0.0001)

    def test_finite_difference_rejects_unresolvable_microsecond_events(self):
        start = datetime(1, 1, 1)
        first = datetime(9998, 1, 1, 10, microsecond=1)
        last = first.replace(microsecond=2)
        note = PhoenixOption(
            observation_clock_time=None,
            coupon_per_period=0.01,
            initial_spot=100,
            knock_in_level=1,
            upper_strike=1,
            lower_strike=1,
            effective_time=start,
            expiry_time=last,
            observations=autocallable_observations([first, last], [1000, 1000]),
            knock_in_observation_mode="at_expiry",
        )
        context = market(start, 100)
        with self.assertRaises(ComputationFailureError):
            FiniteDifferenceAutocallableEngine().price(note, context)
        self.assertAlmostEqual(
            QuadratureAutocallableEngine().price(note, context), 1.02, delta=1e-12
        )
