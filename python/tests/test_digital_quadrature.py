"""Digital Simpson integration, public contracts and independent references."""

import inspect
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import pytest
import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import HestonParameters, PricingContext, RateTermStructure
from kiyosi.pricing import numerical_greeks
from quantlib_reference import digital, oracle
from support.terminal import assert_terminal_greeks
from test_binomial import END, GREEKS, START, contract, market

DIGITAL = (CashOrNothingOption, AssetOrNothingOption)


def test_settings_signatures_and_conversion_boundaries(subtests):
    factory = pricing.QuadratureDigitalEngine
    engine = factory()
    assert engine.asset_step_count == 512
    assert engine.integration_standard_deviations == 8
    signature = inspect.signature(factory)
    assert tuple(signature.parameters) == (
        ("asset_step_count", "integration_standard_deviations")
    )
    for name, parameter in signature.parameters.items():
        assert parameter.kind is inspect.Parameter.KEYWORD_ONLY
        assert parameter.default == getattr(engine, name)
    assert (
        repr(engine) == "QuadratureDigitalEngine(asset_step_count=512, "
        "integration_standard_deviations=8.0)"
    )
    for value, error in (
        (True, TypeError),
        (4.5, TypeError),
        (2**31, OverflowError),
    ):
        with subtests.test(value=value), pytest.raises(error):
            factory(asset_step_count=value)
    for fields in (
        {"asset_step_count": 3},
        {"integration_standard_deviations": 0},
        {"integration_standard_deviations": -1},
        {"integration_standard_deviations": math.nan},
        {"integration_standard_deviations": math.inf},
    ):
        with subtests.test(fields=fields), pytest.raises(InvalidInputError):
            factory(**fields)
    for value in (True, "8", None):
        with pytest.raises(TypeError):
            factory(integration_standard_deviations=value)
    with pytest.raises(TypeError):
        factory(512)
    with pytest.raises(AttributeError):
        engine.asset_step_count = 1024
    with pytest.raises(UnsupportedOperationError):
        engine.price(contract(EuropeanOption), market())


def test_quantlib_prices_and_greeks_across_all_40_scenarios(subtests):
    oracle.require_version()
    config = oracle.load_fixture("digital.json")
    profile = next(
        p for p in config["profiles"] if p["engine"] == "QuadratureDigitalEngine"
    )
    engine = pricing.QuadratureDigitalEngine(**profile["settings"])
    cases = list(digital.scenarios(config))
    assert len(cases) == 40
    for identifier, inputs in cases:
        kind = (
            CashOrNothingOption if inputs["payoff"] == "cash" else AssetOrNothingOption
        )
        option = kind(
            option_type=inputs["option"],
            strike=inputs["strike"],
            effective_time=datetime.fromisoformat(inputs["effective_time"]),
            expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
            **({"payout": inputs["payout"]} if inputs["payoff"] == "cash" else {}),
        )
        context = market(
            inputs["spot"],
            inputs["rate"],
            inputs["dividend"],
            inputs["volatility"],
            datetime.fromisoformat(inputs["valuation"]),
        )
        with ql.SavedSettings():
            reference, uncertainty = oracle.reference(inputs, digital.STABILITY)
        smooth = (option.expiry_time - context.valuation_time).days > 2
        names = tuple(reference.keys() - {"price"}) if smooth else ("delta", "gamma")
        shifts = dict(profile["shifts"])
        if not smooth:
            shifts["spot_shift"] = 0.001
        result = numerical_greeks(engine, option, context, names, **shifts)
        for name in ("price", *names):
            with subtests.test(case=identifier, measure=name):
                assert getattr(result, name) == pytest.approx(
                    reference[name],
                    rel=0,
                    abs=profile["numerical_tolerances"][name]
                    + uncertainty[f"uncertainty_{name}"],
                )
        assert pricing.QuadratureDigitalEngine().price(
            option, context
        ) == pytest.approx(reference["price"], rel=0, abs=1e-7)


@pytest.mark.parametrize("kind", DIGITAL)
@pytest.mark.parametrize("direction", ("call", "put"))
def test_step_count_and_extent_converge_separately(kind, direction):
    option = contract(kind, direction)
    context = market(120, 0.04, 0.01, 0.3)
    expected = pricing.AnalyticDigitalEngine().price(option, context)

    coarse_space, coarse_extent, fine = (
        abs(
            pricing.QuadratureDigitalEngine(
                asset_step_count=steps,
                integration_standard_deviations=extent,
            ).price(option, context)
            - expected
        )
        for steps, extent in ((16, 8), (2048, 1), (2048, 8))
    )
    assert fine < coarse_space
    assert fine < coarse_extent
    assert fine < 1e-8
    odd = pricing.QuadratureDigitalEngine(asset_step_count=511)
    even = pricing.QuadratureDigitalEngine(asset_step_count=512)
    assert odd.price(option, context) == even.price(option, context)


@pytest.mark.parametrize("kind", DIGITAL)
@pytest.mark.parametrize("direction", ("call", "put"))
def test_all_greeks_selection_volga_dividend_rho_and_volatility_recovery(
    subtests, kind, direction
):
    engine = pricing.QuadratureDigitalEngine(asset_step_count=2048)
    context = market(100, 0.04, 0.01, 0.3)
    tolerances = {
        "delta": 1e-6,
        "gamma": 1e-6,
        "speed": 1e-6,
        "theta": 0.000365,
        "charm": 0.000365,
        "color": 0.000365,
        "vega": 0.0001,
        "volga": 0.01,
        "vanna": 0.0001,
        "zomma": 0.0001,
        "rho": 0.0001,
        "dividend_rho": 0.0001,
    }
    option = contract(kind, direction)
    result = engine.price_with_greeks(option, context, "all")
    expected = pricing.AnalyticDigitalEngine().price_with_greeks(option, context, "all")
    for name in GREEKS:
        with subtests.test(kind=kind, direction=direction, measure=name):
            assert getattr(result, name) == pytest.approx(
                getattr(expected, name), rel=0, abs=tolerances[name]
            )
    selected = engine.price_with_greeks(option, context, "delta")
    assert selected.delta == result.delta
    assert selected.gamma is None
    inputs = {
        "payoff": "cash" if kind is CashOrNothingOption else "asset",
        "payout": 10,
        "option": direction,
        "strike": 100,
        "spot": 100,
        "rate": 0.04,
        "dividend": 0.01,
        "volatility": 0.3,
        "effective_time": START.date().isoformat(),
        "valuation": START.date().isoformat(),
        "expiry_time": END.date().isoformat(),
    }
    with ql.SavedSettings():
        base = oracle.measure(inputs, "price")
        vol_up = oracle.measure(dict(inputs, volatility=0.31), "price")
        vol_down = oracle.measure(dict(inputs, volatility=0.29), "price")
        q_up = oracle.measure(dict(inputs, dividend=0.011), "price")
        q_down = oracle.measure(dict(inputs, dividend=0.009), "price")
    bumped = numerical_greeks(
        engine,
        option,
        context,
        [
            "volga",
            "dividend_rho",
        ],
        volatility_shift=0.01,
        rate_shift=0.001,
    )
    assert bumped.volga == pytest.approx(
        (vol_up - 2 * base + vol_down) / 0.01**2, rel=0, abs=0.0001
    )
    assert bumped.dividend_rho == pytest.approx(
        (q_up - q_down) / 0.002, rel=0, abs=0.000001
    )
    iv_context = market()
    quote = pricing.AnalyticDigitalEngine().price(option, iv_context)
    sigma = pricing.implied_volatility(
        engine, option, iv_context, quote, lower_bound=0.1, upper_bound=0.22
    )
    assert sigma == pytest.approx(0.2, rel=0, abs=1e-7)
    with pytest.raises(RootNotBracketedError):
        pricing.implied_volatility(
            engine,
            option,
            iv_context,
            1e4,
            lower_bound=0.0,
            upper_bound=4.0,
        )


def test_strict_expiry_microsecond_life_and_rejections_preserve_state():
    engine = pricing.QuadratureDigitalEngine()
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.95, (0.95) * 0.5],
    )
    for kind in DIGITAL:
        for direction in ("call", "put"):
            option = contract(kind, direction)
            baseline = engine.price(option, market())
            for spot in (80, 100, 120):
                result = engine.price_with_greeks(
                    option, market(spot=spot, valuation=END), "all"
                )
                itm = spot > 100 if direction == "call" else spot < 100
                expected = (10 if kind is CashOrNothingOption else spot) if itm else 0
                assert result.price == expected
                assert_terminal_greeks(
                    result,
                    None
                    if abs(spot - 100) <= 0.01
                    else int(expected > 0 and kind is not CashOrNothingOption),
                )
            with pytest.raises(InvalidInputError):
                engine.price(option, market(valuation=END + timedelta(microseconds=1)))
            almost_expired = datetime(2025, 12, 31, 23, 59, 59, 999999)
            context = market(valuation=almost_expired)
            assert engine.price(option, context) == pytest.approx(
                pricing.AnalyticDigitalEngine().price(option, context),
                rel=0,
                abs=1e-7,
            )
            for at in (START, END):
                for fields in ({"rate": curve}, {"dividend": curve}):
                    context = market(valuation=at, **fields)
                    with pytest.raises(UnsupportedOperationError):
                        engine.price(option, context)
                    with pytest.raises(UnsupportedOperationError):
                        engine.price_with_greeks(option, context, [])
            with pytest.raises(InvalidInputError):
                numerical_greeks(engine, option, market(), "delta", spot_shift=0)
            with pytest.raises(ComputationFailureError):
                engine.price(option, market(rate=1.79e308, dividend=-1.79e308))
            assert engine.price(option, market()) == baseline
    heston = PricingContext(
        model_parameters=HestonParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            initial_variance=0.04,
            mean_reversion_speed=1,
            long_run_variance=0.04,
            vol_of_vol=0.3,
            correlation=-0.5,
        ),
        spot_price=100,
        valuation_time=START,
    )
    with pytest.raises(UnsupportedOperationError):
        engine.price(contract(CashOrNothingOption), heston)


def test_scaled_discounts_and_tiny_volatility():
    engine = pricing.QuadratureDigitalEngine(asset_step_count=2048)
    for kind in DIGITAL:
        for direction in ("call", "put"):
            option = kind(
                option_type=direction,
                strike=1.75e308,
                effective_time=START,
                expiry_time=END,
                **({"payout": 1.75e308} if kind is CashOrNothingOption else {}),
            )
            context = market(1.75e308, -0.03, -0.05)
            expected = pricing.AnalyticDigitalEngine().price(option, context)
            assert math.isclose(engine.price(option, context), expected, rel_tol=1e-10)
            with pytest.raises(ComputationFailureError):
                engine.price(option, market(1.75e308, -0.98, -1))
    for rate in (750, -750):
        amount = 1e300 if rate > 0 else 1e-300
        context = market(amount, rate, rate)
        for kind in DIGITAL:
            for direction in ("call", "put"):
                option = kind(
                    option_type=direction,
                    strike=amount,
                    effective_time=START,
                    expiry_time=END,
                    **({"payout": amount} if kind is CashOrNothingOption else {}),
                )
                expected = pricing.AnalyticDigitalEngine().price(option, context)
                assert math.isclose(
                    engine.price(option, context), expected, rel_tol=1e-10
                )
    for sigma in (1e-200, math.ulp(0.0)):
        for spot in (80, 100, 120):
            context = market(
                spot, 0, 0, sigma, datetime(2025, 12, 31, 23, 59, 59, 999999)
            )
            for kind in DIGITAL:
                for direction in ("call", "put"):
                    option = contract(kind, direction)
                    assert engine.price(option, context) == pytest.approx(
                        pricing.AnalyticDigitalEngine().price(option, context),
                        rel=0,
                        abs=1e-10,
                    )


def test_shared_engine_supports_concurrent_pricing():
    engine = pricing.QuadratureDigitalEngine()
    option = contract(AssetOrNothingOption, "put")
    contexts = [market(spot=spot) for spot in (80, 100, 120)]
    expected = [engine.price(option, context) for context in contexts]
    with ThreadPoolExecutor(max_workers=3) as pool:
        actual = list(pool.map(lambda context: engine.price(option, context), contexts))
    assert actual == expected
