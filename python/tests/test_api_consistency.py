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

    def test_greek_status_distinguishes_requests_support_and_stencils(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=start, expiry_time=end
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        result = pricing.AnalyticEuropeanEngine().price_with_greeks(
            option, context, "delta"
        )
        self.assertEqual(result.status("delta"), "available")
        self.assertEqual(result.status("gamma"), "not_requested")
        self.assertEqual(result.requested, ["delta"])
        terminal = pricing.AnalyticEuropeanEngine().price_with_greeks(
            option, context.replace(valuation_time=end), "delta"
        )
        self.assertEqual(terminal.status("delta"), "undefined")
        collapsed = pricing.QuadratureVanillaEngine().price_with_greeks(
            option, context, "delta", spot_shift=400
        )
        self.assertEqual(collapsed.status("delta"), "undefined")
        heston = context.replace(
            model_parameters=market.HestonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                initial_variance=0.04,
                long_run_variance=0.04,
                mean_reversion_speed=1.5,
                vol_of_vol=0.3,
                correlation=-0.7,
            )
        )
        unsupported = pricing.AnalyticHestonEuropeanEngine().price_with_greeks(
            option, heston, "delta"
        )
        self.assertEqual(unsupported.status("delta"), "unsupported")
        self.assertEqual(unsupported.status("gamma"), "not_requested")
        local = context.replace(
            model_parameters=market.LocalVolatilityParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                surface=market.LocalVolatilitySurface(
                    times=[start, end], spots=[80, 120], values=[[0.2, 0.2], [0.2, 0.2]]
                ),
            )
        )
        partial = pricing.FiniteDifferenceVanillaEngine().price_with_greeks(
            option, local, ["delta", "vega"]
        )
        self.assertEqual(partial.status("delta"), "available")
        self.assertEqual(partial.status("vega"), "unsupported")
        self.assertEqual(partial.requested, ["delta", "vega"])
        for original in (result, terminal, collapsed, unsupported, partial):
            restored = pickle.loads(pickle.dumps(original))
            self.assertEqual(restored.price, original.price)
            self.assertEqual(restored.requested, original.requested)
            self.assertEqual(restored.supported, original.supported)
            for greek in ("delta", "gamma", "vega"):
                self.assertEqual(restored.status(greek), original.status(greek))
                self.assertEqual(getattr(restored, greek), getattr(original, greek))
        with self.assertRaises(ValueError):
            result.status("unknown")
