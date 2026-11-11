"""European CPU Monte Carlo contracts through the installed public API."""

import importlib
import math
import subprocess
import sys
import textwrap
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
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    MonteCarloVanillaEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_decimal_equal, assert_rejection
from support.terminal import assert_terminal_greeks
from test_binomial import END, GREEKS, START, market


def contract(direction="call", strike=100):
    return EuropeanOption(
        option_type=direction, strike=strike, effective_time=START, expiry_time=END
    )


def test_process_pool_configuration_preserves_seeded_prices():
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            textwrap.dedent("""
                from datetime import datetime
                from kiyosi import InvalidInputError
                from kiyosi.instruments import EuropeanOption
                from kiyosi.market import BlackScholesMertonParameters, PricingContext
                from kiyosi.pricing import MonteCarloVanillaEngine, configure_monte_carlo_pool

                for invalid, error in ((0, InvalidInputError), (-1, InvalidInputError),
                                       (True, TypeError), (1.5, TypeError), (2**31, OverflowError)):
                    try:
                        configure_monte_carlo_pool(max_workers=invalid)
                    except error:
                        pass
                    else:
                        raise AssertionError((invalid, error))
                class Workers:
                    def __index__(self):
                        return 2
                configure_monte_carlo_pool(max_workers=Workers())
                start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
                option = EuropeanOption(option_type="call", strike=100,
                    effective_time=start, expiry_time=end)
                context = PricingContext(model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2),
                    spot_price=100, valuation_time=start)
                serial = MonteCarloVanillaEngine(path_count=1024, seed=42, max_active_workers=1)
                parallel = serial.replace(max_active_workers=4)
                expected = serial.price(option, context)
                assert parallel.price(option, context) == expected
                configure_monte_carlo_pool(max_workers=2)
                assert parallel.price(option, context) == expected
            """),
        ],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize("sampling", ("pseudo_random", "sobol"))
def test_default_path_count_supports_both_sampling_methods(sampling):
    engine = MonteCarloVanillaEngine(sampling=sampling)
    assert engine.sampling == sampling
    assert engine.path_count == 131072


def test_defaults_terminal_prices_and_statistics():
    default = MonteCarloVanillaEngine()
    assert (
        (
            default.path_count,
            default.step_count,
            default.seed,
            default.max_active_workers,
        )
    ) == ((131_072, 50, None, 0))
    for direction, reference in (
        ("call", 9.227005508154036),
        ("put", 6.330080627549918),
    ):
        engine = MonteCarloVanillaEngine(seed=42, max_active_workers=4)
        option = contract(direction)
        estimate = engine.price_with_statistics(option, market())
        assert estimate.price == engine.price(option, market())
        assert estimate.price == pytest.approx(reference, rel=0, abs=0.2)
        assert estimate.standard_error > 0
        lower, upper = estimate.confidence_interval
        assert_decimal_equal(
            upper - lower, 2 * 1.959963984540054 * estimate.standard_error
        )
        for steps in (2, 7, 10_000):
            other = MonteCarloVanillaEngine(seed=42, step_count=steps)
            assert other.price(option, market()) == estimate.price
        one = MonteCarloVanillaEngine(path_count=1, seed=42)
        estimate = one.price_with_statistics(option, market())
        assert estimate.standard_error is None
        assert estimate.confidence_interval is None


@pytest.mark.parametrize("sampling", ("pseudo_random", "sobol"))
@pytest.mark.parametrize("antithetic", (False, True))
@pytest.mark.parametrize("amount", (100, 1e18))
@pytest.mark.parametrize("sigma", (1e-17, 1e-200))
def test_terminal_prices_preserve_tiny_positive_volatility(
    subtests, sampling, antithetic, amount, sigma
):
    engine = MonteCarloVanillaEngine(
        path_count=65_536,
        seed=42,
        max_active_workers=1,
        sampling=sampling,
        antithetic=antithetic,
    )
    context = market(spot=amount, rate=0, dividend=0, sigma=sigma)
    for direction in ("call", "put"):
        with subtests.test(
            sampling=sampling,
            antithetic=antithetic,
            amount=amount,
            sigma=sigma,
            direction=direction,
        ):
            option = contract(direction, strike=amount)
            expected = amount * sigma / math.sqrt(math.tau)
            estimate = engine.price_with_statistics(option, context)
            assert estimate.price == engine.price(option, context)
            assert estimate.price / expected == pytest.approx(1, rel=0, abs=0.02)
            if sampling == "pseudo_random":
                assert estimate.standard_error > 0
                assert estimate.standard_error < 0.02 * expected
                assert abs(estimate.price - expected) < 6 * estimate.standard_error
                lower, upper = estimate.confidence_interval
                assert lower < estimate.price
                assert upper > estimate.price
            else:
                assert estimate.standard_error is None
                assert estimate.confidence_interval is None
            if direction == "call":
                american = AmericanOption(
                    option_type=direction,
                    strike=amount,
                    effective_time=START,
                    expiry_time=END,
                )
                assert engine.price(american, context) == estimate.price


def test_strict_representations_and_domain_boundaries(subtests):
    for field in ("path_count", "step_count", "max_active_workers", "seed"):
        for value in (True, False, 1.5, 2.0, Fraction(2, 1), "2", object()):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                MonteCarloVanillaEngine(**{field: value})
        for value in (-(2**64), 2**64):
            with pytest.raises(OverflowError):
                MonteCarloVanillaEngine(**{field: value})
    for field, values in (
        ("path_count", (0, -1, 10_000_001)),
        ("step_count", (0, 10_001)),
        ("max_active_workers", (-1,)),
    ):
        for value in values:
            assert_rejection(
                InvalidInputError,
                MonteCarloVanillaEngine,
                **{field: value},
            )
    for seed in (-1, 2**64):
        with pytest.raises(OverflowError):
            MonteCarloVanillaEngine(seed=seed)
    for paths in (1, 10_000_000):
        for steps in (2, 10_000):
            engine = MonteCarloVanillaEngine(
                path_count=paths,
                step_count=steps,
                seed=2**64 - 1,
                max_active_workers=2**31 - 1,
            )
            assert (
                (
                    engine.path_count,
                    engine.step_count,
                    engine.seed,
                    engine.max_active_workers,
                )
            ) == ((paths, steps, 2**64 - 1, 2**31 - 1))
    with pytest.raises(TypeError):
        MonteCarloVanillaEngine(unknown=1)
    with pytest.raises(TypeError):
        MonteCarloVanillaEngine(100)


def test_exact_path_counts_and_thread_block_boundaries():
    option, context = contract(), market()
    for paths in (1, 2, 255, 256, 257, 511, 512, 513, 769, 1025, 3073, 4097):
        for seed in (0, 42, 2**64 - 1):
            engine = MonteCarloVanillaEngine(
                path_count=paths, seed=seed, max_active_workers=1
            )
            expected = engine.price_with_statistics(option, context)
            expected = (
                expected.price,
                expected.standard_error,
                expected.confidence_interval,
            )
            for threads in (0, 2, 4, 2**31 - 1):
                other = MonteCarloVanillaEngine(
                    path_count=paths, seed=seed, max_active_workers=threads
                )
                actual = other.price_with_statistics(option, context)
                assert (
                    (
                        actual.price,
                        actual.standard_error,
                        actual.confidence_interval,
                    )
                ) == expected
    option = contract(strike=1)
    first = MonteCarloVanillaEngine(path_count=1, seed=42).price(option, context)
    pair = MonteCarloVanillaEngine(path_count=2, seed=42).price_with_statistics(
        option, context
    )
    # For two observations the standard error equals half their absolute difference.
    assert pair.standard_error == pytest.approx(
        abs(first - pair.price), rel=0, abs=1e-12
    )


def test_shared_sample_greeks_match_independent_scalar_stencils():

    differences = importlib.import_module("price_differences").price_differences
    shifts = {"spot_shift": 1, "volatility_shift": 0.01, "rate_shift": 0.001}
    for direction in ("call", "put"):
        option = contract(direction)
        for paths in (1, 257, 1025):
            for seed in (42, 2**64 - 1):
                engine = MonteCarloVanillaEngine(
                    path_count=paths, seed=seed, max_active_workers=4
                )
                context = market(spot=100.25, valuation=datetime(2025, 1, 1, 12))
                expected = differences(engine, option, context, **shifts)
                actual = numerical_greeks(
                    engine, option, context, greeks="all", **shifts
                )
                assert actual.price == expected.price
                for name in GREEKS:
                    assert getattr(actual, name) == pytest.approx(
                        getattr(expected, name), rel=0, abs=1e-9
                    )
                    selected = numerical_greeks(engine, option, context, name, **shifts)
                    assert selected.require(name) == getattr(actual, name)
    engine = MonteCarloVanillaEngine(path_count=1025)
    result = engine.price_with_greeks(
        contract(strike=1e-100), market(), ["delta", "gamma"]
    )
    assert result.delta == pytest.approx(result.price / 100, rel=0, abs=1e-9)
    assert result.gamma == pytest.approx(0, rel=0, abs=1e-7)
    assert engine.seed is None


def test_expiry_validation_failures_and_ownership():
    engine = MonteCarloVanillaEngine(path_count=257, seed=42, max_active_workers=4)
    option, context = contract(), market()
    quote = engine.price(option, context)
    expiry = market(spot=110, valuation=END)
    estimate = engine.price_with_statistics(option, expiry)
    assert (
        (estimate.price, estimate.standard_error, estimate.confidence_interval)
    ) == ((10, 0, (10, 10)))
    result = engine.price_with_greeks(option, expiry, greeks="all")
    assert result.price == 10
    assert_terminal_greeks(result, 1)
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        option,
        expiry,
        10,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    for valuation in (
        END + timedelta(microseconds=1),
        datetime(2026, 1, 1, 0, 0, 0, 1),
    ):
        assert_rejection(
            InvalidInputError,
            engine.price,
            option,
            market(valuation=valuation),
        )
    before = datetime(2025, 12, 31, 23, 59, 59, 999999)
    assert engine.price(option, market(valuation=before)) >= 0
    nodes = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.95, (0.95) * 0.5],
    )
    node_context = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=nodes),
        valuation_time=END,
    )
    assert_rejection(UnsupportedOperationError, engine.price, option, node_context)
    assert_rejection(
        UnsupportedOperationError,
        engine.price_with_greeks,
        option,
        node_context,
        [],
    )
    for invalid_quote in (0, math.nan):
        assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            option,
            node_context,
            invalid_quote,
            max_iterations=0,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        option,
        context,
        "delta",
        spot_shift=0,
    )
    assert_rejection(
        ComputationFailureError,
        partial(numerical_greeks, engine),
        option,
        context,
        "rho",
        rate_shift=1000,
    )
    assert_rejection(ComputationFailureError, engine.price, option, market(sigma=1e308))
    missing = numerical_greeks(
        engine,
        option,
        context,
        ["delta", "vega"],
        spot_shift=100,
        volatility_shift=0.2,
    )
    assert missing.delta is None
    assert missing.vega is None
    assert missing.price == quote
    with pytest.raises(TypeError):
        numerical_greeks(engine, option, context, "delta", spot_shift=True)
    assert engine.price(option, context) == quote
    for obj, field in (
        (engine, "path_count"),
        (estimate, "price"),
        (result, "delta"),
    ):
        with pytest.raises(AttributeError):
            setattr(obj, field, 0)
    del engine, option, context, expiry
    assert estimate.confidence_interval == ((10, 10))
    assert result.price == 10


def test_implied_volatility_seed_sharing_failures_and_thread_invariance():
    option, context = contract(), market()
    for seed in (42, 2**64 - 1):
        engine = MonteCarloVanillaEngine(
            path_count=4097, seed=seed, max_active_workers=1
        )
        quote = engine.price(option, context)
        bounds = {"lower_bound": 0.1, "upper_bound": 0.4}
        expected = implied_volatility(
            engine,
            option,
            context,
            quote,
            **{"lower_bound": 0.0, "upper_bound": 4.0, **bounds},
        )
        assert expected == pytest.approx(0.2, rel=0, abs=1e-8)
        for threads in (0, 2, 4):
            other = MonteCarloVanillaEngine(
                path_count=4097, seed=seed, max_active_workers=threads
            )
            assert (
                implied_volatility(
                    other,
                    option,
                    context,
                    quote,
                    **{"lower_bound": 0.0, "upper_bound": 4.0, **bounds},
                )
                == expected
            )
        assert_rejection(
            RootNotBracketedError,
            implied_volatility,
            engine,
            option,
            context,
            -1,
            **{"lower_bound": 0.0, "upper_bound": 4.0, **bounds},
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
        with pytest.raises(TypeError):
            implied_volatility(
                engine,
                option,
                context,
                quote,
                max_iterations=True,
                lower_bound=0.0,
                upper_bound=4.0,
            )
        assert engine.price(option, context) == quote
        assert engine.seed == seed
    unseeded = MonteCarloVanillaEngine(path_count=4097, max_active_workers=4)
    with pytest.raises(InvalidInputError, match="with_resolved_seed"):
        implied_volatility(
            unseeded,
            option,
            context,
            9.227005508154036,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    replay = unseeded.with_resolved_seed()
    sigma = implied_volatility(
        replay,
        option,
        context,
        9.227005508154036,
        lower_bound=0.1,
        upper_bound=0.4,
    )
    assert 0.1 < sigma < 0.4
    repriced = context.replace(
        model_parameters=context.model_parameters.replace(volatility=sigma)
    )
    assert replay.price(option, repriced) == pytest.approx(
        9.227005508154036, rel=0, abs=1e-8
    )
    assert unseeded.seed is None
    assert context.model_parameters.volatility == 0.2


def test_existing_quantlib_monte_carlo_profile(subtests):
    oracle = importlib.import_module("quantlib_reference.oracle")
    oracle.require_version()
    profile = next(
        p
        for p in oracle.load_fixture("numerical_engines.json")["profiles"]
        if p["engine"] == "MonteCarloVanillaEngine"
    )
    for scenario in oracle.load_fixture("scenarios.json")["scenarios"]:
        inputs = scenario["inputs"]
        if oracle.exclusions(inputs):
            continue
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
            reference, uncertainty = oracle.reference(inputs)
        engine = MonteCarloVanillaEngine(**profile["settings"], max_active_workers=4)
        # Wide bumps are biased at one-day expiries.
        wrapped = inputs["spot"] == 100 and inputs["expiry_time"] == "2026-01-06"
        names = sorted(reference.keys() - {"price"}) if wrapped else []
        result = (
            numerical_greeks(engine, option, context, names, **profile["shifts"])
            if wrapped
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
                    + uncertainty[f"uncertainty_{name}"],
                )


@pytest.mark.parametrize("antithetic", (False, True))
def test_statistical_coverage_and_inverse_square_root_error(antithetic):
    observations, deviation = (
        (4096, 7.286842354) if antithetic else (8192, 13.831466750894657)
    )
    option, context = contract(), market()
    covered, standardized = 0, []
    for seed in range(32):
        engine = MonteCarloVanillaEngine(
            path_count=8192,
            seed=seed,
            max_active_workers=1,
            antithetic=antithetic,
        )
        estimate = engine.price_with_statistics(option, context)
        assert estimate.standard_error * math.sqrt(observations) == pytest.approx(
            deviation, rel=0, abs=1.5
        )
        lower, upper = estimate.confidence_interval
        covered += lower <= 9.227005508154036 <= upper
        standardized.append(
            (estimate.price - 9.227005508154036) / estimate.standard_error
        )
    assert covered >= 26
    assert abs(sum(standardized) / len(standardized)) < 0.5
    small = MonteCarloVanillaEngine(
        path_count=8192, seed=42, antithetic=antithetic
    ).price_with_statistics(option, context)
    large = MonteCarloVanillaEngine(
        path_count=32768, seed=42, antithetic=antithetic
    ).price_with_statistics(option, context)
    assert small.standard_error / large.standard_error == pytest.approx(
        2, rel=0, abs=0.15
    )


def test_concurrent_shared_and_distinct_engines():
    option, context = contract("put"), market()
    engine = MonteCarloVanillaEngine(path_count=4097, seed=42, max_active_workers=4)
    expected = engine.price_with_greeks(option, context, greeks="all")

    def compute(index):
        selected = (
            engine
            if index % 2
            else MonteCarloVanillaEngine(path_count=4097, seed=42, max_active_workers=1)
        )
        result = selected.price_with_greeks(option, context, greeks="all")
        return result.price, tuple(result.require(name) for name in GREEKS)

    with ThreadPoolExecutor(max_workers=4) as executor:
        assert (
            list(executor.map(compute, range(8)))
            == [(expected.price, tuple(expected.require(name) for name in GREEKS))] * 8
        )


def test_sampling_choices_and_exact_representations(subtests):
    from typing import get_args

    from kiyosi.pricing import MonteCarloSampling

    assert get_args(MonteCarloSampling) == (("pseudo_random", "sobol"))
    default = MonteCarloVanillaEngine()
    assert default.sampling == "pseudo_random"
    assert default.antithetic is False
    for antithetic in (False, True):
        engine = MonteCarloVanillaEngine(
            path_count=1024, sampling="sobol", antithetic=antithetic
        )
        assert engine.sampling == "sobol"
        assert engine.antithetic is antithetic
        for field in ("sampling", "antithetic"):
            with pytest.raises(AttributeError):
                setattr(engine, field, getattr(engine, field))
    for value in (True, 1, 1.0, b"sobol", None, []):
        with subtests.test(value=value), pytest.raises(TypeError):
            MonteCarloVanillaEngine(path_count=1024, sampling=value)
    for value in ("Sobol", "", "pseudo-random"):
        with pytest.raises(ValueError):
            MonteCarloVanillaEngine(path_count=1024, sampling=value)
    for value in (0, 1, 1.0, "true", None, [], object()):
        with subtests.test(value=value), pytest.raises(TypeError):
            MonteCarloVanillaEngine(antithetic=value)
    # Representation conversion precedes core validation.
    with pytest.raises(TypeError):
        MonteCarloVanillaEngine(path_count=0, antithetic=1)


@pytest.mark.parametrize("antithetic", (False, True))
def test_sobol_path_boundaries_and_single_point_pair(subtests, antithetic):
    for paths in (1, 2, 1024, 8_388_608):
        assert (
            MonteCarloVanillaEngine(
                path_count=paths, sampling="sobol", antithetic=antithetic
            ).path_count
            == paths
        )
    for paths in (0, 3, 100_000, 8_388_609, 16_777_216):
        with (
            subtests.test(paths=paths),
            pytest.raises(InvalidInputError) as error,
        ):
            MonteCarloVanillaEngine(
                path_count=paths, sampling="sobol", antithetic=antithetic
            )
        assert type(error.value) is InvalidInputError
    for paths in (1, 2):
        engine = MonteCarloVanillaEngine(
            path_count=paths,
            seed=42,
            sampling="sobol",
            antithetic=antithetic,
        )
        # Independent SciPy ndtri reference.
        first = -0.10364940758940244
        positive = 100 * math.exp(0.01 + 0.2 * first) - 90
        negative = 100 * math.exp(0.01 - 0.2 * first) - 90
        expected = math.exp(-0.05) * (
            (positive + negative) / 2 if antithetic and paths == 2 else positive
        )
        if antithetic or paths == 1:
            assert engine.price(contract(strike=90), market()) == pytest.approx(
                expected, rel=0, abs=1e-11
            )


def test_exact_antithetic_counts_and_pair_uncertainty():
    option, context = contract(strike=1e-100), market()

    def engine(paths, antithetic):
        return MonteCarloVanillaEngine(
            path_count=paths, seed=42, antithetic=antithetic, max_active_workers=1
        )

    assert engine(1, True).price(option, context) == engine(1, False).price(
        option, context
    )
    for samples in (1, 2, 3, 128, 129, 256, 257, 513):
        positive = samples * engine(samples, False).price(option, context) - (
            0
            if samples == 1
            else (samples - 1) * engine(samples - 1, False).price(option, context)
        )
        opposite = 10_000 * math.exp(-0.08) / positive
        paired = engine(2 * samples, True).price_with_statistics(option, context)
        odd = engine(2 * samples - 1, True).price_with_statistics(option, context)
        assert 2 * samples * paired.price - (
            2 * samples - 1
        ) * odd.price == pytest.approx(opposite, rel=0, abs=1e-7)
        assert odd.standard_error is None
        assert odd.confidence_interval is None
        assert (paired.standard_error is not None) == (samples > 1)
    first = engine(2, True).price(option, context)
    two = engine(4, True).price_with_statistics(option, context)
    assert two.standard_error == pytest.approx(abs(first - two.price), rel=0, abs=1e-12)


@pytest.mark.parametrize("sampling", ("pseudo_random", "sobol"))
@pytest.mark.parametrize("antithetic", (False, True))
def test_all_sampling_combinations_replay_analytics_and_preserve_failure_state(
    sampling, antithetic
):

    differences = importlib.import_module("price_differences").price_differences
    option, context = contract(), market()
    shifts = {"spot_shift": 1, "volatility_shift": 0.01, "rate_shift": 0.001}
    settings = {
        "path_count": 1024,
        "seed": 42,
        "sampling": sampling,
        "antithetic": antithetic,
    }
    engine = MonteCarloVanillaEngine(**settings, max_active_workers=1)
    reference = differences(engine, option, context, **shifts)
    expected = numerical_greeks(engine, option, context, greeks="all", **shifts)
    assert expected.price == reference.price
    for name in GREEKS:
        assert getattr(expected, name) == pytest.approx(
            getattr(reference, name), rel=0, abs=1e-9
        )
    sigma = implied_volatility(
        engine,
        option,
        context,
        expected.price,
        lower_bound=0.1,
        upper_bound=0.4,
    )
    assert sigma == pytest.approx(0.2, rel=0, abs=1e-8)
    for threads in (0, 2, 4):
        other = MonteCarloVanillaEngine(**settings, max_active_workers=threads)
        actual = numerical_greeks(other, option, context, greeks="all", **shifts)
        assert actual.price == expected.price
        assert tuple(getattr(actual, name) for name in GREEKS) == tuple(
            getattr(expected, name) for name in GREEKS
        )
        assert (
            implied_volatility(
                other,
                option,
                context,
                expected.price,
                lower_bound=0.1,
                upper_bound=0.4,
            )
            == sigma
        )
    with pytest.raises(ComputationFailureError) as error:
        numerical_greeks(
            engine,
            option,
            context,
            "rho",
            rate_shift=1000,
        )
    assert type(error.value) is ComputationFailureError
    assert engine.price(option, context) == expected.price
    for steps in (2, 10_000):
        assert (
            MonteCarloVanillaEngine(**settings, step_count=steps).price(option, context)
            == expected.price
        )
    for seed in (0, 2**64 - 1, 2**32 + 42):
        assert (
            MonteCarloVanillaEngine(**settings | {"seed": seed}).price(option, context)
            != expected.price
        )
    unseeded = MonteCarloVanillaEngine(**(settings | {"seed": None}))
    analysis = unseeded.price_with_greeks(
        contract(strike=1e-100), context, ["delta", "gamma"]
    )
    assert analysis.delta == pytest.approx(analysis.price / 100, rel=0, abs=1e-9)
    assert analysis.gamma == pytest.approx(0, rel=0, abs=1e-7)
    sigma = implied_volatility(
        unseeded.with_resolved_seed(),
        option,
        context,
        9.227005508154036,
        lower_bound=0.1,
        upper_bound=0.4,
    )
    assert 0.1 < sigma < 0.4
    assert unseeded.seed is None
    with ThreadPoolExecutor(max_workers=3) as pool:
        assert (
            list(
                pool.map(
                    lambda _, engine=engine: engine.price(option, context),
                    range(6),
                )
            )
            == [expected.price] * 6
        )


@pytest.mark.parametrize("sampling", ("pseudo_random", "sobol"))
@pytest.mark.parametrize("antithetic", (False, True))
def test_sampling_statistics_at_expiry_and_block_boundaries(sampling, antithetic):
    for paths in (1, 2, 256, 512, 1024):
        for seed in (0, 42, 2**64 - 1):
            engine = MonteCarloVanillaEngine(
                path_count=paths,
                seed=seed,
                sampling=sampling,
                antithetic=antithetic,
                max_active_workers=1,
            )
            for direction in ("call", "put"):
                option = contract(direction)
                expected = engine.price_with_statistics(option, market())
                assert engine.price(option, market()) == expected.price
                if sampling == "sobol":
                    assert expected.standard_error is None
                    assert expected.confidence_interval is None
                for threads in (0, 2, 4, 2**31 - 1):
                    actual = MonteCarloVanillaEngine(
                        path_count=paths,
                        seed=seed,
                        sampling=sampling,
                        antithetic=antithetic,
                        max_active_workers=threads,
                    ).price_with_statistics(option, market())
                    assert (
                        (
                            actual.price,
                            actual.standard_error,
                            actual.confidence_interval,
                        )
                    ) == (
                        (
                            expected.price,
                            expected.standard_error,
                            expected.confidence_interval,
                        )
                    )
    engine = MonteCarloVanillaEngine(
        path_count=1024, seed=42, sampling=sampling, antithetic=antithetic
    )
    option, expiry = contract(strike=90), market(valuation=END)
    known = engine.price_with_statistics(option, expiry)
    assert known.price == 10
    assert known.standard_error == (None if sampling == "sobol" else 0)
    assert known.confidence_interval == (None if sampling == "sobol" else (10, 10))
    analysis = engine.price_with_greeks(option, expiry, greeks="all")
    assert_terminal_greeks(analysis, 1)
    with pytest.raises(UnsupportedOperationError) as error:
        implied_volatility(engine, option, expiry, 10, lower_bound=0.0, upper_bound=4.0)
    assert type(error.value) is UnsupportedOperationError


@pytest.mark.parametrize("antithetic", (False, True))
def test_sobol_greek_accuracy_and_multiseed_rmse(antithetic):
    tolerances = {
        "delta": 2e-4,
        "gamma": 1e-5,
        "speed": 5e-6,
        "vega": 0.005,
        "vanna": 0.001,
        "zomma": 0.0005,
        "rho": 0.002,
        "theta": 0.0073,
        "charm": 0.0001825,
        "color": 0.000073,
    }
    for direction in ("call", "put"):
        option, context = contract(direction), market()
        reference = AnalyticEuropeanEngine().price_with_greeks(
            option, context, greeks="all"
        )
        engine = MonteCarloVanillaEngine(
            path_count=65_536, seed=42, sampling="sobol", antithetic=antithetic
        )
        actual = numerical_greeks(
            engine,
            option,
            context,
            greeks="all",
            spot_shift=1,
            volatility_shift=0.001,
            rate_shift=0.001,
        )
        assert actual.price == pytest.approx(reference.price, rel=0, abs=0.002)
        for name, tolerance in tolerances.items():
            assert getattr(actual, name) == pytest.approx(
                getattr(reference, name), rel=0, abs=tolerance
            ), f"{direction} {antithetic} {name}"
    for sampling in ("pseudo_random", "sobol"):
        prices = [
            MonteCarloVanillaEngine(
                path_count=131_072,
                seed=seed,
                sampling=sampling,
                antithetic=antithetic,
                max_active_workers=1,
            ).price(contract(), market())
            for seed in range(16)
        ]
        errors = [price - 9.227005508154036 for price in prices]
        rmse = math.sqrt(sum(error**2 for error in errors) / len(errors))
        bias = sum(errors) / len(errors)
        assert rmse <= 0.05
        assert abs(bias) <= 0.05


def test_companion_failure_and_domain_validation_preserve_sampling_state():
    from kiyosi.market import BlackScholesMertonParameters

    for sampling in ("pseudo_random", "sobol"):
        for antithetic in (False, True):
            engine = MonteCarloVanillaEngine(
                path_count=1024, seed=42, sampling=sampling, antithetic=antithetic
            )
            option, context = contract(), market()
            baseline = engine.price(option, context)
            for valuation in (
                END + timedelta(microseconds=1),
                END + timedelta(days=1),
            ):
                with pytest.raises(InvalidInputError) as error:
                    engine.price(option, market(valuation=valuation))
                assert type(error.value) is InvalidInputError
            curve = RateTermStructure(
                reference_time=START,
                times=[END, END + (END - START)],
                factors=[0.95, (0.95) * 0.5],
            )
            node = context.replace(
                model_parameters=context.model_parameters.replace(risk_free_rate=curve),
                valuation_time=END,
            )
            with pytest.raises(UnsupportedOperationError) as error:
                engine.price_with_greeks(option, node, [])
            assert type(error.value) is UnsupportedOperationError
            assert engine.price(option, context) == baseline
    # First Sobol path is finite; its opposite overflows, so the paired call fails.
    context = market().replace(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.5, dividend_yield=0, volatility=1
        ),
        spot_price=1.7e308,
    )
    one = MonteCarloVanillaEngine(
        path_count=1, seed=42, sampling="sobol", antithetic=True
    )
    assert math.isfinite(one.price(contract(), context))
    paired = MonteCarloVanillaEngine(
        path_count=2, seed=42, sampling="sobol", antithetic=True
    )
    baseline = paired.price(contract(), market())
    with pytest.raises(ComputationFailureError) as error:
        paired.price(contract(), context)
    assert type(error.value) is ComputationFailureError
    assert paired.price(contract(), market()) == baseline


def test_resolved_seed_preserves_configuration_and_pickle_replay(subtests):
    import inspect
    import pickle

    from kiyosi import pricing

    for name in pricing.__all__:
        if not (name.startswith("MonteCarlo") and name.endswith("Engine")):
            continue
        factory = getattr(pricing, name)
        controls = {
            "path_count": 64,
            "max_active_workers": 1,
            "sampling": "sobol",
            "antithetic": True,
        }
        if "step_count" in inspect.signature(factory).parameters:
            controls["step_count"] = 100
        original = factory(**controls)
        with subtests.test(engine=name):
            seeded = original.with_resolved_seed()
            assert original.seed is None
            assert isinstance(seeded.seed, int)
            assert seeded.seed >= 0
            assert seeded.seed < 2**64
            assert seeded == factory(seed=seeded.seed, **controls)
            assert seeded.with_resolved_seed() == seeded
            assert pickle.loads(pickle.dumps(seeded)) == seeded
