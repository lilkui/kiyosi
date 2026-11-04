"""Public API contracts shared by Rust and Python callers."""

import pickle
import unittest
from datetime import datetime

from kiyosi import UnsupportedOperationError, market, pricing
from kiyosi import instruments as ins


class ApiConsistencyTests(unittest.TestCase):
    def test_terminated_accumulators_have_no_remaining_cashflows(self):
        from kiyosi import InvalidInputError

        start, end = datetime(2025, 1, 1), datetime(2025, 1, 10)
        option = ins.Accumulator(
            strike=90,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            effective_time=start,
            expiry_time=end,
        )
        settled = option.replace_history(terminated=True)
        self.assertFalse(option.terminated)
        self.assertTrue(settled.replace(strike=95).terminated)
        self.assertTrue(settled.replace_history(accumulated_quantity=3).terminated)
        self.assertEqual(pickle.loads(pickle.dumps(settled)), settled)
        self.assertTrue(
            ins.Accumulator(
                strike=90,
                knock_out_level=110,
                daily_quantity=1,
                acceleration_factor=2,
                effective_time=start,
                expiry_time=end,
                terminated=True,
            ).terminated
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime(2025, 1, 3),
            calendar=market.all_days_calendar(),
        )
        for factory in (
            pricing.QuadratureAccumulatorEngine,
            pricing.FiniteDifferenceAccumulatorEngine,
            pricing.MonteCarloAccumulatorEngine,
        ):
            engine = factory()
            with self.subTest(engine=factory.__name__):
                self.assertEqual(engine.price(settled, context), 0)
                self.assertEqual(
                    engine.price(
                        settled.replace_history(accumulated_quantity=3), context
                    ),
                    0,
                )
                active = settled.replace_history(
                    terminated=False, accumulated_quantity=0
                )
                self.assertGreater(engine.price(active, context), 0)
                with self.assertRaises(InvalidInputError):
                    engine.price(settled.replace_history(terminated=False), context)
                result = engine.price_with_greeks(settled, context, "all")
                for greek in ("delta", "gamma", "theta", "vega", "rho"):
                    self.assertEqual(result.require(greek), 0)
                with self.assertRaises(UnsupportedOperationError):
                    pricing.implied_volatility(engine, settled, context, 0)
        for invalid in (None, 0, 1, "yes"):
            with self.subTest(invalid=invalid), self.assertRaises(TypeError):
                option.replace_history(terminated=invalid)
