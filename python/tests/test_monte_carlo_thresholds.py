"""Future threshold decisions retain movements smaller than a spot-price ULP."""

import unittest
from datetime import datetime
from datetime import time as clock_time

from kiyosi import instruments as ins
from kiyosi import pricing
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from support.history import dated as dated_history
from support.history import history_fields

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
SETTINGS = {"path_count": 65536, "seed": 42, "antithetic": True}


def market(volatility, intraday=False):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=volatility
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 1, 12) if intraday else START,
    )


class MonteCarloThresholdTests(unittest.TestCase):
    def test_scheduled_barriers_preserve_sub_ulp_threshold_movements(self):
        single = pricing.MonteCarloBarrierEngine(**SETTINGS)
        double = pricing.MonteCarloDoubleBarrierEngine(**SETTINGS)
        for volatility in (1e-18, 1e-200):
            context = market(volatility)
            for direction, strike in (("call", 90), ("put", 110)):
                for kind, lower, upper in (
                    ("up_and_in", 80, 100),
                    ("up_and_out", 80, 100),
                    ("down_and_in", 100, 120),
                    ("down_and_out", 100, 120),
                ):
                    common = {
                        "option_type": direction,
                        "strike": strike,
                        "effective_time": START,
                        "expiry_time": END,
                        "observation_times": [END],
                        "rebate": 2,
                    }
                    with self.subTest(
                        volatility=volatility, direction=direction, kind=kind
                    ):
                        option = ins.BarrierOption(
                            **history_fields(
                                **common, barrier_level=100, barrier_type=kind
                            )
                        )
                        self.assertAlmostEqual(
                            single.price(dated_history(option, context), context),
                            6,
                            delta=1e-12,
                        )
                        option = ins.DoubleBarrierOption(
                            **history_fields(
                                **common,
                                lower_barrier=lower,
                                upper_barrier=upper,
                                barrier_type="knock_in"
                                if kind.endswith("in")
                                else "knock_out",
                            )
                        )
                        self.assertAlmostEqual(
                            double.price(dated_history(option, context), context),
                            6,
                            delta=1e-12,
                        )

    def test_snowball_thresholds_preserve_price_and_sampling_uncertainty(self):
        note = ins.BinarySnowballOption(
            maturity_coupon_rate=0.02,
            effective_time=START,
            expiry_time=END,
            observations=ins.autocallable_observations(
                [END], [100], knock_out_coupon_rates=[0.12]
            ),
        )
        for volatility in (1e-18, 1e-200):
            for antithetic in (False, True):
                engine = pricing.MonteCarloAutocallableEngine(
                    **{**SETTINGS, "antithetic": antithetic}
                )
                estimate = engine.price_with_statistics(
                    dated_history(note, market(volatility)), market(volatility)
                )
                with self.subTest(volatility=volatility, antithetic=antithetic):
                    if antithetic:
                        self.assertAlmostEqual(estimate.price, 1.07, delta=1e-12)
                        self.assertLess(estimate.standard_error, 1e-14)
                    else:
                        self.assertAlmostEqual(
                            estimate.price, 1.07, delta=4 * estimate.standard_error
                        )
                        self.assertAlmostEqual(
                            estimate.standard_error, 0.05 / 256, delta=1e-6
                        )

    def test_autocallable_knock_in_and_coupon_tests_preserve_sub_ulp_movements(self):
        end = datetime(2025, 1, 2)
        engine = pricing.MonteCarloAutocallableEngine(**SETTINGS)
        common = {"effective_time": START, "expiry_time": end}
        for volatility in (1e-18, 1e-200):
            context = market(volatility, intraday=True)
            for mode in ("every_trading_day", "at_expiry"):
                note = ins.TernarySnowballOption(
                    **history_fields(
                        observation_clock_time=clock_time()
                        if mode == "every_trading_day"
                        else None,
                        **common,
                        maturity_coupon_rate=0.12,
                        minimum_coupon_rate=0.02,
                        knock_in_level=100,
                        knock_in_observation_mode=mode,
                        barrier_state="no_event",
                        observations=ins.autocallable_observations(
                            [end], [120], knock_out_coupon_rates=[0]
                        ),
                    )
                )
                with self.subTest(volatility=volatility, mode=mode):
                    self.assertAlmostEqual(
                        engine.price(dated_history(note, context), context),
                        1 + 0.07 / 365,
                        delta=1e-12,
                    )
            note = ins.PhoenixOption(
                **history_fields(
                    observation_clock_time=None,
                    **common,
                    coupon_per_period=0.12,
                    initial_spot=100,
                    knock_in_level=80,
                    upper_strike=100,
                    lower_strike=100,
                    knock_in_observation_mode="at_expiry",
                    observations=ins.autocallable_observations(
                        [end], [120], coupon_barrier_levels=[100]
                    ),
                )
            )
            self.assertAlmostEqual(
                engine.price(dated_history(note, context), context), 1.06, delta=1e-12
            )

    def test_accumulator_thresholds_preserve_knock_out_and_accelerated_settlements(
        self,
    ):
        end = datetime(2025, 1, 2)
        engine = pricing.MonteCarloAccumulatorEngine(**SETTINGS)
        for volatility in (1e-18, 1e-200):
            context = market(volatility, intraday=True)
            for direction, strike, barrier in (("call", 90, 120), ("put", 110, 80)):
                common = {
                    "option_type": direction,
                    "effective_time": START,
                    "expiry_time": end,
                }
                option = ins.Accumulator(
                    **history_fields(
                        knock_out_settled=False,
                        observation_clock_time=clock_time(),
                        **common,
                        strike=strike,
                        knock_out_level=100,
                        daily_quantity=1,
                        acceleration_factor=1,
                        accumulated_quantity=1,
                    )
                )
                with self.subTest(volatility=volatility, direction=direction):
                    self.assertAlmostEqual(
                        engine.price(dated_history(option, context), context),
                        15,
                        delta=1e-12,
                    )
                    option = ins.Accumulator(
                        **history_fields(
                            knock_out_settled=False,
                            observation_clock_time=clock_time(),
                            **common,
                            strike=100,
                            knock_out_level=barrier,
                            daily_quantity=1 / volatility,
                            acceleration_factor=2,
                            accumulated_quantity=0,
                        )
                    )
                    vanilla = ins.EuropeanOption(**common, strike=100)
                    expected = (
                        -pricing.AnalyticEuropeanEngine().price(
                            dated_history(vanilla, context), context
                        )
                        / volatility
                    )
                    self.assertAlmostEqual(
                        engine.price(dated_history(option, context), context),
                        expected,
                        delta=0.02 * abs(expected),
                    )
