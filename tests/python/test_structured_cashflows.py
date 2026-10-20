import math
import unittest
from datetime import UTC, date, datetime

from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    BinarySnowballOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)

START, FIRST, MIDDLE, END = (
    date(2025, 1, 1),
    date(2025, 1, 11),
    date(2025, 1, 21),
    date(2025, 1, 31),
)


def market(spot, valuation, volatility=0.2, rate=0.04, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
        calendar=all_days_calendar(),
    )


class StructuredCashflowTests(unittest.TestCase):
    def check_autocallable(self, note, context, expected):
        for engine in (
            pricing.MonteCarloAutocallableEngine(path_count=32, seed=7),
            pricing.FiniteDifferenceAutocallableEngine(),
            pricing.QuadratureAutocallableEngine(),
        ):
            with self.subTest(
                engine=type(engine).__name__, product=type(note).__name__
            ):
                self.assertAlmostEqual(
                    engine.price(note, context), expected, delta=1e-12
                )

    def test_current_knockouts_pay_only_contractual_coupon(self):
        common = {"effective_date": START, "expiry_date": END}
        observations = autocallable_observations(
            [FIRST, MIDDLE, END],
            [100, 100, 100],
            knock_out_coupon_rates=[-0.365, 100, 100],
        )
        strikes = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
        notes = (
            BinarySnowballOption(
                **common,
                maturity_coupon_rate=5,
                barrier_state="none",
                observations=observations,
            ),
            SnowballOption(
                **common,
                **strikes,
                maturity_coupon_rate=5,
                knock_in_level=80,
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in",
                observations=observations,
            ),
            TernarySnowballOption(
                **common,
                maturity_coupon_rate=5,
                minimum_coupon_rate=-1,
                knock_in_level=80,
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in",
                observations=observations,
            ),
        )
        for note in notes:
            self.check_autocallable(note, market(100, FIRST), 0.99)
        for barrier, expected in ((100, 0.99), (math.nextafter(100, 200), 1)):
            note = PhoenixOption(
                **common,
                **strikes,
                coupon_rate=-0.365,
                knock_in_level=80,
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in",
                observations=autocallable_observations(
                    [FIRST, END], [100, 100], coupon_barrier_levels=[barrier, 0]
                ),
            )
            self.check_autocallable(note, market(100, FIRST), expected)

    def test_historical_knockin_and_terminal_caps(self):
        common = {
            "initial_spot": 100,
            "knock_in_level": 80,
            "knock_in_observation_mode": "every_trading_day",
            "upper_strike": 100,
            "lower_strike": 60,
            "barrier_state": "knocked_in",
            "principal_ratio": 1.25,
            "effective_date": START,
            "expiry_date": END,
        }
        snowball = SnowballOption(
            **common,
            maturity_coupon_rate=5,
            observations=autocallable_observations(
                [END], [110], knock_out_coupon_rates=[100]
            ),
        )
        phoenix = PhoenixOption(
            **common,
            coupon_rate=-0.365,
            observations=autocallable_observations(
                [FIRST, END], [110, 110], coupon_barrier_levels=[90, 90]
            ),
        )
        for spot, expected in ((50, 0.85), (90, 1.15), (105, 1.25)):
            self.check_autocallable(snowball, market(spot, END), expected)
            self.check_autocallable(
                phoenix, market(spot, END), expected - (0.02 if spot >= 90 else 0)
            )

    def test_accumulator_midnight_and_intraday_purchase_cutoff(self):
        end = date(2025, 1, 2)
        option = Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
            effective_date=START,
            expiry_date=end,
        )
        engines = (
            pricing.MonteCarloAccumulatorEngine(path_count=32, seed=7),
            pricing.FiniteDifferenceAccumulatorEngine(),
            pricing.QuadratureAccumulatorEngine(),
        )
        for valuation, expected in (
            (START, -70),
            (datetime(2025, 1, 1, 12, tzinfo=UTC), -50),
            (end, -50),
        ):
            context = market(90, valuation, volatility=1e-8, rate=0, dividend=0)
            for engine in engines:
                with self.subTest(valuation=valuation, engine=type(engine).__name__):
                    self.assertAlmostEqual(
                        engine.price(option, context), expected, delta=1e-5
                    )
        for barrier in (90, 110):
            current = Accumulator(
                strike=100,
                knock_out_level=barrier,
                daily_quantity=1,
                acceleration_factor=2,
                accumulated_quantity=3,
                effective_date=START,
                expiry_date=end,
            )
            for engine in engines:
                with self.subTest(barrier=barrier, engine=type(engine).__name__):
                    self.assertEqual(
                        engine.price(current, market(barrier, START)),
                        3 * (barrier - 100),
                    )


if __name__ == "__main__":
    unittest.main()
