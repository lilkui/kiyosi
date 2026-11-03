"""Spot defaults remain explicit and replayable for Monte Carlo barrier Greeks."""

import inspect
import unittest
from datetime import date

from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p


class SpotShiftDefaultTests(unittest.TestCase):
    def test_barrier_greek_signature_defaults_can_be_forwarded(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        vanilla = {
            "option_type": "call",
            "strike": 100,
            "effective_date": start,
            "expiry_date": end,
        }
        context = m.PricingContext(
            model_parameters=m.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for engine, option in (
            (
                p.MonteCarloBarrierEngine(path_count=64, seed=42),
                i.BarrierOption(
                    **vanilla, barrier_level=120, barrier_type="up_and_out"
                ),
            ),
            (
                p.MonteCarloDoubleBarrierEngine(path_count=64, seed=42),
                i.DoubleBarrierOption(
                    **vanilla,
                    lower_barrier=80,
                    upper_barrier=120,
                    barrier_type="knock_out",
                ),
            ),
        ):
            with self.subTest(engine=type(engine).__name__):
                default = (
                    inspect.signature(engine.price_with_greeks)
                    .parameters["spot_shift"]
                    .default
                )
                self.assertEqual(default, 0.01)
                expected = engine.price_with_greeks(option, context, "delta")
                actual = engine.price_with_greeks(
                    option, context, "delta", spot_shift=default
                )
                self.assertEqual(actual.delta, expected.delta)

    def test_automatic_american_bump_matches_explicit_core_control(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = i.AmericanOption(
            option_type="put",
            strike=100,
            effective_date=start,
            expiry_date=end,
        )
        context = m.PricingContext(
            model_parameters=m.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.03, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = p.MonteCarloVanillaEngine(
            path_count=8192, step_count=19, seed=42, thread_count=1
        )
        result = engine.price_with_greeks(option, context, "gamma")
        explicit = engine.price_with_greeks(option, context, "gamma", spot_shift=8.0)
        self.assertEqual(result.require("gamma"), explicit.require("gamma"))
