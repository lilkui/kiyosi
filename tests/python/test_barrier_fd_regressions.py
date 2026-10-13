import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import BarrierOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class BarrierFiniteDifferenceRegressionTests(unittest.TestCase):
    def test_fixed_tail_preserves_prices_and_greeks_near_unaligned_barrier(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=119.25,
            valuation_time=start,
        )
        # Full-grid rollback values before pruning fixed nodes.
        cases = (
            (
                "explicit_euler",
                "at_hit",
                2.000449648078252,
                -0.16259424530160185,
                0.004016237591921313,
                -0.0006700352600059977,
            ),
            (
                "explicit_euler",
                "at_expiry",
                1.952864994673153,
                -0.16586073926952621,
                0.0038678679649788023,
                -0.0006722623554978103,
            ),
            (
                "implicit_euler",
                "at_hit",
                2.0000939411154146,
                -0.16248433546958022,
                0.004017016770560959,
                -0.0006707588426657158,
            ),
            (
                "implicit_euler",
                "at_expiry",
                1.9525078708121677,
                -0.1657503847976201,
                0.0038686459923200434,
                -0.0006729886498259832,
            ),
            (
                "crank_nicolson",
                "at_hit",
                2.000271802991153,
                -0.16253929287199972,
                0.0040166270935115345,
                -0.0006703970085115406,
            ),
            (
                "crank_nicolson",
                "at_expiry",
                1.9526864412157319,
                -0.16580556454205944,
                0.0038682568893013626,
                -0.0006726254592109876,
            ),
        )
        for scheme, timing, price, delta, gamma, speed in cases:
            with self.subTest(scheme=scheme, timing=timing):
                option = BarrierOption(
                    option_type="put",
                    strike=100,
                    effective_date=start,
                    expiry_date=end,
                    barrier_level=121.1,
                    barrier_type="up_and_out",
                    rebate=1.5,
                    rebate_timing=timing,
                )
                result = pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=200,
                    time_step_count=2000,
                    scheme=scheme,
                    asset_upper_boundary=480,
                ).price_with_greeks(option, context, ["delta", "gamma", "speed"])
                for actual, expected in zip(
                    (result.price, result.delta, result.gamma, result.speed),
                    (price, delta, gamma, speed),
                ):
                    self.assertAlmostEqual(
                        actual, expected, delta=max(abs(expected) * 1e-12, 1e-14)
                    )


if __name__ == "__main__":
    unittest.main()
