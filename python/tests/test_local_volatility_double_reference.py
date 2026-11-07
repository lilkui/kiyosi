"""Double-barrier local-volatility checks with measured monitoring uncertainty."""

import json
import math
import unittest
from pathlib import Path

from kiyosi import instruments as ins
from kiyosi import pricing
from support.history import dated as dated_history
from test_local_volatility_structured import END, OBSERVATIONS, START, market


def contracts(scheduled):
    terms = {
        "effective_time": START,
        "expiry_time": END,
        "lower_barrier": 85,
        "upper_barrier": 115,
        "touch_state": "untouched",
    }
    if scheduled:
        terms.update(observation_mode="scheduled", observation_times=OBSERVATIONS)
    result = {}
    for direction in ["call", "put"]:
        for kind in ["in", "out"]:
            result[f"{direction}_{kind}"] = ins.DoubleBarrierOption(
                **terms, option_type=direction, strike=100, barrier_type=f"knock_{kind}"
            )
    result["touch_expiry"] = ins.cash_double_touch(
        **terms, payout=10, settlement_timing="at_expiry"
    )
    result["touch_hit"] = ins.cash_double_touch(
        **terms, payout=10, settlement_timing="at_hit"
    )
    result["no_touch"] = ins.cash_double_no_touch(**terms, payout=10)
    return result


def engine(name, space=1600, time=1600, upper=440):
    factory = (
        pricing.FiniteDifferenceDoubleBarrierEngine
        if name.startswith(("call", "put"))
        else pricing.FiniteDifferenceDoubleTouchEngine
    )
    return factory(
        asset_step_count=space, time_step_count=time, asset_upper_boundary=upper
    )


class LocalVolatilityDoubleReferenceTests(unittest.TestCase):
    def test_skew_prices_match_paths_with_separate_monitoring_error(self):
        evidence = json.loads(
            (
                Path(__file__).parent
                / "quantlib_reference/local_volatility_double.json"
            ).read_text()
        )
        coarse, fine = [evidence[key]["results"] for key in ["coarse", "fine"]]
        for scheduled in [False, True]:
            for name, contract in contracts(scheduled).items():
                with self.subTest(name=name, scheduled=scheduled):
                    label = "scheduled" if scheduled else "continuous_1"
                    ref = fine[label][name]
                    expected = ref["price"]
                    if scheduled:
                        bias = abs(expected - coarse[label][name]["price"])
                    else:
                        # Measured nested-monitoring changes scale approximately with sqrt(dt).
                        # Extrapolation is an estimate with a separate residual budget, not an exact bridge.
                        divisor = math.sqrt(2) - 1
                        correction = fine["monitoring_1_2"][name]
                        expected -= correction["price"] / divisor
                        previous = (
                            coarse[label][name]["price"]
                            - coarse["monitoring_1_2"][name]["price"] / divisor
                        )
                        bias = (
                            abs(expected - previous)
                            + 5 * correction["standard_error"] / divisor
                        )
                        first = abs(correction["price"])
                        second = abs(fine["monitoring_2_4"][name]["price"])
                        self.assertLess(first, second)
                    budget = 5 * ref["standard_error"] + bias + 0.003
                    self.assertAlmostEqual(
                        engine(name).price(dated_history(contract, market()), market()),
                        expected,
                        delta=budget,
                    )

    def test_double_boundaries_refine_and_supply_fixed_surface_greeks(self):
        for scheduled in [False, True]:
            for name, contract in contracts(scheduled).items():
                with self.subTest(name=name, scheduled=scheduled):
                    e = engine(name)
                    result = e.price_with_greeks(
                        dated_history(contract, market()), market(), ["delta", "gamma"]
                    )
                    for other in [
                        engine(name, time=800),
                        engine(name, space=800),
                        engine(name, space=2400, upper=660),
                    ]:
                        self.assertAlmostEqual(
                            other.price(dated_history(contract, market()), market()),
                            result.price,
                            delta=0.005,
                        )
                    up, down = [
                        e.price(dated_history(contract, market(spot)), market(spot))
                        for spot in [101.1, 100.9]
                    ]
                    self.assertAlmostEqual(
                        result.delta, (up - down) / 0.2, delta=0.0002
                    )
                    self.assertAlmostEqual(
                        result.gamma,
                        (up + down - 2 * result.price) / 0.01,
                        delta=0.0002,
                    )


if __name__ == "__main__":
    unittest.main()
