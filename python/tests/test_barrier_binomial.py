"""Single-barrier CRR contracts, event handling and independent references."""

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
from kiyosi.instruments import BarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.history import dated as dated_history
from support.history import history_fields

START = datetime(2025, 1, 1)

END = START + timedelta(days=360)

KINDS = ("up_and_in", "up_and_out", "down_and_in", "down_and_out")

OTHER_GREEKS = (
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "volga",
    "vanna",
    "zomma",
    "rho",
    "dividend_rho",
)


def market(spot=100, valuation=START, rate=0.03, volatility=0.2, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def option(kind="down_and_out", direction="call", **changes):
    terms = {
        "option_type": direction,
        "strike": 100,
        "effective_time": START,
        "expiry_time": END,
        "barrier_level": 120 if kind.startswith("up") else 80,
        "barrier_type": kind,
        "calendar": all_days_calendar(),
    }
    terms.update(changes)
    return BarrierOption(**history_fields(**terms))


def test_crr_requires_exact_future_observation_alignment():
    contract = option(
        observation_times=[START + timedelta(days=91), END],
    )
    assert_rejection(
        UnsupportedOperationError,
        pricing.CoxRossRubinsteinBarrierEngine(step_count=256).price,
        dated_history(contract, market()),
        market(),
    )
    aligned = pricing.CoxRossRubinsteinBarrierEngine(step_count=360)
    assert aligned.price(dated_history(contract, market()), market()) > 0
    intraday = datetime(2025, 1, 1, 12)
    assert_rejection(
        UnsupportedOperationError,
        aligned.price,
        dated_history(contract, market(valuation=intraday)),
        market(valuation=intraday),
    )


def test_expiry_only_observation_matches_one_step_payment():
    contract = option(
        "up_and_out",
        expiry_time=START + timedelta(days=2),
        barrier_level=101,
        rebate=2,
        observation_times=[START + timedelta(days=2)],
    )
    context = market(rate=0, dividend=0)
    up = math.exp(0.2 * math.sqrt(2 / 365))
    assert pricing.CoxRossRubinsteinBarrierEngine(step_count=1).price(
        dated_history(contract, context), context
    ) == pytest.approx(2 / (1 + up), rel=0, abs=1e-12)


def test_defaults_strict_settings_and_owned_values():
    factory = pricing.CoxRossRubinsteinBarrierEngine
    assert factory().step_count == 256
    assert repr(factory()) == "CoxRossRubinsteinBarrierEngine(step_count=256)"

    class Index:
        def __index__(self):
            return 17

    numbers.Integral.register(Index)
    assert factory(step_count=Index()).step_count == 17
    for steps in (1, 1_000_000):
        engine = factory(step_count=steps)
        assert engine.step_count == steps
        with pytest.raises(AttributeError):
            engine.step_count = 2
    for value in (0, -1, 1_000_001):
        assert_rejection(InvalidInputError, factory, step_count=value)
    for value in (True, False, 1.0, 1.5, "1", None):
        with pytest.raises(TypeError):
            factory(step_count=value)
    for value in (2**31, -(2**31) - 1):
        with pytest.raises(OverflowError):
            factory(step_count=value)
    with pytest.raises(TypeError):
        factory(unknown=1)
    with pytest.raises(TypeError):
        factory(17)


def test_only_requested_node_greeks_are_returned():
    for steps in (1, 2, 256):
        engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=steps)
        contract, context = option(), market()
        result = engine.price_with_greeks(
            dated_history(contract, context), context, greeks="all"
        )
        assert result.delta is not None
        assert (result.gamma is not None) == (steps >= 2)
        for name in OTHER_GREEKS:
            assert getattr(result, name) is None
        for name in ("delta", "gamma", *OTHER_GREEKS):
            if name in OTHER_GREEKS:
                with pytest.raises(UnsupportedOperationError):
                    engine.price_with_greeks(
                        dated_history(contract, context), context, name
                    )
                continue
            selected = engine.price_with_greeks(
                dated_history(contract, context),
                context,
                name,
            )
            assert getattr(selected, name) == getattr(result, name)
            assert selected.price == result.price


@pytest.mark.parametrize("kind", KINDS)
def test_current_hit_paid_history_and_resolved_vanilla_precede_grid(kind):
    engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=3)
    level = 120 if kind.startswith("up") else 80
    for timing in ("at_hit", "at_expiry") if kind.endswith("out") else ("at_expiry",):
        contract = option(
            kind,
            rebate=7,
            rebate_timing=timing,
            observation_times=[START, START + timedelta(days=1), END],
        )
        context = market(spot=level)
        expected = pricing.AnalyticBarrierEngine().price(
            dated_history(contract, context), context
        )
        assert engine.price(dated_history(contract, context), context) == expected
        result = engine.price_with_greeks(
            dated_history(contract, context), context, greeks="all"
        )
        for name in ("delta", "gamma", *OTHER_GREEKS):
            assert getattr(result, name) is None
        touched = contract.replace_history(
            touch_state="touched",
            history_time=contract.expiry_time,
        )
        context = market(valuation=START + timedelta(days=2))
        expected = pricing.AnalyticBarrierEngine().price(
            dated_history(touched, context), context
        )
        result = engine.price_with_greeks(
            dated_history(touched, context), context, greeks="all"
        )
        assert result.price == expected
        assert result.delta is not None
        assert result.gamma is not None
        if kind.endswith("out"):
            assert result.delta == 0
        assert contract.touch_state is None
        assert_rejection(
            InvalidInputError,
            engine.price,
            dated_history(touched, market()),
            market(),
        )


def test_microsecond_alignment_and_inclusive_noncentral_node_hits():
    end = START + timedelta(days=4)
    contract = option(
        "up_and_out",
        expiry_time=end,
        observation_times=[START + timedelta(days=2)],
    )
    engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=7)
    time = datetime(2025, 1, 1, 12)
    context = market(valuation=time)
    assert engine.price(dated_history(contract, context), context) > 0
    assert_rejection(
        UnsupportedOperationError,
        engine.price,
        dated_history(contract, market(valuation=time + timedelta(microseconds=1))),
        market(valuation=time + timedelta(microseconds=1)),
    )
    for sigma in (0.1, 0.2, 0.3, 0.4):
        for up in (False, True):
            jump = sigma * math.sqrt(1 / 365)
            contract = option(
                "up_and_out" if up else "down_and_out",
                "call" if up else "put",
                expiry_time=START + timedelta(days=2),
                barrier_level=100 * math.exp(jump if up else -jump),
                observation_times=[START + timedelta(days=1)],
            )
            # The sole ITM path is knocked out at its first move, including equality.
            assert pricing.CoxRossRubinsteinBarrierEngine(step_count=2).price(
                dated_history(contract, market(volatility=sigma)),
                market(volatility=sigma),
            ) == pytest.approx(0, rel=0, abs=1e-12)


def test_flat_curve_applicability_calendar_and_validation_order():
    engine = pricing.CoxRossRubinsteinBarrierEngine()
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[math.exp(-0.03 * 360 / 365), (math.exp(-0.03 * 360 / 365)) * 0.5],
    )
    for time in (START, END):
        for changes in ({"rate": curve}, {"dividend": curve}):
            context = market(valuation=time, **changes)
            assert_rejection(
                UnsupportedOperationError,
                engine.price,
                dated_history(option(), context),
                context,
            )
            assert_rejection(
                UnsupportedOperationError,
                engine.price_with_greeks,
                dated_history(option(), context),
                context,
                [],
            )
            assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                dated_history(option(), context),
                context,
                math.nan,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    with pytest.raises(InvalidInputError):
        option(
            observation_times=[datetime(2025, 1, 4)],
            calendar=weekdays_calendar(),
        )
    for sigma, rate in ((1e-200, 0.03), (50, 0.03), (0.2, -1000)):
        assert_rejection(
            ComputationFailureError,
            engine.price,
            dated_history(option(), market(volatility=sigma, rate=rate)),
            market(volatility=sigma, rate=rate),
        )
    for kwargs in (
        {"spot_shift": True},
        {"time_shift": 1.5},
        {"time_shift": True},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            numerical_greeks(
                engine,
                dated_history(option(), market()),
                market(),
                "delta",
                **kwargs,
            )
    with pytest.raises(TypeError):
        numerical_greeks(
            engine,
            dated_history(option(), market()),
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


def test_solver_tree_domain_and_failures_preserve_inputs():
    engine = pricing.CoxRossRubinsteinBarrierEngine()
    contract, context = option(barrier_level=60), market()
    quote = engine.price(dated_history(contract, context), context)
    solved = pricing.implied_volatility(
        engine,
        dated_history(contract, context),
        context,
        quote,
        lower_bound=1e-8,
        upper_bound=0.5,
    )
    assert engine.price(
        dated_history(contract, market(volatility=solved)),
        market(volatility=solved),
    ) == pytest.approx(quote, rel=0, abs=2e-7)
    for price, controls, expected_error in (
        (
            quote,
            {"upper_bound": 0.0002},
            RootNotBracketedError,
        ),
        (-1, {"upper_bound": 0.5}, RootNotBracketedError),
        (math.nan, {}, InvalidInputError),
        (quote, {"max_iterations": 0}, InvalidInputError),
    ):
        assert_rejection(
            expected_error,
            pricing.implied_volatility,
            engine,
            dated_history(contract, context),
            context,
            price,
            **{"lower_bound": 0.0, "upper_bound": 4.0, **controls},
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
    assert engine.step_count == 256


def test_shared_and_distinct_instances_allow_concurrent_pricing_and_solving():
    engine = pricing.CoxRossRubinsteinBarrierEngine()
    contract, context = option(barrier_level=60), market()
    quote = engine.price(dated_history(contract, context), context)

    def evaluate(index):
        selected = engine if index % 2 else pricing.CoxRossRubinsteinBarrierEngine()
        result = selected.price_with_greeks(
            dated_history(contract, context), context, ("delta", "gamma")
        )
        sigma = pricing.implied_volatility(
            selected,
            dated_history(contract, context),
            context,
            result.price,
            lower_bound=0.15,
            upper_bound=0.25,
        )
        return result.price, sigma

    with ThreadPoolExecutor(max_workers=4) as pool:
        for price, sigma in pool.map(evaluate, range(8)):
            assert price == quote
            assert sigma == pytest.approx(0.2, rel=0, abs=0.0002)


def test_continuous_prices_and_greeks_against_live_quantlib(subtests):

    oracle = importlib.import_module("quantlib_reference.barrier")
    oracle.g.require_version()
    engine = pricing.CoxRossRubinsteinBarrierEngine(step_count=2048)
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
                        ("price", 0.25),
                        ("delta", 0.07),
                        ("gamma", 0.01),
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
