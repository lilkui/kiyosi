"""Curve coverage follows unpaid barrier and touch cash flows."""

from datetime import datetime

import pytest
from kiyosi import InvalidInputError, market, pricing
from kiyosi import instruments as ins


def test_settled_binary_and_touch_payoffs_do_not_require_future_curve_coverage(
    subtests,
):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    curve = market.RateTermStructure(
        reference_time=start, times=[datetime(2025, 2, 1)], factors=[1]
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=curve, dividend_yield=curve, volatility=0.2
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 2),
    )
    common = {
        "effective_time": start,
        "expiry_time": end,
        "touch_state": "touched",
        "payout": 10,
    }
    binary = ins.BinaryBarrierOption(
        payoff_type="cash",
        **common,
        option_type="call",
        strike=100,
        barrier_level=120,
        barrier_type="up_and_out",
    )
    touch = ins.TouchOption(
        payoff_type="cash",
        **common,
        barrier_level=120,
        barrier_type="up_and_in",
        settlement_timing="at_hit",
    )
    double = ins.DoubleTouchOption(
        barrier_type="knock_in",
        **common,
        lower_barrier=80,
        upper_barrier=120,
        settlement_timing="at_hit",
    )
    for engine, settled, unpaid in (
        (
            pricing.AnalyticBinaryBarrierEngine(),
            binary,
            binary.replace(touch_state=None),
        ),
        (
            pricing.AnalyticBinaryBarrierEngine(),
            touch,
            touch.replace(settlement_timing="at_expiry"),
        ),
        (
            pricing.AnalyticDoubleTouchEngine(),
            double,
            double.replace(settlement_timing="at_expiry"),
        ),
    ):
        with subtests.test(contract=type(settled).__name__):
            assert engine.price(settled, context) == 0
            with pytest.raises(InvalidInputError):
                engine.price(unpaid, context)
