"""Undated autocallable state through the public pricing and replacement interfaces."""

import copy
import inspect
import pickle
from datetime import datetime, time, timedelta

import pytest
from kiyosi import InvalidInputError, MissingHistoryError
from kiyosi.instruments import (
    KnockInObservation,
    SnowballOption,
    autocallable_observations,
    standard_snowball,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    weekdays_calendar,
)
from kiyosi.pricing import QuadratureAutocallableEngine

START = datetime(2025, 1, 6)
EVENT = START + timedelta(days=2)
END = START + timedelta(days=4)


def contract(state):
    return SnowballOption(
        maturity_coupon_rate=0.06,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=60,
        effective_time=START,
        expiry_time=END,
        knock_in_observation=KnockInObservation.every_trading_day(time()),
        observations=autocallable_observations(
            [EVENT, END], [110, 110], knock_out_coupon_rates=[0.08, 0.08]
        ),
        barrier_state=state,
        calendar=weekdays_calendar(),
    )


def market(spot, at):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0
        ),
        spot_price=spot,
        valuation_time=at,
    )


def test_supplied_state_remains_applicable_across_observations():
    engine = QuadratureAutocallableEngine()
    for state, expected in (("knocked_in", 0.9), ("no_event", 1 + 0.06 * 4 / 365)):
        original = contract(state)
        note = original.replace(knock_in_level=85, barrier_state=Ellipsis)
        for at in (
            EVENT - timedelta(microseconds=1),
            EVENT,
            EVENT + timedelta(microseconds=1),
            END,
        ):
            assert engine.price(note, market(90, at)) == pytest.approx(
                expected, rel=0, abs=1e-14
            )
        assert original.knock_in_level == 80
        assert original.barrier_state == state
        for restored in (
            copy.copy(note),
            copy.deepcopy(note),
            pickle.loads(pickle.dumps(note)),
        ):
            assert restored == note
            assert restored.barrier_state == state
            assert "history_time" not in repr(restored)
    note = contract("no_event")
    assert engine.price(note, market(110, EVENT)) == pytest.approx(
        1 + 0.08 * 2 / 365, rel=0, abs=1e-14
    )
    settled = note.replace(barrier_state="knocked_out")
    with pytest.raises(InvalidInputError):
        engine.price(settled, market(90, EVENT))
    assert engine.price(settled, market(90, EVENT + timedelta(microseconds=1))) == 0
    with pytest.raises(MissingHistoryError):
        engine.price(note.replace(barrier_state=None), market(90, EVENT))
    with pytest.raises(InvalidInputError):
        note.replace(lower_strike=101, barrier_state=None)
    assert note == contract("no_event")
    assert note.replace(lower_strike=50, barrier_state="knocked_in") == contract(
        "knocked_in"
    ).replace(lower_strike=50)


def test_timestamp_and_separate_history_interfaces_are_absent():
    note = contract("no_event")
    assert not hasattr(note, "history_time")
    assert not hasattr(note, "replace_history")
    for operation in (SnowballOption, note.replace, standard_snowball):
        assert "history_time" not in inspect.signature(operation).parameters
        with pytest.raises(TypeError, match="history_time"):
            operation(history_time=START)
