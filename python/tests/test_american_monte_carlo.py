"""American least-squares Monte Carlo through the installed public API."""

import math
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from functools import partial

import pytest
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import AmericanOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import RateTermStructure, weekdays_calendar
from kiyosi.pricing import (
    CoxRossRubinsteinVanillaEngine,
    MonteCarloVanillaEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks
from test_binomial import END, GREEKS, START, market


def contract(direction="put", strike=100):
    return AmericanOption(
        option_type=direction, strike=strike, effective_time=START, expiry_time=END
    )


@pytest.mark.parametrize("antithetic", (False, True))
def test_work_limit_rejects_large_grids_before_sampling(antithetic):
    context = market(rate=0.05, dividend=0)
    put = contract()
    engine = MonteCarloVanillaEngine(
        path_count=100_001, step_count=10_000, seed=42, antithetic=antithetic
    )
    assert_rejection(InvalidInputError, engine.price, put, context)
    assert_rejection(InvalidInputError, engine.price_with_greeks, put, context, "delta")
    assert_rejection(
        InvalidInputError,
        implied_volatility,
        engine,
        put,
        context,
        1,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert engine.price(put, context.replace(valuation_time=END, spot_price=90)) == 10
    european = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
    )
    expected = engine.price(european, context)
    assert expected > 0
    assert engine.price(contract("call"), context) == expected
    assert engine.seed == 42


def test_reverse_training_accepts_paths_beyond_the_full_matrix_limit():
    option = contract()
    context = market(rate=0.05, dividend=0.02, sigma=0.2)
    engine = MonteCarloVanillaEngine(
        path_count=1_000_000, step_count=150, seed=42, max_active_workers=4
    )
    reference = CoxRossRubinsteinVanillaEngine(step_count=2000).price(option, context)
    assert engine.price(option, context) == pytest.approx(reference, rel=0, abs=0.04)


def test_nonfinite_continuation_regressions_preserve_engine_reuse(subtests):
    context = market(rate=-0.1, dividend=0, sigma=0.2, spot=100)
    engine = MonteCarloVanillaEngine(path_count=128, step_count=4, seed=42)
    ordinary = contract("call")
    before = engine.price(ordinary, context)
    for strike in (1e-100, 1e-300):
        with subtests.test(strike=strike):
            extreme = contract("call", strike)
            assert_rejection(ComputationFailureError, engine.price, extreme, context)
            assert_rejection(
                ComputationFailureError,
                engine.price_with_greeks,
                extreme,
                context,
                ["delta"],
            )
    assert engine.price(ordinary, context) == before
    assert engine.seed == 42


def test_step_counts_are_intervals_with_two_required_for_american_exercise():
    from kiyosi.pricing import MonteCarloAsianEngine

    option = contract()
    context = market()
    engine = MonteCarloVanillaEngine(path_count=64, step_count=2, seed=42)
    value = engine.price(option, context)
    assert value > 0
    assert value < 100
    assert engine.step_count == 2
    assert MonteCarloAsianEngine(step_count=2).step_count == 2
    one = MonteCarloVanillaEngine(path_count=64, step_count=1, seed=42)
    assert_rejection(InvalidInputError, one.price, option, context)
    assert one.price(option, context.replace(valuation_time=END, spot_price=90)) == 10


def test_immediate_exercise_and_minimum_grid_validation_timing():
    option = contract()
    context = market(rate=0.1, dividend=0, sigma=0.1, spot=50)
    engine = MonteCarloVanillaEngine(path_count=4096, seed=42)
    assert engine.price(option, context) == pytest.approx(50, rel=0, abs=1e-10)
    engine = MonteCarloVanillaEngine(step_count=1, seed=42)
    assert_rejection(InvalidInputError, engine.price, option, context)
    expiry = context.replace(valuation_time=END)
    assert engine.price(option, expiry) == 50
    result = engine.price_with_greeks(option, expiry, greeks="all")
    assert result.price == 50
    assert_terminal_greeks(result, -1)
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        option,
        expiry,
        50,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    for valuation in (
        START - timedelta(days=1),
        datetime(2026, 1, 1, 0, 0, 0, 1),
    ):
        assert_rejection(
            InvalidInputError,
            engine.price,
            option,
            context.replace(valuation_time=valuation),
        )


def test_sparse_rank_deficient_regressions_and_currency_scale():
    for paths in (1, 3, 4, 127):
        for antithetic in (False, True):
            engine = MonteCarloVanillaEngine(
                path_count=paths, step_count=4, seed=42, antithetic=antithetic
            )
            context = market(rate=0, dividend=0.1, sigma=sys.float_info.min, spot=50)
            price = engine.price(contract(), context)
            assert price == pytest.approx(100 - 50 * math.exp(-0.1), rel=0, abs=1e-12)
            for scale in (0.01, 100):
                assert engine.price(
                    contract(strike=100 * scale),
                    context.replace(spot_price=50 * scale),
                ) / scale == pytest.approx(price, rel=0, abs=1e-11)
    # A deterministic optimal exercise date lies strictly inside this three-year grid.
    option = contract().replace(expiry_time=datetime(2028, 1, 1))
    expected = max(
        100 * math.exp(-0.05 * (3 * step / 50)) - 30 * math.exp(-0.2 * (3 * step / 50))
        for step in range(51)
    )
    for paths in (1, 2, 4, 2000):
        engine = MonteCarloVanillaEngine(path_count=paths, seed=42)
        assert engine.price(
            option, market(rate=0.05, dividend=0.2, sigma=1e-8, spot=30)
        ) == pytest.approx(expected, rel=0, abs=1e-6)


def test_terminal_call_shortcut_and_rate_stencil_crossing_zero():
    call = contract("call")
    european = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    for sampling in ("pseudo_random", "sobol"):
        for antithetic in (False, True):
            for rate, dividend in ((0, 0), (0.05, -0.02)):
                context = market(rate=rate, dividend=dividend)
                settings = {
                    "path_count": 1024,
                    "seed": 42,
                    "sampling": sampling,
                    "antithetic": antithetic,
                }
                price = MonteCarloVanillaEngine(**settings).price(european, context)
                for steps in (3, 50, 10000):
                    assert (
                        MonteCarloVanillaEngine(**settings, step_count=steps).price(
                            call, context
                        )
                        == price
                    )
    engine = MonteCarloVanillaEngine(path_count=2048, step_count=6, seed=42)
    requested = 0.0001
    for rate in (-requested / 2, 0, requested / 2):
        context = market(rate=rate, dividend=0)
        result = engine.price_with_greeks(call, context, "rho")
        shift = (rate + requested) - rate
        if rate - (rate - shift) != shift:
            assert result.rho is None
            continue
        up = engine.price(call, market(rate=rate + shift, dividend=math.ulp(0.0)))
        down = engine.price(call, market(rate=rate - shift, dividend=math.ulp(0.0)))
        assert result.rho == pytest.approx((up - down) / (2 * shift), rel=0, abs=1e-8)
        assert result.price == engine.price(call, context)


def test_weekend_rate_greeks_keep_calendar_accrual():
    start = datetime(2025, 1, 4)
    end = start + timedelta(days=2)
    option = contract("call").replace(effective_time=start, expiry_time=end)
    curve = RateTermStructure.bounded_flat(
        0.05, reference_time=start, max_time=end, calendar=weekdays_calendar()
    )
    context = market(rate=curve, dividend=curve, sigma=0.25, valuation=start)
    engine = MonteCarloVanillaEngine(path_count=2048, step_count=25, seed=42)
    base = engine.price(option, context)
    shift = 0.001
    for field, greek in (("risk_free_rate", "rho"), ("dividend_yield", "dividend_rho")):
        for bump in (-shift, shift):
            model = context.model_parameters.replace(**{field: curve.shifted(bump)})
            assert engine.price(option, context.replace(model_parameters=model)) == base
        result = engine.price_with_greeks(option, context, greek, rate_shift=shift)
        assert getattr(result, greek) == 0


def test_shared_samples_defaults_solving_and_concurrent_calls():
    option, context = contract(), market()
    shifts = {
        "spot_shift": 8,
        "volatility_shift": 0.02,
        "rate_shift": 0.01,
        "time_shift": timedelta(days=2),
    }
    for sampling in ("pseudo_random", "sobol"):
        for antithetic in (False, True):
            engine = MonteCarloVanillaEngine(
                path_count=1024,
                step_count=6,
                seed=42,
                sampling=sampling,
                antithetic=antithetic,
            )
            result = numerical_greeks(engine, option, context, greeks="all", **shifts)
            assert result.price == engine.price(option, context)
            for name in GREEKS:
                selected = numerical_greeks(engine, option, context, name, **shifts)
                assert getattr(selected, name) == getattr(result, name)
            default = engine.price_with_greeks(option, context, ["delta", "gamma"])
            explicit = numerical_greeks(
                engine, option, context, ["delta", "gamma"], spot_shift=8
            )
            none = numerical_greeks(
                engine, option, context, ["delta", "gamma"], spot_shift=None
            )
            assert ((default.delta, default.gamma)) == (
                (explicit.delta, explicit.gamma)
            )
            assert ((none.delta, none.gamma)) == ((explicit.delta, explicit.gamma))
            solved = implied_volatility(
                engine,
                option,
                context,
                result.price,
                lower_bound=0.15,
                upper_bound=0.25,
            )
            assert solved == pytest.approx(0.2, rel=0, abs=1e-7)
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
            unavailable = numerical_greeks(
                engine, option, context, "gamma", spot_shift=200
            )
            assert unavailable.gamma is None
            assert unavailable.price == result.price
            with ThreadPoolExecutor(max_workers=2) as pool:
                values = list(
                    pool.map(
                        lambda _, engine=engine: numerical_greeks(
                            engine, option, context, greeks="all", **shifts
                        ),
                        range(2),
                    )
                )
            for value in values:
                assert value.price == result.price
                assert ([getattr(value, name) for name in GREEKS]) == (
                    [getattr(result, name) for name in GREEKS]
                )
    unseeded = MonteCarloVanillaEngine(path_count=1024, step_count=6)
    result = numerical_greeks(unseeded, option, context, greeks="all", **shifts)
    assert all(math.isfinite(getattr(result, name)) for name in GREEKS)
    assert unseeded.seed is None


def test_validation_representations_and_failure_state():
    engine = MonteCarloVanillaEngine(path_count=127, step_count=6, seed=42)
    option, context = contract(), market()
    price = engine.price(option, context)
    for name in ("spot_shift", "volatility_shift", "rate_shift"):
        for value in (True, "1", object()):
            with pytest.raises(TypeError):
                numerical_greeks(engine, option, context, "delta", **{name: value})
        for value in (0, -1, math.inf, math.nan):
            assert_rejection(
                InvalidInputError,
                partial(numerical_greeks, engine),
                option,
                context,
                "delta",
                **{name: value},
            )
    for value in (True, 1.5, 2.0):
        with pytest.raises(TypeError):
            numerical_greeks(engine, option, context, "theta", time_shift=value)
    with pytest.raises(TypeError):
        numerical_greeks(engine, option, context, "theta", time_shift=2**31)
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.95, (0.95) * 0.5],
    )
    curved = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=curve)
    )
    assert_rejection(
        UnsupportedOperationError,
        engine.price_with_greeks,
        option,
        curved,
        [],
    )
    assert_rejection(ComputationFailureError, engine.price, option, market(sigma=1000))
    assert_rejection(
        ComputationFailureError,
        partial(numerical_greeks, engine),
        option,
        context,
        "rho",
        rate_shift=1000,
    )
    digital = CashOrNothingOption(
        option_type="call",
        strike=100,
        payout=1,
        effective_time=START,
        expiry_time=END,
    )
    with pytest.raises(UnsupportedOperationError):
        engine.price(digital, context)
    with pytest.raises(UnsupportedOperationError):
        engine.price_with_greeks(digital, context, "delta")
    with pytest.raises(UnsupportedOperationError):
        implied_volatility(
            engine, digital, context, 1, lower_bound=0.0, upper_bound=4.0
        )
    assert engine.price(option, context) == price
    assert context.model_parameters.volatility == 0.2
    with pytest.raises(AttributeError):
        engine.step_count = 2


@pytest.mark.parametrize("scale,seed", ((0.01, 1), (1, 7), (100, 42)))
def test_default_spot_greeks_converge_across_seeds_and_price_scales(scale, seed):
    reference = CoxRossRubinsteinVanillaEngine(step_count=3200).price_with_greeks(
        contract(), market(), ["delta", "gamma"]
    )
    option = contract(strike=100 * scale)
    context = market(spot=100 * scale)
    engine = MonteCarloVanillaEngine(seed=seed, max_active_workers=1)
    result = engine.price_with_greeks(option, context, ["delta", "gamma"])
    explicit = numerical_greeks(
        engine, option, context, ["delta", "gamma"], spot_shift=8 * scale
    )
    assert ((result.delta, result.gamma)) == ((explicit.delta, explicit.gamma))
    assert result.delta == pytest.approx(reference.delta, rel=0, abs=0.02)
    assert result.gamma * scale == pytest.approx(reference.gamma, rel=0, abs=0.004)


def test_reverse_training_spot_greeks_match_scalar_price_stencils():
    option = contract()
    context = market(rate=0.05, dividend=0.02, sigma=0.2)
    engine = MonteCarloVanillaEngine(
        path_count=680_000, step_count=50, seed=42, max_active_workers=4
    )
    result = engine.price_with_greeks(option, context, ["delta", "gamma"])
    center, up, down = (
        engine.price(option, context.replace(spot_price=spot))
        for spot in (100, 108, 92)
    )
    assert result.seed == 42
    assert result.price == center
    assert result.delta == (up - down) / 16
    assert result.gamma == ((up - center) - (center - down)) / 64


@pytest.mark.parametrize("sampling", ("pseudo_random", "sobol"))
def test_batched_volatility_scenarios_replay_scalar_exercise_paths(sampling):
    option = contract()
    context = market(rate=0.05, dividend=0.02, sigma=0.25)
    model = context.model_parameters
    shift = 0.03125
    engine = MonteCarloVanillaEngine(
        path_count=16_384,
        step_count=360,
        seed=42,
        max_active_workers=4,
        sampling=sampling,
    )
    result = engine.price_with_greeks(
        option, context, ["vega", "volga"], volatility_shift=shift
    )
    center, up, down = (
        engine.price(
            option, context.replace(model_parameters=model.replace(volatility=vol))
        )
        for vol in (0.25, 0.25 + shift, 0.25 - shift)
    )
    assert result.seed == 42
    assert result.price == center
    assert result.vega == (up - down) / (2 * shift)
    assert result.volga == ((up - center) - (center - down)) / shift**2


def test_price_sampling_error_and_exercise_grid_convergence():
    option, context = contract(), market()
    reference = CoxRossRubinsteinVanillaEngine(step_count=3200).price(option, context)
    rmse = []
    for paths in (1024, 16384):
        errors = [
            MonteCarloVanillaEngine(
                path_count=paths, seed=seed, max_active_workers=1
            ).price(option, context)
            - reference
            for seed in range(16)
        ]
        rmse.append(math.sqrt(sum(error**2 for error in errors) / len(errors)))
    assert rmse[1] < rmse[0]
    assert rmse[1] < 0.25
    prices = [
        MonteCarloVanillaEngine(
            path_count=100000, step_count=steps, seed=42, max_active_workers=1
        ).price(option, context)
        for steps in (25, 50, 100)
    ]
    for price in prices:
        assert price == pytest.approx(reference, rel=0, abs=0.3)
    assert max(prices) - min(prices) < 0.15


@pytest.mark.parametrize("sampling", ["pseudo_random", "sobol"])
def test_out_of_sample_american_policy_replays_and_converges(sampling):
    put = contract()
    snapshot = market(rate=0.05, dividend=0.02, sigma=0.2)
    reference = CoxRossRubinsteinVanillaEngine(step_count=2000).price(put, snapshot)
    engine = MonteCarloVanillaEngine(
        path_count=32768,
        step_count=100,
        max_active_workers=1,
        sampling=sampling,
        antithetic=True,
    )
    errors = []
    for seed in (42, 111, 1729, 1999, 2111):
        seeded = engine.replace(seed=seed)
        value = seeded.price_american_out_of_sample(
            put, snapshot, training_path_count=8192
        )
        errors.append((value - reference) ** 2)
        if seed == 42:
            assert value == seeded.replace(
                max_active_workers=4
            ).price_american_out_of_sample(put, snapshot, training_path_count=8192)
            with pytest.raises(InvalidInputError):
                seeded.price_american_out_of_sample(
                    put, snapshot, training_path_count=0
                )
    assert math.sqrt(sum(errors) / len(errors)) < 0.055
