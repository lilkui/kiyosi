"""Shared European analytics contracts, through the Rust extension."""

import importlib
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from fractions import Fraction
from typing import get_args

import pytest
import QuantLib as ql
from kiyosi import (
    Greek,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import AnalyticEuropeanEngine, implied_volatility, numerical_greeks


@pytest.fixture
def base_option():
    return EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=datetime(2025, 1, 1),
        expiry_time=datetime(2026, 1, 1),
    )


@pytest.fixture
def base_context():
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 1),
    )


@pytest.fixture
def base_engine():
    return AnalyticEuropeanEngine()


def test_implied_volatility_requires_a_bracket_for_nonmonotone_prices(subtests):
    from kiyosi.instruments import BarrierOption, CashOrNothingOption
    from kiyosi.pricing import AnalyticBarrierEngine, AnalyticDigitalEngine

    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 1),
    )
    life = {
        "option_type": "call",
        "effective_time": context.valuation_time,
        "expiry_time": datetime(2026, 1, 1),
    }
    cases = (
        (
            AnalyticBarrierEngine(),
            BarrierOption(
                **life, strike=100, barrier_level=120, barrier_type="up_and_out"
            ),
        ),
        (
            AnalyticDigitalEngine(),
            CashOrNothingOption(**life, strike=110, payout=10),
        ),
    )
    for engine, option in cases:
        quote = engine.price(option, context)
        for bounds in ({}, {"lower_bound": 0.1}, {"upper_bound": 0.3}):
            with subtests.test(bounds=bounds), pytest.raises(TypeError):
                implied_volatility(engine, option, context, quote, **bounds)
        with pytest.raises(RootNotBracketedError):
            implied_volatility(
                engine, option, context, quote, lower_bound=0, upper_bound=4
            )
        assert implied_volatility(
            engine, option, context, quote, lower_bound=0.1, upper_bound=0.3
        ) == pytest.approx(0.2, rel=0, abs=1e-8)


def test_greeks_and_volatility_recovery(base_context, base_engine, base_option):
    result = base_engine.price_with_greeks(base_option, base_context, greeks="all")
    assert result.delta == pytest.approx(0.586851146134764, rel=0, abs=1e-12)
    assert result.gamma == pytest.approx(0.0189505787550087, rel=0, abs=1e-12)
    assert result.vega == pytest.approx(37.9011575100175, rel=0, abs=1e-10)
    assert implied_volatility(
        base_engine,
        base_option,
        base_context,
        result.price,
        lower_bound=0.0,
        upper_bound=4.0,
    ) == pytest.approx(0.2, rel=0, abs=1e-8)


def test_all_selection_matches_explicit_greek_names(
    base_context, base_engine, base_option, subtests
):
    names = get_args(Greek)
    explicit = base_engine.price_with_greeks(base_option, base_context, names)
    for selected in (
        base_engine.price_with_greeks(base_option, base_context, "all"),
        base_engine.price_with_greeks(base_option, base_context, greeks="all"),
    ):
        assert selected.price == explicit.price
        for name in names:
            with subtests.test(greek=name):
                assert selected.require(name) == explicit.require(name)
    for request in ("All", ["all"], ["delta", "all"]):
        with subtests.test(request=request), pytest.raises(ValueError):
            base_engine.price_with_greeks(base_option, base_context, request)


@pytest.mark.parametrize("budget", (100, 101))
@pytest.mark.parametrize("lower", (0.1, 0.2))
def test_volatility_recovery_with_bounded_and_extended_iteration_budgets(
    base_context, base_engine, base_option, budget, lower
):
    quote = base_engine.price(base_option, base_context)
    actual = implied_volatility(
        base_engine,
        base_option,
        base_context,
        quote,
        lower_bound=lower,
        upper_bound=0.3,
        max_iterations=budget,
    )
    assert actual == pytest.approx(0.2, rel=0, abs=1e-8)
    repriced = base_engine.price(
        base_option,
        base_context.replace(
            model_parameters=base_context.model_parameters.replace(volatility=actual)
        ),
    )
    assert repriced == pytest.approx(quote, rel=0, abs=1e-8)


def test_greek_iterables_deduplicate_and_validate_through_exhaustion(
    base_context, base_engine, base_option, subtests
):
    names = get_args(Greek)
    expected = base_engine.price_with_greeks(base_option, base_context, "all")

    def repeated_names():
        for _ in range(10_000):
            yield from names

    actual = base_engine.price_with_greeks(base_option, base_context, repeated_names())
    assert actual.price == expected.price
    for name in names:
        assert actual.require(name) == expected.require(name)

    def invalid_tail(value):
        yield from repeated_names()
        yield value

    for invalid, exception in (("unknown", ValueError), (True, TypeError)):
        with subtests.test(invalid=invalid), pytest.raises(exception):
            base_engine.price_with_greeks(
                base_option, base_context, invalid_tail(invalid)
            )

    def failing_iterator():
        yield from repeated_names()
        raise RuntimeError("selection iterator failed")

    with pytest.raises(RuntimeError, match="selection iterator failed"):
        base_engine.price_with_greeks(base_option, base_context, failing_iterator())


def test_live_quantlib_greeks_scalar_differences_and_recovery(
    base_context, base_engine, subtests
):
    oracle = importlib.import_module("quantlib_reference.oracle")
    differences = importlib.import_module("price_differences").price_differences
    oracle.require_version()
    config = oracle.load_fixture("scenarios.json")
    for scenario in config["scenarios"]:
        inputs = scenario["inputs"]
        option = EuropeanOption(
            option_type=inputs["option"],
            strike=inputs["strike"],
            effective_time=datetime.fromisoformat(inputs["effective_time"]),
            expiry_time=datetime.fromisoformat(inputs["expiry_time"]),
        )
        context = base_context.replace(
            spot_price=inputs["spot"],
            valuation_time=datetime.fromisoformat(inputs["valuation"]),
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=inputs["rate"],
                dividend_yield=inputs["dividend"],
                volatility=inputs["volatility"],
            ),
        )
        saved = ql.SavedSettings()
        try:
            reference, metadata = oracle.reference(inputs)
        finally:
            del saved
        result = base_engine.price_with_greeks(option, context, greeks="all")
        budget = scenario["tolerances"]
        for name, expected in reference.items():
            with subtests.test(case=scenario["case_id"], measure=name):
                assert getattr(result, name) == pytest.approx(
                    expected,
                    rel=0,
                    abs=budget[name] + metadata[f"uncertainty_{name}"],
                )
        # Numerical Greeks use the scalar oracle and per-scenario acceptance budgets.
        numeric_budget = scenario["numerical_tolerances"]
        bumps = (
            (0.008, 0.01)
            if (option.expiry_time - context.valuation_time).days <= 2
            else (0.01, 0.012)
        )
        for bump in bumps:
            numeric = differences(
                base_engine,
                option,
                context,
                sorted(set(reference) - {"price"}),
                spot_shift=bump,
            )
            for name, expected in reference.items():
                with subtests.test(case=scenario["case_id"], measure=name, bump=bump):
                    assert getattr(numeric, name) == pytest.approx(
                        expected,
                        rel=0,
                        abs=numeric_budget[name] + metadata[f"uncertainty_{name}"],
                    )
        assert implied_volatility(
            base_engine,
            option,
            context,
            result.price,
            lower_bound=0.0,
            upper_bound=4.0,
        ) == pytest.approx(inputs["volatility"], rel=0, abs=1e-7)


def test_requests_availability_and_expiry(
    base_context, base_engine, base_option, subtests
):
    base_context = base_context.replace()
    full = base_engine.price_with_greeks(base_option, base_context, greeks="all")
    for request in (
        "delta",
        ["delta", "vega", "delta"],
        (name for name in ("delta", "vega")),
    ):
        selected = base_engine.price_with_greeks(base_option, base_context, request)
        assert selected.delta == full.delta
        assert selected.require("delta") == full.delta
        assert selected.gamma is None
        with pytest.raises(UnsupportedOperationError) as error:
            selected.require("gamma")
        assert isinstance(error.value, UnsupportedOperationError)
        assert error.value.greek == "gamma"
        assert error.value.status == "not_requested"
    expiry = base_context.replace(
        spot_price=110, valuation_time=base_option.expiry_time
    )
    result = base_engine.price_with_greeks(base_option, expiry, greeks="all")
    assert result.price == 10
    for name in (
        "theta",
        "charm",
        "color",
    ):
        assert getattr(result, name) is None
        with pytest.raises(UnsupportedOperationError):
            result.require(name)
    assert result.require("delta") == pytest.approx(1, rel=0, abs=1e-10)
    for name in (
        "gamma",
        "speed",
        "vega",
        "volga",
        "vanna",
        "zomma",
        "rho",
        "dividend_rho",
    ):
        assert result.require(name) == pytest.approx(0, rel=0, abs=1e-8)
    for changes in (
        {},
        {"greeks": None},
        {"greeks": True},
        {"greeks": [1]},
    ):
        with subtests.test(changes=changes), pytest.raises(TypeError):
            numerical_greeks(base_engine, base_option, base_context, **changes)
    for request in ("Delta", "unknown"):
        with pytest.raises(ValueError):
            base_engine.price_with_greeks(base_option, base_context, request)
    for value in (1, "unknown"):
        with pytest.raises(TypeError if value == 1 else ValueError):
            full.require(value)
    with pytest.raises(AttributeError):
        full.delta = 0
    del base_context
    assert full.delta == 0.586851146134764


def test_native_greek_controls_and_solver_representations(
    base_context, base_engine, base_option, subtests
):
    for field in (
        "spot_shift",
        "volatility_shift",
        "rate_shift",
        "time_shift",
    ):
        for bad in (True, "1", complex(1)) + (
            () if field in ("spot_shift", "volatility_shift", "time_shift") else (None,)
        ):
            with subtests.test(field=field, bad=bad), pytest.raises(TypeError):
                numerical_greeks(
                    base_engine, base_option, base_context, "delta", **{field: bad}
                )
    supplied = numerical_greeks(
        base_engine,
        base_option,
        base_context,
        "delta",
        spot_shift=0.001,
        volatility_shift=0.001,
        rate_shift=0.001,
        time_shift=timedelta(days=2),
    )
    spot = base_context.spot_price
    h = (spot + 0.001) - spot
    high = base_engine.price(base_option, base_context.replace(spot_price=spot + h))
    low = base_engine.price(base_option, base_context.replace(spot_price=spot - h))
    assert supplied.require("delta") == pytest.approx(
        (high - low) / (2 * h), rel=0, abs=1e-12
    )
    for field in (
        "lower_bound",
        "upper_bound",
        "price_tolerance",
        "parameter_tolerance",
        "max_iterations",
    ):
        for bad in (True, "1", None, complex(1)):
            with subtests.test(field=field, bad=bad), pytest.raises(TypeError):
                implied_volatility(
                    base_engine,
                    base_option,
                    base_context,
                    9.227,
                    **{"lower_bound": 0.0, "upper_bound": 4.0, field: bad},
                )
    for value in (1.5, 1.0, Fraction(3, 2)):
        with pytest.raises(TypeError):
            implied_volatility(
                base_engine,
                base_option,
                base_context,
                9.227,
                max_iterations=value,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    with pytest.raises(OverflowError):
        implied_volatility(
            base_engine,
            base_option,
            base_context,
            9.227,
            max_iterations=2**40,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    for quote in (True, "1", None):
        with pytest.raises(TypeError):
            implied_volatility(
                base_engine,
                base_option,
                base_context,
                quote,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    with pytest.raises(OverflowError):
        implied_volatility(
            base_engine,
            base_option,
            base_context,
            10**1000,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(TypeError):
        implied_volatility(
            base_engine,
            base_option,
            base_context,
            9.227,
            unknown=1,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(TypeError):
        implied_volatility(
            base_engine,
            base_option,
            base_context,
            9.227,
            0.1,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(TypeError):
        base_engine.price_with_greeks(base_option, base_context, "delta", unknown=1)
    result = base_engine.price_with_greeks(base_option, base_context, "delta")
    assert result.delta == pytest.approx(0.586851146134764, rel=0, abs=1e-12)


def test_domain_errors_validation_order_and_failure_state(
    base_context, base_engine, base_option
):
    quote = base_engine.price(base_option, base_context)

    def solve(context=None, observed=quote, **kw):
        return implied_volatility(
            base_engine,
            base_option,
            context or base_context,
            observed,
            **{"lower_bound": 0.0, "upper_bound": 4.0, **kw},
        )

    for controls in (
        {"lower_bound": -0.1},
        {"lower_bound": 4},
        {"upper_bound": math.inf},
        {"price_tolerance": 0},
        {"parameter_tolerance": math.nan},
        {"max_iterations": 0},
    ):
        with pytest.raises(InvalidInputError) as error:
            solve(**controls)
        assert type(error.value) is InvalidInputError
    for observed, expected_error in (
        (math.nan, InvalidInputError),
        (math.inf, InvalidInputError),
        (-1, RootNotBracketedError),
        (101, RootNotBracketedError),
    ):
        with pytest.raises(expected_error) as error:
            solve(observed=observed)
        assert type(error.value) is expected_error
    with pytest.raises(NonConvergenceError) as error:
        solve(max_iterations=1, price_tolerance=1e-15, parameter_tolerance=1e-15)
    assert type(error.value) is NonConvergenceError
    expiry = base_context.replace(valuation_time=base_option.expiry_time)
    with pytest.raises(UnsupportedOperationError) as error:
        solve(expiry, 0)
    assert type(error.value) is UnsupportedOperationError
    outside = base_context.replace(valuation_time=datetime(2026, 1, 1, 0, 0, 0, 1))
    with pytest.raises(InvalidInputError) as error:
        solve(outside, 0)
    assert type(error.value) is InvalidInputError
    with pytest.raises(InvalidInputError) as error:
        base_engine.price_with_greeks(base_option, outside, [])
    assert type(error.value) is InvalidInputError
    assert base_context.model_parameters.volatility == 0.2
    assert base_engine.price(base_option, base_context) == quote


def test_solver_endpoints_curves_and_concurrent_calls(
    base_context, base_engine, base_option
):
    for vol in (0, 0.03, 0.2, 1.5, 4):
        context = base_context.replace(
            model_parameters=base_context.model_parameters.replace(volatility=vol)
        )
        quote = base_engine.price(base_option, context)
        assert implied_volatility(
            base_engine,
            base_option,
            base_context,
            quote,
            lower_bound=0.0,
            upper_bound=4.0,
        ) == pytest.approx(vol, rel=0, abs=1e-8)
    reference = datetime(2025, 1, 1)
    end = datetime(2026, 1, 1)
    knot = reference + timedelta(days=180)
    curve = RateTermStructure(
        reference_time=reference,
        times=[knot, end],
        factors=[
            math.exp(-0.02 * 180 / 365),
            math.exp(-0.02 * 180 / 365 - 0.08 * 185 / 365),
        ],
    )
    context = base_context.replace(
        model_parameters=base_context.model_parameters.replace(risk_free_rate=curve),
        valuation_time=knot,
    )
    result = base_engine.price_with_greeks(base_option, context, greeks="all")
    after = base_context.replace(
        model_parameters=context.model_parameters,
        valuation_time=knot + timedelta(seconds=1),
    )
    assert result.theta == pytest.approx(
        (base_engine.price(base_option, after) - result.price) * (365 * 86400),
        rel=0,
        abs=0.0000365,
    )
    quote = result.price

    def recover(_):
        return implied_volatility(
            base_engine,
            base_option,
            context,
            quote,
            lower_bound=0.0,
            upper_bound=4.0,
        )

    with ThreadPoolExecutor(max_workers=4) as pool:
        for vol in pool.map(recover, range(16)):
            assert vol == pytest.approx(0.2, rel=0, abs=1e-8)


def test_analytic_speed_preserves_subnormal_volatility_widths(
    base_context, base_engine, base_option, subtests
):
    # Independent 80-digit references for ATM, zero-rate speed = -3 * gamma / (2 * spot).
    for volatility, valuation, expected in (
        (math.ulp(0.0), datetime(2025, 1, 1), -1211.2022473921038),
        (3 * math.ulp(0.0), datetime(2025, 1, 1), -403.7340824640346),
        (
            math.ulp(0.0),
            datetime(2025, 12, 31, 23, 59, 59, 999999),
            -6801739133.529686,
        ),
    ):
        context = base_context.replace(
            spot_price=1e160,
            valuation_time=valuation,
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=volatility
            ),
        )
        for direction in ("call", "put"):
            with subtests.test(
                volatility=volatility, valuation=valuation, direction=direction
            ):
                option = EuropeanOption(
                    option_type=direction,
                    strike=1e160,
                    effective_time=base_option.effective_time,
                    expiry_time=base_option.expiry_time,
                )
                result = base_engine.price_with_greeks(option, context, "speed")
                assert result.speed / expected == pytest.approx(1, rel=0, abs=1e-12)


def test_extreme_greeks_preserve_scaling_and_selected_results(
    base_context, base_engine, base_option
):
    for vol, spot, strike in (
        (1e-200, 100, 100),
        (0.2, 1e300, 1e304),
        (1e308, 100, 100),
    ):
        option = EuropeanOption(
            option_type="call",
            strike=strike,
            effective_time=base_option.effective_time,
            expiry_time=base_option.expiry_time,
        )
        context = base_context.replace(
            spot_price=spot,
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=vol
            ),
        )
        result = base_engine.price_with_greeks(
            option, context, ["delta", "vega", "gamma"]
        )
        for name in ("delta", "vega", "gamma"):
            assert math.isfinite(getattr(result, name))
            assert getattr(result, name) == getattr(
                base_engine.price_with_greeks(option, context, name), name
            )
    # Normal density scaled before exponentiation; unscaled tail density underflows.
    assert result.gamma == 0
