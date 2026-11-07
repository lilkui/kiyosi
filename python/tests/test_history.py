"""History snapshots have an explicit valuation time and cannot silently go stale."""

import math
import pickle
import unittest
from datetime import UTC, datetime, time, timedelta

from kiyosi import InvalidInputError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from test_contract_replacements import contracts

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
                option.replace(history_time=AT + timedelta(days=1)), context()
            )
        self.assertEqual(selected.price(option.replace(history_time=AT), context()), 0)
        note = i.BinarySnowballOption(
            maturity_coupon_rate=0.1,
            effective_time=start,
            expiry_time=end,
            observations=[i.AutocallableObservation(datetime(2025, 1, 3), 110)],
            barrier_state="knocked_out",
            history_time=start,
        )
        selected = engine(note)
        with self.assertRaises(InvalidInputError):
            selected.price(note, context())
        self.assertEqual(selected.price(note.replace(history_time=AT), context()), 0)

    def test_every_path_dependent_family_requires_dated_history(self):
        for original in contracts():
            if hasattr(original, "realized_average"):
                original = original.replace_history(realized_average=100)
            with self.subTest(contract=type(original).__name__):
                selected = engine(original)
                with self.assertRaises(InvalidInputError):
                    selected.price(original, context())
                dated = original.replace(history_time=AT)
                self.assertTrue(math.isfinite(selected.price(dated, context())))
                self.assertIsNone(original.history_time)
                self.assertEqual(dated.history_time, AT)
                self.assertEqual(pickle.loads(pickle.dumps(dated)), dated)
                self.assertEqual(dated.replace(), dated)
                self.assertEqual(dated.replace_history(), dated)
                with self.assertRaises(InvalidInputError):
                    selected.price(
                        dated.replace(history_time=AT + timedelta(microseconds=1)),
                        context(),
                    )
                with self.assertRaises(InvalidInputError):
                    selected.price(dated.replace(history_time=None), context())
                with self.assertRaises(InvalidInputError):
                    selected.price(
                        dated.replace(
                            history_time=original.effective_time
                            - timedelta(microseconds=1)
                        ),
                        context(),
                    )
                for bad in (True, AT.date(), AT.replace(tzinfo=UTC)):
                    with self.assertRaises(TypeError):
                        original.replace(history_time=bad)

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
                        option.replace_history(history_time=rolled.valuation_time),
                        rolled,
                    )
                )
            )
            self.assertEqual(option.history_time, AT)
