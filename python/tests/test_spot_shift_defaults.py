"""Spot defaults remain explicit and replayable for Monte Carlo barrier Greeks."""

import inspect
import unittest
from datetime import datetime

from kiyosi import InvalidInputError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from kiyosi.pricing import numerical_greeks


class SpotShiftDefaultTests(unittest.TestCase):
    def test_barrier_greek_signature_defaults_can_be_forwarded(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        vanilla = {
            "option_type": "call",
            "strike": 100,
            "effective_time": start,
            "expiry_time": end,
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
                    inspect.signature(numerical_greeks).parameters["spot_shift"].default
                )
                self.assertIsNone(default)
                expected = numerical_greeks(engine, option, context, "delta")
                actual = numerical_greeks(
                    engine, option, context, "delta", spot_shift=default
                )
                self.assertEqual(actual.delta, expected.delta)
                explicit = numerical_greeks(
                    engine, option, context, "delta", spot_shift=0.01
                )
                self.assertEqual(actual.delta, explicit.delta)
                near = context.replace(spot_price=119.995)
                default_native = engine.price_with_greeks(
                    option, near, ["delta", "gamma"]
                )
                self.assertEqual(default_native.status("delta"), "stencil_unavailable")
                tuned = engine.price_with_greeks(
                    option, near, ["delta", "gamma"], spot_shift=0.0001
                )
                tighter = engine.price_with_greeks(
                    option, near, ["delta", "gamma"], spot_shift=0.00005
                )
                self.assertEqual(tuned.status("delta"), "available")
                self.assertLess(tuned.require("delta"), 0)
                self.assertEqual(tuned, tighter)
                self.assertEqual(tuned.price, engine.price(option, near))
                for invalid in (0, -1, float("inf"), float("nan")):
                    with self.assertRaises(InvalidInputError):
                        engine.price_with_greeks(
                            option, near, "delta", spot_shift=invalid
                        )
                for invalid in (True, "0.01"):
                    with self.assertRaises(TypeError):
                        engine.price_with_greeks(
                            option, near, "delta", spot_shift=invalid
                        )

    def test_automatic_american_bump_matches_explicit_core_control(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = i.AmericanOption(
            option_type="put",
            strike=100,
            effective_time=start,
            expiry_time=end,
        )
        context = m.PricingContext(
            model_parameters=m.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.03, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = p.MonteCarloVanillaEngine(
            path_count=8192, step_count=19, seed=42, max_active_workers=1
        )
        result = engine.price_with_greeks(option, context, "gamma")
        explicit = numerical_greeks(engine, option, context, "gamma", spot_shift=8.0)
        self.assertEqual(result.require("gamma"), explicit.require("gamma"))
