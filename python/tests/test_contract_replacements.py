"""Validated immutable edits of every exotic contract family."""

import inspect
import unittest
from datetime import datetime
from datetime import time as clock_time

from kiyosi import InvalidInputError
from kiyosi import instruments as i
from kiyosi.market import all_days_calendar

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


def contracts():
    yield i.BarrierOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
        barrier_level=120,
        barrier_type="up_and_out",
        rebate=0,
        rebate_timing="at_expiry",
        observation_mode="continuous",
        observation_times=[],
        touch_state="untouched",
    )
    yield i.DoubleBarrierOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
        lower_barrier=80,
        upper_barrier=120,
        barrier_type="knock_out",
        rebate=0,
        rebate_timing="at_expiry",
        observation_mode="continuous",
        observation_times=[],
        touch_state="untouched",
    )
    yield i.cash_binary_barrier_option(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
        barrier_level=120,
        barrier_type="up_and_out",
        observation_mode="continuous",
        observation_times=[],
        touch_state="untouched",
        payout=10,
    )
    yield i.asset_binary_barrier_option(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
        barrier_level=120,
        barrier_type="up_and_out",
        observation_mode="continuous",
        observation_times=[],
        touch_state="untouched",
    )
    yield i.cash_touch_option(
        effective_time=START,
        expiry_time=END,
        barrier_level=120,
        barrier_type="up_and_in",
        settlement_timing="at_expiry",
        observation_mode="continuous",
        observation_times=[],
        touch_state="untouched",
        payout=10,
    )
    yield i.asset_touch_option(
        effective_time=START,
        expiry_time=END,
        barrier_level=120,
        barrier_type="up_and_in",
        settlement_timing="at_expiry",
        observation_mode="continuous",
        observation_times=[],
        touch_state="untouched",
    )
    yield i.cash_double_touch(
        effective_time=START,
        expiry_time=END,
        lower_barrier=80,
        upper_barrier=120,
        settlement_timing="at_expiry",
        observation_mode="continuous",
        observation_times=[],
        touch_state="untouched",
        payout=10,
    )
    yield i.Accumulator(
        knock_out_settled=False,
        observation_clock_time=clock_time(),
        option_type="call",
        strike=100,
        knock_out_level=120,
        daily_quantity=1,
        acceleration_factor=2,
        accumulated_quantity=3,
        effective_time=START,
        expiry_time=END,
        knock_out_observation_times=[END],
    )
    yield i.RangeAccrualOption(
        observation_clock_time=clock_time(),
        notional=1000,
        annual_coupon_rate=0.12,
        lower_bound=90,
        upper_bound=110,
        effective_time=START,
        expiry_time=END,
        realized_hit_count=1,
    )
    yield i.SnowballOption(
        observation_clock_time=clock_time(),
        maturity_coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        barrier_state="no_event",
        principal_ratio=1,
        effective_time=START,
        expiry_time=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.PhoenixOption(
        observation_clock_time=clock_time(),
        coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        barrier_state="no_event",
        principal_ratio=1,
        effective_time=START,
        expiry_time=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.BinarySnowballOption(
        maturity_coupon_rate=0.1,
        barrier_state="no_event",
        principal_ratio=1,
        effective_time=START,
        expiry_time=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.TernarySnowballOption(
        observation_clock_time=clock_time(),
        maturity_coupon_rate=0.1,
        minimum_coupon_rate=0.02,
        knock_in_level=80,
        knock_in_observation_mode="every_trading_day",
        barrier_state="no_event",
        principal_ratio=1,
        effective_time=START,
        expiry_time=END,
        observations=[i.AutocallableObservation(END, 110)],
    )
    yield i.GeometricAveragePriceOption(
        option_type="call",
        strike=100,
        effective_time=START,
        averaging_start_time=START,
        expiry_time=END,
        realized_average=None,
    )
    yield i.ArithmeticAveragePriceOption(
        option_type="call",
        strike=100,
        effective_time=START,
        averaging_start_time=START,
        expiry_time=END,
        realized_average=None,
    )


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
    "knock_out_settled": True,
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
    "knock_out_settled",
    "knock_out_observation_times",
}


class ContractReplacementTests(unittest.TestCase):
    def test_reactivation_with_changed_rules_requires_explicit_quantity(self):
        active = next(
            value for value in contracts() if isinstance(value, i.Accumulator)
        )
        settled = active.replace_history(knock_out_settled=True)
        with self.assertRaises(InvalidInputError):
            settled.replace(knock_out_settled=False, daily_quantity=2)
        reactivated = settled.replace(
            knock_out_settled=False, daily_quantity=2, accumulated_quantity=0
        )
        self.assertFalse(reactivated.knock_out_settled)
        self.assertEqual(reactivated.accumulated_quantity, 0)
        self.assertTrue(settled.replace(daily_quantity=2).knock_out_settled)
        self.assertTrue(settled.knock_out_settled)
        self.assertEqual(settled.accumulated_quantity, 3)

    def test_event_changes_require_an_explicit_history_choice(self):
        for original in contracts():
            if hasattr(original, "realized_average"):
                original = original.replace_history(realized_average=100)
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
                updates = {
                    "observations": [
                        r.replace(knock_out_level=r.knock_out_level + 5)
                        for r in original.observations
                    ]
                }
            with self.subTest(contract=type(original).__name__):
                saved = original.replace()
                with self.assertRaises(InvalidInputError):
                    original.replace(**updates)
                kept = original.replace(**updates, **history)
                self.assertEqual(
                    {field: getattr(kept, field) for field in history}, history
                )
                cleared_history = {field: None for field in history}
                cleared = original.replace(**updates, **cleared_history)
                self.assertEqual(
                    {field: getattr(cleared, field) for field in history},
                    cleared_history,
                )
                unknown = original.replace(**cleared_history)
                self.assertEqual(unknown.replace(**updates), cleared)
                self.assertEqual(original, saved)

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
                if name in ("observation_mode", "observation_times"):
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
                    updates = {name: value}
                    if isinstance(original, (i.BinaryBarrierOption, i.TouchOption)):
                        if name == "payoff_type":
                            value = (
                                "asset" if original.payoff_type == "cash" else "cash"
                            )
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
                    if (
                        isinstance(original, i.DoubleTouchOption)
                        and name == "barrier_type"
                    ):
                        self.assertFalse(changed.is_one_touch)
                    else:
                        self.assertEqual(getattr(changed, name), value)
                    expected = initial | updates
                    if name == "knock_in_observation_mode":
                        expected["observation_clock_time"] = None
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
                            getattr(original.replace(**(updates | {name: None})), name)
                        )
                    self.assertEqual(original, factory(**initial))

    def test_settlement_replacements_roundtrip_and_switch_cash_and_asset(self):
        for asset in contracts():
            if (
                not isinstance(asset, (i.BinaryBarrierOption, i.TouchOption))
                or asset.payout is not None
            ):
                continue
            with self.subTest(contract=type(asset).__name__):
                self.assertEqual(asset.replace(payout=asset.payout), asset)
                cash = asset.replace(payoff_type="cash", payout=10)
                self.assertEqual(cash.payout, 10)
                self.assertEqual(cash.payoff_type, "cash")
                self.assertEqual(cash.replace(), cash)
                self.assertEqual(cash.replace(payoff_type="asset"), asset)
                with self.assertRaises(TypeError):
                    cash.replace(payout=None)
                self.assertIsNone(asset.payout)
                with self.assertRaises(InvalidInputError):
                    asset.replace(payoff_type="cash", payout=-1)
                self.assertIsNone(asset.payout)
                self.assertEqual(cash.payout, 10)

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
                with self.assertRaises(InvalidInputError):
                    updates = {invalid: value}
                    if invalid == "payout" and isinstance(original, i.TouchOption):
                        updates["payoff_type"] = "cash"
                    original.replace(**updates)
                for field in fields:
                    with self.subTest(field=field), self.assertRaises(TypeError):
                        original.replace(
                            **{field: 1 if field == "knock_out_settled" else True}
                        )
                    nullable_payout = field == "payout" and isinstance(
                        original, (i.BinaryBarrierOption, i.TouchOption)
                    )
                    nullable_coupon = isinstance(
                        original, i.PhoenixOption
                    ) and field in (
                        "coupon_rate",
                        "coupon_per_period",
                    )
                    if (
                        field not in NULLABLE
                        and field != "realized_average"
                        and not nullable_payout
                        and not nullable_coupon
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
                        with self.assertRaises(error):
                            original.replace(**{field: None})
                with self.assertRaises(TypeError):
                    original.replace(unknown=1)
                with self.assertRaises(TypeError):
                    original.replace(1)
                if "observation_times" in fields:
                    with self.assertRaises(InvalidInputError):
                        original.replace(observation_times=[END])
                    scheduled = original.replace(
                        observation_mode="scheduled",
                        observation_times=[END],
                        touch_state=original.touch_state,
                    )
                    self.assertEqual(scheduled.observation_times, [END])
                    self.assertEqual(scheduled.observation_mode, "scheduled")

    def test_monitoring_switch_clears_only_omitted_times_and_requires_history(self):
        for original in contracts():
            if not hasattr(original, "observation_mode"):
                continue
            with self.subTest(contract=type(original).__name__):
                scheduled = original.replace(
                    observation_mode="scheduled",
                    observation_times=[END],
                    touch_state=original.touch_state,
                )
                self.assertEqual(scheduled.replace(), scheduled)
                self.assertEqual(
                    scheduled.replace(observation_mode="scheduled"), scheduled
                )
                with self.assertRaises(InvalidInputError):
                    scheduled.replace(observation_mode="continuous")
                for fields in (
                    {},
                    {"observation_times": []},
                    {"observation_times": ...},
                ):
                    continuous = scheduled.replace(
                        observation_mode="continuous",
                        touch_state=scheduled.touch_state,
                        **fields,
                    )
                    self.assertEqual(continuous, original)
                cleared = scheduled.replace(
                    observation_mode="continuous", touch_state=None
                )
                self.assertIsNone(cleared.touch_state)
                self.assertEqual(cleared.observation_times, [])
                self.assertEqual(
                    scheduled.replace_history(touch_state=None).replace(
                        observation_mode="continuous"
                    ),
                    cleared,
                )
                with self.assertRaises(InvalidInputError):
                    scheduled.replace(
                        observation_mode="continuous",
                        observation_times=[END],
                        touch_state=scheduled.touch_state,
                    )
                with self.assertRaises(InvalidInputError):
                    scheduled.replace(
                        observation_times=[], touch_state=scheduled.touch_state
                    )
                self.assertEqual(scheduled.observation_mode, "scheduled")
                self.assertEqual(scheduled.observation_times, [END])

    def test_replaced_sequences_are_owned_and_phoenix_coupon_choices_are_exclusive(
        self,
    ):
        note = next(
            value for value in contracts() if isinstance(value, i.PhoenixOption)
        )
        records = [i.AutocallableObservation(END, 115)]
        changed = note.replace(
            observations=records,
            coupon_per_period=0.02,
            barrier_state=note.barrier_state,
        )
        records.clear()
        self.assertEqual(changed.knock_out_levels, [115])
        self.assertEqual(changed.coupon_per_period, 0.02)
        self.assertIsNone(changed.coupon_rate)
        self.assertEqual(note.coupon_rate, 0.1)
        self.assertEqual(note.knock_out_levels, [110])
        with self.assertRaises(TypeError):
            note.replace(coupon_rate=0.1, coupon_per_period=0.02)
