"""Curve coverage follows unpaid barrier and touch cash flows."""

import unittest
from datetime import datetime

from kiyosi import InvalidInputError, market, pricing
from kiyosi import instruments as ins
from support.history import dated as dated_history
from support.history import history_fields


class BarrierSettlementTests(unittest.TestCase):
    def test_settled_binary_and_touch_payoffs_do_not_require_future_curve_coverage(
        self,
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
        binary = ins.cash_binary_barrier_option(
            **history_fields(
                **common,
                option_type="call",
                strike=100,
                barrier_level=120,
                barrier_type="up_and_out",
            )
        )
        touch = ins.cash_touch_option(
            **history_fields(
                **common,
                barrier_level=120,
                barrier_type="up_and_in",
                settlement_timing="at_hit",
            )
        )
        double = ins.cash_double_touch(
            **history_fields(
                **common,
                lower_barrier=80,
                upper_barrier=120,
                settlement_timing="at_hit",
            )
        )
        for engine, settled, unpaid in (
            (
                pricing.AnalyticBinaryBarrierEngine(),
                binary,
                binary.replace_history(touch_state=None),
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
            with self.subTest(contract=type(settled).__name__):
                self.assertEqual(
                    engine.price(dated_history(settled, context), context), 0
                )
                with self.assertRaises(InvalidInputError):
                    engine.price(dated_history(unpaid, context), context)
