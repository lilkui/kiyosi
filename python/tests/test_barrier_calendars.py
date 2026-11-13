"""Barrier calendars belong to contracts across construction and reconstruction."""

import inspect
import pickle
from datetime import datetime

import pytest
from kiyosi import InvalidInputError, market, pricing
from kiyosi import instruments as ins


def test_barrier_calendars_are_owned_and_survive_reconstruction(subtests):
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
        "observation_times": [datetime(2025, 1, 4), end],
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
        original = factory(**common, **fields, calendar=market.all_days_calendar())
        owned = original.replace(calendar=market.all_days_calendar())
        with subtests.test(factory=factory.__name__):
            assert original.calendar == market.all_days_calendar()
            assert owned == factory(
                **common, **fields, calendar=market.all_days_calendar()
            )
            assert owned.replace() == owned
            assert owned.replace() == owned
            restored = pickle.loads(pickle.dumps(owned))
            assert restored == owned
            assert "all_days" in repr(owned)
            assert "calendar" in inspect.signature(factory).parameters
            for invalid in (None, True, "all_days"):
                with pytest.raises(TypeError):
                    original.replace(calendar=invalid)
            for engine in engines:
                assert engine.price(owned, context) == engine.price(restored, context)
