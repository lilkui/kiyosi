import unittest
from datetime import date

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import BinarySnowballOption, PhoenixOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)


class ObservationRecordTests(unittest.TestCase):
    def test_observation_records_preserve_associations_and_own_python_sequences(self):
        start, first, end = date(2025, 1, 1), date(2025, 1, 11), date(2025, 1, 31)
        dates, levels, coupons, barriers = (
            [first, end],
            [110, 105],
            [0.365, 0.73],
            [108, 102],
        )
        common = {
            "knock_out_levels": levels,
            "observation_dates": dates,
            "barrier_state": "none",
            "effective_date": start,
            "expiry_date": end,
        }
        snowball = BinarySnowballOption(
            **common,
            knock_out_coupon_rates=coupons,
            maturity_coupon_rate=0.05,
        )
        phoenix = PhoenixOption(
            **common,
            coupon_rate=0.365,
            initial_spot=100,
            knock_in_level=70,
            upper_strike=100,
            lower_strike=60,
            coupon_barrier_levels=barriers,
            knock_in_observation_mode="every_trading_day",
        )
        dates.reverse()
        levels.reverse()
        coupons.reverse()
        barriers.reverse()
        for note in (snowball, phoenix):
            self.assertEqual(note.observation_dates, [first, end])
            self.assertEqual(note.knock_out_levels, [110, 105])
            note.observation_dates.reverse()
            note.knock_out_levels.reverse()
            self.assertEqual(note.observation_dates, [first, end])
            self.assertEqual(note.knock_out_levels, [110, 105])
        self.assertEqual(snowball.knock_out_coupon_rates, [0.365, 0.73])
        self.assertEqual(phoenix.coupon_barrier_levels, [108, 102])
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04,
                dividend_yield=0.01,
                volatility=0.2,
            ),
            spot_price=106,
            valuation_time=end,
            calendar=all_days_calendar(),
        )
        for engine in (
            pricing.FiniteDifferenceBinarySnowballEngine(),
            pricing.MonteCarloBinarySnowballEngine(path_count=8, seed=7),
        ):
            self.assertAlmostEqual(engine.price(snowball, context), 1.06, delta=1e-14)
        for engine in (
            pricing.FiniteDifferencePhoenixEngine(),
            pricing.MonteCarloPhoenixEngine(path_count=8, seed=7),
        ):
            self.assertAlmostEqual(engine.price(phoenix, context), 1.02, delta=1e-14)
        for principal, category in (
            (-1, kiyosi.ErrorCategory.INVALID_PARAMETER),
            (1, kiyosi.ErrorCategory.INVALID_SCHEDULE),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                BinarySnowballOption(
                    **{**common, "observation_dates": [first, end]},
                    knock_out_coupon_rates=[0.365],
                    maturity_coupon_rate=0.05,
                    principal_ratio=principal,
                )
            self.assertEqual(error.exception.category, category)
