from datetime import datetime, timedelta

import pytest
from kiyosi import pricing
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


def test_european_market_greeks_follow_analytic_risk_and_explicit_bumps():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    model = BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
    )
    context = PricingContext(
        model_parameters=model, spot_price=105, valuation_time=start
    )
    engine = pricing.FiniteDifferenceVanillaEngine(
        asset_step_count=1000, time_step_count=1000, asset_upper_boundary=400
    )
    greeks = ("vega", "volga", "vanna", "zomma", "rho", "dividend_rho")
    actual = engine.price_with_greeks(option, context, greeks)
    reference = pricing.AnalyticEuropeanEngine().price_with_greeks(
        option, context, greeks
    )
    assert actual.price == engine.price(option, context)
    for greek in greeks:
        assert getattr(actual, greek) == pytest.approx(
            getattr(reference, greek), rel=0.001, abs=1e-6
        )
    shift = 0.015625
    low, high = (
        engine.price(
            option, context.replace(model_parameters=model.replace(volatility=vol))
        )
        for vol in (0.2 - shift, 0.2 + shift)
    )
    explicit = engine.price_with_greeks(option, context, "vega", volatility_shift=shift)
    assert explicit.vega == pytest.approx((high - low) / (2 * shift), rel=0, abs=1e-9)


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


@pytest.mark.parametrize("contract", (AmericanOption, EuropeanOption))
def test_shared_time_slices_match_independent_vanilla_rollbacks(contract):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = contract(
        option_type="put", strike=100, effective_time=start, expiry_time=end
    )
    at = start + timedelta(days=30)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=at,
    )
    engine = pricing.FiniteDifferenceVanillaEngine(
        asset_step_count=1000, time_step_count=1000
    )
    actual = engine.price_with_greeks(option, context, ["theta", "charm", "color"])
    low, high = (
        engine.price_with_greeks(
            option,
            context.replace(valuation_time=at + timedelta(days=day)),
            ["delta", "gamma"],
        )
        for day in (-1, 1)
    )
    assert actual.price == engine.price(option, context)
    assert actual.delta is None and actual.gamma is None
    for greek, value in (("theta", "price"), ("charm", "delta"), ("color", "gamma")):
        expected = (getattr(high, value) - getattr(low, value)) * 365 / 2
        assert getattr(actual, greek) == pytest.approx(expected, rel=0.002, abs=1e-6)


def test_american_market_greeks_match_independent_grid_prices():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = AmericanOption(
        option_type="put", strike=100, effective_time=start, expiry_time=end
    )
    values = {"volatility": 0.25, "risk_free_rate": 0.0625, "dividend_yield": 0.03125}
    model = BlackScholesMertonParameters(**values)
    context = PricingContext(
        model_parameters=model, spot_price=100, valuation_time=start
    )
    engine = pricing.FiniteDifferenceVanillaEngine(asset_upper_boundary=400)
    shift = 0.015625
    actual = engine.price_with_greeks(
        option,
        context,
        ("vega", "rho", "dividend_rho"),
        volatility_shift=shift,
        rate_shift=shift,
    )
    assert actual.price == engine.price(option, context)
    for greek, parameter in (
        ("vega", "volatility"),
        ("rho", "risk_free_rate"),
        ("dividend_rho", "dividend_yield"),
    ):
        high, low = (
            engine.price(
                option,
                context.replace(
                    model_parameters=model.replace(
                        **{parameter: values[parameter] + bump}
                    )
                ),
            )
            for bump in (shift, -shift)
        )
        assert getattr(actual, greek) == (high - low) / (2 * shift)
