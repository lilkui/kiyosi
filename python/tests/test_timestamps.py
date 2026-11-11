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
    BinarySnowballOption,
    EuropeanOption,
    PhoenixOption,
    RangeAccrualOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    custom_calendar,
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
from support.history import dated as dated_history
from support.history import history_fields


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
    return Accumulator(
        **history_fields(observation_clock_time=clock_time(15), **fields)
    )


def accumulator_engines():
    return (
        QuadratureAccumulatorEngine(),
        FiniteDifferenceAccumulatorEngine(
            asset_step_count=600, time_step_count=100, asset_upper_boundary=160
        ),
        MonteCarloAccumulatorEngine(path_count=128, seed=42),
    )


class TimestampTests(unittest.TestCase):
    def test_closed_date_settlement_preserves_open_date_observations(self):
        start, observation, valuation = time(2, 16), time(3, 15), time(3, 16)
        calendar = custom_calendar(
            base=weekdays_calendar(), closed_dates=[date(2025, 1, 6)]
        )
        for expiry in (time(4, 16), time(5, 16), time(6, 16)):
            life = {
                "effective_time": start,
                "expiry_time": expiry,
                "calendar": calendar,
            }
            range_option = RangeAccrualOption(
                **history_fields(
                    notional=1000,
                    annual_coupon_rate=0.12,
                    lower_bound=80,
                    upper_bound=120,
                    observation_clock_time=clock_time(15),
                    realized_hit_count=1,
                    **life,
                )
            )
            accumulated = Accumulator(
                **history_fields(
                    knock_out_settled=False,
                    strike=90,
                    knock_out_level=110,
                    daily_quantity=1,
                    acceleration_factor=2,
                    accumulated_quantity=3,
                    observation_clock_time=clock_time(15),
                    **life,
                )
            )
            fields = dict(
                maturity_coupon_rate=0.12,
                initial_spot=100,
                knock_in_level=80,
                upper_strike=100,
                lower_strike=0,
                observation_clock_time=clock_time(15),
                observations=autocallable_observations(
                    [observation], [110], knock_out_coupon_rates=[0]
                ),
                barrier_state="no_event",
                **life,
            )
            note = SnowballOption(**history_fields(**fields))
            binary = BinarySnowballOption(
                **history_fields(
                    maturity_coupon_rate=0.12,
                    observations=fields["observations"],
                    barrier_state="no_event",
                    **life,
                )
            )
            for schedule in (
                range_option.observation_schedule,
                accumulated.accumulation_schedule,
                accumulated.knock_out_observation_schedule,
                note.knock_in_observation_schedule,
            ):
                self.assertEqual(list(schedule), [observation])
            with self.assertRaises(InvalidInputError):
                SnowballOption(
                    **history_fields(
                        **(
                            fields
                            | {
                                "knock_in_observation_mode": "at_expiry",
                                "observation_clock_time": None,
                            }
                        )
                    )
                )
            coupon = 0.12 * (expiry - start).total_seconds() / (365 * 86400)
            for volatility in (0, 0.2):
                for at in (valuation, expiry):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.05, dividend_yield=0, volatility=volatility
                        ),
                        spot_price=70,
                        valuation_time=at,
                    )
                    discount = math.exp(
                        -0.05 * (expiry - at).total_seconds() / (365 * 86400)
                    )
                    self.assertAlmostEqual(
                        AnalyticRangeAccrualEngine().price(
                            dated_history(range_option, context), context
                        ),
                        1000 * coupon * discount,
                        delta=1e-12,
                    )
                    for option in (note, binary):
                        self.assertEqual(pickle.loads(pickle.dumps(option)), option)
                        for engine in (
                            QuadratureAutocallableEngine(),
                            FiniteDifferenceAutocallableEngine(),
                            MonteCarloAutocallableEngine(),
                        ):
                            self.assertAlmostEqual(
                                engine.price(dated_history(option, context), context),
                                (1 + coupon) * discount,
                                delta=1e-12,
                            )
                    if at == expiry:
                        for engine in accumulator_engines():
                            self.assertEqual(
                                engine.price(
                                    dated_history(accumulated, context), context
                                ),
                                -60,
                            )

    def test_contract_schedules_expose_effective_observations(self):
        calendar = custom_calendar(
            base=weekdays_calendar(), closed_dates=[date(2025, 1, 7)]
        )
        clock = clock_time(15, microsecond=123456)
        start = datetime(2025, 1, 6, 15, microsecond=123456)
        last = datetime(2025, 1, 8, 15, microsecond=123456)
        expiry = datetime(2025, 1, 8, 17)
        life = {"effective_time": start, "expiry_time": expiry, "calendar": calendar}
        accrual = RangeAccrualOption(
            **history_fields(
                notional=1000,
                annual_coupon_rate=0.12,
                lower_bound=90,
                upper_bound=110,
                observation_clock_time=clock,
                **life,
            )
        )
        schedule = accrual.observation_schedule
        del accrual
        self.assertEqual(list(schedule), [last])
        accumulator = Accumulator(
            **history_fields(
                strike=90,
                knock_out_level=120,
                daily_quantity=1,
                acceleration_factor=2,
                observation_clock_time=clock,
                **life,
            )
        )
        self.assertEqual(list(accumulator.accumulation_schedule), [start, last])
        self.assertEqual(
            accumulator.knock_out_observation_schedule,
            accumulator.accumulation_schedule,
        )
        for explicit in ([], [datetime(2025, 1, 6, 16)]):
            changed = accumulator.replace(knock_out_observation_times=explicit)
            self.assertEqual(list(changed.knock_out_observation_schedule), explicit)
            self.assertEqual(list(changed.accumulation_schedule), [start, last])
        observations = autocallable_observations(
            [expiry], [110], knock_out_coupon_rates=[0]
        )
        terms = dict(
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            observation_clock_time=clock,
            observations=observations,
            **life,
        )
        notes = [
            SnowballOption(**history_fields(maturity_coupon_rate=0.1, **terms)),
            PhoenixOption(
                **history_fields(
                    coupon_per_period=0.1,
                    **dict(
                        terms,
                        observations=autocallable_observations(
                            [expiry], [110], coupon_barrier_levels=[0]
                        ),
                    ),
                )
            ),
            TernarySnowballOption(
                **history_fields(
                    maturity_coupon_rate=0.1,
                    minimum_coupon_rate=0.02,
                    knock_in_level=80,
                    observation_clock_time=clock,
                    observations=observations,
                    **life,
                )
            ),
        ]
        for note in notes:
            with self.subTest(note=type(note).__name__):
                self.assertEqual(
                    list(note.knock_in_observation_schedule), [start, last]
                )
                expiry_only = note.replace(
                    knock_in_observation_mode="at_expiry", observation_clock_time=None
                )
                self.assertEqual(
                    list(expiry_only.knock_in_observation_schedule), [expiry]
                )
                exported = list(note.knock_in_observation_schedule.times)
                exported.clear()
                self.assertEqual(
                    list(note.knock_in_observation_schedule), [start, last]
                )
        binary = BinarySnowballOption(
            **history_fields(
                maturity_coupon_rate=0.1, observations=observations, **life
            )
        )
        self.assertIsNone(binary.knock_in_observation_schedule)

    def test_subday_time_bumps_converge_to_intraday_theta(self):
        class NanosecondDuration(timedelta):
            nanoseconds = 1

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
        exact = engine.price_with_greeks(
            dated_history(option, context), context, "theta"
        ).require("theta")
        errors = [
            abs(
                numerical_greeks(
                    engine,
                    dated_history(option, context),
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
                numerical_greeks(
                    engine,
                    dated_history(option, context),
                    context,
                    "theta",
                    time_shift=shift,
                )
        for shift in (True, 1, 0.001, NanosecondDuration(microseconds=1)):
            with self.subTest(shift=shift), self.assertRaises(TypeError):
                numerical_greeks(
                    engine,
                    dated_history(option, context),
                    context,
                    "theta",
                    time_shift=shift,
                )

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
                lambda value=value: autocallable_observations(
                    [value], [110], knock_out_coupon_rates=[0]
                ),
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
            engine.price(dated_history(call, context), context)
            - engine.price(dated_history(put, context), context),
            100 * (math.exp(-0.02 * years) - math.exp(-0.05 * years)),
            delta=1e-12,
        )
        self.assertAlmostEqual(
            context.model_parameters.risk_free_curve.factor(start, expiry),
            math.exp(-0.05 * years),
            delta=1e-15,
        )
        self.assertEqual(
            engine.price(dated_history(call, market(expiry, 110)), market(expiry, 110)),
            10,
        )
        for at in (time(6, 15, 654323), time(6, 15, 654322)):
            with self.subTest(at=at), self.assertRaises(InvalidInputError):
                engine.price(dated_history(call, market(at)), market(at))
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
        self.assertEqual(schedule.times, (clock(2, 28, 10), clock(3, 31, 10)))
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
            observation_times=[time(6, 10), time(6, 10, 1), time(6, 15)],
        )
        self.assertEqual(
            option.observation_times, (time(6, 10), time(6, 10, 1), time(6, 15))
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
            observation_times=[time(6, 10), time(6, 15)],
            rebate=7,
            rebate_timing="at_hit",
        )
        engine = AnalyticBarrierEngine()
        self.assertEqual(
            engine.price(
                dated_history(option, market(time(6, 10), 110)),
                market(time(6, 10), 110),
            ),
            7,
        )
        with self.assertRaises(InvalidInputError):
            engine.price(
                dated_history(option, market(time(6, 10, 1), 100)),
                market(time(6, 10, 1), 100),
            )
        touched = option.replace_history(
            touch_state="touched",
            history_time=option.expiry_time,
        )
        with self.assertRaises(InvalidInputError):
            engine.price(
                dated_history(touched, market(time(6, 9), 110)), market(time(6, 9), 110)
            )
        self.assertEqual(
            engine.price(
                dated_history(touched, market(time(6, 10, 1), 100)),
                market(time(6, 10, 1), 100),
            ),
            0,
        )

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
            engine.price(
                dated_history(option, market(time(6, 15), 100)),
                market(time(6, 15), 100),
            ),
            120 * 6 / (24 * 365),
            delta=1e-14,
        )
        option = option.replace(effective_time=time(6, 15), expiry_time=time(7, 15))
        self.assertAlmostEqual(
            engine.price(
                dated_history(option, market(time(6, 15, 1), 100)),
                market(time(6, 15, 1), 100),
            ),
            120 / 365,
            delta=1e-14,
        )
        with self.assertRaises(InvalidInputError):
            engine.price(
                dated_history(
                    option.replace_history(
                        realized_hit_count=1,
                        history_time=option.history_time or option.expiry_time,
                    ),
                    market(time(6, 15, 1), 100),
                ),
                market(time(6, 15, 1), 100),
            )

    def test_daily_history_uses_observation_clock_and_inclusive_start(self):
        option = accumulator()
        for engine in accumulator_engines():
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(
                        dated_history(option, market(time(6, 10))), market(time(6, 10))
                    ),
                    10,
                    delta=1e-4,
                )
                with self.assertRaises(InvalidInputError):
                    engine.price(
                        dated_history(option, market(time(6, 15, 1))),
                        market(time(6, 15, 1)),
                    )
                single = option.replace(effective_time=time(7, 15))
                self.assertEqual(
                    engine.price(
                        dated_history(single, market(time(7, 15))), market(time(7, 15))
                    ),
                    5,
                )
        note = SnowballOption(
            calendar=weekdays_calendar(),
            observation_clock_time=clock_time(15),
            maturity_coupon_rate=0.06,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=time(6, 9),
            expiry_time=time(7, 15),
            observations=autocallable_observations(
                [time(7, 15)], [110], knock_out_coupon_rates=[0]
            ),
        )
        engine = QuadratureAutocallableEngine()
        self.assertAlmostEqual(
            engine.price(
                dated_history(note, market(time(6, 10), 100)), market(time(6, 10), 100)
            ),
            1 + 0.06 * 30 / (365 * 24),
            delta=1e-12,
        )
        with self.assertRaises(InvalidInputError):
            engine.price(
                dated_history(note, market(time(6, 15, 1), 100)),
                market(time(6, 15, 1), 100),
            )

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
                        engine.price(
                            dated_history(option, market(time(6, 9))),
                            market(time(6, 9)),
                        ),
                        expected,
                        delta=1e-4,
                    )

    def test_autocallable_explicit_events_do_not_trigger_daily_knock_in(self):
        note = SnowballOption(
            calendar=weekdays_calendar(),
            observation_clock_time=clock_time(15),
            maturity_coupon_rate=0.12,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=time(6, 9),
            expiry_time=time(6, 15),
            observations=autocallable_observations(
                [time(6, 10)], [1000], knock_out_coupon_rates=[0]
            ),
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
                    engine.price(dated_history(note, context), context),
                    1 + 0.12 * 6 / (365 * 24),
                    delta=1e-6,
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
                    engine.price(
                        dated_history(note, market(time(6, 9), 120)),
                        market(time(6, 9), 120),
                    ),
                    1 + 0.12 / (365 * 24),
                    delta=1e-6,
                )

    def test_nontrading_dates_do_not_trigger_daily_knock_out_or_suppress_vega(self):
        option = accumulator(
            effective_time=time(3, 9),
            expiry_time=time(6, 15),
            accumulated_quantity=3,
            knock_out_settled=False,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=110,
            valuation_time=time(4, 15),
        )
        result = QuadratureAccumulatorEngine().price_with_greeks(
            dated_history(option, context), context, "vega"
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
            calendar=weekdays_calendar(),
            observation_clock_time=None,
            coupon_per_period=0.01,
            initial_spot=100,
            knock_in_level=1,
            upper_strike=1,
            lower_strike=1,
            effective_time=start,
            expiry_time=last,
            observations=autocallable_observations(
                [first, last], [1000, 1000], coupon_barrier_levels=[0] * 2
            ),
            knock_in_observation_mode="at_expiry",
        )
        context = market(start, 100)
        with self.assertRaises(ComputationFailureError):
            FiniteDifferenceAutocallableEngine().price(
                dated_history(note, context), context
            )
        self.assertAlmostEqual(
            QuadratureAutocallableEngine().price(dated_history(note, context), context),
            1.02,
            delta=1e-12,
        )
