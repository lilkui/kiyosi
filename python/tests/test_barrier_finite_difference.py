"""Single-barrier FD public contracts, analytics and independent references."""

import importlib
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from functools import partial

import pytest
import QuantLib as ql
from kiyosi import (
    InvalidInputError,
    RootNotBracketedError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.history import dated as dated_history
from test_barrier_binomial import END, KINDS, OTHER_GREEKS, START, market, option


def test_continuous_prices_and_greeks_against_live_quantlib(subtests):

    oracle = importlib.import_module("quantlib_reference.barrier")
    oracle.g.require_version()
    engine = pricing.FiniteDifferenceBarrierEngine(
        asset_step_count=1600, time_step_count=800
    )
    for kind in KINDS:
        for direction in ("call", "put"):
            for timing in (
                ("at_hit", "at_expiry") if kind.endswith("out") else ("at_expiry",)
            ):
                for rate, dividend in ((0.03, 0.01), (-0.02, 0.01)):
                    contract = option(kind, direction, rebate=7, rebate_timing=timing)
                    inputs = {
                        "BarrierType": kind,
                        "settlement": timing,
                        "monitoring": "continuous",
                        "valuation": START.date().isoformat(),
                        "effective_time": START.date().isoformat(),
                        "expiry_time": END.date().isoformat(),
                        "option": direction,
                        "strike": 100,
                        "spot": 100,
                        "barrier": contract.barrier_level,
                        "rebate": 7,
                        "rate": rate,
                        "dividend": dividend,
                        "volatility": 0.2,
                    }

                    def value(spot, inputs=inputs):
                        return oracle.price(
                            tuple(sorted((inputs | {"spot": spot}).items()))
                        )

                    with ql.SavedSettings():
                        center, high, low = value(100), value(100.01), value(99.99)
                    expected = {
                        "price": center,
                        "delta": (high - low) / 0.02,
                        "gamma": (high - 2 * center + low) / 0.01**2,
                    }
                    result = engine.price_with_greeks(
                        dated_history(contract, market(rate=rate, dividend=dividend)),
                        market(rate=rate, dividend=dividend),
                        ("delta", "gamma"),
                    )
                    for name, budget in (
                        ("price", 0.003),
                        ("delta", 0.0003),
                        ("gamma", 0.0001),
                    ):
                        with subtests.test(
                            kind=kind,
                            direction=direction,
                            timing=timing,
                            rate=rate,
                            measure=name,
                        ):
                            assert getattr(result, name) == pytest.approx(
                                expected[name], rel=0, abs=budget
                            )


@pytest.mark.parametrize("kind", KINDS)
@pytest.mark.parametrize("direction", ("call", "put"))
def test_early_ending_schedules_match_independent_bivariate_normal_payoffs(
    subtests, kind, direction
):
    engine = pricing.FiniteDifferenceBarrierEngine(
        asset_step_count=1600, time_step_count=800
    )
    observation = START + timedelta(days=91)
    time, horizon = 91 / 365, 360 / 365
    sigma, rate, dividend = 0.2, 0.03, 0.01
    normal = ql.CumulativeNormalDistribution()
    hit_sign = 1 if kind.startswith("up") else -1
    level = 120 if hit_sign == 1 else 80
    a = (math.log(level / 100) - (rate - dividend - sigma**2 / 2) * time) / (
        sigma * math.sqrt(time)
    )
    payoff_sign = 1 if direction == "call" else -1
    b = -(rate - dividend - sigma**2 / 2) * math.sqrt(horizon) / sigma
    joint = ql.BivariateCumulativeNormalDistributionWe04DP(
        hit_sign * payoff_sign * math.sqrt(time / horizon)
    )
    hit_payoff = payoff_sign * (
        100
        * math.exp(-dividend * horizon)
        * joint(
            -hit_sign * (a - sigma * math.sqrt(time)),
            -payoff_sign * (b - sigma * math.sqrt(horizon)),
        )
        - 100 * math.exp(-rate * horizon) * joint(-hit_sign * a, -payoff_sign * b)
    )
    vanilla = payoff_sign * (
        100
        * math.exp(-dividend * horizon)
        * normal(-payoff_sign * (b - sigma * math.sqrt(horizon)))
        - 100 * math.exp(-rate * horizon) * normal(-payoff_sign * b)
    )
    hit_probability = normal(-hit_sign * a)
    for timing in ("at_expiry",) if kind.endswith("in") else ("at_hit", "at_expiry"):
        if kind.endswith("in"):
            expected = hit_payoff + 3 * math.exp(-rate * horizon) * (
                1 - hit_probability
            )
        else:
            expected = (
                vanilla
                - hit_payoff
                + 3
                * math.exp(-rate * (time if timing == "at_hit" else horizon))
                * hit_probability
            )
        contract = option(
            kind,
            direction,
            rebate=3,
            rebate_timing=timing,
            observation_times=[observation],
        )
        with subtests.test(kind=kind, direction=direction, timing=timing):
            assert engine.price(
                dated_history(contract, market()), market()
            ) == pytest.approx(expected, rel=0, abs=0.003)


def test_scheduled_greeks_solver_and_concurrency():
    engine = pricing.FiniteDifferenceBarrierEngine(asset_step_count=256)
    contract = option(
        observation_times=[START + timedelta(days=91), START + timedelta(days=273)],
    )
    context = market()
    quote = engine.price(dated_history(contract, context), context)
    result = engine.price_with_greeks(
        dated_history(contract, context), context, greeks="all"
    )
    assert result.price == quote
    for name in ("delta", "gamma", *OTHER_GREEKS):
        assert math.isfinite(getattr(result, name)), name

    def evaluate(index):
        selected = (
            engine
            if index % 2
            else pricing.FiniteDifferenceBarrierEngine(asset_step_count=256)
        )
        sigma = pricing.implied_volatility(
            selected,
            dated_history(contract, context),
            context,
            quote,
            lower_bound=0.15,
            upper_bound=0.25,
        )
        return selected.price(dated_history(contract, context), context), sigma

    with ThreadPoolExecutor(max_workers=4) as pool:
        for price, sigma in pool.map(evaluate, range(4)):
            assert price == quote
            assert sigma == pytest.approx(0.2, rel=0, abs=2e-7)
    for observed, controls, expected_error in (
        (-1, {}, RootNotBracketedError),
        (math.nan, {}, InvalidInputError),
        (quote, {"max_iterations": 0}, InvalidInputError),
    ):
        assert_rejection(
            expected_error,
            pricing.implied_volatility,
            engine,
            dated_history(contract, context),
            context,
            observed,
            lower_bound=0.15,
            upper_bound=0.25,
            **controls,
        )
    with pytest.raises(TypeError):
        pricing.implied_volatility(
            engine,
            dated_history(contract, context),
            context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(TypeError):
        pricing.implied_volatility(
            engine,
            dated_history(contract, context),
            context,
            quote,
            max_iterations=1.5,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    assert engine.price(dated_history(contract, context), context) == quote
    assert context.model_parameters.volatility == 0.2


def test_node_curves_preserve_events_discounts_and_coverage_order():
    engine = pricing.FiniteDifferenceBarrierEngine(
        asset_step_count=800, time_step_count=800, asset_upper_boundary=400
    )
    # Zero carry and vanishing volatility isolate the rebate's payment discount.
    curve = RateTermStructure(
        reference_time=START,
        times=[START + timedelta(days=91), END],
        factors=[math.exp(-0.08 * 91 / 365), math.exp(-0.01 * 360 / 365)],
    )
    context = market(spot=150, volatility=1e-12)
    context = context.replace(
        model_parameters=context.model_parameters.replace(
            risk_free_rate=curve, dividend_yield=curve
        )
    )
    for timing in ("at_hit", "at_expiry"):
        contract = option(
            "up_and_out",
            rebate=7,
            rebate_timing=timing,
            observation_times=[START + timedelta(days=91), END],
        )
        expected = 7 * math.exp(
            -0.08 * 91 / 365 if timing == "at_hit" else -0.01 * 360 / 365
        )
        assert engine.price(dated_history(contract, context), context) == pytest.approx(
            expected, rel=0, abs=1e-5
        )
    short = RateTermStructure(
        reference_time=START, times=[START + timedelta(days=91)], factors=[0.99]
    )
    uncovered = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=short)
    )
    assert_rejection(InvalidInputError, engine.price, contract, uncovered)
    current = option("up_and_out", rebate=7, rebate_timing="at_hit")
    assert engine.price(dated_history(current, uncovered), uncovered) == 7


def test_selective_mixed_requests_stencils_and_failure_state():
    engine = pricing.FiniteDifferenceBarrierEngine(asset_upper_boundary=400)
    contract = option(touch_state="untouched")
    context = market()
    all_values = engine.price_with_greeks(
        dated_history(contract, context), context, greeks="all"
    )
    for greek in (
        "speed",
        "vanna",
        "zomma",
        "charm",
        "color",
    ):
        selected = engine.price_with_greeks(
            dated_history(contract, context), context, greek
        )
        assert getattr(selected, greek) == getattr(all_values, greek)
        assert selected.delta is None
        assert selected.gamma is None
    for h in (0.01,):
        actual = engine.price_with_greeks(
            dated_history(contract, context), context, "speed"
        )
        high = engine.price_with_greeks(
            dated_history(contract, context.replace(spot_price=100 + h)),
            context.replace(spot_price=100 + h),
            "gamma",
        )
        low = engine.price_with_greeks(
            dated_history(contract, context.replace(spot_price=100 - h)),
            context.replace(spot_price=100 - h),
            "gamma",
        )
        assert actual.speed == pytest.approx(
            (high.gamma - low.gamma) / (2 * h), rel=0, abs=1e-12
        )
    collapsed = numerical_greeks(
        engine,
        dated_history(contract, context),
        context,
        ["delta", "gamma", "speed"],
        spot_shift=1e-300,
    )
    assert collapsed.delta is None
    assert collapsed.gamma is None
    assert collapsed.speed is None
    for name, value in (
        ("spot_shift", True),
        ("volatility_shift", "0.1"),
        ("rate_shift", object()),
        ("time_shift", 1.5),
    ):
        with pytest.raises(TypeError):
            numerical_greeks(
                engine,
                dated_history(contract, context),
                context,
                "delta",
                **{name: value},
            )
    clipped = pricing.FiniteDifferenceBarrierEngine(asset_upper_boundary=110)
    quote = clipped.price(dated_history(contract, context), context)
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, clipped),
        contract,
        context,
        "speed",
        spot_shift=6,
    )
    assert clipped.price(dated_history(contract, context), context) == quote
    assert engine.price(dated_history(contract, context), context) == all_values.price


@pytest.mark.parametrize("spot", (111.94, 111.97, 112.01))
def test_scheduled_payoff_peak_retains_spline_price_and_greeks(spot):
    from kiyosi.instruments import CashOrNothingOption

    engine = pricing.FiniteDifferenceBarrierEngine(asset_upper_boundary=400)
    contract = option("up_and_out", observation_times=[END])
    common = {"option_type": "call", "effective_time": START, "expiry_time": END}
    lower = EuropeanOption(strike=100, **common)
    upper = EuropeanOption(strike=120, **common)
    cash = CashOrNothingOption(strike=120, payout=20, **common)
    context = market(spot=spot, rate=0.04)
    actual = engine.price_with_greeks(
        dated_history(contract, context), context, ["delta", "gamma"]
    )
    first = pricing.AnalyticEuropeanEngine().price_with_greeks(
        dated_history(lower, context), context, ["delta", "gamma"]
    )
    second = pricing.AnalyticEuropeanEngine().price_with_greeks(
        dated_history(upper, context), context, ["delta", "gamma"]
    )
    digital = pricing.AnalyticDigitalEngine().price_with_greeks(
        dated_history(cash, context), context, ["delta", "gamma"]
    )
    for name, tolerance in (("price", 0.001), ("delta", 5e-5), ("gamma", 1e-5)):
        expected = getattr(first, name) - getattr(second, name) - getattr(digital, name)
        assert getattr(actual, name) == pytest.approx(expected, rel=0, abs=tolerance)
    high = engine.price(
        dated_history(contract, context.replace(spot_price=spot + 0.01)),
        context.replace(spot_price=spot + 0.01),
    )
    low = engine.price(
        dated_history(contract, context.replace(spot_price=spot - 0.01)),
        context.replace(spot_price=spot - 0.01),
    )
    assert actual.delta == pytest.approx((high - low) / 0.02, rel=0, abs=1e-7)
    assert actual.gamma == pytest.approx(
        (high - 2 * actual.price + low) / 0.01**2, rel=0, abs=1e-6
    )


def test_scheduled_explicit_stability_retains_the_uniform_grid_bound():
    end = datetime(2026, 1, 1)
    contract = option(
        "up_and_out",
        expiry_time=end,
        observation_times=[end],
    )
    engine = pricing.FiniteDifferenceBarrierEngine(
        asset_step_count=200,
        time_step_count=1600,
        scheme="explicit_euler",
        asset_upper_boundary=400,
    )
    assert_rejection(
        InvalidInputError,
        engine.price,
        contract,
        market(rate=0.04),
    )
