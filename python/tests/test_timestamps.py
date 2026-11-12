"""Intraday contracts use the same timestamp and event semantics as Rust."""

import inspect
import math
import pickle
from datetime import UTC, date, datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import ComputationFailureError, InvalidInputError
from kiyosi.instruments import (
    Accumulator,
    BarrierOption,
    BinarySnowballOption,
    EuropeanOption,
    KnockInObservation,
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


def test_closed_date_settlement_preserves_open_date_observations():
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
            notional=1000,
            annual_coupon_rate=0.12,
            lower_bound=80,
            upper_bound=120,
            observation_clock_time=clock_time(15),
            realized_hit_count=1,
            **life,
        )
        accumulated = Accumulator(
            knock_out_settled=False,
            strike=90,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
            observation_clock_time=clock_time(15),
            **life,
        )
        fields = dict(
            maturity_coupon_rate=0.12,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            knock_in_observation=KnockInObservation.every_trading_day(clock_time(15)),
            observations=autocallable_observations(
                [observation], [110], knock_out_coupon_rates=[0]
            ),
            barrier_state="no_event",
            **life,
        )
        note = SnowballOption(**fields)
        binary = BinarySnowballOption(
            maturity_coupon_rate=0.12,
            observations=fields["observations"],
            barrier_state="no_event",
            **life,
        )
        for schedule in (
            range_option.observation_schedule,
            accumulated.accumulation_schedule,
            accumulated.knock_out_observation_schedule,
            note.knock_in_observation_schedule,
        ):
            assert list(schedule) == [observation]
        with pytest.raises(InvalidInputError):
            SnowballOption(
                **(
                    fields
                    | {
                        "knock_in_observation": KnockInObservation.at_expiry(),
                    }
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
                assert AnalyticRangeAccrualEngine().price(
                    range_option, context
                ) == pytest.approx(1000 * coupon * discount, rel=0, abs=1e-12)
                for option in (note, binary):
                    assert pickle.loads(pickle.dumps(option)) == option
                    for engine in (
                        QuadratureAutocallableEngine(),
                        FiniteDifferenceAutocallableEngine(),
                        MonteCarloAutocallableEngine(),
                    ):
                        assert engine.price(option, context) == pytest.approx(
                            (1 + coupon) * discount, rel=0, abs=1e-12
                        )
                if at == expiry:
                    for engine in accumulator_engines():
                        assert engine.price(accumulated, context) == (-60)


def test_contract_schedules_expose_effective_observations(subtests):
    calendar = custom_calendar(
        base=weekdays_calendar(), closed_dates=[date(2025, 1, 7)]
    )
    clock = clock_time(15, microsecond=123456)
    start = datetime(2025, 1, 6, 15, microsecond=123456)
    last = datetime(2025, 1, 8, 15, microsecond=123456)
    expiry = datetime(2025, 1, 8, 17)
    life = {"effective_time": start, "expiry_time": expiry, "calendar": calendar}
    accrual = RangeAccrualOption(
        notional=1000,
        annual_coupon_rate=0.12,
        lower_bound=90,
        upper_bound=110,
        observation_clock_time=clock,
        **life,
    )
    schedule = accrual.observation_schedule
    del accrual
    assert list(schedule) == [last]
    accumulator = Accumulator(
        strike=90,
        knock_out_level=120,
        daily_quantity=1,
        acceleration_factor=2,
        observation_clock_time=clock,
        **life,
    )
    assert list(accumulator.accumulation_schedule) == [start, last]
    assert (
        accumulator.knock_out_observation_schedule == accumulator.accumulation_schedule
    )
    for explicit in ([], [datetime(2025, 1, 6, 16)]):
        changed = accumulator.replace(knock_out_observation_times=explicit)
        assert list(changed.knock_out_observation_schedule) == explicit
        assert list(changed.accumulation_schedule) == [start, last]
    observations = autocallable_observations(
        [expiry], [110], knock_out_coupon_rates=[0]
    )
    terms = dict(
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation=KnockInObservation.every_trading_day(clock),
        observations=observations,
        **life,
    )
    notes = [
        SnowballOption(maturity_coupon_rate=0.1, **terms),
        PhoenixOption(
            coupon_per_period=0.1,
            **dict(
                terms,
                observations=autocallable_observations(
                    [expiry], [110], coupon_barrier_levels=[0]
                ),
            ),
        ),
        TernarySnowballOption(
            maturity_coupon_rate=0.1,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            knock_in_observation=KnockInObservation.every_trading_day(clock),
            observations=observations,
            **life,
        ),
    ]
    for note in notes:
        with subtests.test(note=type(note).__name__):
            assert list(note.knock_in_observation_schedule) == [start, last]
            expiry_only = note.replace(
                knock_in_observation=KnockInObservation.at_expiry(),
            )
            assert list(expiry_only.knock_in_observation_schedule) == [expiry]
            exported = list(note.knock_in_observation_schedule.times)
            exported.clear()
            assert list(note.knock_in_observation_schedule) == [start, last]
    binary = BinarySnowballOption(
        maturity_coupon_rate=0.1, observations=observations, **life
    )
    assert binary.knock_in_observation_schedule is None


def test_subday_time_bumps_converge_to_intraday_theta(subtests):
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
    assert errors[1] < errors[0]
    assert errors[1] < abs(exact) * 1e-7
    for shift in (timedelta(), timedelta(microseconds=-1)):
        with subtests.test(shift=shift), pytest.raises(InvalidInputError):
            numerical_greeks(
                engine,
                option,
                context,
                "theta",
                time_shift=shift,
            )
    for shift in (True, 1, 0.001, NanosecondDuration(microseconds=1)):
        with subtests.test(shift=shift), pytest.raises(TypeError):
            numerical_greeks(
                engine,
                option,
                context,
                "theta",
                time_shift=shift,
            )


def test_time_inputs_reject_dates_timezones_and_lost_precision(subtests):
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
            with subtests.test(value=value), pytest.raises(TypeError):
                operation()
    assert weekdays_calendar().is_trading_day(date(2025, 1, 6))
    with pytest.raises(TypeError):
        weekdays_calendar().is_trading_day(time(6, 9))


def test_intraday_expiry_and_accrual_preserve_microseconds(subtests):
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
    assert engine.price(call, context) - engine.price(put, context) == pytest.approx(
        100 * (math.exp(-0.02 * years) - math.exp(-0.05 * years)), rel=0, abs=1e-12
    )
    assert context.model_parameters.risk_free_curve.factor(
        start, expiry
    ) == pytest.approx(math.exp(-0.05 * years), rel=0, abs=1e-15)
    assert engine.price(call, market(expiry, 110)) == 10
    for at in (time(6, 15, 654323), time(6, 15, 654322)):
        with subtests.test(at=at), pytest.raises(InvalidInputError):
            engine.price(call, market(at))
    assert pickle.loads(pickle.dumps(call)) == call
    assert call.expiry_time.tzinfo is None
    assert call.expiry_time == expiry
    assert "effective_time" in inspect.signature(EuropeanOption).parameters
    with pytest.raises(TypeError):
        call.replace(expiry_date=expiry)
    with pytest.raises(InvalidInputError):
        call.replace(expiry_time=time(6, 9))
    assert call.expiry_time == expiry


def test_observation_schedules_preserve_clock_and_full_timestamp_order(subtests):
    def clock(month, day, hour):
        return datetime(2025, month, day, hour, 11, 12, 123456)

    schedule = monthly_schedule(
        observation_clock_time=clock_time(10, 11, 12, 123456),
        start_time=clock(1, 31, 10),
        end_time=clock(3, 31, 15),
        first_observation_month=1,
    )
    assert schedule.times == ((clock(2, 28, 10), clock(3, 31, 10)))
    assert pickle.loads(pickle.dumps(schedule)) == schedule
    fixed = fixed_interval_schedule(
        observation_clock_time=clock_time(10, 11, 12, 123456),
        start_time=clock(1, 3, 10),
        end_time=clock(1, 7, 9),
        interval_days=1,
    )
    assert list(fixed) == [clock(1, 6, 10)]
    option = BarrierOption(
        option_type="call",
        strike=100,
        effective_time=time(6, 9),
        expiry_time=time(6, 16),
        barrier_level=110,
        barrier_type="up_and_out",
        observation_times=[time(6, 10), time(6, 10, 1), time(6, 15)],
    )
    assert option.observation_times == ((time(6, 10), time(6, 10, 1), time(6, 15)))
    for invalid in ([time(6, 10)] * 2, [time(6, 15), time(6, 10)]):
        with subtests.test(invalid=invalid), pytest.raises(InvalidInputError):
            option.replace(observation_times=invalid)


def test_scheduled_barriers_use_exact_current_observations_and_strict_history():
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
    assert (
        engine.price(
            option,
            market(time(6, 10), 110),
        )
        == 7
    )
    with pytest.raises(InvalidInputError):
        engine.price(
            option,
            market(time(6, 10, 1), 100),
        )
    touched = option.replace(
        touch_state="touched",
    )
    with pytest.raises(InvalidInputError):
        engine.price(touched, market(time(6, 9), 110))
    assert (
        engine.price(
            touched,
            market(time(6, 10, 1), 100),
        )
        == 0
    )


def test_range_accrual_observes_same_day_and_excludes_only_the_effective_instant():
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
    assert engine.price(
        option,
        market(time(6, 15), 100),
    ) == pytest.approx(120 * 6 / (24 * 365), rel=0, abs=1e-14)
    option = option.replace(effective_time=time(6, 15), expiry_time=time(7, 15))
    assert engine.price(
        option,
        market(time(6, 15, 1), 100),
    ) == pytest.approx(120 / 365, rel=0, abs=1e-14)
    with pytest.raises(InvalidInputError):
        engine.price(
            option.replace(
                realized_hit_count=1,
            ),
            market(time(6, 15, 1), 100),
        )


def test_daily_history_uses_observation_clock_and_inclusive_start(subtests):
    option = accumulator()
    for engine in accumulator_engines():
        with subtests.test(engine=type(engine).__name__):
            assert engine.price(option, market(time(6, 10))) == pytest.approx(
                10, rel=0, abs=1e-4
            )
            with pytest.raises(InvalidInputError):
                engine.price(
                    option,
                    market(time(6, 15, 1)),
                )
            single = option.replace(effective_time=time(7, 15))
            assert engine.price(single, market(time(7, 15))) == 5
    note = SnowballOption(
        calendar=weekdays_calendar(),
        knock_in_observation=KnockInObservation.every_trading_day(clock_time(15)),
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
    assert engine.price(note, market(time(6, 10), 100)) == pytest.approx(
        1 + 0.06 * 30 / (365 * 24), rel=0, abs=1e-12
    )
    with pytest.raises(InvalidInputError):
        engine.price(
            note,
            market(time(6, 15, 1), 100),
        )


@pytest.mark.parametrize(
    "hour,barrier,expected", ((10, 110, 10), (10, 100, 0), (15, 100, 0), (16, 100, 5))
)
def test_explicit_accumulator_knock_outs_do_not_add_daily_quantity(
    subtests, hour, barrier, expected
):
    option = accumulator(
        knock_out_level=barrier, knock_out_observation_times=[time(6, hour)]
    )
    for engine in accumulator_engines():
        with subtests.test(hour=hour, barrier=barrier, engine=type(engine).__name__):
            assert engine.price(
                option,
                market(time(6, 9)),
            ) == pytest.approx(expected, rel=0, abs=1e-4)


def test_autocallable_explicit_events_do_not_trigger_daily_knock_in(subtests):
    note = SnowballOption(
        calendar=weekdays_calendar(),
        knock_in_observation=KnockInObservation.every_trading_day(clock_time(15)),
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
        with subtests.test(engine=type(engine).__name__):
            assert engine.price(note, context) == pytest.approx(
                1 + 0.12 * 6 / (365 * 24), rel=0, abs=1e-6
            )
    note = note.replace(
        maturity_coupon_rate=0,
        observations=autocallable_observations(
            [time(6, 10)], [110], knock_out_coupon_rates=[0.12]
        ),
    )
    for engine in engines:
        with subtests.test(engine=type(engine).__name__):
            assert engine.price(
                note,
                market(time(6, 9), 120),
            ) == pytest.approx(1 + 0.12 / (365 * 24), rel=0, abs=1e-6)


def test_nontrading_dates_do_not_trigger_daily_knock_out_or_suppress_vega():
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
    result = QuadratureAccumulatorEngine().price_with_greeks(option, context, "vega")
    root_time = math.sqrt(2 / 365)
    expected = (
        -105
        * root_time
        * math.exp(-0.5 * (0.1 * root_time) ** 2)
        / math.sqrt(2 * math.pi)
    )
    assert result.vega == pytest.approx(expected, rel=0, abs=0.0001)


def test_finite_difference_rejects_unresolvable_microsecond_events():
    start = datetime(1, 1, 1)
    first = datetime(9998, 1, 1, 10, microsecond=1)
    last = first.replace(microsecond=2)
    note = PhoenixOption(
        calendar=weekdays_calendar(),
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
        knock_in_observation=KnockInObservation.at_expiry(),
    )
    context = market(start, 100)
    with pytest.raises(ComputationFailureError):
        FiniteDifferenceAutocallableEngine().price(note, context)
    assert QuadratureAutocallableEngine().price(note, context) == pytest.approx(
        1.02, rel=0, abs=1e-12
    )
