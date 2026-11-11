"""Future threshold decisions retain movements smaller than a spot-price ULP."""

from datetime import datetime
from datetime import time as clock_time

import pytest
from kiyosi import instruments as ins
from kiyosi import pricing
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    weekdays_calendar,
)
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


@pytest.mark.parametrize("volatility", (1e-18, 1e-200))
@pytest.mark.parametrize("direction,strike", (("call", 90), ("put", 110)))
@pytest.mark.parametrize(
    "kind,lower,upper",
    (
        ("up_and_in", 80, 100),
        ("up_and_out", 80, 100),
        ("down_and_in", 100, 120),
        ("down_and_out", 100, 120),
    ),
)
def test_scheduled_barriers_preserve_sub_ulp_threshold_movements(
    volatility, direction, strike, kind, lower, upper
):
    single = pricing.MonteCarloBarrierEngine(**SETTINGS)
    double = pricing.MonteCarloDoubleBarrierEngine(**SETTINGS)
    context = market(volatility)
    common = {
        "option_type": direction,
        "strike": strike,
        "effective_time": START,
        "expiry_time": END,
        "observation_times": [END],
        "rebate": 2,
    }
    option = ins.BarrierOption(
        **history_fields(**common, barrier_level=100, barrier_type=kind)
    )
    assert single.price(dated_history(option, context), context) == pytest.approx(
        6, rel=0, abs=1e-12
    )
    option = ins.DoubleBarrierOption(
        **history_fields(
            **common,
            lower_barrier=lower,
            upper_barrier=upper,
            barrier_type="knock_in" if kind.endswith("in") else "knock_out",
        )
    )
    assert double.price(dated_history(option, context), context) == pytest.approx(
        6, rel=0, abs=1e-12
    )


@pytest.mark.parametrize("volatility", (1e-18, 1e-200))
@pytest.mark.parametrize("antithetic", (False, True))
def test_snowball_thresholds_preserve_price_and_sampling_uncertainty(
    volatility, antithetic
):
    note = ins.BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0.02,
        effective_time=START,
        expiry_time=END,
        observations=ins.autocallable_observations(
            [END], [100], knock_out_coupon_rates=[0.12]
        ),
    )
    engine = pricing.MonteCarloAutocallableEngine(
        **{**SETTINGS, "antithetic": antithetic}
    )
    estimate = engine.price_with_statistics(
        dated_history(note, market(volatility)), market(volatility)
    )
    if antithetic:
        assert estimate.price == pytest.approx(1.07, rel=0, abs=1e-12)
        assert estimate.standard_error < 1e-14
    else:
        assert estimate.price == pytest.approx(
            1.07, rel=0, abs=4 * estimate.standard_error
        )
        assert estimate.standard_error == pytest.approx(0.05 / 256, rel=0, abs=1e-6)


@pytest.mark.parametrize("volatility", (1e-18, 1e-200))
def test_autocallable_knock_in_and_coupon_tests_preserve_sub_ulp_movements(
    subtests, volatility
):
    end = datetime(2025, 1, 2)
    engine = pricing.MonteCarloAutocallableEngine(**SETTINGS)
    common = {"effective_time": START, "expiry_time": end}
    context = market(volatility, intraday=True)
    for mode in ("every_trading_day", "at_expiry"):
        note = ins.TernarySnowballOption(
            calendar=weekdays_calendar(),
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
            ),
        )
        with subtests.test(volatility=volatility, mode=mode):
            assert engine.price(dated_history(note, context), context) == pytest.approx(
                1 + 0.07 / 365, rel=0, abs=1e-12
            )
    note = ins.PhoenixOption(
        calendar=weekdays_calendar(),
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
        ),
    )
    assert engine.price(dated_history(note, context), context) == pytest.approx(
        1.06, rel=0, abs=1e-12
    )


@pytest.mark.parametrize("volatility", (1e-18, 1e-200))
@pytest.mark.parametrize(
    "direction,strike,barrier", (("call", 90, 120), ("put", 110, 80))
)
def test_accumulator_thresholds_preserve_knock_out_and_accelerated_settlements(
    volatility, direction, strike, barrier
):
    end = datetime(2025, 1, 2)
    engine = pricing.MonteCarloAccumulatorEngine(**SETTINGS)
    context = market(volatility, intraday=True)
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
    assert engine.price(dated_history(option, context), context) == pytest.approx(
        15, rel=0, abs=1e-12
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
    assert engine.price(dated_history(option, context), context) == pytest.approx(
        expected, rel=0, abs=0.02 * abs(expected)
    )
