"""Caller-owned accrued state uses the requested valuation boundary."""

import copy
import pickle
from datetime import datetime, time, timedelta

import pytest
from kiyosi import InvalidInputError, MissingHistoryError
from kiyosi.instruments import Accumulator, RangeAccrualOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import (
    AnalyticRangeAccrualEngine,
    FiniteDifferenceAccumulatorEngine,
    MonteCarloAccumulatorEngine,
    QuadratureAccumulatorEngine,
)

START = datetime(2025, 1, 6)
EVENT = START + timedelta(days=1)


def market(at):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0
        ),
        spot_price=105,
        valuation_time=at,
    )


def accumulator():
    return Accumulator(
        strike=100,
        knock_out_level=120,
        daily_quantity=1,
        acceleration_factor=2,
        effective_time=START,
        expiry_time=START + timedelta(days=2),
        observation_clock_time=time(),
        accumulated_quantity=1,
        knock_out_settled=False,
    )


def test_accumulator_partial_history_and_combined_replacement_preserve_values():
    original = accumulator()
    settled = original.replace(knock_out_settled=True)
    assert settled.accumulated_quantity == 1
    changed = original.replace(accumulated_quantity=3, daily_quantity=2)
    assert changed.knock_out_settled is False
    assert changed.accumulated_quantity == 3
    assert original.replace(accumulated_quantity=..., knock_out_settled=...) == original
    cleared = original.replace(accumulated_quantity=None)
    assert cleared.accumulated_quantity is None
    assert cleared.knock_out_settled is False
    assert original.replace(accumulated_quantity=0).accumulated_quantity == 0
    with pytest.raises(InvalidInputError):
        original.replace(accumulated_quantity=-1, knock_out_settled=True)
    assert original.accumulated_quantity == 1
    assert original.knock_out_settled is False
    for value in (original, settled, changed, cleared):
        assert copy.copy(value) == value
        assert pickle.loads(pickle.dumps(value)) == value


@pytest.mark.parametrize(
    "engine",
    (
        QuadratureAccumulatorEngine(),
        FiniteDifferenceAccumulatorEngine(),
        MonteCarloAccumulatorEngine(path_count=32, seed=42),
    ),
)
def test_accumulator_counts_current_observation_once_and_uses_supplied_state(engine):
    original = accumulator()
    for at in (EVENT - timedelta(microseconds=1), EVENT):
        assert engine.price(original, market(at)) == pytest.approx(15, abs=1e-12)
    after = market(EVENT + timedelta(microseconds=1))
    assert engine.price(
        original.replace(accumulated_quantity=2), after
    ) == pytest.approx(15, abs=1e-12)
    assert engine.price(original, after) == pytest.approx(10, abs=1e-12)
    settled = original.replace(knock_out_settled=True)
    assert engine.price(settled, after) == 0
    for value in (original, settled):
        with pytest.raises(InvalidInputError):
            engine.price(value, market(START))
    with pytest.raises(MissingHistoryError):
        engine.price(original.replace(accumulated_quantity=None), after)


def test_range_accrual_counts_current_hit_once_and_accepts_undated_history():
    fields = {
        "notional": 365,
        "annual_coupon_rate": 1,
        "lower_bound": 90,
        "upper_bound": 110,
        "effective_time": START,
        "expiry_time": START + timedelta(days=2),
        "observation_clock_time": time(),
    }
    original = RangeAccrualOption(**fields)
    engine = AnalyticRangeAccrualEngine()
    for at in (EVENT - timedelta(microseconds=1), EVENT):
        assert engine.price(original, market(at)) == pytest.approx(2, abs=1e-12)
    known = original.replace(realized_hit_count=1)
    assert known == RangeAccrualOption(**fields, realized_hit_count=1)
    after = market(EVENT + timedelta(microseconds=1))
    assert engine.price(known, after) == pytest.approx(2, abs=1e-12)
    assert engine.price(known.replace(realized_hit_count=0), after) == pytest.approx(
        1, abs=1e-12
    )
    with pytest.raises(InvalidInputError):
        engine.price(known, market(EVENT))
    with pytest.raises(MissingHistoryError):
        engine.price(original, after)
    assert known.replace(realized_hit_count=...) == known
    assert known.replace(realized_hit_count=None) == original
    assert known.replace(lower_bound=80).realized_hit_count == 1
    with pytest.raises(InvalidInputError):
        known.replace(realized_hit_count=-1, notional=100)
    assert known.realized_hit_count == 1
    assert pickle.loads(pickle.dumps(known)) == known
