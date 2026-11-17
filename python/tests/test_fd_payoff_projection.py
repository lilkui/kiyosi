"""Shared Rust/Python accuracy scenarios for ordinary terminal payoff kinks."""

import math
from datetime import datetime

import pytest
from kiyosi import pricing
from kiyosi.instruments import BarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
)

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


def market(spot=100, sigma=0.2, local=False, skew=False):
    rates = {"risk_free_rate": 0.05, "dividend_yield": 0.02}
    model = (
        LocalVolatilityParameters(
            **rates,
            surface=LocalVolatilitySurface(
                times=[START, END],
                spots=[80, 120],
                values=[[0.3, 0.15], [0.35, 0.2]]
                if skew
                else [[sigma, sigma], [sigma, sigma]],
            ),
        )
        if local
        else BlackScholesMertonParameters(**rates, volatility=sigma)
    )
    return PricingContext(model_parameters=model, spot_price=spot, valuation_time=START)


def test_uniform_european_grids_resolve_strike_phases_and_narrow_payoffs(subtests):
    for strike, sigma, end, count, tolerances in (
        (99.9, 0.2, END, 800, (5e-5, 1e-5, 3e-6)),
        (100, 0.2, END, 800, (5e-5, 1e-5, 3e-6)),
        (100.1, 0.2, END, 800, (5e-5, 1e-5, 3e-6)),
        (100.25, 0.2, END, 800, (5e-5, 1e-5, 3e-6)),
        (100, 0.05, END, 800, (0.001, 0.0001, 0.0002)),
        (100, 0.2, datetime(2025, 1, 2), 3200, (0.0001, 0.0002, 0.002)),
    ):
        engine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=count, time_step_count=400, asset_upper_boundary=400
        )
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction,
                strike=strike,
                effective_time=START,
                expiry_time=end,
            )
            previous = None
            for spot in (99.75, 100, 100.25):
                with subtests.test(
                    strike=strike, sigma=sigma, end=end, direction=direction, spot=spot
                ):
                    context = market(spot, sigma)
                    actual = engine.price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    for name, tolerance in zip(
                        ("price", "delta", "gamma"), tolerances, strict=True
                    ):
                        assert getattr(actual, name) == pytest.approx(
                            getattr(expected, name), rel=0, abs=tolerance
                        )
                    assert actual.price == engine.price(option, context)
                    t = (end - START).days / 365
                    asset, cash = (
                        spot * math.exp(-0.02 * t),
                        strike * math.exp(-0.05 * t),
                    )
                    lower, upper = (
                        (max(asset - cash, 0), asset)
                        if direction == "call"
                        else (max(cash - asset, 0), cash)
                    )
                    assert lower <= actual.price <= upper
                    if previous is not None:
                        assert (
                            actual.price > previous
                            if direction == "call"
                            else actual.price < previous
                        )
                    previous = actual.price


def test_continuous_single_barrier_kinks_match_analytic_prices_and_greeks(subtests):
    engine = pricing.FiniteDifferenceBarrierEngine(
        asset_step_count=800, time_step_count=400, asset_upper_boundary=400
    )
    bsm = market()
    for local in (False, True):
        context = market(local=local)
        for direction in ("call", "put"):
            for kind in ("down_and_out", "down_and_in", "up_and_out", "up_and_in"):
                for strike in (99.9, 100, 100.1):
                    with subtests.test(
                        local=local, direction=direction, kind=kind, strike=strike
                    ):
                        option = BarrierOption(
                            option_type=direction,
                            strike=strike,
                            effective_time=START,
                            expiry_time=END,
                            barrier_level=120 if kind.startswith("up") else 80,
                            barrier_type=kind,
                        )
                        actual = engine.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        expected = pricing.AnalyticBarrierEngine().price_with_greeks(
                            option, bsm, ["delta", "gamma"]
                        )
                        for name, tolerance in zip(
                            ("price", "delta", "gamma"),
                            (0.00025, 3e-5, 3e-6),
                            strict=True,
                        ):
                            assert getattr(actual, name) == pytest.approx(
                                getattr(expected, name), rel=0, abs=tolerance
                            )
                        assert actual.price >= 0
                        assert actual.price == engine.price(option, context)


def test_skewed_uniform_payoffs_match_independent_references(subtests):
    context = market(local=True, skew=True)
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "expiry_time": END,
    }
    for option, factory, reference, uncertainty, tolerance in (
        (
            EuropeanOption(**terms),
            pricing.FiniteDifferenceVanillaEngine,
            11.001742706191068,
            1.102296046616996e-5,
            1e-4,
        ),
        (
            BarrierOption(**terms, barrier_level=80, barrier_type="down_and_out"),
            pricing.FiniteDifferenceBarrierEngine,
            10.381742384961965,
            1.3554856561543398e-6,
            0.0002,
        ),
    ):
        with subtests.test(option=type(option).__name__):
            engine = factory(
                asset_step_count=800, time_step_count=400, asset_upper_boundary=400
            )
            result = engine.price_with_greeks(option, context, ["delta", "gamma"])
            # QuantLib 1.44 refined references retain empirical discretization uncertainty.
            assert abs(result.price - reference) + uncertainty < tolerance
            assert result.price == engine.price(option, context)
            if isinstance(option, EuropeanOption):
                assert result.delta == pytest.approx(
                    0.5151635413397592, rel=0, abs=2e-5
                )
                assert result.gamma == pytest.approx(
                    0.015233318213016668, rel=0, abs=5e-6
                )
