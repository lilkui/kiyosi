import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import BinarySnowballOption, SnowballOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)


class StructuredRegressionTests(unittest.TestCase):
    def test_event_only_monte_carlo_ignores_unobserved_trading_days(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0, volatility=0.3
        )
        for observations in ([date(2025, 7, 1)], [date(2025, 7, 1), end]):
            terms = {
                "knock_out_coupon_rates": [0.08] * len(observations),
                "maturity_coupon_rate": 0.04,
                "knock_out_levels": [110] * len(observations),
                "observation_dates": observations,
                "effective_date": start,
                "expiry_date": end,
            }
            for mode in ("at_expiry", "every_trading_day"):
                snowball = SnowballOption(
                    **terms,
                    initial_spot=100,
                    knock_in_level=90,
                    upper_strike=100,
                    lower_strike=60,
                    knock_in_observation_mode=mode,
                )
                for note, engine in (
                    (
                        BinarySnowballOption(**terms),
                        pricing.MonteCarloBinarySnowballEngine(
                            path_count=2000, seed=42
                        ),
                    ),
                    (
                        snowball,
                        pricing.MonteCarloSnowballEngine(path_count=2000, seed=42),
                    ),
                ):
                    with self.subTest(
                        observations=observations, note=type(note).__name__, mode=mode
                    ):
                        prices = [
                            engine.price(
                                note,
                                PricingContext(
                                    model_parameters=parameters,
                                    spot_price=100,
                                    valuation_time=start,
                                    calendar=calendar,
                                ),
                            )
                            for calendar in (weekdays_calendar(), all_days_calendar())
                        ]
                        if (
                            isinstance(note, SnowballOption)
                            and mode == "every_trading_day"
                        ):
                            self.assertNotEqual(*prices)
                        else:
                            self.assertEqual(*prices)
