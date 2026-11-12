"""Observation clocks are explicit and independent of maturity settlement."""

import datetime as datetime_module
import math
import pickle
from datetime import UTC, date, datetime, time

import pytest
from kiyosi import InvalidInputError, UnsupportedOperationError
from kiyosi.instruments import (
    Accumulator,
    AutocallableObservation,
    KnockInObservation,
    RangeAccrualOption,
    SnowballOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    fixed_interval_schedule,
    monthly_schedule,
    weekdays_calendar,
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
from support.assertions import assert_decimal_equal

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
        "autocall_observations": [
            AutocallableObservation.snowball(END, 1000, coupon_rate=0)
        ],
    }


def context(at, spot=100, rate=0):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=0, volatility=0
        ),
        spot_price=spot,
        valuation_time=at,
    )


def test_clock_requires_naive_time_and_preserves_microseconds(subtests):
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
        with subtests.test(value=value), pytest.raises(TypeError):
            RangeAccrualOption(observation_clock_time=value, **range_terms())
    with pytest.raises(TypeError):
        RangeAccrualOption(**range_terms())
    note = RangeAccrualOption(observation_clock_time=CLOCK, **range_terms())
    assert note.observation_clock_time == CLOCK
    assert pickle.loads(pickle.dumps(note)) == note
    replacement = note.replace(observation_clock_time=time(14, 30, 1, 999999))
    assert replacement.observation_clock_time == time(14, 30, 1, 999999)
    assert note.observation_clock_time == CLOCK


def test_generated_sequences_use_explicit_clock_and_complete_upper_bound():
    schedule = monthly_schedule(
        start_time=datetime(2025, 1, 31, 9),
        end_time=datetime(2025, 3, 31, 14),
        first_observation_month=1,
        observation_clock_time=CLOCK,
    )
    assert list(schedule) == [datetime(2025, 2, 28, 15, 0, 0, 123456)]
    empty = fixed_interval_schedule(
        start_time=datetime(2025, 1, 3, 16),
        end_time=datetime(2025, 1, 6, 14),
        interval_days=1,
        observation_clock_time=CLOCK,
    )
    assert list(empty) == []


def test_range_accrual_rejects_empty_schedule_atomically():
    with pytest.raises(InvalidInputError):
        RangeAccrualOption(
            observation_clock_time=time(15),
            **(range_terms() | {"effective_time": datetime(2025, 1, 6, 16)}),
        )
    note = RangeAccrualOption(observation_clock_time=CLOCK, **range_terms())
    with pytest.raises(InvalidInputError):
        note.replace(observation_clock_time=time(8))
    assert note.observation_clock_time == CLOCK


@pytest.mark.parametrize("hour,hits", ((15, None), (16, 1), (18, 1)))
def test_completed_range_observations_discount_coupon_until_maturity(hour, hits):
    engine = AnalyticRangeAccrualEngine()
    note = RangeAccrualOption(
        observation_clock_time=time(15), realized_hit_count=hits, **range_terms()
    )
    market = context(datetime(2025, 1, 6, hour), rate=0.5).replace(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.5, dividend_yield=0, volatility=0.2
        )
    )
    expected = 120 * 9 / (365 * 24) * math.exp(-0.5 * (18 - hour) / (365 * 24))
    actual = engine.price(note, market)
    assert actual == pytest.approx(expected, rel=0, abs=1e-14)
    with pytest.raises(UnsupportedOperationError):
        implied_volatility(
            engine,
            note,
            market,
            actual,
            lower_bound=0.0,
            upper_bound=4.0,
        )


def test_accumulator_allows_no_daily_events_and_does_not_add_units_at_expiry():
    terms = {
        "strike": 100,
        "knock_out_level": 110,
        "daily_quantity": 1,
        "acceleration_factor": 1,
        "effective_time": datetime(2025, 1, 6, 16),
        "expiry_time": END,
        "observation_clock_time": time(15),
    }
    empty = Accumulator(**terms)
    engine = QuadratureAccumulatorEngine()
    assert (
        engine.price(
            empty,
            context(terms["effective_time"], 105),
        )
        == 0
    )
    observed = empty.replace(
        effective_time=START,
        accumulated_quantity=3,
        knock_out_settled=False,
    )
    assert engine.price(observed, context(END, 105)) == 15
    assert pickle.loads(pickle.dumps(observed)) == observed


def test_daily_knock_in_does_not_observe_off_clock_maturity(subtests):
    note = SnowballOption(
        calendar=weekdays_calendar(),
        knock_in_observation=KnockInObservation.every_trading_day(time(15)),
        barrier_state="no_event",
        **snowball_terms(),
    )
    expected = 1 + 0.12 * 9 / (365 * 24)
    for engine in (
        QuadratureAutocallableEngine(),
        FiniteDifferenceAutocallableEngine(),
        MonteCarloAutocallableEngine(path_count=32, seed=42),
    ):
        with subtests.test(engine=type(engine).__name__):
            assert_decimal_equal(
                engine.price(note, context(END, 70)),
                expected,
            )
    empty = note.replace(effective_time=datetime(2025, 1, 6, 16), barrier_state=None)
    assert_decimal_equal(
        QuadratureAutocallableEngine().price(empty, context(END, 70)),
        1 + 0.12 * 2 / (365 * 24),
    )


def test_current_daily_observation_uses_the_exact_microsecond():
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
    assert engine.price(option, context(observed, 105)) == 5
    with pytest.raises(InvalidInputError):
        engine.price(
            option,
            context(observed.replace(microsecond=123457), 105),
        )


def test_future_accumulation_occurs_once_before_off_clock_maturity(subtests):
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
        with subtests.test(engine=type(engine).__name__):
            assert engine.price(option, market) == pytest.approx(5, rel=0, abs=1e-4)


def test_future_pricing_after_last_observation_does_not_add_maturity_knock_in(subtests):
    note = SnowballOption(
        calendar=weekdays_calendar(),
        knock_in_observation=KnockInObservation.every_trading_day(time(15)),
        barrier_state="no_event",
        **snowball_terms(),
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
        with subtests.test(engine=type(engine).__name__):
            assert engine.price(note, market) == pytest.approx(
                expected, rel=0, abs=1e-6
            )


def test_knock_in_observation_replacement_switches_rule_and_clock_atomically():
    expiry = SnowballOption(
        calendar=weekdays_calendar(),
        knock_in_observation=KnockInObservation.at_expiry(),
        **snowball_terms(),
    )
    daily_rule = KnockInObservation.every_trading_day(CLOCK)
    daily = expiry.replace(knock_in_observation=daily_rule)
    assert daily.knock_in_observation == daily_rule
    assert daily.knock_in_observation.observation_clock_time == CLOCK
    assert expiry.knock_in_observation == KnockInObservation.at_expiry()
    assert daily.replace(knock_in_observation=KnockInObservation.at_expiry()) == expiry
    changed_clock = time(14, 30, 1, 999999)
    changed = daily.replace(
        knock_in_observation=KnockInObservation.every_trading_day(changed_clock)
    )
    assert changed.knock_in_observation.observation_clock_time == changed_clock
    assert daily.knock_in_observation == daily_rule
    assert pickle.loads(pickle.dumps(daily)) == daily
    assert daily.replace(knock_in_observation=Ellipsis) == daily
    for invalid in (None, "every_trading_day", CLOCK):
        with pytest.raises(TypeError):
            daily.replace(knock_in_observation=invalid)
    assert daily.knock_in_observation == daily_rule


@pytest.mark.parametrize(
    "observation,kind,clock",
    [
        (KnockInObservation.at_expiry(), "at_expiry", None),
        (KnockInObservation.every_trading_day(CLOCK), "every_trading_day", CLOCK),
    ],
)
def test_knock_in_observation_owned_values_roundtrip(observation, kind, clock):
    assert observation.kind == kind
    assert observation.observation_clock_time == clock
    assert pickle.loads(pickle.dumps(observation)) == observation
    assert (
        eval(
            repr(observation),
            {
                "KnockInObservation": KnockInObservation,
                "datetime": datetime_module,
            },
        )
        == observation
    )
    with pytest.raises(AttributeError):
        observation.observation_clock_time = CLOCK


@pytest.mark.parametrize(
    "clock", [None, True, 15, "15:00", datetime(2025, 1, 6, 15), time(15, tzinfo=UTC)]
)
def test_knock_in_observation_requires_timezone_free_clock(clock):
    with pytest.raises(TypeError):
        KnockInObservation.every_trading_day(clock)


def test_completed_daily_monitoring_has_no_implied_volatility_or_expiry_greeks_jump(
    subtests,
):
    daily = SnowballOption(
        calendar=weekdays_calendar(),
        knock_in_observation=KnockInObservation.every_trading_day(time(15)),
        barrier_state="no_event",
        **(
            snowball_terms()
            | {
                "autocall_observations": [
                    AutocallableObservation.snowball(
                        datetime(2025, 1, 6, 10), 1000, coupon_rate=0
                    )
                ]
            }
        ),
    )
    expiry = daily.replace(
        knock_in_observation=KnockInObservation.at_expiry(),
        barrier_state=daily.barrier_state,
    )
    expected = 1 + 0.12 * 9 / (365 * 24)
    for engine in (
        MonteCarloAutocallableEngine(path_count=128, seed=42),
        FiniteDifferenceAutocallableEngine(),
        QuadratureAutocallableEngine(),
    ):
        for hour in (16, 18):
            for volatility in (0, 0.0001):
                with subtests.test(
                    engine=type(engine).__name__, hour=hour, volatility=volatility
                ):
                    market = context(datetime(2025, 1, 6, hour), 70).replace(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0,
                            dividend_yield=0,
                            volatility=volatility,
                        )
                    )
                    actual = engine.price(daily, market)
                    assert actual == pytest.approx(expected, rel=0, abs=1e-8)
                    with pytest.raises(UnsupportedOperationError):
                        implied_volatility(
                            engine,
                            daily,
                            market,
                            actual,
                            lower_bound=0.0,
                            upper_bound=4.0,
                        )
                    assert engine.price(expiry, market) == pytest.approx(
                        0.7, rel=0, abs=1e-5
                    )
        result = engine.price_with_greeks(
            daily,
            context(END, 80),
            ["delta", "gamma"],
        )
        assert result.delta == 0
        assert result.gamma == 0
    assert (
        QuadratureAutocallableEngine()
        .price_with_greeks(expiry, context(END, 80), "delta")
        .delta
        is None
    )
