import math
import unittest
from datetime import UTC, date, datetime
from itertools import product

from kiyosi import pricing
from kiyosi.instruments import BarrierOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)


class BarrierFiniteDifferenceRegressionTests(unittest.TestCase):
    def test_scheduled_edges_retain_next_observation_settlement(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        valuation = datetime(2025, 1, 1, 12, tzinfo=UTC)
        for scheme, up, timing, rebate, observes_expiry in product(
            ("explicit_euler", "implicit_euler", "crank_nicolson"),
            (False, True),
            ("at_hit", "at_expiry"),
            (0, 2),
            (False, True),
        ):
            with self.subTest(
                scheme=scheme,
                up=up,
                timing=timing,
                rebate=rebate,
                observes_expiry=observes_expiry,
            ):
                dates = [date(2025, 1, 2)]
                if observes_expiry:
                    dates.append(end)
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                    ),
                    spot_price=150 if up else 1,
                    valuation_time=valuation,
                    calendar=all_days_calendar(),
                )
                option = BarrierOption(
                    option_type="call" if up else "put",
                    strike=100,
                    effective_date=start,
                    expiry_date=end,
                    barrier_level=110 if up else 90,
                    barrier_type="up_and_out" if up else "down_and_out",
                    rebate=rebate,
                    rebate_timing=timing,
                    observation_mode="scheduled",
                    observation_dates=dates,
                )
                engine = pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=200,
                    time_step_count=2000 if scheme == "explicit_euler" else 200,
                    scheme=scheme,
                    asset_upper_boundary=151 if up else None,
                )
                payment = dates[0] if timing == "at_hit" else end
                elapsed = (
                    datetime.combine(payment, datetime.min.time(), UTC) - valuation
                ).total_seconds() / (365 * 86400)
                self.assertAlmostEqual(
                    engine.price(option, context),
                    rebate * math.exp(-0.04 * elapsed),
                    delta=1e-6,
                )

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
