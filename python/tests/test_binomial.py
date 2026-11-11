"""CRR public contracts shared by the Rust implementation."""

import importlib
import math
import numbers
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
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
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticDigitalEngine,
    AnalyticEuropeanEngine,
    CoxRossRubinsteinDigitalEngine,
    CoxRossRubinsteinVanillaEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.history import history_fields
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
GREEKS = (
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "volga",
    "vanna",
    "zomma",
    "rho",
    "dividend_rho",
)
VANILLA = (EuropeanOption, AmericanOption)
DIGITAL = (CashOrNothingOption, AssetOrNothingOption)


def contract(kind, direction="call", strike=100):
    terms = {
        "option_type": direction,
        "strike": strike,
        "effective_time": START,
        "expiry_time": END,
    }
    if kind is CashOrNothingOption:
        terms["payout"] = 10
    return kind(**history_fields(**terms))


def market(spot=100, rate=0.05, dividend=0.02, sigma=0.2, valuation=START):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=sigma
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def engine_for(kind, steps=256):
    factory = (
        CoxRossRubinsteinVanillaEngine
        if kind in VANILLA
        else CoxRossRubinsteinDigitalEngine
    )
    return factory(step_count=steps)


def rollback(
    kind, direction, steps, spot=100, rate=0.05, dividend=0.02, sigma=0.2, strike=100
):
    """Compute independent CRR prices and node Greeks by explicit rollback.

    Parameters
    ----------
    kind : type or str
        Contract class or barrier activation choice used by the test.
    direction : {'call', 'put'} or None
        Terminal payoff direction; touch contracts have no strike direction.
    steps : int
        Positive CRR time-step count.
    spot : float, optional
        Underlying spot price.
    rate : float, optional
        Annual continuously compounded risk-free rate.
    dividend : float, optional
        Annual continuously compounded dividend yield.
    sigma : float, optional
        Annualized volatility in decimal units.
    strike : float, optional
        Exercise price.

    Returns
    -------
    tuple of float or None
        Price, node delta and node gamma; unavailable node Greeks are None.
    """
    up = math.exp(sigma / math.sqrt(steps))
    down = 1 / up
    probability = (math.exp((rate - dividend) / steps) - down) / (up - down)
    discount = math.exp(-rate / steps)
    sign = 1 if direction == "call" else -1
    values = []
    for node in range(steps + 1):
        terminal = spot * math.exp((2 * node - steps) * sigma / math.sqrt(steps))
        itm = sign * (terminal - strike) > 0
        values.append(
            (10 if kind is CashOrNothingOption else terminal) * itm
            if kind in DIGITAL
            else max(sign * (terminal - strike), 0)
        )
    first, second = [], []
    for level in range(steps, 0, -1):
        if level == 1:
            first = values[:2]
        if level == 2:
            second = values[:3]
        values = [
            discount * ((1 - probability) * values[n] + probability * values[n + 1])
            for n in range(level)
        ]
        if kind is AmericanOption:
            values = [
                max(
                    v,
                    sign
                    * (
                        spot * math.exp((2 * n - level + 1) * sigma / math.sqrt(steps))
                        - strike
                    ),
                )
                for n, v in enumerate(values)
            ]
    delta = (first[1] - first[0]) / (spot * (up - down))
    gamma = (
        None
        if steps == 1
        else (
            (second[2] - second[1]) / (spot * (up**2 - 1))
            - (second[1] - second[0]) / (spot * (1 - down**2))
        )
        / (spot * (up**2 - down**2) / 2)
    )
    return values[0], delta, gamma


@pytest.mark.parametrize("rate,dividend", ((0, 0), (0.05, 0.05), (1e-20, 0)))
@pytest.mark.parametrize("kind", (EuropeanOption, CashOrNothingOption))
@pytest.mark.parametrize("steps", (1, 256))
def test_implied_volatility_recovers_zero_and_tiny_carry_quotes(
    rate, dividend, kind, steps
):
    context = market(rate=rate, dividend=dividend)
    option, engine = contract(kind), engine_for(kind, steps)
    quote = engine.price(option, context)
    sigma = implied_volatility(
        engine,
        option,
        context,
        quote,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert sigma == pytest.approx(0.2, rel=0, abs=1e-6)
    shifted = context.replace(
        model_parameters=context.model_parameters.replace(volatility=sigma)
    )
    assert engine.price(option, shifted) == pytest.approx(quote, rel=0, abs=1e-8)


def test_implied_volatility_requires_a_matching_quote():
    option = contract(CashOrNothingOption, strike=110)
    context = market(rate=0, dividend=0)
    engine = CoxRossRubinsteinDigitalEngine(step_count=1)
    with pytest.raises(NonConvergenceError):
        implied_volatility(
            engine, option, context, 1, lower_bound=0.01, upper_bound=0.2
        )


def test_one_step_speed_respects_automatic_and_explicit_spot_bumps():
    engine = engine_for(EuropeanOption, 1)
    option = contract(EuropeanOption, strike=0.015)
    context = market(spot=0.015)
    result = engine.price_with_greeks(option, context, "speed")
    assert result.price == engine.price(option, context)
    assert result.require("speed") * context.spot_price**2 == pytest.approx(
        0, rel=0, abs=1e-8
    )
    explicit = numerical_greeks(engine, option, context, "speed", spot_shift=0.01)
    assert explicit.status("speed") == "stencil_unavailable"


def test_default_steps_and_european_price():
    option = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=datetime(2025, 1, 1),
        expiry_time=datetime(2026, 1, 1),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 1),
    )
    engine = CoxRossRubinsteinVanillaEngine()
    assert engine.step_count == 256
    assert engine.price(option, context) == CoxRossRubinsteinVanillaEngine(
        step_count=256
    ).price(option, context)
    assert engine.price(option, context) == pytest.approx(
        9.227005508154036, rel=0, abs=0.01
    )


def test_configuration_representation_and_domain_boundaries():
    @numbers.Integral.register
    class Index:
        def __index__(self):
            return 17

    for factory in (CoxRossRubinsteinVanillaEngine, CoxRossRubinsteinDigitalEngine):
        assert factory().step_count == 256
        assert factory(step_count=Index()).step_count == 17
        for count in (1, 1_000_000):
            engine = factory(step_count=count)
            assert engine.step_count == count
            with pytest.raises(AttributeError):
                engine.step_count = 5
        for count in (0, -1, 1_000_001):
            assert_rejection(InvalidInputError, factory, step_count=count)
        for count in (True, False, 1.0, 1.5, "1", None):
            with pytest.raises(TypeError):
                factory(step_count=count)
        for count in (2**31, -(2**31) - 1):
            with pytest.raises(OverflowError):
                factory(step_count=count)
        with pytest.raises(TypeError):
            factory(unknown=1)


@pytest.mark.parametrize("steps", (1, 2, 3, 17, 255, 256, 257))
@pytest.mark.parametrize("kind", (*VANILLA, *DIGITAL))
@pytest.mark.parametrize("direction", ("call", "put"))
def test_node_greeks_and_prices_match_explicit_rollback(steps, kind, direction):
    option = contract(kind, direction)
    engine = engine_for(kind, steps)
    expected = rollback(kind, direction, steps)
    result = engine.price_with_greeks(option, market(), ("delta", "gamma"))
    assert result.price == pytest.approx(expected[0], rel=0, abs=1e-10)
    assert result.delta == pytest.approx(expected[1], rel=0, abs=1e-10)
    if steps >= 2:
        assert result.gamma == pytest.approx(expected[2], rel=0, abs=1e-10)
        gamma_only = engine.price_with_greeks(option, market(), "gamma")
        assert result.gamma == gamma_only.gamma
        assert gamma_only.delta is None
    elif kind in VANILLA:
        assert result.gamma is not None
    else:
        assert result.gamma is None
    assert result.vega is None
    assert result.price == engine.price(option, market())


@pytest.mark.parametrize(
    "rate,dividend,sigma",
    ((0.05, 0.02, 0.2), (-0.03, 0.01, 0.45), (0.01, 0.07, 0.3), (-0.02, -0.05, 0.15)),
)
@pytest.mark.parametrize("spot", (95, 100, 105))
@pytest.mark.parametrize("kind", (EuropeanOption, *DIGITAL))
@pytest.mark.parametrize("direction", ("call", "put"))
def test_grid_refinement_and_negative_carry(
    rate, dividend, sigma, spot, kind, direction
):
    context = market(spot, rate, dividend, sigma)
    analytic = (
        AnalyticEuropeanEngine() if kind is EuropeanOption else AnalyticDigitalEngine()
    )
    option = contract(kind, direction)
    expected = analytic.price_with_greeks(option, context, ("delta", "gamma"))
    for steps in (255, 256, 4095, 4096):
        result = engine_for(kind, steps).price_with_greeks(
            option, context, ("delta", "gamma")
        )
        budget = (10 if kind is CashOrNothingOption else spot) / math.sqrt(steps)
        assert result.price == pytest.approx(expected.price, rel=0, abs=budget)
        assert result.delta == pytest.approx(
            expected.delta, rel=0, abs=budget / spot / sigma
        )
        assert result.gamma == pytest.approx(
            expected.gamma, rel=0, abs=budget / spot**2 / sigma**2
        )


@pytest.mark.parametrize("steps", (1, 2, 3, 17, 255, 256, 257))
def test_american_immediate_exercise_and_european_limit(steps):
    engine = engine_for(AmericanOption, steps)
    for direction, strike, spot, rate, dividend, delta in (
        ("put", 100, 88, 0.15, 0, -1),
        ("call", 88, 100, 0, 0.15, 1),
    ):
        result = engine.price_with_greeks(
            contract(AmericanOption, direction, strike),
            market(spot, rate, dividend),
            greeks="all",
        )
        assert ((result.price, result.delta, result.gamma)) == ((12, delta, 0))
    for rate, dividend in ((0, 0), (0.04, 0), (0.04, -0.01)):
        context = market(rate=rate, dividend=dividend)
        american = engine.price_with_greeks(
            contract(AmericanOption), context, greeks="all"
        )
        european = engine.price_with_greeks(
            contract(EuropeanOption), context, greeks="all"
        )
        assert american.price == pytest.approx(european.price, rel=0, abs=1e-10)
        for name in GREEKS:
            assert getattr(american, name) is not None
            if name in ("delta", "gamma"):
                assert getattr(american, name) == pytest.approx(
                    getattr(european, name), rel=0, abs=1e-10
                )


@pytest.mark.parametrize("kind", (*VANILLA, *DIGITAL))
@pytest.mark.parametrize("direction", ("call", "put"))
def test_expiry_strict_equality_and_lifetime_boundaries(kind, direction):
    engine = engine_for(kind)
    option = contract(kind, direction)
    for spot in (90, 100, 110):
        context = market(spot, 100, -100, 1e-200, END)
        intrinsic = max((1 if direction == "call" else -1) * (spot - 100), 0)
        expected = (
            (10 if kind is CashOrNothingOption else spot) * (intrinsic > 0)
            if kind in DIGITAL
            else intrinsic
        )
        result = engine.price_with_greeks(option, context, greeks="all")
        assert result.price == expected
        delta = (
            None
            if spot == 100
            else ((1 if direction == "call" else -1) if intrinsic else 0)
            if kind in VANILLA
            else int(kind is AssetOrNothingOption and intrinsic > 0)
        )
        assert_terminal_greeks(result, delta)
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
    for time in (
        START - timedelta(days=1),
        datetime(2026, 1, 1) + timedelta(microseconds=1),
    ):
        if kind is not AmericanOption and time < START:
            assert math.isfinite(engine.price(option, market(valuation=time)))
        else:
            assert_rejection(
                InvalidInputError,
                engine.price,
                option,
                market(valuation=time),
            )


@pytest.mark.parametrize("kind", (*VANILLA, *DIGITAL))
def test_flat_rate_applicability_validation_order_and_failures(kind):
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[math.exp(-0.05), (math.exp(-0.05)) * 0.5],
    )
    engine, option = engine_for(kind), contract(kind)
    for valuation in (START, END):
        context = market(rate=curve, valuation=valuation)
        assert_rejection(UnsupportedOperationError, engine.price, option, context)
        assert_rejection(
            UnsupportedOperationError,
            engine.price_with_greeks,
            option,
            context,
            [],
        )
    for field in ("rate", "dividend"):
        context = market(**{field: curve})
        assert_rejection(UnsupportedOperationError, engine.price, option, context)
    flat = RateTermStructure.flat(rate=0.05, reference_time=START)
    assert engine.price(option, market(rate=flat)) == engine.price(option, market())
    for sigma, rate, dividend in (
        (1e-200, 0.05, 0.02),
        (50, 0.05, 0.02),
        (0.2, -1000, -1000),
    ):
        assert_rejection(
            ComputationFailureError,
            engine.price,
            option,
            market(sigma=sigma, rate=rate, dividend=dividend),
        )
    if kind in VANILLA:
        for shifts in (
            {"spot_shift": 0},
            {"volatility_shift": math.nan},
            {"rate_shift": -1},
            {"time_shift": timedelta(days=0)},
        ):
            assert_rejection(
                InvalidInputError,
                partial(numerical_greeks, engine),
                option,
                market(),
                "delta",
                **shifts,
            )
    for shifts in (
        {"spot_shift": True},
        {"time_shift": 1.5},
        {"time_shift": True},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            numerical_greeks(engine, option, market(), "delta", **shifts)
    with pytest.raises(TypeError):
        numerical_greeks(engine, option, market(), "delta", time_shift=2**31)
    mismatch = contract(CashOrNothingOption if kind in VANILLA else EuropeanOption)
    for operation, args in (
        (engine.price, (mismatch, market())),
        (engine.price_with_greeks, (mismatch, market(), "delta")),
        (
            partial(implied_volatility, lower_bound=0.0, upper_bound=4.0),
            (engine, mismatch, market(), 10),
        ),
    ):
        with pytest.raises(UnsupportedOperationError):
            operation(*args)


def test_digital_greek_availability():
    for kind in DIGITAL:
        for steps in (1, 2, 256):
            engine, option = engine_for(kind, steps), contract(kind)
            expected = engine.price_with_greeks(option, market(), greeks="all")
            for name in GREEKS:
                assert (getattr(expected, name) is not None) == (
                    name == "delta" or (name == "gamma" and steps >= 2)
                )
                if name not in ("delta", "gamma"):
                    with pytest.raises(UnsupportedOperationError):
                        engine.price_with_greeks(option, market(), name)
                    continue
                selected = engine.price_with_greeks(
                    option,
                    market(),
                    name,
                )
                if getattr(expected, name) is not None:
                    assert getattr(selected, name) == pytest.approx(
                        getattr(expected, name), rel=0, abs=1e-12
                    )
                assert selected.price == expected.price


@pytest.mark.parametrize("kind", (*VANILLA, *DIGITAL))
@pytest.mark.parametrize("direction", ("call", "put"))
def test_implied_volatility_tree_bounds_failures_and_preserved_state(kind, direction):
    engine = engine_for(kind, 17)
    option, context = (
        contract(kind, direction),
        market(rate=0.15, dividend=0),
    )
    quote = engine.price(option, context)
    assert implied_volatility(
        engine, option, context, quote, upper_bound=0.4, lower_bound=0.0
    ) == pytest.approx(0.2, rel=0, abs=1e-6)
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        option,
        context,
        quote,
        upper_bound=0.01,
        lower_bound=0.0,
    )
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        option,
        context,
        -1,
        upper_bound=0.4,
        lower_bound=0.0,
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
        InvalidInputError,
        implied_volatility,
        engine,
        option,
        context,
        quote,
        max_iterations=0,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert engine.price(option, context) == quote
    assert engine.step_count == 17
    assert context.spot_price == 100
    assert context.model_parameters.volatility == 0.2


def test_deterministic_endpoints_and_extreme_tail_regression():
    for rate in (-3.2, 3.2):
        for kind in DIGITAL:
            for direction in ("call", "put"):
                pays = (direction == "call") == (rate > 0)
                expected = (
                    (10 * math.exp(-rate) if kind is CashOrNothingOption else 100)
                    if pays
                    else 0
                )
                assert engine_for(kind).price(
                    contract(kind, direction), market(rate=rate, dividend=0)
                ) == pytest.approx(expected, rel=0, abs=1e-9)
    context = market(spot=1e280, rate=0.04, dividend=0.01)
    option = contract(EuropeanOption, strike=1e280 * math.exp(8))
    expected = rollback(
        EuropeanOption,
        "call",
        2048,
        spot=1e280,
        rate=0.04,
        dividend=0.01,
        strike=option.strike,
    )[0]
    assert expected > 0
    assert engine_for(EuropeanOption, 2048).price(
        option, context
    ) / expected == pytest.approx(1, rel=0, abs=1e-9)


@pytest.mark.parametrize("kind", (*VANILLA, *DIGITAL))
def test_shared_and_distinct_instances_support_concurrent_pricing(kind):
    option, context, engine = contract(kind), market(), engine_for(kind)
    expected = engine.price(option, context)

    def price(index, engine=engine, kind=kind, option=option, context=context):
        selected = engine if index % 2 else engine_for(kind)
        quote = selected.price(option, context)
        sigma = implied_volatility(
            selected, option, context, quote, lower_bound=0.1, upper_bound=0.22
        )
        return quote, sigma

    with ThreadPoolExecutor(max_workers=4) as pool:
        for quote, sigma in pool.map(price, range(8)):
            assert quote == expected
            assert sigma == pytest.approx(0.2, rel=0, abs=1e-6)


def test_existing_quantlib_european_and_american_profiles(subtests):
    oracle = importlib.import_module("quantlib_reference.oracle")
    american = importlib.import_module("quantlib_reference.american")
    oracle.require_version()
    european_profile = next(
        p
        for p in oracle.load_fixture("numerical_engines.json")["profiles"]
        if p["engine"] == "CoxRossRubinsteinVanillaEngine"
    )
    american_config = oracle.load_fixture("american.json")
    american_profile = next(
        p
        for p in american_config["profiles"]
        if p["engine"] == "CoxRossRubinsteinVanillaEngine"
    )
    european_scenarios = oracle.load_fixture("scenarios.json")["scenarios"]
    for kind, rows, profile in (
        (EuropeanOption, european_scenarios, european_profile),
        (AmericanOption, american_config["scenarios"], american_profile),
    ):
        engine = CoxRossRubinsteinVanillaEngine(**profile["settings"])
        for scenario in rows:
            inputs = scenario["inputs"]
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
            if kind is EuropeanOption and oracle.exclusions(inputs):
                # The numerical profile excludes wide-bump near-expiry stencils.
                reference = {
                    name: value
                    for name, value in reference.items()
                    if name in ("price", "delta", "gamma")
                }
            names = sorted(reference.keys() - {"price"})
            result = engine.price_with_greeks(option, context, names)
            volatility_greek = numerical_greeks(
                engine,
                option,
                context,
                "vega",
                **profile["shifts"],
            )
            for name, expected in reference.items():
                budget = (
                    profile["tolerances"]
                    if name in ("price", "delta", "gamma")
                    else profile["numerical_tolerances"]
                )
                with subtests.test(kind=kind, case=scenario["case_id"], measure=name):
                    assert (
                        volatility_greek.vega
                        if name == "vega"
                        else getattr(result, name)
                    ) == pytest.approx(
                        expected,
                        rel=0,
                        abs=budget[name] + metadata[f"uncertainty_{name}"],
                    )
