"""Spot defaults remain explicit and replayable for Monte Carlo barrier Greeks."""

import inspect
from datetime import datetime, timedelta

import pytest
from kiyosi import InvalidInputError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from kiyosi.pricing import numerical_greeks


def test_barrier_greek_signature_defaults_can_be_forwarded(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    vanilla = {
        "option_type": "call",
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
    }
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    for engine, option in (
        (
            p.MonteCarloBarrierEngine(path_count=64, seed=42),
            i.BarrierOption(**vanilla, barrier_level=120, barrier_type="up_and_out"),
        ),
        (
            p.MonteCarloBarrierEngine(path_count=64, seed=42),
            i.DoubleBarrierOption(
                **vanilla, lower_barrier=80, upper_barrier=120, barrier_type="knock_out"
            ),
        ),
    ):
        with subtests.test(engine=type(engine).__name__):
            default = (
                inspect.signature(numerical_greeks).parameters["spot_shift"].default
            )
            assert default is Ellipsis
            expected = numerical_greeks(engine, option, context, "delta")
            actual = numerical_greeks(
                engine, option, context, "delta", spot_shift=default
            )
            assert actual.delta == expected.delta
            explicit = numerical_greeks(
                engine, option, context, "delta", spot_shift=0.01
            )
            assert actual.delta == explicit.delta
            near = context.replace(spot_price=119.995)
            default_native = engine.price_with_greeks(option, near, ["delta", "gamma"])
            assert default_native.status("delta") == "stencil_unavailable"
            tuned = engine.price_with_greeks(
                option, near, ["delta", "gamma"], spot_shift=0.0001
            )
            tighter = engine.price_with_greeks(
                option, near, ["delta", "gamma"], spot_shift=0.00005
            )
            assert tuned.status("delta") == "available"
            assert tuned.require("delta") < 0
            assert tuned == tighter
            assert tuned.price == engine.price(option, near)
            for invalid in (0, -1, float("inf"), float("nan")):
                with pytest.raises(InvalidInputError):
                    engine.price_with_greeks(option, near, "delta", spot_shift=invalid)
            for invalid in (True, "0.01"):
                with pytest.raises(TypeError):
                    engine.price_with_greeks(option, near, "delta", spot_shift=invalid)


def test_automatic_american_bump_matches_explicit_core_control():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = i.AmericanOption(
        option_type="put",
        strike=100,
        effective_time=start,
        expiry_time=end,
    )
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.03, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = p.MonteCarloVanillaEngine(
        path_count=8192, step_count=19, seed=42, max_active_workers=1
    )
    result = engine.price_with_greeks(option, context, "gamma")
    explicit = numerical_greeks(engine, option, context, "gamma", spot_shift=8.0)
    assert result.require("gamma") == explicit.require("gamma")


def test_numerical_greeks_inherit_and_override_engine_controls(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = i.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start + timedelta(days=100),
    )
    controls = {
        "spot_shift": 0.5,
        "volatility_shift": 0.02,
        "rate_shift": 0.1,
        "time_shift": timedelta(days=2),
    }
    engine = p.FiniteDifferenceVanillaEngine(
        asset_step_count=100, time_step_count=100, **controls
    )
    request = ["gamma", "vega", "rho", "theta"]
    expected = numerical_greeks(engine, option, context, request, **controls)
    assert numerical_greeks(engine, option, context, request) == expected
    assert (
        numerical_greeks(
            engine,
            option,
            context,
            request,
            **dict.fromkeys(controls, Ellipsis),
        )
        == expected
    )
    for field, value, greek in (
        ("spot_shift", 10.0, "gamma"),
        ("volatility_shift", 0.1, "vega"),
        ("rate_shift", 0.2, "rho"),
        ("time_shift", timedelta(days=30), "theta"),
    ):
        with subtests.test(field=field):
            overridden = numerical_greeks(
                engine, option, context, request, **{field: value}
            )
            assert overridden == numerical_greeks(
                engine, option, context, request, **controls | {field: value}
            )
            assert overridden.require(greek) != expected.require(greek)
    automatic = {"spot_shift": None, "time_shift": None}
    assert numerical_greeks(
        engine, option, context, request, **automatic
    ) == numerical_greeks(engine.replace(**automatic), option, context, request)
    assert expected.require("rho") == engine.price_with_greeks(
        option, context, "rho"
    ).require("rho")
