"""American approximation contracts shared by the Rust implementation."""

import importlib
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from fractions import Fraction
from functools import partial

import pytest
import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import AmericanOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    BjerksundStenslandVanillaEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

GREEKS = (
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "vanna",
    "zomma",
    "rho",
)


@pytest.fixture
def base_terms():
    return {
        "option_type": "call",
        "strike": 100,
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }


@pytest.fixture
def base_option(base_terms):
    return AmericanOption(**base_terms)


@pytest.fixture
def base_context(make_market):
    return make_market()


@pytest.fixture
def base_engine():
    return BjerksundStenslandVanillaEngine()


@pytest.fixture
def make_market(base_terms):
    def create(spot=100, rate=0.05, dividend=0.03, volatility=0.2, at=None):
        return PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
            ),
            spot_price=spot,
            valuation_time=at or base_terms["effective_time"],
        )

    return create


def test_early_exercise_and_european_limits(base_engine, base_terms, make_market):
    for direction, spot, rate, dividend, expected in (
        ("call", 200, 0.03, 0.1, 100),
        ("put", 50, 0.1, 0.03, 50),
    ):
        option = AmericanOption(**dict(base_terms, option_type=direction))
        assert base_engine.price(option, make_market(spot, rate, dividend)) == expected
    for direction, rate, dividend in (
        ("call", 0.05, 0),
        ("call", 0.05, -0.01),
        ("put", 0, 0.05),
    ):
        terms = dict(base_terms, option_type=direction)
        context = make_market(rate=rate, dividend=dividend)
        assert base_engine.price(
            AmericanOption(**terms), context
        ) == AnalyticEuropeanEngine().price(EuropeanOption(**terms), context)


def test_expiry_and_microsecond_exercise_boundaries(
    base_engine, base_terms, make_market
):
    end = base_terms["expiry_time"]
    for direction in ("call", "put"):
        option = AmericanOption(**dict(base_terms, option_type=direction))
        for spot in (90, 100, 110):
            context = make_market(spot=spot, rate=-0.05, dividend=-0.03, at=end)
            expected = max(spot - 100 if direction == "call" else 100 - spot, 0)
            result = base_engine.price_with_greeks(option, context, greeks="all")
            assert result.price == expected
            assert_terminal_greeks(
                result,
                None
                if spot == 100
                else (1 if direction == "call" else -1)
                if expected
                else 0,
            )
            assert_rejection(
                UnsupportedOperationError,
                implied_volatility,
                base_engine,
                option,
                context,
                expected,
                lower_bound=0.0,
                upper_bound=4.0,
            )
        for at in (
            datetime(2024, 12, 31, 23, 59, 59, 999999),
            datetime(2026, 1, 1, 0, 0, 0, 1),
        ):
            assert_rejection(
                InvalidInputError,
                base_engine.price,
                option,
                make_market(at=at),
            )
        assert (
            base_engine.price(
                option,
                make_market(at=datetime(2025, 12, 31, 23, 59, 59, 999999)),
            )
            >= 0
        )
    same_day = AmericanOption(**dict(base_terms, effective_time=end))
    assert base_engine.price(same_day, make_market(spot=110, at=end)) == 10


def test_construction_protocols_validation_order_and_ownership(base_option, base_terms):
    for strike in (0, -1, float("nan"), float("inf")):
        assert_rejection(
            InvalidInputError,
            AmericanOption,
            **dict(base_terms, strike=strike, expiry_time=datetime(2024, 1, 1)),
        )
    assert_rejection(
        InvalidInputError,
        AmericanOption,
        **dict(base_terms, expiry_time=datetime(2024, 1, 1)),
    )
    for fields in (
        {"strike": True},
        {"strike": "100"},
        {"strike": 1 + 0j},
        {"strike": None},
        {"effective_time": datetime(2025, 1, 1).date()},
        {"expiry_time": "2026-01-01"},
        {"option_type": 1},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            AmericanOption(**dict(base_terms, **fields))
    with pytest.raises(ValueError):
        AmericanOption(**dict(base_terms, option_type="CALL"))
    with pytest.raises(OverflowError):
        AmericanOption(**dict(base_terms, strike=10**1000))
    assert AmericanOption(**dict(base_terms, strike=Fraction(201, 2))).strike == 100.5
    assert base_option == AmericanOption(**base_terms)
    assert base_option.effective_time == base_terms["effective_time"]
    assert base_option.expiry_time == base_terms["expiry_time"]
    assert base_option.option_type == "call"
    with pytest.raises(AttributeError):
        base_option.strike = 105


def test_applicability_and_error_order(
    base_context, base_engine, base_option, base_terms, make_market
):
    for direction in ("call", "put"):
        option = AmericanOption(**dict(base_terms, option_type=direction))
        for r, q, sigma, expected_error in (
            (-0.01, 0.02, 0.2, UnsupportedOperationError),
            (0.05, 0.1, 0.02, UnsupportedOperationError),
            (0, 0.02, 0.01, UnsupportedOperationError),
            (0, 0.1, 0.05, UnsupportedOperationError),
            (0.05, 0.03, 1e200, ComputationFailureError),
            (0.1, 0.05, 1e-200, ComputationFailureError),
        ):
            context = make_market(
                rate=r if direction == "call" else q,
                dividend=q if direction == "call" else r,
                volatility=sigma,
            )
            assert_rejection(expected_error, base_engine.price, option, context)
            assert_rejection(
                expected_error,
                base_engine.price_with_greeks,
                option,
                context,
                "delta",
            )
        curve = RateTermStructure(
            reference_time=base_terms["effective_time"],
            times=[
                base_terms["expiry_time"],
                base_terms["expiry_time"]
                + (base_terms["expiry_time"] - base_terms["effective_time"]),
            ],
            factors=[math.exp(-0.05), (math.exp(-0.05)) * 0.5],
        )
        for context in (
            make_market(rate=curve),
            make_market(dividend=curve),
            make_market(rate=curve, at=base_terms["expiry_time"]),
        ):
            assert_rejection(
                UnsupportedOperationError,
                base_engine.price,
                option,
                context,
            )
            assert_rejection(
                UnsupportedOperationError,
                base_engine.price_with_greeks,
                option,
                context,
                [],
            )
            assert_rejection(
                UnsupportedOperationError,
                implied_volatility,
                base_engine,
                option,
                context,
                10,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    explicit = make_market(
        rate=RateTermStructure.flat(0.05, reference_time=base_terms["effective_time"]),
        dividend=RateTermStructure.flat(0.03),
    )
    assert base_engine.price(base_option, explicit) == base_engine.price(
        base_option, base_context
    )
    context = make_market(rate=0, dividend=0.02)
    assert base_engine.price(base_option, context) > 0
    assert_rejection(
        UnsupportedOperationError,
        base_engine.price_with_greeks,
        base_option,
        context,
        "rho",
    )


def test_greeks_defaults_scalar_differences_and_shared_solving(
    base_context, base_engine, base_option, base_terms, subtests
):

    differences = importlib.import_module("price_differences").price_differences
    for direction in ("call", "put"):
        option = AmericanOption(**dict(base_terms, option_type=direction))
        result = base_engine.price_with_greeks(option, base_context, greeks="all")
        numerical = differences(base_engine, option, base_context)
        for name in GREEKS:
            with subtests.test(direction=direction, name=name):
                assert getattr(result, name) == pytest.approx(
                    getattr(numerical, name), rel=0, abs=1e-7
                )
                selected = base_engine.price_with_greeks(option, base_context, name)
                assert selected.require(name) == getattr(result, name)
                assert (
                    getattr(selected, "gamma" if name == "delta" else "delta") is None
                )
        assert implied_volatility(
            base_engine,
            option,
            base_context,
            result.price,
            lower_bound=0.0,
            upper_bound=4.0,
        ) == pytest.approx(0.2, rel=0, abs=1e-6)
        assert implied_volatility(
            base_engine,
            option,
            base_context,
            result.price,
            lower_bound=0.1,
            upper_bound=0.4,
        ) == pytest.approx(0.2, rel=0, abs=1e-6)
    quote = base_engine.price(base_option, base_context)
    with ThreadPoolExecutor(max_workers=4) as pool:
        values = list(
            pool.map(
                lambda _: implied_volatility(
                    base_engine,
                    base_option,
                    base_context,
                    quote,
                    lower_bound=0.0,
                    upper_bound=4.0,
                ),
                range(8),
            )
        )
    for value in values:
        assert value == pytest.approx(0.2, rel=0, abs=1e-6)
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        base_engine,
        base_option,
        base_context,
        -1,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert_rejection(
        NonConvergenceError,
        implied_volatility,
        base_engine,
        base_option,
        base_context,
        quote,
        max_iterations=1,
        price_tolerance=1e-16,
        parameter_tolerance=1e-16,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert base_context.spot_price == 100
    assert base_context.model_parameters.volatility == 0.2
    assert base_engine.price(base_option, base_context) == quote


def test_analysis_rejects_protocols_and_unsupported_pairs(
    base_context, base_engine, base_option, base_terms
):
    for settings in (
        {"spot_shift": 0},
        {"volatility_shift": float("nan")},
        {"rate_shift": -1},
        {"time_shift": timedelta(days=0)},
    ):
        assert_rejection(
            InvalidInputError,
            partial(numerical_greeks, base_engine),
            base_option,
            base_context,
            "delta",
            **settings,
        )
    for settings in (
        {"spot_shift": True},
        {"volatility_shift": "0.01"},
        {"time_shift": 1.5},
        {"time_shift": True},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            numerical_greeks(
                base_engine, base_option, base_context, "delta", **settings
            )
    with pytest.raises(TypeError):
        numerical_greeks(
            base_engine, base_option, base_context, "delta", time_shift=2**31
        )
    for settings in (
        {"max_iterations": True},
        {"max_iterations": 1.5},
        {"lower_bound": True},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            implied_volatility(
                base_engine,
                base_option,
                base_context,
                10,
                **{"lower_bound": 0.0, "upper_bound": 4.0, **settings},
            )
    with pytest.raises(OverflowError):
        implied_volatility(
            base_engine,
            base_option,
            base_context,
            10,
            max_iterations=2**31,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(TypeError):
        implied_volatility(
            base_engine,
            base_option,
            base_context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    for kwargs in (
        {"greeks": None},
        {"greeks": 1},
    ):
        with pytest.raises(TypeError):
            numerical_greeks(base_engine, base_option, base_context, **kwargs)
    for instrument in (
        EuropeanOption(**base_terms),
        CashOrNothingOption(**base_terms, payout=10),
    ):
        with pytest.raises(UnsupportedOperationError):
            base_engine.price(instrument, base_context)
        with pytest.raises(UnsupportedOperationError):
            base_engine.price_with_greeks(instrument, base_context, "delta")
        with pytest.raises(UnsupportedOperationError):
            implied_volatility(
                base_engine,
                instrument,
                base_context,
                10,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    with pytest.raises(UnsupportedOperationError):
        AnalyticEuropeanEngine().price(base_option, base_context)
    with pytest.raises(UnsupportedOperationError):
        implied_volatility(
            AnalyticEuropeanEngine(),
            base_option,
            base_context,
            10,
            lower_bound=0.0,
            upper_bound=4.0,
        )


def test_live_quantlib_refinement_and_existing_error_budgets(
    base_engine, make_market, subtests
):
    american = importlib.import_module("quantlib_reference.american")
    oracle = importlib.import_module("quantlib_reference.oracle")
    oracle.require_version()
    config = oracle.load_fixture("american.json")
    profile = next(
        p
        for p in config["profiles"]
        if p["engine"] == "BjerksundStenslandVanillaEngine"
    )
    for scenario in config["scenarios"]:
        inputs = scenario["inputs"]
        option = AmericanOption(
            option_type=inputs["option"],
            strike=inputs["strike"],
            effective_time=datetime.fromisoformat(inputs["effective_time"]),
            expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
        )
        context = make_market(
            inputs["spot"],
            inputs["rate"],
            inputs["dividend"],
            inputs["volatility"],
            datetime.fromisoformat(inputs["valuation"]),
        )
        with ql.SavedSettings():
            reference, uncertainty = american.reference(inputs)
        names = sorted(reference.keys() - {"price"})
        result = numerical_greeks(
            base_engine, option, context, names, **profile["shifts"]
        )
        assert result.price == pytest.approx(
            reference["price"], rel=0, abs=profile["tolerances"]["price"]
        )
        for name in names:
            with subtests.test(case=scenario["case_id"], measure=name):
                assert getattr(result, name) == pytest.approx(
                    reference[name],
                    rel=0,
                    abs=profile["numerical_tolerances"][name]
                    + uncertainty[f"uncertainty_{name}"],
                )
