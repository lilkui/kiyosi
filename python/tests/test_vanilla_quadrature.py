"""Vanilla quadrature contracts and independent numerical references."""

import inspect
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import pytest
import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import numerical_greeks
from quantlib_reference import american, oracle
from support.terminal import assert_terminal_greeks
from test_binomial import END, GREEKS, START, contract, market


@pytest.mark.parametrize("direction", ("call", "put"))
def test_european_quadrature_and_sampling_preserve_quoted_forward(direction):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    risk, dividend = (
        RateTermStructure(reference_time=start, times=[end], factors=[factor])
        for factor in (0.75, 0.625)
    )
    context = market(1.2e17, risk, dividend, 1e-17, valuation=start)
    option = EuropeanOption(
        option_type=direction, strike=1e17, effective_time=start, expiry_time=end
    )
    expected = pricing.AnalyticEuropeanEngine().price(option, context)
    assert pricing.QuadratureVanillaEngine().price(option, context) == pytest.approx(
        expected, abs=1e-8, rel=0
    )
    mc = pricing.MonteCarloVanillaEngine(
        path_count=16_384, max_active_workers=1, seed=42, sampling="sobol"
    )
    assert mc.price(option, context) == pytest.approx(expected, abs=1e-3, rel=0)


@pytest.mark.parametrize("direction", ("call", "put"))
def test_european_quadrature_preserves_finite_large_prices(direction):
    option = contract(EuropeanOption, direction, strike=1e308)
    context = market(spot=1e308)
    expected = pricing.AnalyticEuropeanEngine().price(option, context)
    assert pricing.QuadratureVanillaEngine().price(option, context) == pytest.approx(
        expected, rel=1e-7
    )


def test_settings_signatures_and_conversion_boundaries(subtests):
    factory = pricing.QuadratureVanillaEngine
    engine = factory()
    assert engine.asset_step_count == 512
    assert engine.time_step_count == 200
    assert engine.integration_standard_deviations == 8
    signature = inspect.signature(factory)
    assert tuple(signature.parameters) == (
        ("asset_step_count", "time_step_count", "integration_standard_deviations")
    )
    for name in signature.parameters:
        assert signature.parameters[name].default == getattr(engine, name)
        assert signature.parameters[name].kind is inspect.Parameter.KEYWORD_ONLY
    assert (
        repr(engine)
        == "QuadratureVanillaEngine(asset_step_count=512, time_step_count=200, "
        "integration_standard_deviations=8.0)"
    )
    for field in ("asset_step_count", "time_step_count"):
        for value, error in (
            (True, TypeError),
            (4.5, TypeError),
            (2**31, OverflowError),
        ):
            with subtests.test(field=field, value=value), pytest.raises(error):
                factory(**{field: value})
    for fields in (
        {"asset_step_count": 3},
        {"time_step_count": 0},
        {"integration_standard_deviations": 0},
        {"integration_standard_deviations": math.nan},
        {"integration_standard_deviations": math.inf},
    ):
        with subtests.test(fields=fields), pytest.raises(InvalidInputError):
            factory(**fields)
    for value in (True, "8", None):
        with pytest.raises(TypeError):
            factory(integration_standard_deviations=value)
    with pytest.raises(AttributeError):
        engine.time_step_count = 400


def test_american_quadrature_rejects_excessive_rollback_work():
    engine = pricing.QuadratureVanillaEngine(
        asset_step_count=32_768, time_step_count=300
    )
    with pytest.raises(InvalidInputError, match="rollback"):
        engine.price(contract(AmericanOption, "put"), market(100, 0.05, 0.02, 0.2))


@pytest.mark.parametrize("direction", ("call", "put"))
@pytest.mark.parametrize(
    "spot,rate,dividend,sigma",
    (
        (80, 0.04, 0.01, 0.3),
        (100, 0.05, 0.02, 0.2),
        (120, -0.02, -0.03, 0.4),
        (100, 0, 0, 0.01),
    ),
)
def test_european_prices_and_all_greeks_match_analytic(
    subtests, direction, spot, rate, dividend, sigma
):
    engine = pricing.QuadratureVanillaEngine()
    analytic = pricing.AnalyticEuropeanEngine()
    tolerances = {
        "delta": 1e-4,
        "gamma": 1e-4,
        "speed": 1e-4,
        "theta": 0.0365,
        "charm": 0.0365,
        "color": 0.0365,
        "vega": 0.01,
        "volga": 1.0,
        "vanna": 0.01,
        "zomma": 0.01,
        "rho": 0.01,
        "dividend_rho": 0.01,
    }
    option = contract(EuropeanOption, direction)
    context = market(spot, rate, dividend, sigma)
    actual = engine.price_with_greeks(option, context, "all")
    expected = analytic.price_with_greeks(option, context, "all")
    assert actual.price == pytest.approx(expected.price, rel=0, abs=1e-6)
    for name in GREEKS:
        with subtests.test(direction=direction, spot=spot, greek=name):
            assert getattr(actual, name) == pytest.approx(
                getattr(expected, name), rel=0, abs=tolerances[name]
            )


def test_time_space_and_extent_converge_separately():
    option, context = contract(AmericanOption, "put"), market()

    def price(space, time, extent):
        return pricing.QuadratureVanillaEngine(
            asset_step_count=space,
            time_step_count=time,
            integration_standard_deviations=extent,
        ).price(option, context)

    inputs = {
        "option": "put",
        "strike": 100,
        "spot": 100,
        "rate": 0.05,
        "dividend": 0.02,
        "volatility": 0.2,
        "effective_time": START.date().isoformat(),
        "valuation": START.date().isoformat(),
        "expiry_time": END.date().isoformat(),
    }
    with ql.SavedSettings():
        reference = american.measure(inputs, "price")
    fine_error = abs(price(768, 800, 8) - reference)
    assert fine_error < 0.01
    for coarse in ((128, 800, 8), (768, 25, 8), (768, 800, 1)):
        coarse_error = abs(price(*coarse) - reference)
        assert fine_error < coarse_error
    european = contract(EuropeanOption)
    assert pricing.QuadratureVanillaEngine(time_step_count=1).price(
        european, context
    ) == pricing.QuadratureVanillaEngine(time_step_count=400).price(european, context)


def test_quantlib_european_profile(subtests):
    oracle.require_version()
    profile = next(
        p
        for p in oracle.load_fixture("numerical_engines.json")["profiles"]
        if p["engine"] == "QuadratureVanillaEngine"
    )
    engine = pricing.QuadratureVanillaEngine(**profile["settings"])
    for scenario in oracle.load_fixture("scenarios.json")["scenarios"]:
        inputs = scenario["inputs"]
        option = EuropeanOption(
            option_type=inputs["option"],
            strike=inputs["strike"],
            effective_time=datetime.fromisoformat(inputs["effective_time"]),
            expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
        )
        context = market(
            inputs["spot"],
            inputs["rate"],
            inputs["dividend"],
            inputs["volatility"],
            datetime.fromisoformat(inputs["valuation"]),
        )
        with ql.SavedSettings():
            reference, metadata = oracle.reference(inputs)
        if oracle.exclusions(inputs):
            reference = {
                k: v for k, v in reference.items() if k in ("price", "delta", "gamma")
            }
        wrapped = inputs["spot"] == 100 and inputs["expiry_time"] == "2026-01-06"
        names = sorted(reference.keys() - {"price"}) if wrapped else []
        result = (
            numerical_greeks(engine, option, context, names, **profile["shifts"])
            if names
            else None
        )
        for name, expected in reference.items():
            if name != "price" and name not in names:
                continue
            with subtests.test(case=scenario["case_id"], measure=name):
                assert (
                    getattr(result, name)
                    if result is not None
                    else engine.price(option, context)
                ) == pytest.approx(
                    expected,
                    rel=0,
                    abs=profile["numerical_tolerances"][name]
                    + metadata[f"uncertainty_{name}"],
                )


@pytest.mark.parametrize("kind", (EuropeanOption, AmericanOption))
def test_implied_volatility_reprices_both_exercise_styles(kind):
    engine, context = pricing.QuadratureVanillaEngine(), market()
    option = contract(kind, "put")
    quote = engine.price(option, context)
    sigma = pricing.implied_volatility(
        engine,
        option,
        context,
        quote,
        lower_bound=0.1,
        upper_bound=0.3,
        price_tolerance=1e-8,
    )
    assert sigma == pytest.approx(0.2, rel=0, abs=1e-6)


@pytest.mark.parametrize(
    "direction,spot,rate,dividend",
    (("put", 80, -0.02, -0.05), ("call", 120, -0.04, -0.01), ("call", 100, 0.05, 0)),
)
def test_american_negative_carry_and_european_limit(direction, spot, rate, dividend):
    engine = pricing.QuadratureVanillaEngine(
        asset_step_count=768,
        time_step_count=800,
        integration_standard_deviations=10,
    )
    option = contract(AmericanOption, direction)
    context = market(spot, rate, dividend, 0.3)
    inputs = {
        "option": direction,
        "strike": 100,
        "spot": spot,
        "rate": rate,
        "dividend": dividend,
        "volatility": 0.3,
        "effective_time": START.date().isoformat(),
        "valuation": START.date().isoformat(),
        "expiry_time": END.date().isoformat(),
    }
    with ql.SavedSettings():
        expected = american.measure(inputs, "price")
    price = engine.price(option, context)
    assert price == pytest.approx(expected, rel=0, abs=0.01)
    if direction == "call" and dividend == 0:
        european = pricing.AnalyticEuropeanEngine().price(
            contract(EuropeanOption), context
        )
        assert price == pytest.approx(european, rel=0, abs=0.0005)


def test_american_call_without_early_exercise_preserves_deep_tail():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = market(1e20, 0.05, 0, 0.1, valuation=start)
    option = AmericanOption(
        option_type="call", strike=2.5e20, effective_time=start, expiry_time=end
    )
    assert pricing.QuadratureVanillaEngine().price(option, context) == pytest.approx(
        3.98590645571204, abs=1e-10, rel=0
    )


def test_american_volga_and_dividend_rho_match_quantlib_bumps():
    engine = pricing.QuadratureVanillaEngine(
        asset_step_count=768,
        time_step_count=800,
        integration_standard_deviations=10,
    )
    inputs = {
        "option": "put",
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
        base = american.measure(inputs, "price")
        vol_up = american.measure(dict(inputs, volatility=0.31), "price")
        vol_down = american.measure(dict(inputs, volatility=0.29), "price")
        q_up = american.measure(dict(inputs, dividend=0.011), "price")
        q_down = american.measure(dict(inputs, dividend=0.009), "price")
    result = numerical_greeks(
        engine,
        contract(AmericanOption, "put"),
        market(100, 0.04, 0.01, 0.3),
        ["volga", "dividend_rho"],
        volatility_shift=0.01,
        rate_shift=0.001,
    )
    assert result.volga == pytest.approx(
        (vol_up - 2 * base + vol_down) / 0.01**2, rel=0, abs=0.5
    )
    assert result.dividend_rho == pytest.approx((q_up - q_down) / 0.002, rel=0, abs=0.2)


@pytest.mark.parametrize("kind", (EuropeanOption, AmericanOption))
def test_life_curve_rejection_and_numerical_failures_preserve_state(kind):
    engine = pricing.QuadratureVanillaEngine()
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[math.exp(-0.05), (math.exp(-0.05)) * 0.5],
    )
    option = contract(kind, "put")
    baseline = engine.price(option, market())
    for at in (END + timedelta(microseconds=1), END + timedelta(days=1)):
        with pytest.raises(InvalidInputError):
            engine.price(option, market(valuation=at))
    expiry = engine.price_with_greeks(option, market(spot=80, valuation=END), "all")
    assert expiry.price == 20
    assert_terminal_greeks(expiry, -1)
    for at in (START, END):
        for fields in ({"rate": curve}, {"dividend": curve}):
            context = market(valuation=at, **fields)
            with pytest.raises(UnsupportedOperationError):
                engine.price(option, context)
            with pytest.raises(UnsupportedOperationError):
                engine.price_with_greeks(option, context, [])
    with pytest.raises(ComputationFailureError):
        engine.price(option, market(sigma=1e200))
    almost_expired = datetime(2025, 12, 31, 23, 59, 59, 999999)
    assert engine.price(option, market(spot=80, valuation=almost_expired)) >= 19.999
    assert engine.price(option, market()) == baseline


def test_shared_engine_supports_concurrent_pricing():
    engine = pricing.QuadratureVanillaEngine(asset_step_count=128, time_step_count=50)
    option = contract(AmericanOption, "put")
    contexts = [market(spot=spot) for spot in (80, 100, 120)]
    expected = [engine.price(option, context) for context in contexts]
    with ThreadPoolExecutor(max_workers=3) as pool:
        actual = list(pool.map(lambda context: engine.price(option, context), contexts))
    assert actual == expected


@pytest.mark.parametrize("spot", [50, 100])
def test_american_one_interval_is_exercise_now_or_at_expiry_without_a_grid(spot):
    engine = pricing.QuadratureVanillaEngine(
        asset_step_count=2**31 - 1, time_step_count=1
    )
    context = market(spot=spot)
    continuation = pricing.AnalyticEuropeanEngine().price(
        contract(EuropeanOption, "put"), context
    )
    actual = engine.price(contract(AmericanOption, "put"), context)
    assert actual == pytest.approx(max(100 - spot, continuation), rel=1e-12)


def test_american_quadrature_rejects_an_oversized_grid_before_rollback():
    engine = pricing.QuadratureVanillaEngine(
        asset_step_count=2_000_000, time_step_count=2
    )
    with pytest.raises(InvalidInputError, match="workspace"):
        engine.price(contract(AmericanOption, "put"), market())


def test_american_uniform_convolution_accepts_a_refined_grid():
    option = contract(AmericanOption, "put")
    context = market(100, 0.05, 0.02, 0.2)
    reference = pricing.QuadratureVanillaEngine(asset_step_count=2048).price(
        option, context
    )
    refined = pricing.QuadratureVanillaEngine(asset_step_count=4096).price(
        option, context
    )
    assert refined == pytest.approx(reference, rel=0, abs=3e-6)


def test_american_quadrature_retains_european_lower_bound():
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "expiry_time": START + timedelta(days=3650),
    }
    context = market(100, -0.02, 0, 1.5)
    floor = pricing.AnalyticEuropeanEngine().price(EuropeanOption(**terms), context)
    price = pricing.QuadratureVanillaEngine().price(AmericanOption(**terms), context)
    assert price >= floor


def test_european_quadrature_counts_panels_across_sections():
    engine = pricing.QuadratureVanillaEngine(asset_step_count=125_000_001)
    instrument = contract(EuropeanOption, "call")
    with pytest.raises(InvalidInputError, match="panels"):
        engine.price(instrument, market(100, 0.05, 0.02, 0.2))
    assert engine.price(instrument, market(100, 0.05, 0.02, 0)) > 0
