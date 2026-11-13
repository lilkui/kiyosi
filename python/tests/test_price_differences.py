"""Scalar-price references preserve intraday contract boundaries."""

from datetime import datetime

import pytest
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticEuropeanEngine, BjerksundStenslandVanillaEngine
from price_differences import price_differences


@pytest.mark.parametrize(
    "contract, engine",
    [
        (EuropeanOption, AnalyticEuropeanEngine()),
        (AmericanOption, BjerksundStenslandVanillaEngine()),
    ],
)
def test_time_sensitivities_respect_intraday_contract_life(contract, engine):
    option = contract(
        option_type="call",
        strike=100,
        effective_time=datetime(2025, 1, 2, 9, microsecond=123456),
        expiry_time=datetime(2025, 1, 2, 17, microsecond=123456),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0
        ),
        spot_price=110,
        valuation_time=datetime(2025, 1, 2, 12, microsecond=123456),
    )

    result = price_differences(
        engine, option, context, greeks=("theta", "charm", "color")
    )

    assert result.price == pytest.approx(10, abs=1e-12)
    assert result.theta == 0
    assert result.charm == 0
    assert result.color == 0
