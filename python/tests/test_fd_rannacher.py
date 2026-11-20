import math
from datetime import datetime

import pytest
from kiyosi import ComputationFailureError, pricing
from kiyosi.instruments import (
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


def test_damping_still_rejects_oscillatory_prices():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = CashOrNothingOption(
        option_type="call",
        strike=100.5,
        payout=100,
        effective_time=start,
        expiry_time=end,
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=10
        ),
        spot_price=20,
        valuation_time=start,
    )
    engine = pricing.FiniteDifferenceDigitalEngine(
        asset_step_count=1000, time_step_count=3, asset_upper_boundary=400
    )
    with pytest.raises(ComputationFailureError) as rejected:
        engine.price(option, context)
    assert type(rejected.value) is ComputationFailureError


@pytest.mark.parametrize("rate", (-0.05, 0.05))
@pytest.mark.parametrize("steps", (1, 200))
def test_payoff_bounds_allow_known_carry_truncation(subtests, rate, steps):
    start, end = datetime(2025, 1, 6), datetime(2026, 1, 6)
    dividend = 0.4 * rate
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.FiniteDifferenceDigitalEngine(
        asset_step_count=800, time_step_count=steps
    )
    terms = {
        "option_type": "call",
        "strike": 1,
        "effective_time": start,
        "expiry_time": end,
    }
    for option, upper, delta in (
        (
            CashOrNothingOption(**terms, payout=100),
            100 * math.exp(-rate),
            0,
        ),
        (
            AssetOrNothingOption(**terms),
            100 * math.exp(-dividend),
            math.exp(-dividend),
        ),
    ):
        with subtests.test(rate=rate, steps=steps, option=type(option).__name__):
            actual = engine.price_with_greeks(option, context, ["delta", "gamma"])
            assert actual.price >= 0
            assert actual.price <= upper
            assert actual.price == pytest.approx(upper, rel=0, abs=1e-8)
            assert actual.delta == pytest.approx(delta, rel=0, abs=0.001)
            assert actual.gamma == pytest.approx(0, rel=0, abs=1e-8)


@pytest.mark.parametrize("steps", (8, 32))
@pytest.mark.parametrize("option_type", ("call", "put"))
def test_coarse_rannacher_time_grids_track_analytic_greeks(
    subtests, steps, option_type
):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100.25,
        valuation_time=start,
    )
    terms = {
        "option_type": option_type,
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
    }
    for option, factory, analytic, tolerances in (
        (
            EuropeanOption(**terms),
            pricing.FiniteDifferenceVanillaEngine,
            pricing.AnalyticEuropeanEngine,
            (0.02, 0.001, 0.0005),
        ),
        (
            CashOrNothingOption(**terms, payout=10),
            pricing.FiniteDifferenceDigitalEngine,
            pricing.AnalyticDigitalEngine,
            (0.02, 0.002, 0.0002),
        ),
        (
            AssetOrNothingOption(**terms),
            pricing.FiniteDifferenceDigitalEngine,
            pricing.AnalyticDigitalEngine,
            (0.1, 0.02, 0.002),
        ),
    ):
        with subtests.test(
            steps=steps,
            option_type=option_type,
            option=type(option).__name__,
        ):
            engine = factory(
                asset_step_count=1600,
                time_step_count=steps,
                asset_upper_boundary=400,
            )
            actual = engine.price_with_greeks(option, context, ["delta", "gamma"])
            expected = analytic().price_with_greeks(option, context, ["delta", "gamma"])
            for name, tolerance in zip(
                ("price", "delta", "gamma"), tolerances, strict=True
            ):
                assert getattr(actual, name) == pytest.approx(
                    getattr(expected, name), rel=0, abs=tolerance
                )
