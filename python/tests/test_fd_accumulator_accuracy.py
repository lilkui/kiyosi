from datetime import datetime

import pytest
from kiyosi import pricing
from test_accumulator import contract, exposure, market, terminal_reference


def test_market_greeks_and_implied_volatility_preserve_independent_results(subtests):
    option = contract(datetime(2025, 4, 1))
    context = market(100.25, datetime(2025, 1, 4, 12))
    engine = pricing.FiniteDifferenceAccumulatorEngine(
        asset_step_count=80, time_step_count=80
    )
    joint = engine.price_with_greeks(option, context, greeks="all")
    assert joint.theta is not None
    for greek in (
        "delta",
        "gamma",
        "speed",
        "vega",
        "vanna",
        "zomma",
        "rho",
        "theta",
        "charm",
        "color",
    ):
        with subtests.test(greek=greek):
            single = engine.price_with_greeks(option, context, greek)
            assert single.price == joint.price
            assert getattr(single, greek) == getattr(joint, greek)
    quote = engine.price(option, context)
    recovered = pricing.implied_volatility(
        engine,
        option,
        context,
        quote,
        lower_bound=0.15,
        upper_bound=0.25,
    )
    assert recovered == pytest.approx(0.2, rel=0, abs=1e-6)


@pytest.mark.parametrize(
    "scheme", ("explicit_euler", "implicit_euler", "crank_nicolson", "rannacher")
)
@pytest.mark.parametrize("knock_out", (99.7, 100, 100.3, 109.75, 110, 110.25))
def test_terminal_averaging_converges_against_analytic_prices_and_greeks(
    scheme, knock_out
):
    context = market(100.25, datetime(2025, 1, 3, 12))
    option = contract(datetime(2025, 1, 6), knock_out)
    expected = terminal_reference(option, context)
    h = 0.01
    low = terminal_reference(option, market(100.25 - h, context.valuation_time))
    high = terminal_reference(option, market(100.25 + h, context.valuation_time))
    results = [
        pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=nodes,
            time_step_count=2000,
            scheme=scheme,
            asset_upper_boundary=440,
        ).price_with_greeks(option, context, ["delta", "gamma"])
        for nodes in (400, 1600)
    ]
    coarse, fine = results
    assert abs(coarse.price - expected) < 0.01
    assert abs(fine.price - expected) < 0.001
    assert abs(fine.price - expected) < abs(coarse.price - expected)
    assert fine.delta == pytest.approx((high - low) / (2 * h), rel=0, abs=0.002)
    assert fine.gamma == pytest.approx(
        (high - 2 * expected + low) / h**2, rel=0, abs=0.002
    )


@pytest.mark.parametrize("knock_out", (109.75, 110, 110.25))
def test_daily_event_averaging_and_damping_stabilize_prices_and_greeks(knock_out):
    context = market()
    reference_engine = pricing.QuadratureAccumulatorEngine(asset_step_count=1024)
    option = contract(datetime(2025, 4, 1), knock_out)
    reference = reference_engine.price_with_greeks(option, context, ["delta", "gamma"])
    for scheme in ("explicit_euler", "implicit_euler"):
        actual = pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=400,
            time_step_count=2000,
            scheme=scheme,
            asset_upper_boundary=440,
        ).price(option, context)
        assert abs(actual - reference.price) / exposure(option, context) < 1e-4
    coarse, fine = [
        pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=nodes,
            time_step_count=2000,
            asset_upper_boundary=440,
        ).price_with_greeks(option, context, ["delta", "gamma"])
        for nodes in (800, 3200)
    ]
    scale = exposure(option, context)
    assert abs(coarse.price - reference.price) / scale < 2e-5
    assert abs(fine.price - reference.price) / scale < 1e-6
    for greek in ("delta", "gamma"):
        coarse_error = abs(getattr(coarse, greek) - getattr(reference, greek))
        fine_error = abs(getattr(fine, greek) - getattr(reference, greek))
        assert fine_error < coarse_error
        assert fine_error < 0.01
    short_grid = pricing.FiniteDifferenceAccumulatorEngine(
        asset_step_count=1600,
        time_step_count=200,
        asset_upper_boundary=440,
    ).price_with_greeks(option, context, ["gamma"])
    assert short_grid.gamma == pytest.approx(reference.gamma, rel=0, abs=0.15)
