"""Barrier calendars belong to contracts across construction and replacement."""

import inspect
import math
from datetime import datetime, timedelta

import pytest
from kiyosi import InvalidInputError, market, pricing
from kiyosi import instruments as ins


def test_deterministic_barrier_uses_current_and_future_fixings_after_dense_history():
    start = datetime(2025, 1, 1)
    valuation = start + timedelta(days=1)
    first, expiry = (valuation + timedelta(days=days) for days in (30, 60))
    option = ins.BarrierOption(
        option_type="call",
        strike=90,
        effective_time=start,
        expiry_time=expiry,
        barrier_level=100,
        barrier_type="up_and_out",
        rebate=2,
        rebate_timing="at_hit",
        touch_state="untouched",
        calendar=market.TradingCalendar.all_days(),
        observation_schedule=(
            *(start + timedelta(seconds=offset) for offset in range(4096)),
            valuation,
            first,
            expiry,
        ),
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.2, dividend_yield=0, volatility=0
        ),
        spot_price=99,
        valuation_time=valuation,
    )
    expected = 2 * math.exp(-0.2 * 30 / 365)
    assert pricing.AnalyticBarrierEngine().price(option, context) == pytest.approx(
        expected, rel=0, abs=1e-14
    )
    saved = pricing.FiniteDifferenceBarrierEngine().solve(
        option, context, spot_range=(98, 101)
    )
    assert saved.price(99) == pytest.approx(expected, rel=0, abs=1e-14)
    assert saved.price(100) == 2


def test_barrier_calendars_are_owned_and_preserved_by_replacement(subtests):
    start, end = datetime(2025, 1, 3), datetime(2025, 1, 5)
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.02, dividend_yield=0, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    common = {
        "effective_time": start,
        "expiry_time": end,
        "observation_schedule": [datetime(2025, 1, 4), end],
    }
    cases = [
        (
            ins.BarrierOption,
            {
                "option_type": "call",
                "strike": 100,
                "barrier_level": 120,
                "barrier_type": "up_and_out",
            },
            [
                pricing.AnalyticBarrierEngine(),
                pricing.CoxRossRubinsteinBarrierEngine(step_count=2),
                pricing.QuadratureBarrierEngine(),
                pricing.FiniteDifferenceBarrierEngine(),
                pricing.MonteCarloBarrierEngine(
                    path_count=32, seed=42, max_active_workers=1
                ),
            ],
        ),
        (
            ins.DoubleBarrierOption,
            {
                "option_type": "call",
                "strike": 100,
                "lower_barrier": 80,
                "upper_barrier": 120,
                "barrier_type": "knock_out",
            },
            [pricing.FiniteDifferenceDoubleBarrierEngine()],
        ),
        (
            ins.BinaryBarrierOption,
            {
                "payoff_type": "cash",
                "option_type": "call",
                "strike": 100,
                "barrier_level": 120,
                "barrier_type": "up_and_out",
                "payout": 10,
            },
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.BinaryBarrierOption,
            {
                "payoff_type": "asset",
                "option_type": "call",
                "strike": 100,
                "barrier_level": 120,
                "barrier_type": "up_and_out",
            },
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.TouchOption,
            {
                "payoff_type": "cash",
                "barrier_level": 120,
                "barrier_type": "up_and_in",
                "payout": 10,
            },
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.TouchOption,
            {"payoff_type": "asset", "barrier_level": 120, "barrier_type": "up_and_in"},
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.DoubleTouchOption,
            {
                "barrier_type": "knock_in",
                "lower_barrier": 80,
                "upper_barrier": 120,
                "payout": 10,
            },
            [pricing.FiniteDifferenceDoubleTouchEngine()],
        ),
        (
            ins.DoubleTouchOption,
            {
                "barrier_type": "knock_out",
                "lower_barrier": 80,
                "upper_barrier": 120,
                "payout": 10,
            },
            [pricing.FiniteDifferenceDoubleTouchEngine()],
        ),
    ]
    for factory, fields, engines in cases:
        with pytest.raises(InvalidInputError):
            factory(**common, **fields)
        original = factory(
            **common, **fields, calendar=market.TradingCalendar.all_days()
        )
        owned = original.replace(calendar=market.TradingCalendar.all_days())
        with subtests.test(factory=factory.__name__):
            assert original.calendar == market.TradingCalendar.all_days()
            assert owned == factory(
                **common, **fields, calendar=market.TradingCalendar.all_days()
            )
            schedule = owned.observation_schedule
            assert schedule is not None
            assert schedule.times == tuple(common["observation_schedule"])
            assert owned.replace() == owned
            continuous = owned.replace(observation_schedule=None)
            assert continuous.calendar == market.TradingCalendar.all_days()
            weekdays = continuous.replace(calendar=market.TradingCalendar.weekdays())
            assert weekdays.replace(observation_schedule=schedule) == owned
            assert (
                factory(**(common | {"observation_schedule": schedule}), **fields)
                == owned
            )
            with pytest.raises(InvalidInputError, match="not a trading day"):
                owned.replace(
                    observation_schedule=schedule,
                    calendar=market.TradingCalendar.weekdays(),
                )
            with pytest.raises(InvalidInputError, match="within life bounds"):
                owned.replace(expiry_time=start)
            assert "all_days" in repr(owned)
            assert "calendar" in inspect.signature(factory).parameters
            for invalid in (None, True, "all_days"):
                with pytest.raises(TypeError):
                    original.replace(calendar=invalid)
            for engine in engines:
                assert engine.price(owned, context) == engine.price(original, context)


def test_raw_barrier_schedule_preserves_contract_validation_order():
    common = {
        "option_type": "call",
        "effective_time": datetime(2025, 1, 3),
        "expiry_time": datetime(2025, 1, 5),
        "barrier_level": 120,
        "barrier_type": "up_and_out",
        "observation_schedule": [datetime(2025, 1, 4)],
    }
    with pytest.raises(InvalidInputError, match="not a trading day"):
        ins.BarrierOption(**common, strike=-1)
    with pytest.raises(InvalidInputError, match="payout"):
        ins.BinaryBarrierOption(**common, strike=100, payoff_type="cash", payout=0)
