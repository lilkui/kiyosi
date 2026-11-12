"""Sampling uncertainty follows the same contracts across Monte Carlo products."""

import math
import pickle
from datetime import datetime, time, timedelta

import pytest
from kiyosi import ComputationFailureError, InvalidInputError, UnsupportedOperationError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p

START, END = datetime(2025, 1, 2), datetime(2026, 1, 2)
LIFE = {"effective_time": START, "expiry_time": END}


def contracts():
    vanilla = {"option_type": "call", "strike": 100, **LIFE}
    return (
        (
            "cash",
            p.MonteCarloDigitalEngine,
            i.CashOrNothingOption(payout=10, **vanilla),
        ),
        ("asset", p.MonteCarloDigitalEngine, i.AssetOrNothingOption(**vanilla)),
        (
            "geometric",
            p.MonteCarloAsianEngine,
            i.GeometricAveragePriceOption(averaging_start_time=START, **vanilla),
        ),
        (
            "arithmetic",
            p.MonteCarloAsianEngine,
            i.ArithmeticAveragePriceOption(averaging_start_time=START, **vanilla),
        ),
        (
            "barrier",
            p.MonteCarloBarrierEngine,
            i.BarrierOption(barrier_type="up_and_out", barrier_level=120, **vanilla),
        ),
        (
            "double",
            p.MonteCarloDoubleBarrierEngine,
            i.DoubleBarrierOption(
                barrier_type="knock_out", lower_barrier=80, upper_barrier=120, **vanilla
            ),
        ),
    )


def context():
    return m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=START,
    )


def engine(factory, **changes):
    settings = {"path_count": 4096, "seed": 42, "max_active_workers": 1, **changes}
    if factory is p.MonteCarloAsianEngine:
        settings["step_count"] = 8
    return factory(**settings)


def seed_cases():
    return (
        *contracts(),
        (
            "vanilla",
            p.MonteCarloVanillaEngine,
            i.EuropeanOption(option_type="call", strike=100, **LIFE),
        ),
        (
            "snowball",
            p.MonteCarloAutocallableEngine,
            i.standard_snowball(
                coupon_rate=0.1,
                initial_spot=100,
                knock_in_level=80,
                knock_out_level=110,
                autocall_times=[END],
                knock_in_observation_clock_time=time(),
                **LIFE,
            ),
        ),
        (
            "accumulator",
            p.MonteCarloAccumulatorEngine,
            i.Accumulator(
                strike=100,
                knock_out_level=120,
                daily_quantity=1,
                acceleration_factor=2,
                observation_clock_time=time(),
                **LIFE,
            ),
        ),
    )


def test_scalar_seeds_replay_all_products_without_mutating_engines(subtests):
    snapshot = context()
    cases = (
        *seed_cases(),
        (
            "american",
            p.MonteCarloVanillaEngine,
            i.AmericanOption(option_type="put", strike=100, **LIFE),
        ),
    )
    deterministic = snapshot.replace(
        model_parameters=snapshot.model_parameters.replace(volatility=0)
    )
    for name, factory, option in cases:
        for sampling in ("pseudo_random", "sobol"):
            for configured in (None, 2**64 - 1):
                with subtests.test(product=name, sampling=sampling, seed=configured):
                    sampler = engine(
                        factory, path_count=32, seed=configured, sampling=sampling
                    )
                    value, seed = sampler.price_with_seed(option, snapshot)
                    assert seed is not None
                    if configured is not None:
                        assert seed == configured
                    assert sampler.seed == configured
                    replay = sampler.replace(seed=seed, max_active_workers=4)
                    assert replay.price(option, snapshot) == value
                    assert replay.price_with_seed(option, snapshot) == ((value, seed))
                    assert sampler.price_with_seed(option, deterministic) == (
                        (sampler.price(option, deterministic), None)
                    )
                    with pytest.raises(InvalidInputError):
                        sampler.price_with_seed(
                            option,
                            snapshot.replace(valuation_time=END + timedelta(days=1)),
                        )


def test_greek_seeds_replay_native_numerical_and_pickled_results(subtests):
    snapshot = context()
    cases = (
        *seed_cases(),
        (
            "american",
            p.MonteCarloVanillaEngine,
            i.AmericanOption(option_type="put", strike=100, **LIFE),
        ),
    )
    for name, factory, option in cases:
        sampler = engine(factory, path_count=32, seed=None)
        for numerical in (False, True):
            with subtests.test(product=name, numerical=numerical):
                calculate = (
                    (
                        lambda e, option=option: p.numerical_greeks(
                            e, option, snapshot, "delta"
                        )
                    )
                    if numerical
                    else (
                        lambda e, option=option: e.price_with_greeks(
                            option, snapshot, "delta"
                        )
                    )
                )
                result = calculate(sampler)
                assert result.seed is not None
                assert sampler.seed is None
                assert calculate(sampler.replace(seed=result.seed)) == result
                assert pickle.loads(pickle.dumps(result)) == result
                assert (
                    sampler.replace(seed=result.seed).price(option, snapshot)
                    == result.price
                )
        deterministic = snapshot.replace(
            model_parameters=snapshot.model_parameters.replace(volatility=0)
        )
        assert sampler.price_with_greeks(option, deterministic, "delta").seed is None


def test_reported_seeds_replay_unseeded_and_configured_estimates(subtests):
    cases = seed_cases()
    snapshot = context()
    deterministic = snapshot.replace(
        model_parameters=snapshot.model_parameters.replace(volatility=0)
    )
    for name, factory, option in cases:
        for sampling in ("pseudo_random", "sobol"):
            for configured in (None, 2**64 - 1):
                with subtests.test(product=name, sampling=sampling, seed=configured):
                    sampler = engine(
                        factory,
                        path_count=256,
                        seed=configured,
                        sampling=sampling,
                        antithetic=True,
                    )
                    estimate = sampler.price_with_statistics(option, snapshot)
                    assert estimate.seed is not None
                    assert estimate.seed >= 0
                    assert estimate.seed <= 2**64 - 1
                    if configured is not None:
                        assert estimate.seed == configured
                    assert sampler.seed == configured
                    replay = engine(
                        factory,
                        path_count=256,
                        seed=estimate.seed,
                        sampling=sampling,
                        antithetic=True,
                        max_active_workers=4,
                    )
                    assert replay.price_with_statistics(option, snapshot) == estimate
                    assert replay.price(option, snapshot) == estimate.price
                    restored = pickle.loads(pickle.dumps(estimate))
                    assert restored == estimate
                    assert restored.seed == estimate.seed
                    assert (
                        sampler.price_with_statistics(option, deterministic).seed
                        is None
                    )


def test_statistics_preserve_prices_and_uncertainty_across_products_and_threads(
    subtests,
):
    snapshot = context()
    for name, factory, option in contracts():
        for antithetic in (False, True):
            with subtests.test(product=name, antithetic=antithetic):
                serial = engine(factory, antithetic=antithetic)
                estimate = serial.price_with_statistics(option, snapshot)
                assert estimate.price == serial.price(option, snapshot)
                assert estimate.standard_error > 0
                assert estimate.uncertainty_status == "available"
                lower, upper = estimate.confidence_interval
                assert lower < estimate.price
                assert estimate.price < upper
                parallel = engine(factory, max_active_workers=4, antithetic=antithetic)
                for other in (
                    parallel.price_with_statistics(option, snapshot),
                    pickle.loads(pickle.dumps(estimate)),
                ):
                    assert other.price == estimate.price
                    assert other.standard_error == estimate.standard_error
                    assert other.confidence_interval == estimate.confidence_interval


@pytest.mark.parametrize("antithetic", (False, True))
def test_digital_uncertainty_matches_bernoulli_and_pair_formulas(antithetic):
    option = contracts()[0][2]
    amount = 10 * math.exp(-0.05)
    estimate = engine(
        p.MonteCarloDigitalEngine, antithetic=antithetic
    ).price_with_statistics(option, context())
    probability = estimate.price / amount
    scale, count = amount, 4096
    if antithetic:
        probability = 2 * probability - 1
        scale, count = amount / 2, 2048
    error = scale * math.sqrt(probability * (1 - probability) / (count - 1))
    assert estimate.standard_error == pytest.approx(error, rel=0, abs=1e-13)
    lower, upper = estimate.confidence_interval
    assert lower == pytest.approx(
        estimate.price - 1.959963984540054 * error, rel=0, abs=1e-12
    )
    assert upper == pytest.approx(
        estimate.price + 1.959963984540054 * error, rel=0, abs=1e-12
    )


def test_reported_uncertainty_agrees_with_analytic_prices(subtests):
    references = {
        "cash": p.AnalyticDigitalEngine(),
        "asset": p.AnalyticDigitalEngine(),
        "geometric": p.AnalyticGeometricAveragePriceEngine(),
        "barrier": p.AnalyticBarrierEngine(),
        "double": p.AnalyticDoubleBarrierEngine(),
    }
    for name, factory, option in contracts():
        if name not in references:
            continue
        with subtests.test(product=name):
            snapshot = context()
            estimate = engine(factory).price_with_statistics(option, snapshot)
            expected = references[name].price(option, snapshot)
            assert abs(estimate.price - expected) < 6 * estimate.standard_error


def test_uncertainty_omission_follows_sampling_and_independent_counts(subtests):
    cases = (
        ("single", {"path_count": 1}, "insufficient_observations"),
        (
            "single_pair",
            {"path_count": 2, "antithetic": True},
            "insufficient_observations",
        ),
        (
            "unpaired",
            {"path_count": 3, "antithetic": True},
            "incomplete_antithetic_pair",
        ),
        ("sobol", {"path_count": 16, "sampling": "sobol"}, "unsupported_sampling"),
    )
    for case, settings, status in cases:
        for name, factory, option in contracts():
            with subtests.test(case=case, product=name):
                sampler = engine(factory, **settings)
                estimate = sampler.price_with_statistics(option, context())
                assert estimate.price == sampler.price(option, context())
                assert estimate.standard_error is None
                assert estimate.confidence_interval is None
                assert estimate.uncertainty_status == status
                assert pickle.loads(pickle.dumps(estimate)) == estimate


def test_terminal_statistics_report_known_values(subtests):
    snapshot = context().replace(spot_price=110, valuation_time=END)
    for name, factory, original in contracts():
        if name in ("barrier", "double"):
            option = original.replace(
                touch_state="untouched",
            )
        elif name in ("geometric", "arithmetic"):
            option = original.replace(
                realized_average=110,
            )
        else:
            option = original
        expected_price = 110 if name == "asset" else 10
        for sampling in ("pseudo_random", "sobol"):
            with subtests.test(product=name, sampling=sampling):
                estimate = engine(
                    factory, path_count=1, sampling=sampling
                ).price_with_statistics(option, snapshot)
                assert estimate.price == expected_price
                if sampling == "sobol":
                    assert estimate.standard_error is None
                    assert estimate.confidence_interval is None
                else:
                    assert estimate.standard_error == 0
                    assert estimate.confidence_interval == (
                        (expected_price, expected_price)
                    )


def test_analytic_rebate_shifts_do_not_add_sampling_variance(subtests):
    for name, factory, original in contracts()[-2:]:
        zero = original.replace(rebate_timing="at_hit")
        rebate = zero.replace(rebate=7)
        sampler = engine(factory)
        snapshot = context()
        reference = (
            p.AnalyticBarrierEngine()
            if name == "barrier"
            else p.AnalyticDoubleBarrierEngine()
        )
        with subtests.test(product=name):
            base = sampler.price_with_statistics(zero, snapshot)
            actual = sampler.price_with_statistics(rebate, snapshot)
            shift = reference.price(rebate, snapshot) - reference.price(zero, snapshot)
            assert actual.standard_error == base.standard_error
            assert actual.price - base.price == pytest.approx(shift, rel=0, abs=1e-12)


def test_statistics_preserve_scheduled_barrier_prices(subtests):
    for name, factory, original in contracts()[-2:]:
        option = original.replace(
            touch_state=original.touch_state,
            observation_times=[START + timedelta(days=182), END],
        )
        with subtests.test(product=name):
            sampler = engine(factory)
            estimate = sampler.price_with_statistics(option, context())
            assert estimate.price == sampler.price(option, context())
            assert estimate.standard_error > 0


def test_resolved_barriers_report_zero_error_before_expiry(subtests):
    snapshot = context().replace(spot_price=120)
    vanilla = i.EuropeanOption(option_type="call", strike=100, **LIFE)
    for name, factory, original in contracts()[-2:]:
        for knock_in in (False, True):
            kind = (
                ("up_and_in" if knock_in else "up_and_out")
                if name == "barrier"
                else ("knock_in" if knock_in else "knock_out")
            )
            option = original.replace(barrier_type=kind, rebate=7)
            expected = (
                p.AnalyticEuropeanEngine().price(vanilla, snapshot)
                if knock_in
                else 7 * math.exp(-0.05)
            )
            with subtests.test(product=name, knock_in=knock_in):
                estimate = engine(factory).price_with_statistics(option, snapshot)
                assert estimate.price == pytest.approx(expected, rel=0, abs=1e-12)
                assert estimate.standard_error == 0
                assert estimate.confidence_interval == (
                    (estimate.price, estimate.price)
                )


def test_uncertainty_failure_does_not_prevent_scalar_pricing():
    option = contracts()[0][2].replace(payout=1.6e308)
    sampler = engine(p.MonteCarloDigitalEngine, path_count=2, seed=0)
    price = sampler.price(option, context())
    assert price / (8e307 * math.exp(-0.05)) == pytest.approx(1, rel=0, abs=1e-13)
    with pytest.raises(ComputationFailureError):
        sampler.price_with_statistics(option, context())
    assert sampler.price(option, context()) == price
    assert sampler.price_with_seed(option, context()) == ((price, 0))


def test_arithmetic_statistics_support_curves_and_known_history(subtests):
    option = contracts()[3][2]
    curve = m.RateTermStructure(reference_time=START, times=[END], factors=[0.95])
    snapshot = context().replace(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=curve, dividend_yield=0.02, volatility=0.2
        )
    )
    sampler = engine(p.MonteCarloAsianEngine)
    estimate = sampler.price_with_statistics(option, snapshot)
    assert estimate.price == sampler.price(option, snapshot)
    assert estimate.standard_error > 0
    snapshot = context().replace(valuation_time=START + timedelta(days=182, hours=12))
    for direction in ("call", "put"):
        with subtests.test(direction=direction):
            seasoned = option.replace(
                option_type=direction,
                realized_average=1000,
            )
            estimate = sampler.price_with_statistics(seasoned, snapshot)
            expected = (
                (400 + 50 * math.expm1(0.015) / 0.015) * math.exp(-0.025)
                if direction == "call"
                else 0
            )
            assert estimate.price == pytest.approx(expected, rel=0, abs=1e-12)
            assert estimate.standard_error == 0


def test_statistics_preserve_life_history_and_model_rejections(subtests):
    snapshot = context()
    heston = m.HestonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        initial_variance=0.04,
        long_run_variance=0.04,
        mean_reversion_speed=1,
        vol_of_vol=0.2,
        correlation=-0.5,
    )
    for name, factory, option in contracts():
        sampler = engine(factory)
        with subtests.test(product=name):
            with pytest.raises(InvalidInputError):
                sampler.price_with_statistics(
                    option,
                    snapshot.replace(valuation_time=END + timedelta(days=1)),
                )
            with pytest.raises(UnsupportedOperationError):
                sampler.price_with_statistics(
                    option,
                    snapshot.replace(model_parameters=heston),
                )
            if name not in ("cash", "asset"):
                with pytest.raises(InvalidInputError):
                    sampler.price_with_statistics(
                        option,
                        snapshot.replace(valuation_time=START + timedelta(days=1)),
                    )
    american = i.AmericanOption(option_type="put", strike=100, **LIFE)
    with pytest.raises(UnsupportedOperationError):
        engine(p.MonteCarloVanillaEngine).price_with_statistics(american, snapshot)
