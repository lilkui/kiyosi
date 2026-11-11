import math
from datetime import datetime
from itertools import product

import pytest
from kiyosi import pricing
from kiyosi.instruments import BarrierOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
)


def test_continuous_single_barriers_retain_exact_levels_near_knock_out(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    parameters = BlackScholesMertonParameters(
        risk_free_rate=0.04, dividend_yield=0, volatility=0.2
    )
    for barrier, knock_in, rebate, distance in product(
        (120, 120.13, 80, 79.87), (False, True), (0, 2), (0.01, 1e-6, 0)
    ):
        with subtests.test(
            barrier=barrier, knock_in=knock_in, rebate=rebate, distance=distance
        ):
            up = barrier > 100
            direction = "up" if up else "down"
            activation = "in" if knock_in else "out"
            option = BarrierOption(
                option_type="call" if up else "put",
                strike=100,
                effective_time=start,
                expiry_time=end,
                barrier_level=barrier,
                barrier_type=f"{direction}_and_{activation}",
                rebate=rebate,
            )
            context = PricingContext(
                model_parameters=parameters,
                spot_price=barrier + (-distance if up else distance),
                valuation_time=start,
            )
            expected = pricing.AnalyticBarrierEngine().price(option, context)
            actual = pricing.FiniteDifferenceBarrierEngine().price(option, context)
            assert actual == pytest.approx(
                expected, rel=0, abs=1e-9 + 0.005 * abs(expected)
            )


def test_scheduled_edges_retain_next_observation_settlement(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    valuation = datetime(2025, 1, 1, 12)
    for scheme, up, timing, rebate, observes_expiry in product(
        ("explicit_euler", "implicit_euler", "crank_nicolson", "rannacher"),
        (False, True),
        ("at_hit", "at_expiry"),
        (0, 2),
        (False, True),
    ):
        with subtests.test(
            scheme=scheme,
            up=up,
            timing=timing,
            rebate=rebate,
            observes_expiry=observes_expiry,
        ):
            dates = [datetime(2025, 1, 2)]
            if observes_expiry:
                dates.append(end)
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                ),
                spot_price=150 if up else 1,
                valuation_time=valuation,
            )
            option = BarrierOption(
                option_type="call" if up else "put",
                strike=100,
                effective_time=start,
                expiry_time=end,
                barrier_level=110 if up else 90,
                barrier_type="up_and_out" if up else "down_and_out",
                rebate=rebate,
                rebate_timing=timing,
                observation_times=dates,
            )
            engine = pricing.FiniteDifferenceBarrierEngine(
                asset_step_count=200,
                time_step_count=2000 if scheme == "explicit_euler" else 200,
                scheme=scheme,
                asset_upper_boundary=151 if up else None,
            )
            payment = dates[0] if timing == "at_hit" else end
            elapsed = (
                datetime.combine(payment, datetime.min.time()) - valuation
            ).total_seconds() / (365 * 86400)
            assert engine.price(option, context) == pytest.approx(
                rebate * math.exp(-0.04 * elapsed), rel=0, abs=1e-6
            )
