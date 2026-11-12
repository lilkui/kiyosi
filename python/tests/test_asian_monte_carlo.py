"""Asian Monte Carlo sampling contracts."""

import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import ArithmeticAveragePriceOption, GeometricAveragePriceOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
KINDS = (ArithmeticAveragePriceOption, GeometricAveragePriceOption)


def market(spot=100, volatility=0.2, valuation=START, rate=0.04, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def option(kind, direction="call", strike=100, averaging=START, realized=None):
    return kind(
        option_type=direction,
        strike=strike,
        effective_time=START,
        averaging_start_time=averaging,
        expiry_time=END,
        realized_average=realized,
    )


def test_constructor_exports_defaults_and_representation_errors(subtests):
    cls = pricing.MonteCarloAsianEngine
    assert "MonteCarloAsianEngine" in pricing.__all__
    engine, vanilla = cls(), pricing.MonteCarloVanillaEngine()
    for name in (
        "path_count",
        "step_count",
        "seed",
        "max_active_workers",
        "sampling",
        "antithetic",
    ):
        assert getattr(engine, name) == getattr(vanilla, name)
    with pytest.raises(AttributeError):
        engine.seed = 5
    for name in ("path_count", "step_count", "max_active_workers", "seed"):
        for value in (True, 2.5):
            with subtests.test(name=name, value=value), pytest.raises(TypeError):
                cls(**{name: value})
        with subtests.test(name=name), pytest.raises(OverflowError):
            cls(**{name: 1 << 100})
    with pytest.raises(TypeError):
        cls(100)
    with pytest.raises(TypeError):
        cls(antithetic=1)
    for settings in (
        {"path_count": 0},
        {"step_count": 1},
        {"step_count": 10001},
        {"max_active_workers": -1},
        {"sampling": "sobol", "path_count": 100},
    ):
        assert_rejection(InvalidInputError, cls, **settings)


@pytest.mark.parametrize("kind", KINDS)
def test_history_start_intraday_life_and_expiry(kind):
    engine = pricing.MonteCarloAsianEngine(path_count=32, seed=42)
    averaging = START + timedelta(days=90)
    instant = datetime(2025, 4, 1)
    for shift in (-1, 0, 1):
        for realized in (None, 100):
            instrument = option(kind, averaging=averaging, realized=realized)
            context = market(valuation=instant + timedelta(seconds=shift))
            if (shift > 0) == (realized is not None):
                assert engine.price(instrument, context) >= 0
            else:
                assert_rejection(
                    MissingHistoryError if realized is None else InvalidInputError,
                    engine.price,
                    instrument,
                    context,
                )
    for when in (START - timedelta(days=1), END + timedelta(days=1)):
        assert_rejection(
            InvalidInputError,
            engine.price,
            option(kind),
            market(valuation=when),
        )
    settled = pricing.MonteCarloAsianEngine(path_count=1, seed=42)
    for direction, expected in (("call", 10), ("put", 0)):
        result = settled.price_with_greeks(
            option(kind, direction, realized=110),
            market(valuation=END),
            greeks="all",
        )
        assert result.price == expected
        assert_terminal_greeks(result, 0)
        assert (
            settled.price(
                option(kind, direction, averaging=END),
                market(110, valuation=END),
            )
            == expected
        )


@pytest.mark.parametrize("seed", (7, 42, 103))
def test_geometric_independent_reference_and_arithmetic_published_convergence(seed):
    engine = pricing.MonteCarloAsianEngine(
        path_count=65536,
        step_count=64,
        seed=seed,
        sampling="sobol",
        antithetic=True,
    )
    assert engine.price(
        option(GeometricAveragePriceOption),
        market(),
    ) == pytest.approx(5.035867546515384, rel=0, abs=0.002)
    # Published Vecer continuous references, one year in matching ACT/365 time.
    for spot, expected in ((1.9, 0.193174), (2.0, 0.246416), (2.1, 0.306220)):
        context = market(spot, 0.5, rate=0.05, dividend=0)
        call = option(ArithmeticAveragePriceOption, strike=2)
        actual = engine.price(call, context)
        assert actual == pytest.approx(expected, rel=0, abs=0.001)
        coarse = pricing.MonteCarloAsianEngine(
            path_count=65536,
            step_count=2,
            seed=seed,
            sampling="sobol",
            antithetic=True,
        )
        assert abs(actual - expected) < abs(coarse.price(call, context) - expected)
        put = option(ArithmeticAveragePriceOption, "put", strike=2)
        parity = math.exp(-0.05) * (spot * math.expm1(0.05) / 0.05 - 2)
        assert actual - engine.price(put, context) == pytest.approx(
            parity, rel=0, abs=0.001
        )


@pytest.mark.parametrize("kind", KINDS)
@pytest.mark.parametrize("direction", ("call", "put"))
@pytest.mark.parametrize("window", (0, 1, 2))
def test_requested_greeks_forward_and_seasoned_fixed_history(kind, direction, window):
    engine = pricing.MonteCarloAsianEngine(
        path_count=65536, step_count=32, seed=42, sampling="sobol", antithetic=True
    )
    averaging = START + timedelta(days=120) if window == 1 else START
    valuation = START + timedelta(days=180) if window == 2 else START
    instrument = option(
        kind,
        direction,
        averaging=averaging,
        realized=105 if window == 2 else None,
    )
    context = market(valuation=valuation, rate=-0.02)
    result = engine.price_with_greeks(instrument, context, greeks="all")
    assert result.price == engine.price(instrument, context)
    for name in (
        "speed",
        "theta",
        "charm",
        "color",
        "vega",
        "vanna",
        "zomma",
        "rho",
    ):
        assert getattr(result, name) is None
    if kind is GeometricAveragePriceOption:
        expected = pricing.AnalyticGeometricAveragePriceEngine().price_with_greeks(
            instrument,
            context,
            greeks=["delta", "gamma"],
        )
        assert result.price == pytest.approx(expected.price, rel=0, abs=0.003)
        assert result.delta == pytest.approx(expected.delta, rel=0, abs=0.001)
        assert result.gamma == pytest.approx(expected.gamma, rel=0, abs=0.0005)
    else:
        for h in (0.05, 0.025):
            up = engine.price(
                instrument,
                market(100 + h, valuation=valuation, rate=-0.02),
            )
            down = engine.price(
                instrument,
                market(100 - h, valuation=valuation, rate=-0.02),
            )
            assert result.delta == pytest.approx(
                (up - down) / (2 * h), rel=0, abs=0.00002
            )
            assert result.gamma == pytest.approx(
                (up - 2 * result.price + down) / h**2, rel=0, abs=0.00002
            )
    selected = engine.price_with_greeks(instrument, context, greeks="gamma")
    assert selected.gamma == result.gamma
    assert selected.delta is None
    controlled = numerical_greeks(
        engine,
        instrument,
        context,
        "delta",
        spot_shift=0.1,
        volatility_shift=0.001,
        rate_shift=0.001,
        time_shift=timedelta(days=2),
    )
    assert controlled.price == result.price
    assert controlled.delta == pytest.approx(result.delta, rel=0, abs=0.00002)


@pytest.mark.parametrize("sampling,paths", (("pseudo_random", 4097), ("sobol", 4096)))
@pytest.mark.parametrize("kind", KINDS)
def test_sampling_threading_concurrency_and_exact_distribution_step_invariance(
    sampling, paths, kind
):
    engine = pricing.MonteCarloAsianEngine(
        path_count=paths,
        step_count=16,
        seed=42,
        sampling=sampling,
        antithetic=True,
        max_active_workers=1,
    )
    parallel = pricing.MonteCarloAsianEngine(
        path_count=paths,
        step_count=16,
        seed=42,
        sampling=sampling,
        antithetic=True,
        max_active_workers=4,
    )
    instrument = option(kind)
    expected = engine.price_with_greeks(instrument, market(), greeks="all")
    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(
            pool.map(
                lambda _, parallel=parallel, instrument=instrument: (
                    parallel.price_with_greeks(
                        instrument,
                        market(),
                        greeks="all",
                    )
                ),
                range(8),
            )
        )
    for result in results:
        assert ((result.price, result.delta, result.gamma)) == (
            (expected.price, expected.delta, expected.gamma)
        )
    for steps in (2, 10000):
        changed = pricing.MonteCarloAsianEngine(
            path_count=paths,
            step_count=steps,
            seed=42,
            sampling=sampling,
            antithetic=True,
        )
        single = option(kind, averaging=END)
        assert changed.price(single, market()) == engine.price(single, market())
        if kind is GeometricAveragePriceOption:
            assert changed.price(instrument, market()) == expected.price


def test_tiny_volatility_scaled_prices_and_negative_seasoned_geometric_gamma():
    engine = pricing.MonteCarloAsianEngine(
        path_count=65536, step_count=64, seed=42, sampling="sobol", antithetic=True
    )
    for spot, volatility in (
        (100, 1e-170),
        (1e160, 1e-160),
        (1e300, 1e-200),
        (1e300, math.ulp(0.0)),
    ):
        expected = math.exp(math.log(spot) + math.log(volatility)) / math.sqrt(
            6 * math.pi
        )
        for kind in KINDS:
            for direction in ("call", "put"):
                actual = engine.price(
                    option(kind, direction, strike=spot),
                    market(spot, volatility, rate=0, dividend=0),
                )
                assert math.isclose(actual, expected, rel_tol=0.002), (actual, expected)
    instrument = option(GeometricAveragePriceOption, strike=10, realized=100)
    context = market(valuation=START + timedelta(days=180))
    actual = engine.price_with_greeks(instrument, context, greeks="gamma")
    expected = pricing.AnalyticGeometricAveragePriceEngine().price_with_greeks(
        instrument, context, greeks="gamma"
    )
    assert actual.gamma < 0
    assert actual.gamma == pytest.approx(expected.gamma, rel=0, abs=0.0001)


def test_implied_volatility_recovery_and_affine_backend_independence():
    engine = pricing.MonteCarloAsianEngine(
        path_count=16384, step_count=16, seed=42, sampling="sobol"
    )
    for kind in KINDS:
        instrument = option(kind)
        observed = engine.price(instrument, market())
        solved = pricing.implied_volatility(
            engine,
            instrument,
            market(volatility=0.35),
            observed,
            lower_bound=0.0,
            upper_bound=4.0,
        )
        assert solved == pytest.approx(0.2, rel=0, abs=0.00001)
    settled = pricing.MonteCarloAsianEngine(path_count=1, seed=42)
    instrument = option(ArithmeticAveragePriceOption, strike=25, realized=100)
    context = market(valuation=START + timedelta(days=180))
    actual = settled.price_with_greeks(instrument, context, greeks="all")
    assert actual.price == pricing.TurnbullWakemanArithmeticAveragePriceEngine().price(
        instrument, context
    )
    assert actual.gamma == 0
    future = 185 / 365
    assert actual.delta == pytest.approx(
        math.exp(-0.04 * future) * math.expm1(0.03 * future) / 0.03,
        rel=0,
        abs=1e-12,
    )
    assert_rejection(
        UnsupportedOperationError,
        pricing.implied_volatility,
        settled,
        instrument,
        context,
        actual.price,
        lower_bound=0.0,
        upper_bound=4.0,
    )


def test_curves_support_arithmetic_and_keep_history_validation_order():
    curve = RateTermStructure(
        reference_time=START,
        times=[START + timedelta(days=146), END],
        factors=[math.exp(-0.02 * 0.4), math.exp(-0.02 * 0.4 - 0.08 * 0.6)],
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=curve, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=START,
    )
    engine = pricing.MonteCarloAsianEngine(
        path_count=65536, step_count=5, sampling="sobol", antithetic=True, seed=42
    )
    # Independent expected trapezoids under the two piecewise forward rates.
    growth = [
        math.exp(0.01 * min(t, 0.4) + 0.07 * max(t - 0.4, 0))
        for t in (0, 0.2, 0.4, 0.6, 0.8, 1)
    ]
    expected = math.exp(-0.056) * (
        100 * (0.5 * growth[0] + sum(growth[1:-1]) + 0.5 * growth[-1]) / 5 - 1
    )
    assert engine.price(
        option(ArithmeticAveragePriceOption, strike=1),
        context,
    ) == pytest.approx(expected, rel=0, abs=0.002)
    for averaging in (START, END):
        assert_rejection(
            UnsupportedOperationError,
            engine.price,
            option(GeometricAveragePriceOption, averaging=averaging),
            context,
        )
    assert_rejection(
        InvalidInputError,
        engine.price,
        option(GeometricAveragePriceOption, realized=100),
        context,
    )
    assert_rejection(
        UnsupportedOperationError,
        engine.price_with_greeks,
        option(GeometricAveragePriceOption),
        context,
        [],
    )
    settled = context.replace(valuation_time=END)
    assert (
        engine.price(
            option(GeometricAveragePriceOption, realized=110),
            settled,
        )
        == 10
    )


def test_analysis_protocols_solver_failures_and_owned_results(subtests):
    engine = pricing.MonteCarloAsianEngine(path_count=1024, seed=42)
    for kind in KINDS:
        instrument, context = option(kind), market()
        quote = engine.price(instrument, context)
        for settings, exception in (
            ({}, TypeError),
            ({"greeks": "Delta"}, ValueError),
            ({"greeks": True}, TypeError),
            ({"greeks": "delta", "spot_shift": True}, TypeError),
            ({"greeks": "delta", "time_shift": 1.5}, TypeError),
            ({"greeks": "delta", "time_shift": 2**40}, TypeError),
        ):
            with pytest.raises(exception):
                numerical_greeks(engine, instrument, context, **settings)
        for observed, settings, expected_error in (
            (-1, {}, RootNotBracketedError),
            (math.nan, {}, InvalidInputError),
            (quote, {"max_iterations": 0}, InvalidInputError),
            (
                quote,
                {
                    "lower_bound": 0.11,
                    "upper_bound": 0.23,
                    "max_iterations": 1,
                    "price_tolerance": 1e-15,
                    "parameter_tolerance": 1e-15,
                },
                NonConvergenceError,
            ),
        ):
            assert_rejection(
                expected_error,
                pricing.implied_volatility,
                engine,
                instrument,
                context,
                observed,
                **{"lower_bound": 0.0, "upper_bound": 4.0, **settings},
            )
        with pytest.raises(TypeError):
            pricing.implied_volatility(
                engine,
                instrument,
                context,
                quote,
                max_iterations=True,
                lower_bound=0.0,
                upper_bound=4.0,
            )
        assert engine.price(instrument, context) == quote
        assert engine.seed == 42
        unseeded = pricing.MonteCarloAsianEngine(path_count=4096, step_count=16)
        assert_rejection(
            InvalidInputError,
            pricing.implied_volatility,
            unseeded,
            instrument,
            context,
            quote,
            lower_bound=0.0,
            upper_bound=4.0,
        )
        solved = pricing.implied_volatility(
            unseeded.with_resolved_seed(),
            instrument,
            context,
            quote,
            lower_bound=0.1,
            upper_bound=0.4,
        )
        assert 0.1 < solved < 0.4
        assert unseeded.seed is None
        result = engine.price_with_greeks(instrument, context, "delta")
        assert result.price == quote
        del instrument, context
        assert result.delta > 0
        with pytest.raises(AttributeError):
            result.delta = 0
    from kiyosi.instruments import EuropeanOption

    wrong = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    with pytest.raises(UnsupportedOperationError):
        engine.price(wrong, market())
    with pytest.raises(UnsupportedOperationError):
        pricing.implied_volatility(
            engine,
            wrong,
            market(),
            5,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    for settings, exception in (
        ({"sampling": "Sobol"}, ValueError),
        ({"sampling": None}, TypeError),
    ):
        with subtests.test(settings=settings), pytest.raises(exception):
            pricing.MonteCarloAsianEngine(**settings)


def test_extreme_volatility_uses_arithmetic_interval_validation():
    instrument = ArithmeticAveragePriceOption(
        option_type="put",
        strike=100,
        effective_time=START,
        averaging_start_time=START,
        expiry_time=datetime(2035, 1, 1),
    )
    engine = pricing.MonteCarloAsianEngine(path_count=1, step_count=100, seed=42)
    assert engine.price(
        instrument,
        market(volatility=1e154, rate=0, dividend=0),
    ) == pytest.approx(99.5, rel=0, abs=1e-12)


def test_node_curve_history_retains_fused_accrual_residual():
    curve = RateTermStructure(reference_time=START, times=[END], factors=[1.0])
    context = market(
        spot=1e-100, valuation=START + timedelta(days=90), rate=curve, dividend=0
    )
    instrument = option(
        ArithmeticAveragePriceOption, strike=(90 / 365) * 1e308, realized=1e308
    )
    engine = pricing.MonteCarloAsianEngine(path_count=1, seed=42)
    expected = 2.0487666640489847e291
    assert engine.price(instrument, context) / expected == pytest.approx(
        1, rel=0, abs=1e-12
    )
