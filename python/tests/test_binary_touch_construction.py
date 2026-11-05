"""Construct binary and touch contracts through their public classes."""

import inspect
import pickle
import unittest
from datetime import datetime

from kiyosi import InvalidInputError, UnsupportedOperationError, market, pricing
from kiyosi import instruments as ins


class BinaryTouchConstructionTests(unittest.TestCase):
    def test_constructors_preserve_settlement_monitoring_and_terminal_payments(self):
        start, end = datetime(2025, 1, 3), datetime(2025, 1, 4)
        life = {"effective_time": start, "expiry_time": end}
        single = {"barrier_level": 120, "barrier_type": "up_and_in"}
        binary = dict(single, option_type="call", strike=100)
        double = {"lower_barrier": 80, "upper_barrier": 120}
        calendar = market.all_days_calendar()
        monitoring = {
            "observation_mode": "scheduled",
            "observation_times": [end],
            "touch_state": "untouched",
            "calendar": calendar,
        }
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.02, dividend_yield=0, volatility=0.2
            ),
            spot_price=125,
            valuation_time=end,
        )
        cases = [
            (
                ins.BinaryBarrierOption,
                ins.cash_binary_barrier_option,
                dict(binary, payout=10),
                10,
            ),
            (
                ins.BinaryBarrierOption,
                ins.asset_binary_barrier_option,
                dict(binary, payout=None),
                125,
            ),
            (ins.TouchOption, ins.cash_touch_option, dict(single, payout=10), 10),
            (ins.TouchOption, ins.asset_touch_option, dict(single, payout=None), 125),
            (
                ins.TouchOption,
                ins.cash_touch_option,
                dict(single, barrier_type="up_and_out", payout=10),
                0,
            ),
            (
                ins.TouchOption,
                ins.asset_touch_option,
                dict(single, barrier_type="up_and_out", payout=None),
                0,
            ),
            (
                ins.DoubleTouchOption,
                ins.cash_double_touch,
                dict(double, barrier_type="knock_in", payout=10),
                10,
            ),
            (
                ins.DoubleTouchOption,
                ins.cash_double_no_touch,
                dict(double, barrier_type="knock_out", payout=10),
                0,
            ),
        ]
        for cls, factory, terms, expected in cases:
            with self.subTest(contract=cls.__name__, factory=factory.__name__):
                fields = dict(life, **terms, **monitoring)
                arguments = inspect.signature(cls).bind(**fields)
                arguments.apply_defaults()
                contract = cls(*arguments.args, **arguments.kwargs)
                factory_fields = fields.copy()
                if cls is ins.DoubleTouchOption:
                    factory_fields.pop("barrier_type")
                elif fields["payout"] is None:
                    factory_fields.pop("payout")
                self.assertEqual(contract, factory(**factory_fields))
                self.assertEqual(contract.payout, terms["payout"])
                self.assertEqual(contract.calendar, calendar)
                self.assertEqual(contract.observation_times, [end])
                self.assertEqual(contract.touch_state, "untouched")
                engine = (
                    pricing.AnalyticDoubleTouchEngine()
                    if cls is ins.DoubleTouchOption
                    else pricing.AnalyticBinaryBarrierEngine()
                )
                self.assertEqual(engine.price(contract, context), expected)
                restored = pickle.loads(pickle.dumps(contract))
                self.assertEqual(restored, contract)
                self.assertEqual(engine.price(restored, context), expected)
                if terms["payout"] is None:
                    with self.assertRaises(UnsupportedOperationError):
                        contract.replace(payout=5)
                else:
                    self.assertEqual(
                        engine.price(contract.replace(payout=5), context), expected / 2
                    )
                with self.assertRaises(InvalidInputError):
                    contract.replace(calendar=market.weekdays_calendar())
                self.assertEqual(contract.calendar, calendar)

    def test_constructors_validate_representations_and_domain_inputs(self):
        start, end = datetime(2025, 1, 3), datetime(2025, 1, 4)
        life = {"effective_time": start, "expiry_time": end, "payout": 10}
        for cls, specific in (
            (
                ins.BinaryBarrierOption,
                {
                    "option_type": "call",
                    "strike": 100,
                    "barrier_level": 120,
                    "barrier_type": "up_and_in",
                },
            ),
            (ins.TouchOption, {"barrier_level": 120, "barrier_type": "up_and_in"}),
            (
                ins.DoubleTouchOption,
                {"lower_barrier": 80, "upper_barrier": 120, "barrier_type": "knock_in"},
            ),
        ):
            fields = dict(life, **specific, calendar=market.all_days_calendar())
            for changes, error in (
                ({"payout": True}, TypeError),
                ({"payout": "10"}, TypeError),
                ({"payout": -1}, InvalidInputError),
                ({"payout": float("inf")}, InvalidInputError),
                ({"barrier_type": "invalid"}, ValueError),
                ({"expiry_time": start.replace(year=2024)}, InvalidInputError),
                (
                    {
                        "calendar": market.weekdays_calendar(),
                        "observation_mode": "scheduled",
                        "observation_times": [end],
                    },
                    InvalidInputError,
                ),
            ):
                with (
                    self.subTest(contract=cls.__name__, changes=changes),
                    self.assertRaises(error),
                ):
                    cls(**(fields | changes))
            without_payout = fields.copy()
            without_payout.pop("payout")
            with self.assertRaises(TypeError):
                cls(**without_payout)
            with self.assertRaises(TypeError):
                cls(10)
        with self.assertRaises(TypeError):
            ins.DoubleTouchOption(
                **(life | {"payout": None}),
                lower_barrier=80,
                upper_barrier=120,
                barrier_type="knock_in",
            )

    def test_at_hit_settlement_requires_one_touch(self):
        life = {
            "effective_time": datetime(2025, 1, 3),
            "expiry_time": datetime(2025, 1, 6),
        }
        for payout in (10, None):
            contract = ins.TouchOption(
                **life,
                barrier_level=120,
                barrier_type="up_and_in",
                payout=payout,
                settlement_timing="at_hit",
            )
            self.assertEqual(contract.settlement_timing, "at_hit")
            self.assertTrue(contract.is_one_touch)
            with self.subTest(payout=payout), self.assertRaises(InvalidInputError):
                ins.TouchOption(
                    **life,
                    barrier_level=120,
                    barrier_type="up_and_out",
                    payout=payout,
                    settlement_timing="at_hit",
                )
        contract = ins.DoubleTouchOption(
            **life,
            lower_barrier=80,
            upper_barrier=120,
            barrier_type="knock_in",
            payout=10,
            settlement_timing="at_hit",
        )
        self.assertEqual(contract.settlement_timing, "at_hit")
        self.assertTrue(contract.is_one_touch)
        with self.assertRaises(InvalidInputError):
            ins.DoubleTouchOption(
                **life,
                lower_barrier=80,
                upper_barrier=120,
                barrier_type="knock_out",
                payout=10,
                settlement_timing="at_hit",
            )
