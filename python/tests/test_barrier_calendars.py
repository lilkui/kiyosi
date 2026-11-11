"""Barrier calendars belong to contracts across construction and reconstruction."""

import inspect
import pickle
from datetime import datetime

import pytest
from kiyosi import InvalidInputError, market, pricing
from kiyosi import instruments as ins
from support.history import dated as dated_history
from support.history import history_fields


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
            ins.cash_binary_barrier_option,
            {
                "option_type": "call",
                "strike": 100,
                "barrier_level": 120,
                "barrier_type": "up_and_out",
                "payout": 10,
            },
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.asset_binary_barrier_option,
            {
                "option_type": "call",
                "strike": 100,
                "barrier_level": 120,
                "barrier_type": "up_and_out",
            },
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.cash_touch_option,
            {"barrier_level": 120, "barrier_type": "up_and_in", "payout": 10},
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.asset_touch_option,
            {"barrier_level": 120, "barrier_type": "up_and_in"},
            [pricing.FiniteDifferenceBinaryBarrierEngine()],
        ),
        (
            ins.cash_double_touch,
            {"lower_barrier": 80, "upper_barrier": 120, "payout": 10},
            [pricing.FiniteDifferenceDoubleTouchEngine()],
        ),
        (
            ins.cash_double_no_touch,
            {"lower_barrier": 80, "upper_barrier": 120, "payout": 10},
            [pricing.FiniteDifferenceDoubleTouchEngine()],
        ),
    ]
    for factory, fields, engines in cases:
        with pytest.raises(InvalidInputError):
            factory(**history_fields(**common, **fields))
        original = factory(
            **history_fields(**common, **fields, calendar=market.all_days_calendar())
        )
        owned = original.replace(calendar=market.all_days_calendar())
        with subtests.test(factory=factory.__name__):
            assert original.calendar == market.all_days_calendar()
            assert owned == factory(
                **history_fields(
                    **common, **fields, calendar=market.all_days_calendar()
                )
            )
            assert owned.replace() == owned
            assert owned.replace_history() == owned
            restored = pickle.loads(pickle.dumps(owned))
            assert restored == owned
            assert "all_days" in repr(owned)
            assert "calendar" in inspect.signature(factory).parameters
            for invalid in (None, True, "all_days"):
                with pytest.raises(TypeError):
                    original.replace(calendar=invalid)
            for engine in engines:
                assert engine.price(
                    dated_history(owned, context), context
                ) == engine.price(dated_history(restored, context), context)
