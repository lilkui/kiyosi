"""Single-barrier quadrature through the public Python API."""

import importlib
import math
import numbers
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from functools import partial

import pytest
import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import RateTermStructure, weekdays_calendar
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from test_barrier_binomial import END, KINDS, OTHER_GREEKS, START, market, option


@pytest.mark.parametrize("rate", (-0.02, 0.04))
@pytest.mark.parametrize("timing", ("at_hit", "at_expiry"))
def test_early_observation_rebate_discount(rate, timing):
    engine = pricing.QuadratureBarrierEngine()
    observation = datetime(2025, 4, 1)
    contract = option(
        "up_and_out",
        strike=1e9,
        rebate=7,
        rebate_timing=timing,
        observation_times=[observation],
    )
    time = 90 / 365
    horizon = 360 / 365
    z = (math.log(1.2) - (rate - 0.03) * time) / (0.2 * math.sqrt(time))
    expected = (
        7
        * math.exp(-rate * (time if timing == "at_hit" else horizon))
        * 0.5
        * math.erfc(z / math.sqrt(2))
    )
    assert engine.price(contract, market(rate=rate)) == pytest.approx(
        expected, rel=0, abs=2e-11
    )


@pytest.mark.parametrize("steps,extent", ((256, 8), (512, 8), (512, 10)))
def test_quadrature_refinement_and_requested_higher_greeks(steps, extent):
    contract, context = option(rebate=2), market()
    names = (
        "delta",
        "gamma",
        "vega",
        "rho",
    )
    expected = pricing.AnalyticBarrierEngine().price_with_greeks(
        contract, context, names
    )
    engine = pricing.QuadratureBarrierEngine(
        asset_step_count=steps, integration_standard_deviations=extent
    )
    result = engine.price_with_greeks(contract, context, names)
    for name in ("price", *names):
        assert getattr(result, name) == pytest.approx(
            getattr(expected, name), rel=0, abs=(0.001 if steps == 256 else 0.0002)
        )
    assert result.speed is None


def test_continuous_prices_and_greeks_against_live_quantlib(subtests):

    oracle = importlib.import_module("quantlib_reference.barrier")
    oracle.g.require_version()
    engine = pricing.QuadratureBarrierEngine(
        asset_step_count=2048, integration_standard_deviations=10
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
                        contract,
                        market(rate=rate, dividend=dividend),
                        ("delta", "gamma"),
                    )
                    for name, budget in (
                        ("price", 3e-8),
                        ("delta", 2e-7),
                        ("gamma", 2e-7),
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
    engine = pricing.QuadratureBarrierEngine(
        asset_step_count=1024, integration_standard_deviations=10
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
            assert engine.price(contract, market()) == pytest.approx(
                expected, rel=0, abs=2e-6
            )


@pytest.mark.parametrize("kind", ("up_and_out", "down_and_out"))
def test_two_observations_match_independent_joint_survival_probability(kind):
    engine = pricing.QuadratureBarrierEngine(
        asset_step_count=2048, integration_standard_deviations=10
    )
    first, last = 91 / 365, 273 / 365
    sign = 1 if kind.startswith("up") else -1
    level = 120 if sign == 1 else 80
    # An unreachable call strike isolates the rebate; the joint normal gives no-hit probability.
    threshold = lambda time, level=level: (
        (math.log(level / 100) - (0.03 - 0.01 - 0.02) * time) / (0.2 * math.sqrt(time))
    )
    joint = ql.BivariateCumulativeNormalDistributionWe04DP(math.sqrt(first / last))
    survival = joint(sign * threshold(first), sign * threshold(last))
    contract = option(
        kind,
        strike=1e9,
        rebate=7,
        observation_times=[
            START + timedelta(days=91),
            START + timedelta(days=273),
        ],
    )
    expected = 7 * math.exp(-0.03 * 360 / 365) * (1 - survival)
    assert engine.price(contract, market()) == pytest.approx(expected, rel=0, abs=2e-6)


def test_settings_and_representation_errors_keep_native_exceptions():
    constructor = pricing.QuadratureBarrierEngine
    engine = constructor()
    assert ((engine.asset_step_count, engine.integration_standard_deviations)) == (
        (512, 8)
    )
    assert "asset_step_count=512" in repr(engine)

    class Index:
        def __index__(self):
            return 513

    assert constructor(asset_step_count=Index()).asset_step_count == 513
    numbers.Integral.register(Index)
    assert constructor(asset_step_count=Index()).asset_step_count == 513
    with pytest.raises(AttributeError):
        engine.asset_step_count = 4
    for value in (True, 4.5, "4", None):
        with pytest.raises(TypeError):
            constructor(asset_step_count=value)
    with pytest.raises(OverflowError):
        constructor(asset_step_count=2**31)
    for value in (True, "8", None, 1j):
        with pytest.raises(TypeError):
            constructor(integration_standard_deviations=value)
    for changes in (
        {"asset_step_count": 3},
        {"integration_standard_deviations": 0},
        {"integration_standard_deviations": math.inf},
        {"integration_standard_deviations": math.nan},
    ):
        assert_rejection(InvalidInputError, constructor, **changes)
    with pytest.raises(TypeError):
        constructor(512)
    with pytest.raises(TypeError):
        constructor(unknown=8)
    for changes in (
        {"spot_shift": True},
        {"time_shift": 1.5},
        {"time_shift": True},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            numerical_greeks(
                engine,
                option(),
                market(),
                "delta",
                **changes,
            )
    with pytest.raises(TypeError):
        numerical_greeks(
            engine,
            option(),
            market(),
            "delta",
            time_shift=2**31,
        )
    mismatch = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    for operation, args in (
        (engine.price, (mismatch, market())),
        (engine.price_with_greeks, (mismatch, market(), "delta")),
        (
            partial(pricing.implied_volatility, lower_bound=0.0, upper_bound=4.0),
            (engine, mismatch, market(), 10),
        ),
    ):
        with pytest.raises(UnsupportedOperationError):
            operation(*args)


def test_applicability_precedes_request_solver_and_expiry_validation():
    engine = pricing.QuadratureBarrierEngine()
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[math.exp(-0.03 * 360 / 365), (math.exp(-0.03 * 360 / 365)) * 0.5],
    )
    for context in (market(rate=curve), market(rate=curve, valuation=END)):
        for operation, args in (
            (engine.price, (option(), context)),
            (engine.price_with_greeks, (option(), context, [])),
            (
                partial(pricing.implied_volatility, lower_bound=0.0, upper_bound=4.0),
                (engine, option(), context, math.nan),
            ),
        ):
            assert_rejection(UnsupportedOperationError, operation, *args)
    with pytest.raises(InvalidInputError):
        option(
            observation_times=[datetime(2025, 1, 4)],
            touch_state="touched",
            calendar=weekdays_calendar(),
        )
    for sigma in (1e200, 1e308):
        assert_rejection(
            ComputationFailureError,
            engine.price,
            option(),
            market(volatility=sigma),
        )
    huge_grid = pricing.QuadratureBarrierEngine(integration_standard_deviations=1e308)
    scheduled = option(observation_times=[START + timedelta(days=91)])
    assert_rejection(
        ComputationFailureError,
        huge_grid.price,
        scheduled,
        market(volatility=100),
    )


def test_scheduled_greeks_solver_and_concurrency():
    engine = pricing.QuadratureBarrierEngine(asset_step_count=256)
    contract = option(
        observation_times=[START + timedelta(days=91), START + timedelta(days=273)],
    )
    context = market()
    quote = engine.price(contract, context)
    result = engine.price_with_greeks(contract, context, greeks="all")
    assert result.price == quote
    for name in ("delta", "gamma", *OTHER_GREEKS):
        assert math.isfinite(getattr(result, name)), name

    def evaluate(index):
        selected = (
            engine
            if index % 2
            else pricing.QuadratureBarrierEngine(asset_step_count=256)
        )
        sigma = pricing.implied_volatility(
            selected,
            contract,
            context,
            quote,
            lower_bound=0.15,
            upper_bound=0.25,
        )
        return selected.price(contract, context), sigma

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
            contract,
            context,
            observed,
            lower_bound=0.15,
            upper_bound=0.25,
            **controls,
        )
    with pytest.raises(TypeError):
        pricing.implied_volatility(
            engine,
            contract,
            context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(TypeError):
        pricing.implied_volatility(
            engine,
            contract,
            context,
            quote,
            max_iterations=1.5,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    assert engine.price(contract, context) == quote
    assert context.model_parameters.volatility == 0.2
