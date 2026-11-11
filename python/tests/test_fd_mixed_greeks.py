from datetime import datetime

import pytest
from kiyosi import pricing
from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


@pytest.mark.parametrize("option_type", ("call", "put"))
def test_one_sided_native_time_greeks_remain_selective_and_accurate(
    subtests, option_type
):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
        ),
        spot_price=100,
        valuation_time=start,
    )
    option = EuropeanOption(
        option_type=option_type,
        strike=100,
        effective_time=start,
        expiry_time=end,
    )
    for engine in (
        pricing.CoxRossRubinsteinVanillaEngine(step_count=1000),
        pricing.FiniteDifferenceVanillaEngine(),
    ):
        for greek, dependency in (("charm", "delta"), ("color", "gamma")):
            with subtests.test(
                option_type=option_type, engine=type(engine), greek=greek
            ):
                single = engine.price_with_greeks(option, context, [greek])
                joint = engine.price_with_greeks(option, context, [greek, dependency])
                expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
                    option, context, [greek]
                )
                assert single.price == joint.price
                assert getattr(single, greek) == getattr(joint, greek)
                assert single.delta is None
                assert single.gamma is None
                reference = getattr(expected, greek)
                assert getattr(single, greek) == pytest.approx(
                    reference, rel=0, abs=abs(reference) * 0.02 + 1e-7
                )


def test_charm_at_effective_date_remains_selective():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
        ),
        spot_price=100,
        valuation_time=start,
    )
    expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
        option, context, ["charm"]
    )
    actual = pricing.FiniteDifferenceVanillaEngine(
        asset_step_count=400, time_step_count=400
    ).price_with_greeks(option, context, ["charm"])
    assert actual.charm == pytest.approx(
        expected.charm, rel=0, abs=abs(expected.charm) * 0.02 + 1e-7
    )
    assert actual.delta is None
    assert actual.gamma is None
    assert actual.color is None


def test_mixed_greeks_track_analytic_sensitivities(subtests):
    option = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=datetime(2025, 1, 1),
        expiry_time=datetime(2026, 1, 1),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
        ),
        spot_price=100,
        valuation_time=datetime(2025, 7, 1),
    )
    request = [
        "vanna",
        "zomma",
        "charm",
        "color",
    ]
    expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
        option, context, request
    )
    actual = pricing.FiniteDifferenceVanillaEngine(
        asset_step_count=400, time_step_count=400
    ).price_with_greeks(option, context, request)
    for name in request:
        with subtests.test(greek=name):
            assert getattr(actual, name) == pytest.approx(
                getattr(expected, name),
                rel=0,
                abs=abs(getattr(expected, name)) * 0.02 + 1e-6,
            )
    assert actual.delta is None
    assert actual.gamma is None
