"""Volga and parallel dividend-yield sensitivities through the public API."""

import math
from datetime import datetime

import pytest
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    FiniteDifferenceVanillaEngine,
    MonteCarloVanillaEngine,
    numerical_greeks,
)
from quantlib_reference import oracle
from test_binomial import END, START, market


@pytest.mark.parametrize("direction", ("call", "put"))
@pytest.mark.parametrize("spot", (80, 100, 120))
@pytest.mark.parametrize("dividend", (-0.02, 0, 0.02))
def test_units_selection_and_independent_quantlib_references(
    subtests, quantlib_settings, direction, spot, dividend
):
    engine = AnalyticEuropeanEngine()
    option = EuropeanOption(
        option_type=direction, strike=100, effective_time=START, expiry_time=END
    )
    context = market(spot=spot, rate=0.05, dividend=dividend, sigma=0.2)
    inputs = {
        "option": direction,
        "spot": spot,
        "strike": 100,
        "effective_time": START.date().isoformat(),
        "expiry_time": END.date().isoformat(),
        "valuation": START.date().isoformat(),
        "rate": 0.05,
        "dividend": dividend,
        "volatility": 0.2,
    }
    reference = oracle.vanilla_option(inputs)
    expected_dividend = reference.dividendRho()
    # Differentiate independent QuantLib vega at two resolutions.
    volga = []
    for h in (0.0001, 0.00005):
        up = oracle.vanilla_option({**inputs, "volatility": 0.2 + h})
        down = oracle.vanilla_option({**inputs, "volatility": 0.2 - h})
        volga.append((up.vega() - down.vega()) / (2 * h))
    assert volga[0] == pytest.approx(volga[1], rel=0, abs=1e-4)
    expected = {
        "volga": volga[1],
        "dividend_rho": expected_dividend,
    }
    full = engine.price_with_greeks(option, context, "all")
    for name, value in expected.items():
        with subtests.test(
            direction=direction, spot=spot, dividend=dividend, name=name
        ):
            selected = engine.price_with_greeks(option, context, name)
            assert selected.require(name) == pytest.approx(
                value, rel=0, abs=(1e-4 if name == "volga" else 1e-6)
            )
            assert selected.require(name) == full.require(name)
            assert selected.delta is None
            assert selected.price == full.price
            other = "volga" if name == "dividend_rho" else "dividend_rho"
            assert getattr(selected, other) is None
    if spot == 100 and dividend == 0.02:
        assert full.volga == pytest.approx(2.36882234437609, rel=0, abs=1e-10)
        expected_q = -58.6851146134764 if direction == "call" else 39.3347527171254
        assert full.dividend_rho == pytest.approx(expected_q, rel=0, abs=1e-10)


def test_parallel_dividend_curves_numerical_bumps_and_availability():
    option = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    curve = RateTermStructure(
        reference_time=START,
        times=[datetime(2025, 7, 1), END],
        factors=[math.exp(-0.01 * 181 / 365), math.exp(-0.03)],
    )
    context = market().replace(
        model_parameters=market().model_parameters.replace(dividend_yield=curve)
    )
    engine = FiniteDifferenceVanillaEngine(asset_step_count=400, time_step_count=400)
    exact = AnalyticEuropeanEngine().price_with_greeks(option, context, "all")
    assert exact.dividend_rho == pytest.approx(
        -context.spot_price * exact.delta, rel=0, abs=1e-10
    )
    for bump in (0.0001, 0.0002):
        actual = numerical_greeks(
            engine,
            option,
            context,
            [
                "volga",
                "dividend_rho",
            ],
            volatility_shift=bump,
            rate_shift=bump,
        )
        assert actual.volga == pytest.approx(exact.volga, rel=0, abs=0.2)
        assert actual.dividend_rho == pytest.approx(exact.dividend_rho, rel=0, abs=0.02)
        shifted_prices = [
            engine.price(
                option,
                context.replace(
                    model_parameters=context.model_parameters.replace(
                        dividend_yield=curve.shifted(h)
                    )
                ),
            )
            for h in (bump, -bump)
        ]
        assert actual.dividend_rho == pytest.approx(
            (shifted_prices[0] - shifted_prices[1]) / (2 * bump), rel=0, abs=1e-8
        )
        assert actual.delta is None
        assert actual.rho is None
    flat = market(dividend=0.02)
    actual = numerical_greeks(
        engine,
        option,
        flat,
        ["volga", "dividend_rho"],
        volatility_shift=0.2,
    )
    assert actual.volga is None
    assert actual.dividend_rho is not None
    actual = numerical_greeks(
        engine,
        option,
        flat,
        ["volga", "dividend_rho"],
        rate_shift=math.ulp(0.0),
    )
    assert actual.volga is not None
    assert actual.dividend_rho is None
    assert context.model_parameters.dividend_curve.factors == curve.factors


def test_american_dividend_stencil_crossing_zero_keeps_one_estimator():
    option = AmericanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    engine = MonteCarloVanillaEngine(path_count=2048, step_count=6, seed=42)
    requested = 0.0001
    for dividend in (-requested / 2, 0, requested / 2, requested):
        context = market(rate=0, dividend=dividend)
        actual = engine.price_with_greeks(option, context, "dividend_rho")
        shift = (dividend + requested) - dividend
        if dividend - (dividend - shift) != shift:
            assert actual.dividend_rho is None
            continue
        # A subnormal negative rate forces the exercise grid without changing BSM coefficients.
        up, down = [
            engine.price(option, market(rate=-math.ulp(0.0), dividend=dividend + bump))
            for bump in (shift, -shift)
        ]
        assert actual.dividend_rho == pytest.approx(
            (up - down) / (2 * shift), rel=0, abs=1e-8
        )
        assert actual.price == engine.price(option, context)
        assert actual.rho is None
