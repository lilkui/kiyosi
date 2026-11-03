"""Validated immutable edits of every exotic contract family."""

import inspect
import unittest
from datetime import date

from kiyosi import InvalidInputError, UnsupportedOperationError
from kiyosi import instruments as i

START, END = date(2025, 1, 1), date(2026, 1, 1)


def contracts():
    yield i.BarrierOption(
        option_type="call",
        strike=100,
        effective_date=START,
        expiry_date=END,
        barrier_level=120,
        barrier_type="up_and_out",
        rebate=0,
        rebate_timing="at_expiry",
        observation_mode="continuous",
        observation_dates=[],
        touch_state="untouched",
    )
    yield i.DoubleBarrierOption(
        option_type="call",
        strike=100,
        effective_date=START,
        expiry_date=END,
        lower_barrier=80,
        upper_barrier=120,
        barrier_type="knock_out",
        rebate=0,
        rebate_timing="at_expiry",
        observation_mode="continuous",
        observation_dates=[],
        touch_state="untouched",
    )
    yield i.cash_binary_barrier_option(
        option_type="call",
        strike=100,
        effective_date=START,
        expiry_date=END,
        barrier_level=120,
        barrier_type="up_and_out",
        observation_mode="continuous",
        observation_dates=[],
        touch_state="untouched",
        payout=10,
    )
    yield i.asset_binary_barrier_option(
        option_type="call",
        strike=100,
        effective_date=START,
        expiry_date=END,
        barrier_level=120,
        barrier_type="up_and_out",
        observation_mode="continuous",
        observation_dates=[],
        touch_state="untouched",
    )
    yield i.cash_touch_option(
        effective_date=START,
        expiry_date=END,
        barrier_level=120,
        barrier_type="up_and_in",
        settlement_timing="at_expiry",
        observation_mode="continuous",
        observation_dates=[],
        touch_state="untouched",
        payout=10,
    )
    yield i.asset_touch_option(
        effective_date=START,
        expiry_date=END,
        barrier_level=120,
        barrier_type="up_and_in",
        settlement_timing="at_expiry",
        observation_mode="continuous",
        observation_dates=[],
        touch_state="untouched",
    )
    yield i.cash_double_touch(
        effective_date=START,
        expiry_date=END,
        lower_barrier=80,
        upper_barrier=120,
        settlement_timing="at_expiry",
        observation_mode="continuous",
        observation_dates=[],
        touch_state="untouched",
        payout=10,
    )
    yield i.Accumulator(
        option_type="call",
        strike=100,
        knock_out_level=120,
        daily_quantity=1,
        acceleration_factor=2,
        accumulated_quantity=3,
        effective_date=START,
        expiry_date=END,
        knock_out_observation_dates=[END],
    )
    yield i.RangeAccrualOption(
        notional=1000,
        annual_coupon_rate=0.12,
        lower_bound=90,
        upper_bound=110,
        effective_date=START,
        expiry_date=END,
        realized_hit_count=1,
    )
    yield i.SnowballOption(
        maturity_coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        barrier_state="no_event",
        principal_ratio=1,
        effective_date=START,
        expiry_date=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.PhoenixOption(
        coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        barrier_state="no_event",
        principal_ratio=1,
        effective_date=START,
        expiry_date=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.BinarySnowballOption(
        maturity_coupon_rate=0.1,
        barrier_state="no_event",
        principal_ratio=1,
        effective_date=START,
        expiry_date=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.TernarySnowballOption(
        maturity_coupon_rate=0.1,
        minimum_coupon_rate=0.02,
        knock_in_level=80,
        knock_in_observation_mode="every_trading_day",
        barrier_state="no_event",
        principal_ratio=1,
        effective_date=START,
        expiry_date=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.GeometricAveragePriceOption(
        option_type="call",
        strike=100,
        effective_date=START,
        averaging_start_date=START,
        expiry_date=END,
        realized_average=0,
    )
    yield i.ArithmeticAveragePriceOption(
        option_type="call",
        strike=100,
        effective_date=START,
        averaging_start_date=START,
        expiry_date=END,
        realized_average=0,
    )


UPDATES = {
    "option_type": "put",
    "strike": 105,
    "effective_date": date(2025, 1, 2),
    "expiry_date": date(2026, 1, 2),
    "averaging_start_date": date(2025, 1, 2),
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
    "knock_out_observation_dates": [START],
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
    "barrier_state": "knocked_out",
    "principal_ratio": 0.9,
    "observations": [i.AutocallableObservation(END, 115)],
    "maturity_coupon_rate": 0.15,
    "minimum_coupon_rate": 0.03,
    "coupon_rate": 0.15,
    "coupon_per_period": 0.02,
    "realized_average": 105,
}
NULLABLE = {
    "touch_state",
    "barrier_state",
    "realized_hit_count",
    "accumulated_quantity",
    "knock_out_observation_dates",
}


class ContractReplacementTests(unittest.TestCase):
    def test_every_field_replaces_without_mutating_omitted_terms_or_history(self):
        from kiyosi._serialization import _reduce

        for original in contracts():
            _, (factory, initial) = _reduce(original)
            with self.subTest(contract=type(original).__name__, operation="omission"):
                self.assertEqual(original.replace(), original)
                self.assertIsNot(original.replace(), original)
                forwarded = {
                    name: field.default
                    for name, field in inspect.signature(
                        original.replace
                    ).parameters.items()
                }
                self.assertEqual(original.replace(**forwarded), original)
            for name in inspect.signature(original.replace).parameters:
                if name in ("observation_mode", "observation_dates"):
                    continue
                value = UPDATES.get(name)
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
                with self.subTest(contract=type(original).__name__, field=name):
                    if name == "payout" and original.payout is None:
                        with self.assertRaises(UnsupportedOperationError):
                            original.replace(payout=value)
                        continue
                    updates = {name: value}
                    if name == "effective_date" and isinstance(
                        original,
                        (i.GeometricAveragePriceOption, i.ArithmeticAveragePriceOption),
                    ):
                        updates["averaging_start_date"] = value
                    changed = original.replace(**updates)
                    if (
                        isinstance(original, i.DoubleTouchOption)
                        and name == "barrier_type"
                    ):
                        self.assertFalse(changed.is_one_touch)
                    else:
                        self.assertEqual(getattr(changed, name), value)
                    expected = initial | updates
                    if isinstance(original, i.PhoenixOption) and name in (
                        "coupon_rate",
                        "coupon_per_period",
                    ):
                        expected.pop(
                            "coupon_rate"
                            if name == "coupon_per_period"
                            else "coupon_per_period",
                            None,
                        )
                    if not (
                        isinstance(original, i.DoubleTouchOption)
                        and name == "barrier_type"
                    ):
                        self.assertEqual(changed, factory(**expected))
                    if name in NULLABLE:
                        self.assertIsNone(
                            getattr(original.replace(**{name: None}), name)
                        )
                    self.assertEqual(original, factory(**initial))

    def test_validation_representation_errors_and_failure_state(self):
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
            with self.subTest(contract=type(original).__name__):
                if not (invalid == "payout" and original.payout is None):
                    with self.assertRaises(InvalidInputError):
                        original.replace(**{invalid: value})
                for field in fields:
                    with self.subTest(field=field), self.assertRaises(TypeError):
                        original.replace(**{field: True})
                    if field not in NULLABLE:
                        with self.assertRaises(TypeError):
                            original.replace(**{field: None})
                with self.assertRaises(TypeError):
                    original.replace(unknown=1)
                with self.assertRaises(TypeError):
                    original.replace(1)
                if "observation_dates" in fields:
                    with self.assertRaises(InvalidInputError):
                        original.replace(observation_dates=[END])
                    scheduled = original.replace(
                        observation_mode="scheduled", observation_dates=[END]
                    )
                    self.assertEqual(scheduled.observation_dates, [END])
                    self.assertEqual(scheduled.observation_mode, "scheduled")

    def test_replaced_sequences_are_owned_and_phoenix_coupon_choices_are_exclusive(
        self,
    ):
        note = next(
            value for value in contracts() if isinstance(value, i.PhoenixOption)
        )
        records = [i.AutocallableObservation(END, 115)]
        changed = note.replace(observations=records, coupon_per_period=0.02)
        records.clear()
        self.assertEqual(changed.knock_out_levels, [115])
        self.assertEqual(changed.coupon_per_period, 0.02)
        self.assertIsNone(changed.coupon_rate)
        self.assertEqual(note.coupon_rate, 0.1)
        self.assertEqual(note.knock_out_levels, [110])
        with self.assertRaises(TypeError):
            note.replace(coupon_rate=0.1, coupon_per_period=0.02)
