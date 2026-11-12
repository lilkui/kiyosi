"""Validated immutable edits of every exotic contract family."""

import inspect
from datetime import datetime
from datetime import time as clock_time

import pytest
from kiyosi import InvalidInputError
from kiyosi import instruments as i
from kiyosi.market import all_days_calendar, weekdays_calendar

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


def contract_inputs():
    yield (
        i.BarrierOption,
        {
            "option_type": "call",
            "strike": 100,
            "effective_time": START,
            "expiry_time": END,
            "barrier_level": 120,
            "barrier_type": "up_and_out",
            "rebate": 0,
            "rebate_timing": "at_expiry",
            "observation_times": None,
            "touch_state": "untouched",
        },
    )
    yield (
        i.DoubleBarrierOption,
        {
            "option_type": "call",
            "strike": 100,
            "effective_time": START,
            "expiry_time": END,
            "lower_barrier": 80,
            "upper_barrier": 120,
            "barrier_type": "knock_out",
            "rebate": 0,
            "rebate_timing": "at_expiry",
            "observation_times": None,
            "touch_state": "untouched",
        },
    )
    yield (
        i.cash_binary_barrier_option,
        {
            "option_type": "call",
            "strike": 100,
            "effective_time": START,
            "expiry_time": END,
            "barrier_level": 120,
            "barrier_type": "up_and_out",
            "observation_times": None,
            "touch_state": "untouched",
            "payout": 10,
        },
    )
    yield (
        i.asset_binary_barrier_option,
        {
            "option_type": "call",
            "strike": 100,
            "effective_time": START,
            "expiry_time": END,
            "barrier_level": 120,
            "barrier_type": "up_and_out",
            "observation_times": None,
            "touch_state": "untouched",
        },
    )
    yield (
        i.cash_touch_option,
        {
            "effective_time": START,
            "expiry_time": END,
            "barrier_level": 120,
            "barrier_type": "up_and_in",
            "settlement_timing": "at_expiry",
            "observation_times": None,
            "touch_state": "untouched",
            "payout": 10,
        },
    )
    yield (
        i.asset_touch_option,
        {
            "effective_time": START,
            "expiry_time": END,
            "barrier_level": 120,
            "barrier_type": "up_and_in",
            "settlement_timing": "at_expiry",
            "observation_times": None,
            "touch_state": "untouched",
        },
    )
    yield (
        i.cash_double_touch,
        {
            "effective_time": START,
            "expiry_time": END,
            "lower_barrier": 80,
            "upper_barrier": 120,
            "settlement_timing": "at_expiry",
            "observation_times": None,
            "touch_state": "untouched",
            "payout": 10,
        },
    )
    yield (
        i.Accumulator,
        {
            "knock_out_settled": False,
            "observation_clock_time": clock_time(),
            "option_type": "call",
            "strike": 100,
            "knock_out_level": 120,
            "daily_quantity": 1,
            "acceleration_factor": 2,
            "accumulated_quantity": 3,
            "effective_time": START,
            "expiry_time": END,
            "knock_out_observation_times": [END],
        },
    )
    yield (
        i.RangeAccrualOption,
        {
            "observation_clock_time": clock_time(),
            "notional": 1000,
            "annual_coupon_rate": 0.12,
            "lower_bound": 90,
            "upper_bound": 110,
            "effective_time": START,
            "expiry_time": END,
            "realized_hit_count": 1,
        },
    )
    yield (
        i.SnowballOption,
        {
            "calendar": weekdays_calendar(),
            "observation_clock_time": clock_time(),
            "maturity_coupon_rate": 0.1,
            "initial_spot": 100,
            "knock_in_level": 80,
            "upper_strike": 100,
            "lower_strike": 0,
            "knock_in_observation_mode": "every_trading_day",
            "barrier_state": "no_event",
            "principal_redemption_ratio": 1,
            "effective_time": START,
            "expiry_time": END,
            "observations": [
                i.AutocallableObservation.snowball(END, 110, coupon_rate=0)
            ],
        },
    )
    yield (
        i.PhoenixOption,
        {
            "calendar": weekdays_calendar(),
            "observation_clock_time": clock_time(),
            "coupon_per_period": 0.1,
            "initial_spot": 100,
            "knock_in_level": 80,
            "upper_strike": 100,
            "lower_strike": 0,
            "knock_in_observation_mode": "every_trading_day",
            "barrier_state": "no_event",
            "principal_redemption_ratio": 1,
            "effective_time": START,
            "expiry_time": END,
            "observations": [
                i.AutocallableObservation.phoenix(
                    datetime(2025, 12, 31), 110, coupon_barrier_level=0
                ),
                i.AutocallableObservation.phoenix(END, 110, coupon_barrier_level=0),
            ],
        },
    )
    yield (
        i.BinarySnowballOption,
        {
            "calendar": weekdays_calendar(),
            "maturity_coupon_rate": 0.1,
            "barrier_state": "no_event",
            "principal_redemption_ratio": 1,
            "effective_time": START,
            "expiry_time": END,
            "observations": [
                i.AutocallableObservation.snowball(END, 110, coupon_rate=0)
            ],
        },
    )
    yield (
        i.TernarySnowballOption,
        {
            "calendar": weekdays_calendar(),
            "observation_clock_time": clock_time(),
            "maturity_coupon_rate": 0.1,
            "minimum_coupon_rate": 0.02,
            "knock_in_level": 80,
            "knock_in_observation_mode": "every_trading_day",
            "barrier_state": "no_event",
            "principal_redemption_ratio": 1,
            "effective_time": START,
            "expiry_time": END,
            "observations": [
                i.AutocallableObservation.snowball(END, 110, coupon_rate=0)
            ],
        },
    )
    yield (
        i.GeometricAveragePriceOption,
        {
            "option_type": "call",
            "strike": 100,
            "effective_time": START,
            "averaging_start_time": START,
            "expiry_time": END,
            "realized_average": None,
        },
    )
    yield (
        i.ArithmeticAveragePriceOption,
        {
            "option_type": "call",
            "strike": 100,
            "effective_time": START,
            "averaging_start_time": START,
            "expiry_time": END,
            "realized_average": None,
        },
    )


def contracts():
    for factory, fields in contract_inputs():
        yield factory(**fields)


UPDATES = {
    "calendar": all_days_calendar(),
    "option_type": "put",
    "strike": 105,
    "effective_time": datetime(2025, 1, 2),
    "expiry_time": datetime(2026, 1, 2),
    "averaging_start_time": datetime(2025, 1, 2),
    "barrier_level": 125,
    "lower_barrier": 85,
    "upper_barrier": 125,
    "rebate": 5,
    "rebate_timing": "at_hit",
    "settlement_timing": "at_hit",
    "touch_state": "touched",
    "payout": 15,
    "knock_out_level": 125,
    "daily_quantity": 2,
    "acceleration_factor": 3,
    "accumulated_quantity": 4,
    "knock_out_settled": None,
    "knock_out_observation_times": [START],
    "notional": 2000,
    "annual_coupon_rate": 0.15,
    "lower_bound": 85,
    "upper_bound": 115,
    "realized_hit_count": 2,
    "initial_spot": 110,
    "knock_in_level": 75,
    "upper_strike": 105,
    "lower_strike": 5,
    "knock_in_observation_mode": "at_expiry",
    "observation_clock_time": clock_time(1, 2, 3, 456789),
    "barrier_state": None,
    "principal_redemption_ratio": 0.9,
    "observations": [i.AutocallableObservation.snowball(END, 115, coupon_rate=0)],
    "maturity_coupon_rate": 0.15,
    "minimum_coupon_rate": 0.03,
    "coupon_rate": 0.15,
    "coupon_per_period": 0.02,
    "lockout_periods": 1,
    "realized_average": 105,
}
NULLABLE = {
    "observation_times",
    "touch_state",
    "barrier_state",
    "realized_hit_count",
    "accumulated_quantity",
    "knock_out_settled",
    "knock_out_observation_times",
}


def test_monitoring_schedules_round_trip_for_continuous_and_scheduled_contracts(
    subtests,
):
    times = (datetime(2025, 1, 4), END)
    for factory, fields in contract_inputs():
        if "touch_state" not in fields:
            continue
        with subtests.test(contract=factory.__name__):
            continuous = factory(
                **(fields | {"touch_state": None, "calendar": all_days_calendar()})
            )
            assert continuous.observation_schedule is None
            assert (
                continuous.replace(observation_times=continuous.observation_schedule)
                == continuous
            )
            scheduled = continuous.replace(observation_times=times)
            schedule = scheduled.observation_schedule
            assert schedule.times == times
            assert scheduled.replace(observation_times=schedule) == scheduled
            assert (
                factory(
                    **fields
                    | {
                        "touch_state": None,
                        "observation_times": schedule,
                        "calendar": scheduled.calendar,
                    }
                )
                == scheduled
            )
            assert scheduled.replace(observation_times=None) == continuous


def test_reactivation_with_changed_rules_retains_omitted_quantity():
    active = next(value for value in contracts() if isinstance(value, i.Accumulator))
    active = active.replace(
        knock_out_observation_times=None,
        accumulated_quantity=active.accumulated_quantity,
        knock_out_settled=active.knock_out_settled,
    )
    settled = active.replace(knock_out_settled=True)
    retained = settled.replace(knock_out_settled=False, daily_quantity=2)
    assert retained.accumulated_quantity == 3
    reactivated = settled.replace(
        knock_out_settled=False, daily_quantity=2, accumulated_quantity=0
    )
    assert not (reactivated.knock_out_settled)
    assert reactivated.accumulated_quantity == 0
    assert settled.replace(daily_quantity=2).knock_out_settled
    assert settled.knock_out_settled
    assert settled.accumulated_quantity == 3


def test_event_changes_preserve_omitted_history(subtests):
    for original in contracts():
        if hasattr(original, "realized_average"):
            original = original.replace(realized_average=100)
        history = {
            field: getattr(original, field)
            for field in (
                "touch_state",
                "barrier_state",
                "realized_average",
                "realized_hit_count",
                "accumulated_quantity",
                "knock_out_settled",
            )
            if hasattr(original, field)
        }
        if hasattr(original, "barrier_level"):
            updates = {"barrier_level": original.barrier_level + 5}
        elif hasattr(original, "lower_barrier"):
            updates = {"lower_barrier": original.lower_barrier - 5}
        elif isinstance(original, i.Accumulator):
            updates = {"daily_quantity": original.daily_quantity + 1}
        elif isinstance(original, i.RangeAccrualOption):
            updates = {"lower_bound": original.lower_bound - 5}
        elif hasattr(original, "averaging_start_time"):
            from datetime import timedelta

            updates = {
                "averaging_start_time": original.averaging_start_time
                + timedelta(days=1)
            }
        else:
            original = original.replace(
                observations=[
                    original.observations[0].replace(time=datetime(2025, 1, 3)),
                    *original.observations,
                ],
                barrier_state=original.barrier_state,
            )
            updates = {
                "observations": [
                    r.replace(knock_out_level=r.knock_out_level + 5)
                    for r in original.observations
                ]
            }
        with subtests.test(contract=type(original).__name__):
            saved = original.replace()
            retained = original.replace(**updates)
            assert {field: getattr(retained, field) for field in history} == history
            kept = original.replace(**updates, **history)
            assert ({field: getattr(kept, field) for field in history}) == history
            cleared_history = {field: None for field in history}
            cleared = original.replace(**updates, **cleared_history)
            assert (
                {field: getattr(cleared, field) for field in history}
            ) == cleared_history
            unknown = original.replace(**cleared_history)
            assert unknown.replace(**updates) == cleared
            assert original == saved


def test_every_field_replaces_without_mutating_omitted_terms_or_history(subtests):
    from kiyosi._serialization import _reduce

    for original in contracts():
        _, (factory, initial) = _reduce(original)
        with subtests.test(contract=type(original).__name__, operation="omission"):
            assert original.replace() == original
            assert original.replace() is not original
            forwarded = {
                name: field.default
                for name, field in inspect.signature(
                    original.replace
                ).parameters.items()
            }
            assert original.replace(**forwarded) == original
        for name in inspect.signature(original.replace).parameters:
            if name in ("observation_mode", "observation_times"):
                continue
            value = UPDATES.get(name)
            if name == "observations" and isinstance(original, i.PhoenixOption):
                value = [
                    i.AutocallableObservation.phoenix(END, 115, coupon_barrier_level=0)
                ]
            if name == "barrier_type":
                value = (
                    "knock_out"
                    if isinstance(original, i.DoubleTouchOption)
                    else "knock_in"
                    if isinstance(original, i.DoubleBarrierOption)
                    else "down_and_out"
                    if isinstance(original, i.TouchOption)
                    else "down_and_in"
                )
            with subtests.test(contract=type(original).__name__, field=name):
                updates = {name: value}
                if name == "knock_in_observation_mode":
                    updates["observation_clock_time"] = None
                if isinstance(original, (i.BinaryBarrierOption, i.TouchOption)):
                    if name == "payoff_type":
                        value = "asset" if original.payoff_type == "cash" else "cash"
                        updates = {
                            name: value,
                            "payout": None if value == "asset" else 10,
                        }
                    elif name == "payout" and original.payoff_type == "asset":
                        updates["payoff_type"] = "cash"
                if name == "effective_time" and isinstance(
                    original,
                    (i.GeometricAveragePriceOption, i.ArithmeticAveragePriceOption),
                ):
                    updates["averaging_start_time"] = value
                for field in (
                    "touch_state",
                    "barrier_state",
                    "realized_average",
                    "realized_hit_count",
                    "accumulated_quantity",
                    "knock_out_settled",
                ):
                    if hasattr(original, field):
                        updates.setdefault(field, getattr(original, field))
                changed = original.replace(**updates)
                if isinstance(original, i.DoubleTouchOption) and name == "barrier_type":
                    assert not (changed.is_one_touch)
                else:
                    assert getattr(changed, name) == (
                        tuple(value) if isinstance(value, list) else value
                    )
                expected = initial | updates
                if name == "knock_in_observation_mode":
                    expected["observation_clock_time"] = None
                if not (
                    isinstance(original, i.DoubleTouchOption) and name == "barrier_type"
                ):
                    assert changed == factory(**expected)
                if name in NULLABLE:
                    assert (
                        getattr(original.replace(**updates | {name: None}), name)
                        is None
                    )
                assert original == factory(**initial)


def test_settlement_replacements_roundtrip_and_switch_cash_and_asset(subtests):
    for asset in contracts():
        if (
            not isinstance(asset, (i.BinaryBarrierOption, i.TouchOption))
            or asset.payout is not None
        ):
            continue
        with subtests.test(contract=type(asset).__name__):
            assert asset.replace(payout=asset.payout) == asset
            cash = asset.replace(payoff_type="cash", payout=10)
            assert cash.payout == 10
            assert cash.payoff_type == "cash"
            assert cash.replace() == cash
            assert cash.replace(payoff_type="asset") == asset
            with pytest.raises(TypeError):
                cash.replace(payout=None)
            assert asset.payout is None
            with pytest.raises(InvalidInputError):
                asset.replace(payoff_type="cash", payout=-1)
            assert asset.payout is None
            assert cash.payout == 10


def test_validation_representation_errors_and_failure_state(subtests):
    for original in contracts():
        fields = inspect.signature(original.replace).parameters
        invalid = (
            "strike"
            if "strike" in fields
            else "payout"
            if "payout" in fields
            else "notional"
            if "notional" in fields
            else "initial_spot"
            if "initial_spot" in fields
            else "maturity_coupon_rate"
        )
        value = float("nan") if invalid == "maturity_coupon_rate" else -1
        with subtests.test(contract=type(original).__name__):
            with pytest.raises(InvalidInputError):
                updates = {invalid: value}
                if invalid == "payout" and isinstance(original, i.TouchOption):
                    updates["payoff_type"] = "cash"
                original.replace(**updates)
            for field in fields:
                with subtests.test(field=field), pytest.raises(TypeError):
                    original.replace(
                        **{field: 1 if field == "knock_out_settled" else True}
                    )
                nullable_payout = field == "payout" and isinstance(
                    original, (i.BinaryBarrierOption, i.TouchOption)
                )
                if (
                    field not in NULLABLE
                    and field != "realized_average"
                    and not nullable_payout
                ):
                    error = (
                        InvalidInputError
                        if field == "observation_clock_time"
                        and isinstance(
                            original,
                            (
                                i.SnowballOption,
                                i.PhoenixOption,
                                i.TernarySnowballOption,
                            ),
                        )
                        else TypeError
                    )
                    with pytest.raises(error):
                        original.replace(**{field: None})
            with pytest.raises(TypeError):
                original.replace(unknown=1)
            with pytest.raises(TypeError):
                original.replace(1)
            if "observation_times" in fields:
                assert (
                    original.replace(observation_times=[END]).touch_state
                    == original.touch_state
                )
                scheduled = original.replace(
                    observation_times=[END],
                    touch_state=original.touch_state,
                )
                assert scheduled.observation_times == ((END,))
                assert scheduled.observation_mode == "scheduled"


def test_monitoring_schedule_selects_mode_and_preserves_history_validation(subtests):
    for original in contracts():
        if not hasattr(original, "observation_mode"):
            continue
        with subtests.test(contract=type(original).__name__):
            scheduled = original.replace(
                observation_times=[END], touch_state=original.touch_state
            )
            assert scheduled.observation_mode == "scheduled"
            assert scheduled.observation_times == ((END,))
            assert scheduled.replace() == scheduled
            assert scheduled.replace(observation_times=...) == scheduled
            with pytest.raises(InvalidInputError):
                scheduled.replace(observation_times=[])
            assert scheduled.replace(observation_times=None) == original
            continuous = scheduled.replace(
                observation_times=None, touch_state=scheduled.touch_state
            )
            assert continuous == original
            assert continuous.observation_mode == "continuous"
            assert continuous.observation_times is None
            cleared = scheduled.replace(observation_times=None, touch_state=None)
            assert cleared.touch_state is None
            assert (
                scheduled.replace(touch_state=None).replace(observation_times=None)
                == cleared
            )
            assert scheduled.observation_times == ((END,))


def test_replaced_phoenix_observations_are_owned_and_coupon_is_periodic():
    note = next(value for value in contracts() if isinstance(value, i.PhoenixOption))
    records = [i.AutocallableObservation.phoenix(END, 115, coupon_barrier_level=0)]
    changed = note.replace(
        observations=records,
        coupon_per_period=0.02,
        barrier_state=note.barrier_state,
    )
    records.clear()
    assert changed.knock_out_levels == ((115,))
    assert changed.coupon_per_period == 0.02
    assert note.coupon_per_period == 0.1
    assert note.knock_out_levels == ((110, 110))
    with pytest.raises(TypeError):
        note.replace(coupon_rate=0.1, coupon_per_period=0.02)
