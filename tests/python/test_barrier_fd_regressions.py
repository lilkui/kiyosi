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


if __name__ == "__main__":
    unittest.main()
