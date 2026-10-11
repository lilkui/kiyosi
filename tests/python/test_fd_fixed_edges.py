import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import BarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class FixedEdgeTests(unittest.TestCase):
    def test_continuous_barrier_fixed_edges_retain_off_node_greeks(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=end,
            barrier_level=110,
            barrier_type="up_and_out",
            rebate=2,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=start,
        )
        for scheme, expected in (
            (
                "crank_nicolson",
                (
                    1.374681875919217,
                    0.057342386651520846,
                    7.95571374527515e-6,
                    -9.302439092745403e-5,
                ),
            ),
            (
                "implicit_euler",
                (
                    1.374383050055775,
                    0.057379104672503844,
                    6.7888065698046285e-6,
                    -9.315599817910664e-5,
                ),
            ),
            (
                "explicit_euler",
                (
                    1.3747121867706569,
                    0.057338758234461294,
                    8.050051664994229e-6,
                    -9.301081278631506e-5,
                ),
            ),
        ):
            with self.subTest(scheme=scheme):
                engine = pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=200,
                    time_step_count=2000 if scheme == "explicit_euler" else 200,
                    scheme=scheme,
                    asset_upper_boundary=400,
                )
                result = engine.price_with_greeks(
                    option, context, ["delta", "gamma", "speed"]
                )
                self.assertEqual(engine.price(option, context), result.price)
                for name, value in zip(("price", "delta", "gamma", "speed"), expected):
                    self.assertAlmostEqual(getattr(result, name), value, delta=1e-11)
