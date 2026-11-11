"""Finite-difference public contracts and independent reference profiles."""

import importlib
import math
import numbers
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from functools import partial
from typing import get_args

import pytest
import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    AnalyticDigitalEngine,
    AnalyticEuropeanEngine,
    FiniteDifferenceBarrierEngine,
    FiniteDifferenceDigitalEngine,
    FiniteDifferenceDoubleBarrierEngine,
    FiniteDifferenceScheme,
    FiniteDifferenceVanillaEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.history import history_fields
from support.terminal import assert_terminal_greeks
from test_binomial import DIGITAL, END, START, VANILLA, contract, market

KINDS = (*VANILLA, *DIGITAL)
FACTORIES = (
    FiniteDifferenceVanillaEngine,
    FiniteDifferenceDigitalEngine,
    FiniteDifferenceBarrierEngine,
    FiniteDifferenceDoubleBarrierEngine,
)
SCHEMES = ("explicit_euler", "implicit_euler", "crank_nicolson", "rannacher")


def engine_for(kind, **settings):
    return (
        FiniteDifferenceVanillaEngine
        if kind in VANILLA
        else FiniteDifferenceDigitalEngine
    )(**settings)


def test_coarse_automatic_grids_resolve_short_and_annual_payoffs_and_spot_greeks(
    subtests,
):
    engine = FiniteDifferenceVanillaEngine(asset_step_count=200, time_step_count=50)
    for end in (START + timedelta(days=1), END):
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction,
                strike=100,
                effective_time=START,
                expiry_time=end,
            )
            for spot in (99.875, 100, 100.125):
                with subtests.test(end=end, direction=direction, spot=spot):
                    context = market(spot=spot, rate=0.05, dividend=0.02, sigma=0.2)
                    expected = AnalyticEuropeanEngine().price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    actual = engine.price_with_greeks(
                        option, context, ["delta", "gamma"]
                    )
                    for name, tolerance in (
                        ("price", 0.002),
                        ("delta", 1e-4),
                        ("gamma", 3e-4),
                    ):
                        assert getattr(actual, name) == pytest.approx(
                            getattr(expected, name), rel=0, abs=tolerance
                        )
                    assert actual.price == engine.price(option, context)


def test_automatic_mesh_policy_is_preserved_for_bumped_greeks():
    engine = FiniteDifferenceVanillaEngine(asset_step_count=200, time_step_count=50)
    option = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    context = market(spot=100, rate=0.05, dividend=0.02, sigma=0.2)
    names = ["vega", "volga"]
    shift = 0.01
    actual = numerical_greeks(engine, option, context, names, volatility_shift=shift)
    low, high = (
        engine.price(option, market(spot=100, rate=0.05, dividend=0.02, sigma=sigma))
        for sigma in (0.2 - shift, 0.2 + shift)
    )
    assert actual.vega == pytest.approx((high - low) / (2 * shift), rel=0, abs=0.000001)
    assert actual.volga == pytest.approx(
        (high + low - 2 * actual.price) / shift**2, rel=0, abs=0.0001
    )


@pytest.mark.parametrize("scale", (1e-100, 1.0, 1e100))
def test_materially_negative_prices_fail_instead_of_being_clipped(scale):
    option = EuropeanOption(
        option_type="call",
        strike=100 * scale,
        effective_time=START,
        expiry_time=END,
    )
    context = market(spot=150 * scale, rate=0.05, dividend=2, sigma=0.5)
    engine = FiniteDifferenceVanillaEngine(
        asset_step_count=20,
        time_step_count=3,
    )
    assert_rejection(ComputationFailureError, engine.price, option, context)
    assert_rejection(
        ComputationFailureError,
        engine.price_with_greeks,
        option,
        context,
        ["delta"],
    )
    refined = FiniteDifferenceVanillaEngine().price(option, context)
    expected = AnalyticEuropeanEngine().price(option, context)
    assert refined / scale == pytest.approx(expected / scale, rel=0, abs=0.0001)


def test_settings_defaults_representation_validation_and_immutability():
    @numbers.Integral.register
    class Index:
        def __index__(self):
            return 17

    assert get_args(FiniteDifferenceScheme) == SCHEMES
    for factory in FACTORIES:
        engine = factory()
        assert (
            (
                engine.asset_step_count,
                engine.time_step_count,
                engine.scheme,
                engine.asset_upper_boundary,
            )
        ) == ((800, 400, "rannacher", None))
        assert factory(asset_step_count=Index()).asset_step_count == 17
        for count in (3, 10_001, 2**31 - 1):
            assert factory(asset_step_count=count).asset_step_count == count
        for count in (1, 100_001, 2**31 - 1):
            assert factory(time_step_count=count).time_step_count == count
        for name, minimum in (("asset_step_count", 3), ("time_step_count", 1)):
            for bad in (-1, 0, minimum - 1):
                assert_rejection(InvalidInputError, factory, **{name: bad})
            for bad in (True, False, 1.0, 1.5, "3", None):
                with pytest.raises(TypeError):
                    factory(**history_fields(**{name: bad}))
            for bad in (2**31, -(2**31) - 1):
                with pytest.raises(OverflowError):
                    factory(**history_fields(**{name: bad}))
        for bad in (0, -1, math.nan, math.inf):
            assert_rejection(InvalidInputError, factory, asset_upper_boundary=bad)
        for bad in (True, "400", object()):
            with pytest.raises(TypeError):
                factory(asset_upper_boundary=bad)
        for bad in (None, 0, True):
            with pytest.raises(TypeError):
                factory(scheme=bad)
        with pytest.raises(ValueError):
            factory(scheme="CrankNicolson")
        for choice in SCHEMES:
            assert factory(scheme=choice).scheme == choice
        with pytest.raises(TypeError):
            factory(unknown=1)
        with pytest.raises(TypeError):
            factory(400)
        with pytest.raises(TypeError):
            factory(asset_step_count=0, asset_upper_boundary=True)
        for name in (
            "asset_step_count",
            "time_step_count",
            "scheme",
            "asset_upper_boundary",
        ):
            with pytest.raises(AttributeError):
                setattr(engine, name, 1)


@pytest.mark.parametrize("kind", KINDS)
def test_expiry_life_and_clipped_grid_validation(kind):
    clipped = engine_for(kind, time_step_count=2**31 - 1, asset_upper_boundary=1)
    assert_rejection(InvalidInputError, clipped.price, contract(kind), market())
    assert_rejection(
        InvalidInputError,
        clipped.price_with_greeks,
        contract(kind),
        market(),
        "delta",
    )
    engine = engine_for(kind, asset_upper_boundary=1)
    for direction, sign in (("call", 1), ("put", -1)):
        option = contract(kind, direction)
        assert_rejection(InvalidInputError, engine.price, option, market())
        for spot in (math.nextafter(100, 0), 100, math.nextafter(100, 101)):
            context = market(spot=spot, valuation=END)
            pays = sign * (spot - 100) > 0
            expected = (
                max(sign * (spot - 100), 0)
                if kind in VANILLA
                else (10 if kind is CashOrNothingOption else spot)
                if pays
                else 0
            )
            result = engine.price_with_greeks(option, context, greeks="all")
            assert result.price == expected
            assert engine.price(option, context) == expected
            at_kink = spot == 100 if kind in VANILLA else abs(spot - 100) <= 0.01
            assert_terminal_greeks(
                result,
                None
                if at_kink
                else sign
                if kind in VANILLA and pays
                else int(kind is AssetOrNothingOption and pays),
            )
            assert_rejection(
                UnsupportedOperationError,
                implied_volatility,
                engine,
                option,
                context,
                expected,
                lower_bound=0.0,
                upper_bound=4.0,
            )
        for valuation in (
            START - timedelta(days=1),
            datetime(2026, 1, 1, microsecond=1),
        ):
            assert_rejection(
                InvalidInputError,
                engine.price,
                option,
                market(valuation=valuation),
            )
    if kind is not AmericanOption:
        tiny = math.ulp(0.0)
        assert_rejection(
            InvalidInputError,
            engine_for(kind).price,
            contract(kind, strike=tiny),
            market(spot=tiny),
        )


@pytest.mark.parametrize("kind", KINDS)
def test_analysis_requests_shifts_failures_and_input_ownership(kind):
    engine, option, context = engine_for(kind), contract(kind), market()
    baseline = engine.price(option, context)
    for settings in (
        {"spot_shift": 0},
        {"volatility_shift": math.nan},
        {"rate_shift": math.inf},
        {"time_shift": timedelta(days=0)},
    ):
        assert_rejection(
            InvalidInputError,
            partial(numerical_greeks, engine),
            option,
            context,
            "delta",
            **settings,
        )
    for settings in (
        {"spot_shift": True},
        {"rate_shift": "1"},
        {"time_shift": True},
        {"time_shift": 1.5},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            numerical_greeks(engine, option, context, "delta", **settings)
    with pytest.raises(TypeError):
        numerical_greeks(engine, option, context, "delta", time_shift=2**31)
    with pytest.raises(TypeError):
        engine.price_with_greeks(option, context)
    with pytest.raises(TypeError):
        engine.price_with_greeks(option, context, "delta", greeks="all")
    with pytest.raises(ValueError):
        engine.price_with_greeks(option, context, "unknown")
    other = contract(CashOrNothingOption if kind in VANILLA else EuropeanOption)
    with pytest.raises(UnsupportedOperationError):
        engine.price(other, context)
    with pytest.raises(UnsupportedOperationError):
        implied_volatility(
            engine, other, context, baseline, lower_bound=0.0, upper_bound=4.0
        )
    for shift in (1e-300, 100):
        assert (
            numerical_greeks(engine, option, context, "speed", spot_shift=shift).speed
            is None
        )
    assert engine.price(option, context) == baseline
    assert (
        (
            context.spot_price,
            context.model_parameters.volatility,
            engine.asset_upper_boundary,
        )
    ) == ((100, 0.2, None))
    result = engine.price_with_greeks(option, context, "delta")
    del engine, option, context
    assert math.isfinite(result.price) and math.isfinite(result.delta)


def test_curve_support_and_validation_order():
    middle = datetime(2025, 7, 1)
    risk = RateTermStructure(
        reference_time=START, times=[middle, END], factors=[math.exp(0.1), 1]
    )
    context = market(rate=risk, dividend=0, spot=0.01)
    option = contract(AmericanOption, "put")
    engine = FiniteDifferenceVanillaEngine()
    for valuation in (START, datetime(2025, 4, 1), middle, datetime(2025, 10, 1)):
        shifted = context.replace(valuation_time=valuation)
        expected = 100 * risk.factor(valuation, max(valuation, middle)) - 0.01
        assert engine.price(option, shifted) == pytest.approx(expected, rel=0, abs=1e-5)
    for kind in DIGITAL:
        engine, option = engine_for(kind), contract(kind)
        for valuation in (START, END):
            shifted = context.replace(valuation_time=valuation)
            assert_rejection(UnsupportedOperationError, engine.price, option, shifted)
            assert_rejection(
                UnsupportedOperationError,
                engine.price_with_greeks,
                option,
                shifted,
                [],
            )
            with pytest.raises(TypeError):
                numerical_greeks(engine, option, shifted, [], spot_shift=True)
    short = RateTermStructure(reference_time=START, times=[middle], factors=[0.99])
    assert_rejection(
        InvalidInputError,
        FiniteDifferenceVanillaEngine().price,
        contract(EuropeanOption),
        market(rate=short),
    )


def test_curve_greek_bumps_preserve_existing_parallel_spread(subtests):
    risk = RateTermStructure(
        reference_time=START,
        times=[datetime(2025, 6, 30), END],
        factors=[
            math.exp(-0.02 * 180 / 365),
            math.exp(-(0.02 * 180 + 0.08 * 185) / 365),
        ],
    ).shifted(0.01)
    context = market(rate=risk)
    original = context.model_parameters
    option = contract(EuropeanOption)
    greeks = ("delta", "vega", "rho")
    expected = AnalyticEuropeanEngine().price_with_greeks(option, context, greeks)
    actual = FiniteDifferenceVanillaEngine().price_with_greeks(option, context, greeks)
    assert actual.price == pytest.approx(expected.price, rel=0, abs=1e-3)
    for greek in greeks:
        with subtests.test(greek=greek):
            assert actual.require(greek) == pytest.approx(
                expected.require(greek), rel=0, abs=1e-3
            )
    assert context.model_parameters == original
    assert context.model_parameters.risk_free_curve == risk


def test_implied_volatility_and_solver_failures():
    for kind in KINDS:
        for direction in ("call", "put"):
            engine, option, context = (
                engine_for(kind),
                contract(kind, direction),
                market(),
            )
            quote = engine.price(option, context)
            assert implied_volatility(
                engine,
                option,
                context,
                quote,
                lower_bound=0.1,
                upper_bound=0.22,
            ) == pytest.approx(0.2, rel=0, abs=1e-7)
            assert_rejection(
                RootNotBracketedError,
                implied_volatility,
                engine,
                option,
                context,
                -1,
                lower_bound=0.1,
                upper_bound=0.22,
            )
            assert_rejection(
                InvalidInputError,
                implied_volatility,
                engine,
                option,
                context,
                math.nan,
                lower_bound=0.0,
                upper_bound=4.0,
            )
            assert_rejection(
                NonConvergenceError,
                implied_volatility,
                engine,
                option,
                context,
                quote,
                lower_bound=0.11,
                upper_bound=0.23,
                max_iterations=1,
                price_tolerance=1e-15,
                parameter_tolerance=1e-15,
            )
            assert engine.price(option, context) == quote
    engine = FiniteDifferenceVanillaEngine(
        scheme="explicit_euler", asset_step_count=80, time_step_count=500
    )
    option, context = contract(EuropeanOption), market()
    assert_rejection(
        InvalidInputError,
        implied_volatility,
        engine,
        option,
        context,
        engine.price(option, context),
        lower_bound=0.0,
        upper_bound=4.0,
    )


@pytest.mark.parametrize("kind", (EuropeanOption, *DIGITAL))
@pytest.mark.parametrize("direction", ("call", "put"))
def test_grid_refinement_schemes_interpolation_and_long_expiry(kind, direction):
    analytic = AnalyticEuropeanEngine() if kind in VANILLA else AnalyticDigitalEngine()
    option, context = contract(kind, direction), market(spot=100.25)
    expected = analytic.price(option, context)
    previous = math.inf
    for count in (100, 200, 400, 800):
        actual = engine_for(
            kind,
            asset_step_count=count,
            time_step_count=count,
            asset_upper_boundary=400,
        ).price(option, context)
        assert abs(actual - expected) < previous
        previous = abs(actual - expected)
    assert previous < 0.01
    for scheme in SCHEMES:
        engine = engine_for(
            kind,
            asset_step_count=200,
            time_step_count=2000,
            scheme=scheme,
            asset_upper_boundary=400,
        )
        result = engine.price_with_greeks(option, context, ["delta", "gamma"])
        assert result.price == pytest.approx(expected, rel=0, abs=0.05)
        h = 0.01
        high = engine.price(option, context.replace(spot_price=100.25 + h))
        low = engine.price(option, context.replace(spot_price=100.25 - h))
        assert result.delta == pytest.approx((high - low) / (2 * h), rel=0, abs=1e-6)
        assert result.gamma == pytest.approx(
            (high - 2 * result.price + low) / h**2, rel=0, abs=1e-6
        )
    option = type(option)(
        option_type=direction,
        strike=100,
        effective_time=START,
        expiry_time=datetime(2035, 1, 1),
        **({"payout": 10} if kind is CashOrNothingOption else {}),
    )
    context = market(rate=0.03, dividend=0.02, sigma=0.6)
    assert engine_for(kind, time_step_count=1000).price(
        option, context
    ) == pytest.approx(analytic.price(option, context), rel=0, abs=0.01)


@pytest.mark.parametrize("kind", KINDS)
def test_shared_and_distinct_engines_support_concurrent_analysis(kind):
    engine, option, context = engine_for(kind), contract(kind), market()
    expected = engine.price_with_greeks(option, context, "delta")

    def compute(index, engine=engine, kind=kind, option=option, context=context):
        selected = engine if index % 2 else engine_for(kind)
        result = selected.price_with_greeks(option, context, "delta")
        return result.price, result.delta

    with ThreadPoolExecutor(max_workers=4) as pool:
        assert (
            list(pool.map(compute, range(8))) == [(expected.price, expected.delta)] * 8
        )


def test_existing_quantlib_european_and_american_profiles(subtests):
    oracle = importlib.import_module("quantlib_reference.oracle")
    american = importlib.import_module("quantlib_reference.american")
    oracle.require_version()
    european_profile = next(
        p
        for p in oracle.load_fixture("numerical_engines.json")["profiles"]
        if p["engine"] == "FiniteDifferenceVanillaEngine"
    )
    american_config = oracle.load_fixture("american.json")
    american_profile = next(
        p
        for p in american_config["profiles"]
        if p["engine"] == "FiniteDifferenceVanillaEngine"
    )
    european_scenarios = oracle.load_fixture("scenarios.json")["scenarios"]
    for kind, rows, profile in (
        (EuropeanOption, european_scenarios, european_profile),
        (AmericanOption, american_config["scenarios"], american_profile),
    ):
        engine = FiniteDifferenceVanillaEngine(**profile["settings"])
        for scenario in rows:
            inputs = scenario["inputs"]
            if kind is EuropeanOption and oracle.exclusions(inputs):
                continue
            option = kind(
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
                reference, metadata = (
                    oracle.reference(inputs)
                    if kind is EuropeanOption
                    else american.reference(inputs)
                )
            names = sorted(reference.keys() - {"price"})
            result = engine.price_with_greeks(option, context, names)
            for name, expected in reference.items():
                budget = (
                    profile["tolerances"]
                    if name in ("price", "delta", "gamma")
                    else profile["numerical_tolerances"]
                )
                with subtests.test(kind=kind, case=scenario["case_id"], measure=name):
                    assert getattr(result, name) == pytest.approx(
                        expected,
                        rel=0,
                        abs=budget[name] + metadata[f"uncertainty_{name}"],
                    )


def test_existing_quantlib_digital_profiles(subtests):

    digital = importlib.import_module("quantlib_reference.digital")
    oracle = importlib.import_module("quantlib_reference.oracle")
    oracle.require_version()
    config = oracle.load_fixture("digital.json")
    profile = next(
        p for p in config["profiles"] if p["engine"] == "FiniteDifferenceDigitalEngine"
    )
    for identifier, inputs in digital.scenarios(config):
        kind = (
            CashOrNothingOption if inputs["payoff"] == "cash" else AssetOrNothingOption
        )
        option = kind(
            **history_fields(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_time=datetime.fromisoformat(inputs["effective_time"]),
                expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
                **({"payout": inputs["payout"]} if kind is CashOrNothingOption else {}),
            )
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
        boundary = (option.expiry_time - context.valuation_time).days <= 2
        settings = profile["settings"] | (
            profile["boundary_settings"] if boundary else {}
        )
        # Exercise boundary cases and wide-bump acceptance scenarios.
        wrapped = (
            not boundary
            and inputs["spot"] == 100
            and inputs["expiry_time"] == "2026-01-06"
        )
        names = sorted(reference.keys() - {"price"}) if wrapped else ["delta", "gamma"]
        result = FiniteDifferenceDigitalEngine(**settings).price_with_greeks(
            option, context, names
        )
        for name in ["price", *names]:
            with subtests.test(case=identifier, measure=name):
                assert getattr(result, name) == pytest.approx(
                    reference[name],
                    rel=0,
                    abs=profile["numerical_tolerances"][name]
                    + uncertainty[f"uncertainty_{name}"],
                )


@pytest.mark.parametrize("direction", ("call", "put"))
@pytest.mark.parametrize(
    "name,tolerance", (("price", 0.001), ("delta", 0.0001), ("gamma", 0.0003))
)
def test_refined_one_day_vanilla_prices_and_native_greeks(direction, name, tolerance):
    end = START + timedelta(days=1)
    engine = FiniteDifferenceVanillaEngine(
        asset_step_count=6400, time_step_count=1600, asset_upper_boundary=400
    )
    option = EuropeanOption(
        option_type=direction, strike=100, effective_time=START, expiry_time=end
    )
    context = market(sigma=0.3)
    expected = AnalyticEuropeanEngine().price_with_greeks(
        option, context, ["delta", "gamma"]
    )
    actual = engine.price_with_greeks(option, context, ["delta", "gamma"])
    assert getattr(actual, name) == pytest.approx(
        getattr(expected, name), rel=0, abs=tolerance
    )


def test_american_long_expiry_domain_matches_live_quantlib():

    american = importlib.import_module("quantlib_reference.american")
    inputs = {
        "option": "call",
        "strike": 100,
        "spot": 100,
        "rate": 0.03,
        "dividend": 0.05,
        "volatility": 1.0,
        "effective_time": "2025-01-01",
        "valuation": "2025-01-01",
        "expiry_time": "2035-01-01",
    }
    with ql.SavedSettings():
        reference = american.native(tuple(sorted(inputs.items())), (3200, 3200))[
            "price"
        ]
    option = AmericanOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=datetime(2035, 1, 1),
    )
    context = market(rate=0.03, dividend=0.05, sigma=1)
    coarse = FiniteDifferenceVanillaEngine(time_step_count=200).price(option, context)
    engine = FiniteDifferenceVanillaEngine(time_step_count=1600)
    refined = engine.price(option, context)
    assert coarse == pytest.approx(reference, rel=0, abs=0.2)
    assert refined == pytest.approx(reference, rel=0, abs=0.03)
    assert abs(refined - reference) < abs(coarse - reference)
    assert engine.price_with_greeks(option, context, "vega").price == refined
