"""Asian public contracts and independent geometric/Levy approximation references."""

import importlib
import math
from datetime import datetime, timedelta
from fractions import Fraction
from functools import partial

import pytest
import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    ArithmeticAveragePriceOption,
    EuropeanOption,
    GeometricAveragePriceOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    AnalyticGeometricAveragePriceEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
PAIRS = (
    (GeometricAveragePriceOption, AnalyticGeometricAveragePriceEngine),
    (ArithmeticAveragePriceOption, TurnbullWakemanArithmeticAveragePriceEngine),
)
GREEKS = [
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
]


def terms(**changes):
    return {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "averaging_start_time": START,
        "expiry_time": END,
    } | changes


def market(at=START, rate=0.04, dividend=0.01, volatility=0.2, spot=100):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=at,
    )


def test_owned_history_replacement_and_native_representation_errors(subtests):
    for kind, engine_type in PAIRS:
        option = kind(**terms())
        assert option.realized_average is None
        assert option.replace() == option
        updated = option.replace(
            realized_average=Fraction(211, 2),
        )
        assert updated.realized_average == 105.5
        for name in terms():
            assert getattr(option, name) == getattr(updated, name)
        assert_rejection(InvalidInputError, option.replace, realized_average=-1)
        assert option.realized_average is None
        for name in (*terms(), "realized_average"):
            with pytest.raises(AttributeError):
                setattr(option, name, 1)
        for name in ("strike", "realized_average"):
            for invalid in (True, False, "100", None, 1 + 0j, object()):
                if name == "realized_average" and invalid is None:
                    assert kind(**terms(realized_average=None)).realized_average is None
                    continue
                with (
                    subtests.test(kind=kind, name=name, value=invalid),
                    pytest.raises(TypeError),
                ):
                    kind(**terms(**{name: invalid}))
            with pytest.raises(OverflowError):
                kind(**terms(**{name: 10**400}))
        for name in ("effective_time", "averaging_start_time", "expiry_time"):
            for invalid in (True, "2025-01-01", datetime(2025, 1, 1).date()):
                with pytest.raises(TypeError):
                    kind(**terms(**{name: invalid}))
        with pytest.raises(ValueError):
            kind(**terms(option_type="CALL"))
        with pytest.raises(TypeError):
            kind("call", 100, START, START, END)
        for invalid in (True, "100"):
            with pytest.raises(TypeError):
                option.replace(
                    realized_average=invalid,
                )
        assert option.replace(strike=101).strike == 101
        with pytest.raises(TypeError):
            option.replace(history_time=START)
        with pytest.raises(TypeError):
            kind(**terms(), history_time=START)
        assert not hasattr(option, "replace_history")
        assert not hasattr(option, "history_time")
        other = PAIRS[1 if kind is GeometricAveragePriceOption else 0][0](**terms())
        engine = engine_type()
        for wrong in (
            other,
            EuropeanOption(
                option_type="call",
                strike=100,
                effective_time=START,
                expiry_time=END,
            ),
        ):
            with pytest.raises(UnsupportedOperationError):
                engine.price(wrong, market())
            with pytest.raises(UnsupportedOperationError):
                implied_volatility(
                    engine,
                    wrong,
                    market(),
                    5,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )


@pytest.mark.parametrize("kind,engine_type", PAIRS)
def test_validation_schedule_history_life_and_single_fixing(kind, engine_type):
    averaging = datetime(2025, 2, 1)
    for strike in (0, -1, math.nan, math.inf):
        assert_rejection(InvalidInputError, kind, **terms(strike=strike))
    for strike, average, expected in (
        (0, -1, InvalidInputError),
        (100, -1, InvalidInputError),
        (100, 0, InvalidInputError),
    ):
        assert_rejection(
            expected,
            kind,
            **terms(
                strike=strike,
                realized_average=average,
                effective_time=END,
                expiry_time=START,
            ),
        )
    for average in (-1, math.nan, math.inf):
        assert_rejection(
            InvalidInputError,
            kind,
            **terms(realized_average=average),
        )
    for invalid in (START - timedelta(days=1), END + timedelta(days=1)):
        assert_rejection(
            InvalidInputError,
            kind,
            **terms(averaging_start_time=invalid),
        )
    original = kind(**terms(averaging_start_time=averaging))
    seasoned = original.replace(
        realized_average=110,
    )
    engine = engine_type()
    for at in (START, averaging):
        assert engine.price(original, market(at)) > 0
        assert_rejection(
            InvalidInputError,
            engine.price,
            seasoned,
            market(at),
        )
    for at in (averaging + timedelta(days=1), END):
        assert engine.price(seasoned, market(at)) > 0
        assert_rejection(
            MissingHistoryError,
            engine.price,
            original,
            market(at),
        )
    for at in (
        START - timedelta(days=1),
        datetime(2026, 1, 1, microsecond=1),
    ):
        assert_rejection(
            InvalidInputError,
            engine.price,
            seasoned,
            market(at),
        )
    assert engine.price(seasoned, market(END)) == 10
    result = engine.price_with_greeks(seasoned, market(END), greeks="all")
    assert_terminal_greeks(result, 0)
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        seasoned,
        market(END),
        10,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    for direction in ("call", "put"):
        single = kind(**terms(option_type=direction, averaging_start_time=END))
        vanilla = EuropeanOption(
            option_type=direction,
            strike=100,
            effective_time=START,
            expiry_time=END,
        )
        for at in (START, END - timedelta(days=1), END):
            assert engine.price(single, market(at)) == AnalyticEuropeanEngine().price(
                vanilla, market(at)
            )
        with pytest.raises(InvalidInputError):
            engine.price(single.replace(realized_average=110), market(END))


@pytest.mark.parametrize("kind,engine_type", PAIRS)
def test_greek_requests_stencils_and_solver_controls(kind, engine_type):
    engine, option, context = (
        engine_type(),
        kind(**terms()),
        market(),
    )
    full = engine.price_with_greeks(option, context, greeks="all")
    for g in ("theta", "charm", "color"):
        assert getattr(full, g) is None
    for request in (
        "delta",
        ["delta", "delta"],
        (g for g in ("delta", "vega")),
    ):
        selected = engine.price_with_greeks(option, context, request)
        assert selected.delta == full.delta
        assert selected.gamma is None
    for settings, exception in (
        ({}, TypeError),
        ({"greeks": "Delta"}, ValueError),
        ({"greeks": True}, TypeError),
        ({"greeks": "delta", "spot_shift": True}, TypeError),
        ({"greeks": "delta", "time_shift": 1.5}, TypeError),
        ({"greeks": "delta", "time_shift": 2**40}, TypeError),
    ):
        with pytest.raises(exception):
            numerical_greeks(engine, option, context, **settings)
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        option,
        context,
        "delta",
        spot_shift=0,
    )
    collapsed = numerical_greeks(
        engine,
        option,
        context,
        "delta",
        spot_shift=math.ulp(0.0),
    )
    assert collapsed.delta is None
    seasoned = option.replace(
        realized_average=100,
    )
    active = market(datetime(2025, 7, 1))
    result = engine.price_with_greeks(seasoned, active, greeks="all")
    assert all(getattr(result, g) is not None for g in GREEKS)
    assert implied_volatility(
        engine,
        seasoned,
        active,
        result.price,
        lower_bound=0.0,
        upper_bound=4.0,
    ) == pytest.approx(0.2, rel=0, abs=1e-7)
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        option,
        context,
        -1,
        lower_bound=0.0,
        upper_bound=4.0,
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
    for field, value, exception in (
        ("max_iterations", True, TypeError),
        ("max_iterations", 2.5, TypeError),
        ("max_iterations", 2**40, OverflowError),
        ("lower_bound", True, TypeError),
    ):
        with pytest.raises(exception):
            implied_volatility(
                engine,
                option,
                context,
                full.price,
                **{"lower_bound": 0.0, "upper_bound": 4.0, field: value},
            )
    assert option.realized_average is None
    assert context.model_parameters.volatility == 0.2


def test_arithmetic_solver_prices_only_the_requested_bracket():
    option = ArithmeticAveragePriceOption(**terms())
    engine = TurnbullWakemanArithmeticAveragePriceEngine()
    quote = engine.price(option, market())
    assert implied_volatility(
        engine,
        option,
        market(volatility=100),
        quote,
        lower_bound=0.05,
        upper_bound=0.4,
    ) == pytest.approx(0.2, rel=0, abs=1e-7)


def test_single_fixing_preserves_independent_asset_discount_at_extreme_rates():
    context = market(rate=1e16, dividend=0.02)
    expected = 100 * math.exp(-0.02)
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    assert AnalyticEuropeanEngine().price(vanilla, context) == pytest.approx(
        expected, rel=0, abs=1e-12
    )
    for kind, engine_type in PAIRS:
        option = kind(**terms(averaging_start_time=END))
        assert engine_type().price(option, context) == pytest.approx(
            expected, rel=0, abs=1e-12
        )


@pytest.mark.parametrize("sigma", (1e-06, 0.001))
def test_non_finite_small_volatility_moments_preserve_invalid_result(sigma):
    option = ArithmeticAveragePriceOption(**terms())
    assert_rejection(
        ComputationFailureError,
        TurnbullWakemanArithmeticAveragePriceEngine().price,
        option,
        market(rate=400, dividend=0, volatility=sigma),
    )


def test_curve_applicability_coverage_and_expiry_order():
    middle = START + timedelta(days=182)
    curve = RateTermStructure(
        reference_time=START,
        times=[middle, END],
        factors=[math.exp(-0.02 * 182 / 365), math.exp(-0.05)],
    )
    context = market(rate=curve, dividend=0)
    option = GeometricAveragePriceOption(**terms())
    h = 182 / 365
    second = (0.05 - 0.02 * h) / (1 - h)
    drift = 0.02 * (h - h * h / 2) + second * (1 - h) ** 2 / 2 - 0.04 / 4
    width = math.sqrt(0.04 / 3)

    def cdf(x):
        return math.erfc(-x / math.sqrt(2)) / 2

    expected = (
        math.exp(-0.05)
        * 100
        * (
            math.exp(drift + width * width / 2) * cdf(drift / width + width)
            - cdf(drift / width)
        )
    )
    assert AnalyticGeometricAveragePriceEngine().price(
        option, context
    ) == pytest.approx(expected, rel=0, abs=1e-11)
    engine = TurnbullWakemanArithmeticAveragePriceEngine()
    arithmetic = ArithmeticAveragePriceOption(**terms())
    assert_rejection(
        UnsupportedOperationError,
        engine.price,
        arithmetic,
        context,
    )
    assert_rejection(
        UnsupportedOperationError,
        engine.price_with_greeks,
        arithmetic,
        context,
        [],
    )
    assert_rejection(
        InvalidInputError,
        engine.price,
        arithmetic.replace(
            realized_average=110,
        ),
        context,
    )
    assert (
        engine.price(
            arithmetic.replace(
                realized_average=110,
            ),
            context.replace(valuation_time=END),
        )
        == 10
    )
    short = RateTermStructure(reference_time=START, times=[middle], factors=[0.99])
    for kind, engine_type in PAIRS:
        assert_rejection(
            InvalidInputError,
            engine_type().price,
            kind(**terms()),
            market(rate=short),
        )
    dated_flat = RateTermStructure.flat(rate=0.04, reference_time=START)
    assert engine.price(
        arithmetic,
        market(rate=dated_flat),
    ) == engine.price(arithmetic, market())


def test_live_quantlib_geometric_and_same_levy_approximation(subtests):

    asian = importlib.import_module("quantlib_reference.asian")
    oracle = importlib.import_module("quantlib_reference.oracle")
    differences = importlib.import_module("price_differences").price_differences
    oracle.require_version()
    with ql.SavedSettings():
        asian.check_bindings()
        rows = list(asian.rows())
    assert len(rows) == 24
    for row in rows:
        inputs = row["inputs"]
        kind, engine_type = PAIRS[0 if inputs["averaging"] == "geometric" else 1]
        option = kind(
            **terms(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_time=datetime.fromisoformat(inputs["effective_time"]),
                averaging_start_time=datetime.fromisoformat(
                    inputs["averaging_start_time"]
                ),
                expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
                realized_average=inputs["realized_average"] or None,
            )
        )
        context = market(
            datetime.fromisoformat(inputs["valuation"]),
            inputs["rate"],
            inputs["dividend"],
            inputs["volatility"],
            inputs["spot"],
        )
        engine = engine_type()
        with subtests.test(case=row["case_id"]):
            assert engine.price(option, context) == pytest.approx(
                row["outputs"]["price"], rel=0, abs=row["tolerances"]["price"]
            )
            requested = sorted(set(row["outputs"]) - {"price"})
            if requested:
                joint = numerical_greeks(
                    engine,
                    option,
                    context,
                    requested,
                    spot_shift=inputs["spot_shift"],
                )
                numeric = differences(
                    engine,
                    option,
                    context,
                    requested,
                    spot_shift=inputs["spot_shift"],
                )
                for name in requested:
                    budget = (
                        inputs[f"numerical_tolerance_{name}"]
                        + inputs[f"uncertainty_{name}"]
                    )
                    assert getattr(joint, name) == pytest.approx(
                        row["outputs"][name], rel=0, abs=budget
                    )
                    assert getattr(numeric, name) == pytest.approx(
                        row["outputs"][name], rel=0, abs=budget
                    )


@pytest.mark.parametrize("kind,engine_type", PAIRS)
def test_undated_average_weight_and_fixed_history_theta(kind, engine_type):
    end = START + timedelta(days=10)
    original = kind(**terms(expiry_time=end, realized_average=110))
    engine = engine_type()
    for days, arithmetic, geometric in (
        (2, 2.0, 100 * 1.1**0.2 - 100),
        (5, 5.0, math.sqrt(110 * 100) - 100),
        (8, 8.0, 100 * 1.1**0.8 - 100),
    ):
        context = market(START + timedelta(days=days), 0, 0, 0)
        expected = geometric if kind is GeometricAveragePriceOption else arithmetic
        assert engine.price(original, context) == pytest.approx(expected, abs=1e-12)
    context = market(START + timedelta(days=5), 0, 0, 0)
    expected_theta = (
        365 / 10 * math.sqrt(110 * 100) * math.log(1.1)
        if kind is GeometricAveragePriceOption
        else 365
    )
    assert numerical_greeks(
        engine, original, context, "theta", time_shift=timedelta(hours=1)
    ).require("theta") == pytest.approx(expected_theta, rel=1e-6)
    assert original.realized_average == 110


@pytest.mark.parametrize("kind,engine_type", PAIRS)
def test_combined_replacement_retains_average_and_roundtrips(kind, engine_type):
    import copy
    import pickle

    original = kind(**terms(realized_average=105))
    assert original.replace() == original
    assert original.replace(realized_average=...) == original
    assert original.replace(realized_average=None).realized_average is None
    moved = original.replace(averaging_start_time=START + timedelta(days=1))
    assert moved.realized_average == 105
    combined = original.replace(strike=101, realized_average=110)
    assert combined == kind(**terms(strike=101, realized_average=110))
    for operation in (
        copy.copy,
        copy.deepcopy,
        lambda value: pickle.loads(pickle.dumps(value)),
    ):
        assert operation(combined) == combined
    with pytest.raises(InvalidInputError):
        original.replace(strike=101, realized_average=-1)
    assert original.strike == 100
    assert original.realized_average == 105
