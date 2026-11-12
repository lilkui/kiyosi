"""FCN/DCN coupon units and staged snowballs through the public Python API."""

import copy
import datetime as datetime_module
import inspect
import math
import pickle
from datetime import UTC, datetime
from datetime import time as clock_time

import pytest
from kiyosi import InvalidInputError
from kiyosi.instruments import (
    AutocallableObservation,
    KnockInObservation,
    PhoenixOption,
    autocallable_observations,
    butterfly_snowball,
    digital_coupon_note,
    early_profit_snowball,
    fixed_coupon_note,
    standard_phoenix,
    step_down_phoenix,
)
from kiyosi.market import weekdays_calendar
from kiyosi.pricing import (
    FiniteDifferenceAutocallableEngine,
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_phoenix_coupon,
)
from support.assertions import assert_decimal_equal
from test_snowball import market

START, MIDDLE, END = datetime(2025, 1, 1), datetime(2025, 2, 3), datetime(2025, 4, 1)


def terms(**changes):
    fields = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "knock_out_level": 110,
        "autocall_times": [MIDDLE, END],
        "effective_time": START,
        "expiry_time": END,
    }
    fields.update(changes)
    return fields


def phoenix_fields(**changes):
    fields = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 80,
        "lower_strike": 0,
        "knock_in_observation": KnockInObservation.at_expiry(),
        "effective_time": START,
        "expiry_time": END,
        "barrier_state": "no_event",
        "autocall_observations": autocallable_observations(
            [MIDDLE, END], [110, 110], coupon_barrier_levels=[0] * 2
        ),
    }
    fields.update(changes)
    return fields


def engines():
    return (
        QuadratureAutocallableEngine(),
        FiniteDifferenceAutocallableEngine(),
        MonteCarloAutocallableEngine(path_count=4096, seed=42),
    )


def test_phoenix_presets_expand_lockout_and_start_ladder_at_first_eligible_observation(
    subtests,
):
    dates = [datetime(2025, 1, 2), MIDDLE, datetime(2025, 3, 3), END]
    common = {
        "coupon_per_period": 0.01,
        "initial_spot": 100,
        "knock_in_level": 80,
        "knock_in_observation_clock_time": clock_time(),
        "coupon_barrier_level": 90,
        "autocall_times": dates,
        "effective_time": START,
        "expiry_time": END,
        "lockout_periods": 2,
    }
    note = step_down_phoenix(
        initial_knock_out_level=110, knock_out_level_decrement=2, **common
    )
    expected = PhoenixOption(
        calendar=weekdays_calendar(),
        coupon_per_period=0.01,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        effective_time=START,
        expiry_time=END,
        lockout_periods=2,
        autocall_observations=autocallable_observations(
            dates, [110, 110, 110, 108], coupon_barrier_levels=[90] * 4
        ),
    )
    assert note == expected
    assert note.knock_in_observation.kind == "every_trading_day"
    assert note.replace(lockout_periods=3).knock_out_levels == note.knock_out_levels
    assert note.replace(lockout_periods=3).knock_out_levels[-1] == 108
    assert standard_phoenix(knock_out_level=110, **common) == step_down_phoenix(
        initial_knock_out_level=110, knock_out_level_decrement=0, **common
    )
    for restored in (
        copy.copy(note),
        copy.deepcopy(note),
        pickle.loads(pickle.dumps(note)),
    ):
        assert restored == note
        assert restored.lockout_periods == 2
    restored = eval(
        repr(note),
        {
            "datetime": datetime_module,
            "PhoenixOption": PhoenixOption,
            "KnockInObservation": KnockInObservation,
            "AutocallableObservation": AutocallableObservation,
            "weekdays_calendar": weekdays_calendar,
        },
    )
    assert restored == note
    for factory, extra in (
        (fixed_coupon_note, {}),
        (digital_coupon_note, {"coupon_barrier_level": 90}),
    ):
        locked = factory(coupon_per_period=0.01, lockout_periods=1, **terms(), **extra)
        assert locked.lockout_periods == 1
        assert locked.upper_strike == 80
        assert locked.knock_in_observation.kind == "at_expiry"
    for factory in (
        fixed_coupon_note,
        digital_coupon_note,
        standard_phoenix,
        step_down_phoenix,
    ):
        assert inspect.signature(factory).parameters["lockout_periods"].default == 0
    for decrement in (-1, math.nan, math.inf, -math.inf, 110):
        with (
            subtests.test(decrement=decrement),
            pytest.raises(InvalidInputError),
        ):
            step_down_phoenix(
                initial_knock_out_level=110,
                knock_out_level_decrement=decrement,
                **common,
            )
    for count, error in (
        (4, InvalidInputError),
        (True, TypeError),
        (1.5, TypeError),
        (-1, OverflowError),
    ):
        with pytest.raises(error):
            step_down_phoenix(
                initial_knock_out_level=110,
                knock_out_level_decrement=2,
                **(common | {"lockout_periods": count}),
            )


@pytest.mark.parametrize(
    "factory,extra,barrier",
    (
        (fixed_coupon_note, {}, 0),
        (digital_coupon_note, {"coupon_barrier_level": 90}, 90),
    ),
)
def test_fcn_dcn_expand_to_plain_phoenix_and_own_their_terms(factory, extra, barrier):
    dates = [MIDDLE, END]
    note = factory(coupon_per_period=0.01, **terms(autocall_times=dates), **extra)
    expected = PhoenixOption(
        calendar=weekdays_calendar(),
        coupon_per_period=0.01,
        **phoenix_fields(
            barrier_state=None,
            autocall_observations=autocallable_observations(
                dates, [110, 110], coupon_barrier_levels=[barrier, barrier]
            ),
        ),
    )
    assert type(note) is PhoenixOption
    assert note == expected
    assert note.coupon_per_period == 0.01
    assert note.upper_strike == 80
    assert note.lower_strike == 0
    assert note.knock_in_observation.kind == "at_expiry"
    assert note.barrier_state is None
    assert note.principal_redemption_ratio == 1
    dates.clear()
    assert note.autocall_times == ((MIDDLE, END))
    assert (
        note.replace(
            barrier_state="no_event",
        ).coupon_per_period
        == 0.01
    )
    assert (
        note.replace(
            barrier_state="no_event",
        ).replace(barrier_state=None)
        == note
    )


def test_phoenix_periodic_coupon_signature_getter_and_repr_roundtrip(subtests):
    signature = inspect.signature(PhoenixOption)
    assert "coupon_rate" not in signature.parameters
    assert signature.parameters["coupon_per_period"].default is inspect.Parameter.empty
    note = PhoenixOption(
        calendar=weekdays_calendar(),
        coupon_per_period=-0.01,
        **phoenix_fields(),
    )
    assert note.coupon_per_period == (-0.01)
    assert "coupon_per_period=-0.01" in repr(note)
    assert (
        eval(
            repr(note),
            {
                "datetime": datetime_module,
                "PhoenixOption": PhoenixOption,
                "KnockInObservation": KnockInObservation,
                "AutocallableObservation": AutocallableObservation,
                "weekdays_calendar": weekdays_calendar,
            },
        )
        == note
    )
    with pytest.raises(AttributeError):
        note.coupon_per_period = 0.02
    with pytest.raises(AttributeError):
        _ = note.coupon_rate
    for fields in (
        {},
        {"coupon_rate": 0.1},
        {"coupon_rate": 0.1, "coupon_per_period": 0.01},
    ):
        with subtests.test(fields=fields), pytest.raises(TypeError):
            PhoenixOption(
                calendar=weekdays_calendar(),
                **fields,
                **phoenix_fields(),
            )
    for operation in (note.replace, note.with_coupon):
        with pytest.raises(TypeError):
            operation(coupon_rate=0.1)
    for value in (True, "0.01", None, Ellipsis):
        with subtests.test(value=value), pytest.raises(TypeError):
            PhoenixOption(
                calendar=weekdays_calendar(),
                coupon_per_period=value,
                **phoenix_fields(),
            )
    for value in (math.nan, math.inf, -math.inf):
        with subtests.test(value=value), pytest.raises(InvalidInputError):
            PhoenixOption(
                calendar=weekdays_calendar(),
                coupon_per_period=value,
                **phoenix_fields(),
            )


def test_shared_fixed_coupon_and_downside_boundary_scenarios_in_all_engines(subtests):
    for factory, extra, barrier in (
        (fixed_coupon_note, {}, 0),
        (digital_coupon_note, {"coupon_barrier_level": 90}, 90),
    ):
        for coupon in (0.01, -0.01):
            note = factory(
                coupon_per_period=coupon,
                **terms(principal_redemption_ratio=0.25, barrier_state="no_event"),
                **extra,
            )
            for engine in engines():
                for spot, base in (
                    (70, 0.15),
                    (80, 0.25),
                    (89.999, 0.25),
                    (90, 0.25),
                    (110, 0.25),
                ):
                    with subtests.test(
                        factory=factory, engine=engine, spot=spot, coupon=coupon
                    ):
                        expected = base + (coupon if spot >= barrier else 0)
                        assert_decimal_equal(
                            engine.price(
                                note,
                                market(spot, END),
                            ),
                            expected,
                            places=14,
                        )
                assert_decimal_equal(
                    engine.price(
                        note,
                        market(110, MIDDLE),
                    ),
                    0.25 + coupon,
                    places=14,
                )
    note = digital_coupon_note(
        coupon_per_period=0.01,
        coupon_barrier_level=90,
        **terms(knock_out_level=85, barrier_state="no_event"),
    )
    for engine in engines():
        assert engine.price(note, market(85, MIDDLE)) == 1


def test_multiple_fixed_payments_do_not_accrue_days_or_repay_past_coupons(subtests):
    for factory, extra, spot, expected in (
        (fixed_coupon_note, {}, 100, 1.02),
        (fixed_coupon_note, {}, 70, 0.92),
        (digital_coupon_note, {"coupon_barrier_level": 90}, 100, 1.02),
        (digital_coupon_note, {"coupon_barrier_level": 90}, 85, 1.0),
    ):
        note = factory(
            coupon_per_period=0.01, **terms(barrier_state="no_event"), **extra
        )
        for engine in engines():
            with subtests.test(factory=factory, spot=spot, engine=engine):
                context = market(spot, START, rate=0, dividend=0, volatility=1e-8)
                assert_decimal_equal(
                    engine.price(note, context),
                    expected,
                    places=9,
                )
                after = market(
                    spot, datetime(2025, 2, 4), rate=0, dividend=0, volatility=1e-8
                )
                remaining = expected - (
                    0.01 if factory is fixed_coupon_note or spot >= 90 else 0
                )
                assert_decimal_equal(engine.price(note, after), remaining, places=9)
                assert (
                    engine.price(
                        note.replace(
                            barrier_state="knocked_out",
                        ),
                        after,
                    )
                    == 0
                )
    fixed = PhoenixOption(
        calendar=weekdays_calendar(),
        coupon_per_period=0.01,
        **phoenix_fields(),
    )
    engine = QuadratureAutocallableEngine()
    for at in (MIDDLE, END):
        assert_decimal_equal(engine.price(fixed, market(110, at)), 1.01)


@pytest.mark.parametrize(
    "factory,extra",
    ((fixed_coupon_note, {}), (digital_coupon_note, {"coupon_barrier_level": 90})),
)
def test_implied_coupon_bounds_tolerance_and_result_use_contract_units(factory, extra):
    note = factory(coupon_per_period=0.01, **terms(), **extra)
    target = factory(coupon_per_period=-0.02, **terms(), **extra)
    for engine in engines():
        context = market()
        quote = engine.price(target, context)
        solved = implied_phoenix_coupon(
            engine,
            note,
            context,
            quote,
            lower_bound=-0.05,
            upper_bound=0.05,
        )
        assert set(solved) == {"coupon_per_period"}
        assert_decimal_equal(solved["coupon_per_period"], -0.02, places=7)
        assert note.coupon_per_period == 0.01


def test_staged_snowballs_use_inclusive_cutoffs_and_whole_holding_coupons():
    dates = [
        MIDDLE,
        datetime(2025, 2, 4),
        datetime(2025, 3, 3),
        datetime(2025, 3, 4),
        END,
    ]
    cases = (
        (early_profit_snowball, {}, [0.2, 0.05, 0.05, 0.05, 0.05]),
        (
            butterfly_snowball,
            {
                "third_coupon_rate": -0.02,
                "second_stage_end_time": datetime(2025, 3, 3),
            },
            [0.2, 0.05, 0.05, -0.02, -0.02],
        ),
    )
    for factory, extra, rates in cases:
        note = factory(
            knock_in_observation_clock_time=clock_time(),
            first_coupon_rate=0.2,
            second_coupon_rate=0.05,
            first_stage_end_time=MIDDLE,
            **terms(autocall_times=dates, barrier_state="no_event"),
            **extra,
        )
        assert note.knock_out_coupon_rates == tuple(rates)
        assert note.maturity_coupon_rate == rates[-1]
        for day, rate in zip(dates, rates, strict=True):
            expected = 1 + rate * (day - START).days / 365
            for engine in engines():
                assert_decimal_equal(
                    engine.price(note, market(110, day)),
                    expected,
                    places=14,
                )
        assert_decimal_equal(
            QuadratureAutocallableEngine().price(note, market(100, END)),
            1 + rates[-1] * 90 / 365,
        )


def test_stage_validation_checks_unused_rates_and_accepts_empty_stages(subtests):
    fields = {
        "first_coupon_rate": 0.2,
        "second_coupon_rate": -0.02,
        "first_stage_end_time": datetime(2025, 2, 1),
    }
    note = early_profit_snowball(
        knock_in_observation_clock_time=clock_time(),
        **fields,
        **terms(autocall_times=[END]),
    )
    assert note.knock_out_coupon_rates == ((-0.02,))
    increasing = butterfly_snowball(
        knock_in_observation_clock_time=clock_time(),
        first_coupon_rate=-0.02,
        second_coupon_rate=0,
        third_coupon_rate=0.2,
        first_stage_end_time=datetime(2025, 2, 1),
        second_stage_end_time=datetime(2025, 3, 1),
        **terms(),
    )
    assert increasing.knock_out_coupon_rates == ((0, 0.2))
    assert increasing.maturity_coupon_rate == 0.2
    for cutoff in (datetime(2024, 12, 31), END, datetime(2025, 4, 2)):
        with pytest.raises(InvalidInputError):
            early_profit_snowball(
                knock_in_observation_clock_time=clock_time(),
                **(fields | {"first_stage_end_time": cutoff}),
                **terms(),
            )
    for cutoff in (datetime(2025, 1, 31), datetime(2025, 2, 1), END):
        with pytest.raises(InvalidInputError):
            butterfly_snowball(
                knock_in_observation_clock_time=clock_time(),
                **fields,
                third_coupon_rate=0.01,
                second_stage_end_time=cutoff,
                **terms(),
            )
    for key in ("first_coupon_rate", "second_coupon_rate"):
        for value in (math.nan, math.inf, -math.inf):
            with (
                subtests.test(key=key, value=value),
                pytest.raises(InvalidInputError),
            ):
                early_profit_snowball(
                    knock_in_observation_clock_time=clock_time(),
                    **(fields | {key: value}),
                    **terms(autocall_times=[END]),
                )
    for value in (math.nan, math.inf, -math.inf):
        with pytest.raises(InvalidInputError):
            butterfly_snowball(
                knock_in_observation_clock_time=clock_time(),
                **fields,
                third_coupon_rate=value,
                second_stage_end_time=datetime(2025, 3, 1),
                **terms(),
            )
    for value in (True, "2025-02-01", datetime(2025, 2, 1, tzinfo=UTC)):
        with pytest.raises(TypeError):
            early_profit_snowball(
                knock_in_observation_clock_time=clock_time(),
                **(fields | {"first_stage_end_time": value}),
                **terms(),
            )


def test_preset_representation_and_domain_errors_use_native_categories():
    for value in (True, "0.01", None):
        with pytest.raises(TypeError):
            fixed_coupon_note(coupon_per_period=value, **terms())
    for value in (math.nan, math.inf, -math.inf):
        with pytest.raises(InvalidInputError):
            fixed_coupon_note(coupon_per_period=value, **terms())
    for barrier in (-1, math.nan, math.inf):
        with pytest.raises(InvalidInputError):
            digital_coupon_note(
                coupon_per_period=0.01, coupon_barrier_level=barrier, **terms()
            )
    for schedule in ([], [END, MIDDLE], [MIDDLE, MIDDLE], [datetime(2025, 4, 2)]):
        with pytest.raises(InvalidInputError):
            fixed_coupon_note(coupon_per_period=0.01, **terms(autocall_times=schedule))
    with pytest.raises(TypeError):
        digital_coupon_note(coupon_per_period=0.01, **terms())
    for factory, parameter in (
        (fixed_coupon_note, "coupon_per_period"),
        (digital_coupon_note, "coupon_barrier_level"),
        (early_profit_snowball, "first_stage_end_time"),
        (butterfly_snowball, "second_stage_end_time"),
    ):
        assert (
            inspect.signature(factory).parameters[parameter].kind
            == inspect.Parameter.KEYWORD_ONLY
        )
