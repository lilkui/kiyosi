import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import BarrierOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)


class FiniteDifferenceInterpolationTests(unittest.TestCase):
    def test_scheduled_barrier_peak_preserves_price_and_greeks(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        terms = {"option_type": "call", "effective_date": start, "expiry_date": end}
        option = BarrierOption(
            **terms,
            strike=100,
            barrier_level=120,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_dates=[end],
        )
        lower = EuropeanOption(**terms, strike=100)
        upper = EuropeanOption(**terms, strike=120)
        cash = CashOrNothingOption(**terms, strike=120, payout=20)
        engine = pricing.FiniteDifferenceBarrierEngine(asset_upper_boundary=400)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        )

        def market(spot):
            return PricingContext(
                model_parameters=parameters,
                spot_price=spot,
                valuation_time=start,
                calendar=all_days_calendar(),
            )

        for spot in (111.94, 111.97, 112.01):
            with self.subTest(spot=spot):
                context = market(spot)
                result = engine.price_with_greeks(option, context, ["delta", "gamma"])
                reference = [
                    pricing.AnalyticVanillaEngine().price_with_greeks(
                        lower, context, ["delta", "gamma"]
                    ),
                    pricing.AnalyticVanillaEngine().price_with_greeks(
                        upper, context, ["delta", "gamma"]
                    ),
                    pricing.AnalyticDigitalEngine().price_with_greeks(
                        cash, context, ["delta", "gamma"]
                    ),
                ]
                for name, tolerance in (
                    ("price", 0.001),
                    ("delta", 5e-5),
                    ("gamma", 1e-5),
                ):
                    expected = (
                        getattr(reference[0], name)
                        - getattr(reference[1], name)
                        - getattr(reference[2], name)
                    )
                    self.assertAlmostEqual(
                        getattr(result, name), expected, delta=tolerance
                    )
                bump = 0.01
                high = engine.price(option, market(spot + bump))
                low = engine.price(option, market(spot - bump))
                self.assertAlmostEqual(
                    result.delta, (high - low) / (2 * bump), delta=1e-7
                )
                self.assertAlmostEqual(
                    result.gamma,
                    (high - 2 * result.price + low) / bump**2,
                    delta=1e-6,
                )


if __name__ == "__main__":
    unittest.main()
