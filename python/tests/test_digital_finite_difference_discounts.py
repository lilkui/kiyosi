import math
from datetime import datetime

import pytest
from kiyosi import pricing
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
)


def test_finite_difference_digitals_resolve_low_volatility_with_carry():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    rates = {"risk_free_rate": 0.05, "dividend_yield": 0.15}
    sigma = 0.001
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(**rates, volatility=sigma),
        spot_price=110,
        valuation_time=start,
    )
    surface = LocalVolatilitySurface(
        times=[start, end], spots=[80, 120], values=[[sigma, sigma]] * 2
    )
    local = context.replace(
        model_parameters=LocalVolatilityParameters(**rates, surface=surface)
    )
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
    }
    engine = pricing.FiniteDifferenceDigitalEngine()
    for option in (
        CashOrNothingOption(**terms, payout=1),
        AssetOrNothingOption(**terms),
    ):
        expected = pricing.AnalyticDigitalEngine().price(option, context)
        for market in (context, local):
            assert engine.price(option, market) == pytest.approx(expected, rel=0.10)


def test_near_certain_digital_payout_allows_rollback_roundoff():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = CashOrNothingOption(
        option_type="call", strike=100, payout=1, effective_time=start, expiry_time=end
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.01
        ),
        spot_price=200,
        valuation_time=start,
    )
    price = pricing.FiniteDifferenceDigitalEngine().price(option, context)
    assert 1 - 1e-12 <= price <= 1


@pytest.mark.parametrize("rate", [-0.05, 0.05])
def test_implicit_euler_prices_digitals_with_near_certain_payouts(rate):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
    }
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=rate, volatility=0.15
        ),
        spot_price=250,
        valuation_time=start,
    )
    engine = pricing.FiniteDifferenceDigitalEngine(scheme="implicit_euler")
    for option, payout in (
        (CashOrNothingOption(**terms, payout=10), 10),
        (AssetOrNothingOption(**terms), 250),
    ):
        price = engine.price(option, context)
        expected = pricing.AnalyticDigitalEngine().price(option, context)
        assert price <= payout * math.exp(-rate) + 1e-12
        assert price == pytest.approx(expected, rel=1e-7)


@pytest.mark.parametrize("direction,sign", (("call", 1), ("put", -1)))
def test_finite_difference_digital_preserves_scaled_discounts(direction, sign):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    payout = 1e-308
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=-710, dividend_yield=-710, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.FiniteDifferenceDigitalEngine(
        asset_step_count=100, time_step_count=100_000
    )
    option = CashOrNothingOption(
        option_type=direction,
        strike=100,
        payout=payout,
        effective_time=start,
        expiry_time=end,
    )
    expected = (
        math.exp(math.log(payout) + 710) * 0.5 * math.erfc(sign * 0.1 / math.sqrt(2))
    )
    assert math.isclose(engine.price(option, context), expected, rel_tol=0.006)


def test_automatic_grid_resolves_a_short_dated_digital():
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 1, 1)
    option = CashOrNothingOption(
        option_type="call",
        strike=100.2,
        payout=1,
        effective_time=start,
        expiry_time=end,
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0.01, volatility=0.08
        ),
        spot_price=100,
        valuation_time=start,
    )
    expected = pricing.AnalyticDigitalEngine().price(option, context)
    assert pricing.FiniteDifferenceDigitalEngine().price(
        option, context
    ) == pytest.approx(expected, abs=1e-5)


def test_automatic_digital_vega_stays_stable_across_cell_width():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = CashOrNothingOption(
        option_type="call", strike=100, payout=1, effective_time=start, expiry_time=end
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.005
        ),
        spot_price=100.2,
        valuation_time=start,
    )
    expected = pricing.AnalyticDigitalEngine().price_with_greeks(
        option, context, "vega", volatility_shift=1e-5
    )
    actual = pricing.FiniteDifferenceDigitalEngine().price_with_greeks(
        option, context, "vega", volatility_shift=1e-5
    )
    assert actual.vega == pytest.approx(expected.vega, rel=1e-3)
