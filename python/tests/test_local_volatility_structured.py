"""Multi-event local-volatility contracts against independent path references."""

import json
import math
import unittest
from pathlib import Path

import QuantLib as ql
from kiyosi import instruments as ins
from kiyosi import pricing
from kiyosi.market import (
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
)
from quantlib_reference.local_volatility_paths import (
    END,
    KNOCK_OUT_LEVELS,
    NAMES,
    OBSERVATIONS,
    START,
    events,
    payoffs,
    trading_dates,
)


def market(spot=101):
    return PricingContext(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.04,
            dividend_yield=0.01,
            surface=LocalVolatilitySurface(
                times=[START, END],
                spots=[60, 100, 160],
                values=[[0.32, 0.24, 0.16]] * 2,
            ),
        ),
        spot_price=spot,
        valuation_time=START,
    )


def contracts():
    dates = {"effective_date": START, "expiry_date": END}
    observations = ins.autocallable_observations(
        OBSERVATIONS, KNOCK_OUT_LEVELS, knock_out_coupon_rates=[0.08] * 3
    )
    common = dict(dates, observations=observations)
    loss = {
        "initial_spot": 100,
        "knock_in_level": 95,
        "upper_strike": 100,
        "lower_strike": 60,
    }
    result = {
        "accumulator": ins.Accumulator(
            **dates,
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
        )
    }
    for suffix, mode in [("daily", "every_trading_day"), ("terminal", "at_expiry")]:
        state = {"knock_in_observation_mode": mode, "barrier_state": "no_event"}
        result[f"snowball_{suffix}"] = ins.SnowballOption(
            **common, **loss, **state, maturity_coupon_rate=0.06
        )
        result[f"ternary_{suffix}"] = ins.TernarySnowballOption(
            **common,
            **state,
            maturity_coupon_rate=0.06,
            minimum_coupon_rate=0.01,
            knock_in_level=95,
        )
        result[f"phoenix_{suffix}"] = ins.PhoenixOption(
            **dates,
            **loss,
            **state,
            coupon_rate=0.08,
            observations=ins.autocallable_observations(
                OBSERVATIONS, KNOCK_OUT_LEVELS, coupon_barrier_levels=[95] * 3
            ),
        )
    result["binary_snowball"] = ins.BinarySnowballOption(
        **common, maturity_coupon_rate=0.06
    )
    return result


def engine(name, space=1600, time=1600, upper=440):
    factory = (
        pricing.FiniteDifferenceAccumulatorEngine
        if name == "accumulator"
        else pricing.FiniteDifferenceAutocallableEngine
    )
    return factory(
        asset_step_count=space, time_step_count=time, asset_upper_boundary=upper
    )


class LocalVolatilityStructuredTests(unittest.TestCase):
    def test_independent_payoff_evaluator_on_known_paths(self):
        grid = ql.TimeGrid([(day - START).days / 365 for day in trading_dates()])
        records = events(grid)
        maturity = (END - START).days / 365
        discount = math.exp(-0.04 * maturity)
        low = dict(zip(NAMES, payoffs([80.0] * len(grid), records), strict=True))
        self.assertAlmostEqual(
            low["accumulator"], -(4 + 2 * len(records)) * 20 * discount
        )
        for name in [
            "snowball_daily",
            "snowball_terminal",
            "phoenix_daily",
            "phoenix_terminal",
        ]:
            self.assertAlmostEqual(low[name], 0.8 * discount)
        self.assertAlmostEqual(low["binary_snowball"], (1 + 0.06 * maturity) * discount)
        for name in ["ternary_daily", "ternary_terminal"]:
            self.assertAlmostEqual(low[name], (1 + 0.01 * maturity) * discount)
        high = dict(zip(NAMES, payoffs([111.0] * len(grid), records), strict=True))
        self.assertAlmostEqual(high["accumulator"], 44 * math.exp(-0.04 / 365))
        first = (OBSERVATIONS[0] - START).days / 365
        for name in NAMES[1:]:
            self.assertAlmostEqual(
                high[name], (1 + 0.08 * first) * math.exp(-0.04 * first)
            )

    def test_skew_multievent_prices_match_independent_controlled_paths(self):
        evidence = json.loads(
            (
                Path(__file__).parent
                / "quantlib_reference/local_volatility_structured.json"
            ).read_text()
        )
        for name, contract in contracts().items():
            with self.subTest(product=name):
                ref = evidence["reference"]["results"][name]
                bias = evidence["time_refinement"][-1]["results"][name]
                budget = (
                    5 * ref["standard_error"]
                    + ref["control_reference_error"]
                    + 2 * (abs(bias["fine_minus_coarse"]) + 5 * bias["standard_error"])
                    + (0.5 if name == "accumulator" else 0.0001)
                )
                actual = engine(name).price(contract, market())
                self.assertAlmostEqual(actual, ref["price"], delta=budget)

    def test_skew_refines_time_space_domain_and_preserves_surface_greeks(self):
        for name, contract in contracts().items():
            with self.subTest(product=name):
                e = engine(name)
                result = e.price_with_greeks(contract, market(), ["delta", "gamma"])
                budget = 0.5 if name == "accumulator" else 0.00015
                for other in [
                    engine(name, time=800),
                    engine(name, space=800),
                    engine(name, space=2400, upper=660),
                ]:
                    self.assertAlmostEqual(
                        other.price(contract, market()), result.price, delta=budget
                    )
                up, down = [e.price(contract, market(spot)) for spot in [101.1, 100.9]]
                self.assertAlmostEqual(
                    result.delta,
                    (up - down) / 0.2,
                    delta=0.02 if name == "accumulator" else 0.00001,
                )
                self.assertAlmostEqual(
                    result.gamma,
                    (up + down - 2 * result.price) / 0.01,
                    delta=0.02 if name == "accumulator" else 0.00001,
                )

    def test_structured_greeks_match_independent_common_random_paths(self):
        evidence = json.loads(
            (
                Path(__file__).parent
                / "quantlib_reference/local_volatility_structured.json"
            ).read_text()
        )
        fine = evidence["sensitivities"]["fine"]["results"]
        coarse = evidence["sensitivities"]["coarse"]["results"]
        for name, contract in contracts().items():
            actual = engine(name).price_with_greeks(
                contract, market(103), ["delta", "gamma"]
            )
            for greek in ["delta", "gamma"]:
                with self.subTest(product=name, greek=greek):
                    ref = fine[f"{greek}_2"][name]
                    small = fine[f"{greek}_1"][name]
                    previous = coarse[f"{greek}_2"][name]
                    # Wider common-random bumps reduce variance; keep their measured O(h²) bias.
                    budget = 5 * ref["standard_error"] + 4 / 3 * abs(
                        ref["value"] - small["value"]
                    )
                    self.assertLess(budget, abs(ref["value"]))
                    self.assertAlmostEqual(
                        ref["value"],
                        previous["value"],
                        delta=5
                        * math.hypot(ref["standard_error"], previous["standard_error"]),
                    )
                    self.assertAlmostEqual(
                        getattr(actual, greek), ref["value"], delta=budget
                    )


if __name__ == "__main__":
    unittest.main()
