"""History snapshots have an explicit valuation time and cannot silently go stale."""

import math
import pickle
import unittest
from datetime import UTC, datetime, time, timedelta

from kiyosi import InvalidInputError, MissingHistoryError, StaleHistoryError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from kiyosi.market import custom_calendar
from test_contract_replacements import contract_inputs, contracts

AT = datetime(2025, 1, 6, 12)


def context(at=AT, volatility=0.2):
    return m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=volatility
        ),
        spot_price=100,
        valuation_time=at,
    )


def engine(instrument):
    for cls, factory in (
        (i.BarrierOption, p.AnalyticBarrierEngine),
        (i.DoubleBarrierOption, p.AnalyticDoubleBarrierEngine),
        (i.BinaryBarrierOption, p.AnalyticBinaryBarrierEngine),
        (i.TouchOption, p.AnalyticBinaryBarrierEngine),
        (i.DoubleTouchOption, p.AnalyticDoubleTouchEngine),
        (i.RangeAccrualOption, p.AnalyticRangeAccrualEngine),
        (i.Accumulator, p.QuadratureAccumulatorEngine),
        (i.GeometricAveragePriceOption, p.AnalyticGeometricAveragePriceEngine),
        (i.ArithmeticAveragePriceOption, p.TurnbullWakemanArithmeticAveragePriceEngine),
    ):
        if isinstance(instrument, cls):
            return factory()
    return p.QuadratureAutocallableEngine()


class HistoryTests(unittest.TestCase):
    def test_autocallable_history_respects_first_daily_observation_on_sparse_calendar(
        self,
    ):
        start, first, end = (
            datetime(2025, 1, 2, 13),
            datetime(2025, 1, 6, 12),
            datetime(2025, 1, 6, 16),
        )
        note = i.SnowballOption(
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=start,
            expiry_time=end,
            observations=[i.AutocallableObservation.snowball(end, 110, coupon_rate=0)],
            observation_clock_time=time(12),
            calendar=custom_calendar(
                base=m.all_days_calendar(),
                closed_dates=[
                    (start + timedelta(days=day)).date() for day in (1, 2, 3)
                ],
            ),
        )
        selected = p.QuadratureAutocallableEngine()
        for at in (
            start,
            first - timedelta(microseconds=1),
            first,
            first + timedelta(microseconds=1),
        ):
            snapshot = context(at, volatility=0)
            with self.subTest(at=at):
                if at <= first:
                    self.assertEqual(selected.price(note, snapshot), 1)
                else:
                    with self.assertRaises(MissingHistoryError):
                        selected.price(note, snapshot)
                knocked_in = note.replace_history(
                    barrier_state="knocked_in", history_time=at
                )
                if at <= first:
                    with self.assertRaises(InvalidInputError):
                        selected.price(knocked_in, snapshot)
                else:
                    self.assertEqual(selected.price(knocked_in, snapshot), 1)
                known = note.replace_history(barrier_state="no_event", history_time=at)
                self.assertEqual(selected.price(known, snapshot), 1)

    def test_average_snapshots_require_elapsed_averaging_at_construction_and_replacement(
        self,
    ):
        start = datetime(2025, 1, 2)
        averaging, end = start + timedelta(days=1), start + timedelta(days=10)
        for cls in (i.GeometricAveragePriceOption, i.ArithmeticAveragePriceOption):
            fields = {
                "option_type": "call",
                "strike": 100,
                "effective_time": start,
                "averaging_start_time": averaging,
                "expiry_time": end,
            }
            original = cls(**fields)
            for at in (start, averaging):
                with self.subTest(contract=cls.__name__, at=at):
                    with self.assertRaises(InvalidInputError):
                        cls(**fields, realized_average=100, history_time=at)
                    with self.assertRaises(InvalidInputError):
                        original.replace_history(realized_average=100, history_time=at)
            at = averaging + timedelta(microseconds=1)
            seasoned = original.replace_history(realized_average=100, history_time=at)
            self.assertEqual(seasoned.history_time, at)
            with self.assertRaises(InvalidInputError):
                seasoned.replace(averaging_start_time=at, realized_average=100)
            self.assertIsNone(original.realized_average)

    def test_redating_an_average_requires_explicit_history_and_preserves_the_price(
        self,
    ):
        start = datetime(2025, 1, 2)
        at, later = start + timedelta(days=4), start + timedelta(days=18)
        option = i.ArithmeticAveragePriceOption(
            option_type="call",
            strike=100,
            effective_time=start,
            averaging_start_time=start,
            expiry_time=start + timedelta(days=32),
            realized_average=100,
            history_time=at,
        )
        for method in (option.replace, option.replace_history):
            with self.assertRaises(StaleHistoryError):
                method(history_time=later)
        refreshed = option.replace_history(
            realized_average=(4 * 100 + 14 * 120) / 18,
            history_time=later,
        )
        market = context(later, volatility=0).replace(spot_price=120)
        self.assertAlmostEqual(
            engine(option).price(refreshed, market), 17.5, delta=1e-12
        )
        self.assertEqual(option.history_time, at)
        self.assertEqual(option.realized_average, 100)

    def test_clearing_history_clears_only_implicit_timestamps_in_every_family(self):
        names = (
            "touch_state",
            "barrier_state",
            "realized_average",
            "realized_hit_count",
            "accumulated_quantity",
            "knock_out_settled",
        )
        for original in contracts():
            if hasattr(original, "realized_average"):
                original = original.replace_history(
                    realized_average=100, history_time=AT
                )
            original = original.replace(history_time=AT)
            fields = {name: None for name in names if hasattr(original, name)}
            for method in ("replace", "replace_history"):
                with self.subTest(contract=type(original).__name__, method=method):
                    replace = getattr(original, method)
                    cleared = replace(**fields)
                    self.assertIsNone(cleared.history_time)
                    self.assertTrue(
                        all(getattr(cleared, name) is None for name in fields)
                    )
                    self.assertEqual(replace(), original)
                    self.assertEqual(
                        replace(**{name: ... for name in fields}), original
                    )
                    kept = replace(**fields, history_time=AT)
                    self.assertEqual(kept.history_time, AT)
                    self.assertEqual(kept.replace_history(), kept)
                    with self.assertRaises(InvalidInputError):
                        replace(
                            **fields,
                            history_time=original.expiry_time + timedelta(days=1),
                        )
                    self.assertEqual(original.history_time, AT)
                    self.assertEqual(pickle.loads(pickle.dumps(cleared)), cleared)

    def test_cleared_average_can_shorten_expiry_before_its_old_snapshot(self):
        start, at, end = (
            datetime(2025, 1, 2),
            datetime(2025, 2, 1),
            datetime(2026, 1, 2),
        )
        expiry = datetime(2025, 1, 15)
        for cls in (i.ArithmeticAveragePriceOption, i.GeometricAveragePriceOption):
            original = cls(
                option_type="call",
                strike=100,
                effective_time=start,
                averaging_start_time=start,
                expiry_time=end,
                realized_average=100,
                history_time=at,
            )
            with self.subTest(contract=cls.__name__):
                cleared = original.replace_history(realized_average=None)
                self.assertEqual(
                    cleared.replace(expiry_time=expiry).expiry_time, expiry
                )
                self.assertIsNone(
                    original.replace(
                        realized_average=None, expiry_time=expiry
                    ).history_time
                )
                with self.assertRaises(InvalidInputError):
                    original.replace(
                        realized_average=None, expiry_time=expiry, history_time=at
                    )
                self.assertEqual(original.history_time, at)

    def test_partial_accumulator_history_clears_retain_the_shared_timestamp(self):
        original = next(
            option for option in contracts() if isinstance(option, i.Accumulator)
        ).replace_history(
            accumulated_quantity=0, knock_out_settled=False, history_time=AT
        )
        for first, last in (
            ("accumulated_quantity", "knock_out_settled"),
            ("knock_out_settled", "accumulated_quantity"),
        ):
            for method in ("replace", "replace_history"):
                with self.subTest(first=first, method=method):
                    partial = getattr(original, method)(**{first: None})
                    self.assertEqual(partial.history_time, AT)
                    self.assertEqual(getattr(partial, last), getattr(original, last))
                    self.assertIsNone(
                        getattr(partial, method)(**{last: None}).history_time
                    )
        self.assertEqual(original.accumulated_quantity, 0)
        self.assertFalse(original.knock_out_settled)

    def test_dated_average_refresh_updates_value_and_time_atomically(self):
        start = datetime(2025, 1, 2)
        at, end = start + timedelta(days=4), start + timedelta(days=10)
        market = context(at, volatility=0)
        later = market.replace(valuation_time=at + timedelta(days=1))
        for cls, expected in (
            (i.GeometricAveragePriceOption, math.sqrt(104 * 100) - 100),
            (i.ArithmeticAveragePriceOption, 2),
        ):
            with self.subTest(contract=cls.__name__):
                original = cls(
                    option_type="call",
                    strike=100,
                    effective_time=start,
                    averaging_start_time=start,
                    expiry_time=end,
                    realized_average=100,
                    history_time=at,
                )
                refreshed = original.replace(
                    realized_average=104, history_time=later.valuation_time
                )
                self.assertEqual(refreshed.realized_average, 104)
                self.assertEqual(refreshed.history_time, later.valuation_time)
                self.assertAlmostEqual(
                    engine(original).price(refreshed, later), expected, delta=1e-12
                )
                with self.assertRaises(InvalidInputError):
                    original.replace(
                        realized_average=104, history_time=end + timedelta(days=1)
                    )
                self.assertEqual(original.realized_average, 100)
                self.assertEqual(original.history_time, at)
                self.assertEqual(engine(original).price(original, market), 0)

    def test_dated_history_survives_future_schedule_edits(self):
        past, future = datetime(2025, 1, 3), datetime(2025, 1, 8)
        for original in contracts():
            with self.subTest(contract=type(original).__name__):
                if hasattr(original, "touch_state"):
                    original = original.replace(
                        observation_times=[past, original.expiry_time],
                        touch_state=original.touch_state,
                        history_time=AT,
                    )
                    updated = original.replace(
                        observation_times=[past, AT, future, original.expiry_time]
                    )
                    self.assertEqual(
                        updated.observation_times,
                        (past, AT, future, original.expiry_time),
                    )
                    self.assertEqual(updated.touch_state, original.touch_state)
                    with self.assertRaises(InvalidInputError):
                        original.replace(
                            observation_times=[future, original.expiry_time]
                        )
                elif hasattr(original, "observations"):
                    record = original.observations[0]
                    original = original.replace(
                        observations=[
                            record.replace(time=past),
                            record.replace(time=original.expiry_time),
                        ],
                        barrier_state=original.barrier_state,
                        history_time=AT,
                    )
                    updated = original.replace(
                        observations=[
                            original.observations[0],
                            record.replace(time=AT),
                            record.replace(time=future),
                            original.observations[-1],
                        ]
                    )
                    self.assertEqual(
                        updated.observation_times,
                        (past, AT, future, original.expiry_time),
                    )
                    self.assertEqual(updated.barrier_state, original.barrier_state)
                    with self.assertRaises(InvalidInputError):
                        original.replace(observations=original.observations[1:])
                else:
                    continue
                self.assertEqual(updated.history_time, AT)
                self.assertEqual(
                    original.observation_times, (past, original.expiry_time)
                )
                extended = original.replace(
                    expiry_time=original.expiry_time + timedelta(days=1)
                )
                self.assertEqual(extended.history_time, AT)

    def test_dated_daily_history_survives_future_calendar_edits(self):
        for original in contracts():
            if not hasattr(original, "calendar") or hasattr(
                original, "observation_mode"
            ):
                continue
            if isinstance(original, i.BinarySnowballOption):
                continue
            original = original.replace(history_time=AT)
            with self.subTest(contract=type(original).__name__):
                future_calendar = custom_calendar(
                    base=original.calendar, closed_dates=[datetime(2025, 1, 8).date()]
                )
                updated = original.replace(calendar=future_calendar)
                self.assertEqual(updated.history_time, AT)
                self.assertEqual(updated.calendar, future_calendar)
                past_calendar = custom_calendar(
                    base=original.calendar, closed_dates=[datetime(2025, 1, 3).date()]
                )
                with self.assertRaises(InvalidInputError):
                    original.replace(calendar=past_calendar)

    def test_settled_history_must_be_possible_at_its_own_timestamp(self):
        start, end = datetime(2025, 1, 2), datetime(2025, 1, 10)
        option = i.Accumulator(
            strike=100,
            knock_out_level=120,
            daily_quantity=1,
            acceleration_factor=2,
            effective_time=start,
            expiry_time=end,
            observation_clock_time=time(15),
            knock_out_settled=True,
            history_time=start,
        )
        selected = engine(option)
        with self.assertRaises(InvalidInputError):
            selected.price(option, context())
        with self.assertRaises(InvalidInputError):
            selected.price(
                option.replace_history(
                    knock_out_settled=True,
                    accumulated_quantity=option.accumulated_quantity,
                    history_time=AT + timedelta(days=1),
                ),
                context(),
            )
        self.assertEqual(
            selected.price(
                option.replace_history(
                    knock_out_settled=True,
                    accumulated_quantity=option.accumulated_quantity,
                    history_time=AT,
                ),
                context(),
            ),
            0,
        )
        note = i.BinarySnowballOption(
            maturity_coupon_rate=0.1,
            effective_time=start,
            expiry_time=end,
            observations=[
                i.AutocallableObservation.snowball(
                    datetime(2025, 1, 3), 110, coupon_rate=0
                )
            ],
            barrier_state="knocked_out",
            history_time=start,
        )
        selected = engine(note)
        with self.assertRaises(InvalidInputError):
            selected.price(note, context())
        self.assertEqual(
            selected.price(
                note.replace_history(barrier_state=note.barrier_state, history_time=AT),
                context(),
            ),
            0,
        )

    def test_every_path_dependent_family_requires_dated_history(self):
        for factory, fields in contract_inputs():
            if "realized_average" in fields:
                fields["realized_average"] = 100
            if "observations" in fields:
                fields["observations"] = [
                    fields["observations"][0].replace(time=AT - timedelta(days=3)),
                    *fields["observations"],
                ]
            with self.subTest(contract=factory.__name__):
                for omitted in ({}, {"history_time": None}):
                    with self.assertRaises(MissingHistoryError):
                        factory(**fields, **omitted)
                original = factory(**fields, history_time=AT)
                selected = engine(original)
                dated = original.replace(history_time=AT)
                self.assertTrue(math.isfinite(selected.price(dated, context())))
                payloads = {
                    name: None
                    for name in (
                        "touch_state",
                        "barrier_state",
                        "realized_average",
                        "realized_hit_count",
                        "accumulated_quantity",
                        "knock_out_settled",
                    )
                    if hasattr(dated, name)
                }
                missing = dated.replace_history(**payloads)
                with self.assertRaises(MissingHistoryError):
                    selected.price(missing, context())
                with self.assertRaises(StaleHistoryError):
                    dated.replace(history_time=AT - timedelta(microseconds=1))
                self.assertEqual(original.history_time, AT)
                self.assertEqual(dated.history_time, AT)
                self.assertEqual(pickle.loads(pickle.dumps(dated)), dated)
                self.assertEqual(dated.replace(), dated)
                self.assertEqual(dated.replace_history(), dated)
                for method in (dated.replace, dated.replace_history):
                    with self.assertRaises(StaleHistoryError):
                        method(history_time=AT + timedelta(microseconds=1))
                with self.assertRaises(InvalidInputError):
                    selected.price(dated.replace(history_time=None), context())
                for invalid in (
                    original.effective_time - timedelta(microseconds=1),
                    original.expiry_time + timedelta(microseconds=1),
                ):
                    with self.assertRaises(InvalidInputError):
                        dated.replace(history_time=invalid)
                    with self.assertRaises(InvalidInputError):
                        dated.replace_history(history_time=invalid)
                for boundary in (original.effective_time, original.expiry_time):
                    self.assertEqual(
                        dated.replace(**payloads, history_time=boundary).history_time,
                        boundary,
                    )
                self.assertEqual(dated.history_time, AT)
                for bad in (True, AT.date(), AT.replace(tzinfo=UTC)):
                    with self.assertRaises(TypeError):
                        original.replace(history_time=bad)

    def test_history_construction_rejects_timestamps_outside_contract_life(self):
        start, end = datetime(2025, 1, 2), datetime(2025, 1, 10)
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_time": start,
            "expiry_time": end,
            "barrier_level": 120,
            "barrier_type": "up_and_out",
            "touch_state": "untouched",
        }
        for at in (start - timedelta(microseconds=1), end + timedelta(microseconds=1)):
            with self.assertRaises(InvalidInputError):
                i.BarrierOption(**terms, history_time=at)
        for at in (start, end):
            self.assertEqual(i.BarrierOption(**terms, history_time=at).history_time, at)

    def test_history_time_and_contract_life_are_replaced_together(self):
        start, end = datetime(2025, 1, 2), datetime(2025, 1, 10)
        option = i.BarrierOption(
            option_type="call",
            strike=100,
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_out",
            touch_state="untouched",
            history_time=AT,
        )
        expiry = start + timedelta(days=1)
        with self.assertRaises(InvalidInputError):
            option.replace(expiry_time=expiry, touch_state="untouched")
        replaced = option.replace(
            expiry_time=expiry, touch_state="untouched", history_time=expiry
        )
        self.assertEqual(replaced.history_time, expiry)
        self.assertEqual(replaced.expiry_time, expiry)
        self.assertEqual(option.history_time, AT)
        self.assertEqual(option.expiry_time, end)

    def test_discrete_history_reuses_between_events_and_requires_refresh_after_them(
        self,
    ):
        option = i.RangeAccrualOption(
            notional=100,
            annual_coupon_rate=0.1,
            lower_bound=90,
            upper_bound=110,
            effective_time=datetime(2025, 1, 2),
            expiry_time=datetime(2025, 1, 10),
            observation_clock_time=time(15),
            realized_hit_count=1,
            history_time=AT,
        )
        selected = engine(option)
        market = context(volatility=0)
        price = selected.price(option, market)
        event = AT.replace(hour=15)
        for at in (AT, event - timedelta(microseconds=1), event):
            self.assertEqual(
                selected.price(option, market.replace(valuation_time=at)), price
            )
        rolled = market.replace(valuation_time=event + timedelta(microseconds=1))
        with self.assertRaises(InvalidInputError):
            selected.price(option, rolled)
        refreshed = option.replace_history(
            realized_hit_count=2, history_time=rolled.valuation_time
        )
        self.assertEqual(selected.price(refreshed, rolled), price)
        self.assertEqual(option.history_time, AT)
        with self.assertRaises(InvalidInputError):
            selected.price(option, market.replace(valuation_time=option.effective_time))

    def test_continuous_history_rejects_reuse_but_numerical_time_risk_keeps_it_frozen(
        self,
    ):
        for cls, factory in (
            (i.GeometricAveragePriceOption, p.AnalyticGeometricAveragePriceEngine),
            (
                i.ArithmeticAveragePriceOption,
                p.TurnbullWakemanArithmeticAveragePriceEngine,
            ),
        ):
            option = cls(
                option_type="call",
                strike=100,
                effective_time=datetime(2025, 1, 2),
                averaging_start_time=datetime(2025, 1, 2),
                expiry_time=datetime(2025, 2, 2),
                realized_average=100,
                history_time=AT,
            )
            selected = factory()
            market = context()
            self.assertTrue(
                math.isfinite(
                    selected.price_with_greeks(option, market, "theta").require("theta")
                )
            )
            rolled = market.replace(valuation_time=AT + timedelta(microseconds=1))
            with self.assertRaises(InvalidInputError):
                selected.price(option, rolled)
            self.assertTrue(
                math.isfinite(
                    selected.price(
                        option.replace_history(
                            realized_average=100, history_time=rolled.valuation_time
                        ),
                        rolled,
                    )
                )
            )
            self.assertEqual(option.history_time, AT)
