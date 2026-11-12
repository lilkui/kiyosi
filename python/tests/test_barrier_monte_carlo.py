"""Single-barrier CPU Monte Carlo through the installed public Python API."""

import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from functools import partial
from itertools import product

import pytest
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from test_barrier_binomial import END, KINDS, OTHER_GREEKS, START, market, option


def test_tiny_volatility_payoffs_match_the_vanilla_limit(subtests):
    sigma = 1e-17
    end = START + timedelta(days=365)
    engine = pricing.MonteCarloBarrierEngine(
        path_count=32768, seed=42, sampling="sobol"
    )
    for spot, direction, mode in product(
        (1, 1e16), ("call", "put"), ("continuous", "scheduled")
    ):
        contract = option(
            "down_and_out",
            direction,
            strike=spot,
            barrier_level=0.8 * spot,
            expiry_time=end,
            observation_times=[end] if mode == "scheduled" else None,
        )
        context = market(spot, rate=0, dividend=0, volatility=sigma)
        expected = spot * sigma / math.sqrt(math.tau)
        with subtests.test(spot=spot, direction=direction, mode=mode):
            assert engine.price(contract, context) == pytest.approx(
                expected, rel=0, abs=1e-3 * expected
            )


@pytest.mark.parametrize("spot,shift", ((0.005, 0.01), (100, 1e-300)))
def test_at_hit_rebate_greeks_require_a_usable_spot_stencil(spot, shift):
    engine = pricing.MonteCarloBarrierEngine(path_count=32, seed=42)
    context = market(spot, rate=0.04, volatility=1)
    contract = option(
        "up_and_out",
        strike=5 * spot,
        barrier_level=5 * spot,
        rebate=1,
        rebate_timing="at_hit",
    )
    result = numerical_greeks(
        engine,
        contract,
        context,
        ("delta", "gamma"),
        spot_shift=shift,
    )
    assert result.price == engine.price(contract, context)
    for name in ("delta", "gamma"):
        assert getattr(result, name) is None
        with pytest.raises(UnsupportedOperationError):
            result.require(name)
    controls = {"spot_shift": 1e-4 * spot}
    actual = numerical_greeks(
        engine,
        contract,
        context,
        ("delta", "gamma"),
        **controls,
    )
    expected = numerical_greeks(
        pricing.AnalyticBarrierEngine(),
        contract,
        context,
        ("delta", "gamma"),
        **controls,
    )
    assert actual.delta == expected.delta
    assert actual.gamma == expected.gamma


@pytest.mark.parametrize("sampling", ("pseudo_random", "sobol"))
@pytest.mark.parametrize("kind", KINDS)
@pytest.mark.parametrize("direction", ("call", "put"))
def test_continuous_prices_and_native_greeks_match_analytic(
    subtests, sampling, kind, direction
):
    engine = pricing.MonteCarloBarrierEngine(
        path_count=131072, seed=42, sampling=sampling
    )
    contract = option(kind, direction, rebate=2)
    actual = engine.price_with_greeks(contract, market(), greeks="all")
    expected = pricing.AnalyticBarrierEngine().price_with_greeks(
        contract, market(), ("delta", "gamma")
    )
    for name, budget in (
        ("price", 0.12),
        ("delta", 0.04),
        ("gamma", 0.012),
    ):
        with subtests.test(
            sampling=sampling, kind=kind, direction=direction, name=name
        ):
            assert getattr(actual, name) == pytest.approx(
                getattr(expected, name), rel=0, abs=budget
            )
    assert actual.price == engine.price(contract, market())
    for name in OTHER_GREEKS:
        assert getattr(actual, name) is None


def test_scheduled_prices_greeks_and_early_monitoring_match_quadrature(subtests):
    engine = pricing.MonteCarloBarrierEngine(
        path_count=131072, seed=42, sampling="sobol"
    )
    for early in (False, True):
        for kind in KINDS:
            dates = [START + timedelta(days=90), START + timedelta(days=180)]
            if not early:
                dates.append(END)
            contract = option(
                kind,
                "put",
                rebate=2,
                observation_times=dates,
            )
            actual = engine.price_with_greeks(contract, market(), greeks="all")
            expected = pricing.QuadratureBarrierEngine().price_with_greeks(
                contract, market(), ("delta", "gamma")
            )
            for name, budget in (
                ("price", 0.08),
                ("delta", 0.03),
                ("gamma", 0.015),
            ):
                with subtests.test(early=early, kind=kind, name=name):
                    assert getattr(actual, name) == pytest.approx(
                        getattr(expected, name), rel=0, abs=budget
                    )
    contract = option(
        "up_and_out",
        strike=130,
        observation_times=[START + timedelta(days=90)],
    )
    assert engine.price(contract, market()) > 0.01


def test_hit_rebates_and_negative_rates_match_analytic_values():
    engine = pricing.MonteCarloBarrierEngine(
        path_count=65536, seed=17, sampling="sobol"
    )
    for kind in ("up_and_out", "down_and_out"):
        contract = option(kind, rebate=10, rebate_timing="at_hit")
        for rate in (-0.02, 0.04):
            context = market(rate=rate)
            actual = engine.price_with_greeks(contract, context, greeks="all")
            expected = pricing.AnalyticBarrierEngine().price_with_greeks(
                contract, context, ("delta", "gamma")
            )
            for name, budget in (
                ("price", 0.015),
                ("delta", 0.006),
                ("gamma", 0.003),
            ):
                assert getattr(actual, name) == pytest.approx(
                    getattr(expected, name), rel=0, abs=budget
                )
    context = market(rate=0.04)
    observation = START + timedelta(days=180)
    terms = {
        "rebate_timing": "at_hit",
        "observation_times": [observation],
    }
    with_rebate = option("up_and_out", "put", rebate=10, **terms)
    without_rebate = option("up_and_out", "put", **terms)
    engine = pricing.MonteCarloBarrierEngine(path_count=131072, seed=42)
    time = 180 / 365
    threshold = (math.log(1.2) - 0.01 * time) / (0.2 * math.sqrt(time))
    expected = 10 * math.exp(-0.04 * time) * 0.5 * math.erfc(threshold / math.sqrt(2))
    assert engine.price(with_rebate, context) - engine.price(
        without_rebate, context
    ) == pytest.approx(expected, rel=0, abs=0.025)


def test_steps_threads_pairing_and_concurrent_calls_preserve_samples():
    context = market()
    for scheduled in (False, True):
        contract = option(
            "up_and_out",
            "put",
            rebate=3,
            rebate_timing="at_hit",
            **(
                {
                    "observation_times": [START + timedelta(days=90), END],
                }
                if scheduled
                else {}
            ),
        )
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                settings = {
                    "path_count": 1024,
                    "seed": 17,
                    "sampling": sampling,
                    "antithetic": antithetic,
                }
                serial = pricing.MonteCarloBarrierEngine(
                    max_active_workers=1, **settings
                )
                parallel = pricing.MonteCarloBarrierEngine(
                    max_active_workers=4, **settings
                )
                expected = serial.price_with_greeks(contract, context, greeks="all")

                def evaluate(
                    index, parallel=parallel, serial=serial, contract=contract
                ):
                    engine = parallel if index % 2 else serial
                    result = engine.price_with_greeks(contract, context, greeks="all")
                    return result.price, result.delta, result.gamma

                with ThreadPoolExecutor(max_workers=4) as pool:
                    for actual in pool.map(evaluate, range(8)):
                        assert actual == (
                            (expected.price, expected.delta, expected.gamma)
                        )
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    for paths in (1, 2, 3, 5, 257):
        settings = {"path_count": paths, "seed": 42, "antithetic": True}
        engine = pricing.MonteCarloBarrierEngine(**settings)
        total = engine.price(option("down_and_in"), context) + engine.price(
            option(), context
        )
        expected = pricing.MonteCarloVanillaEngine(**settings).price(vanilla, context)
        assert total == pytest.approx(expected, rel=0, abs=1e-12)


def test_history_current_equality_expiry_and_finished_monitoring():
    engine = pricing.MonteCarloBarrierEngine(path_count=1, seed=42)
    for kind in KINDS:
        for timing in (
            ("at_expiry",) if kind.endswith("in") else ("at_hit", "at_expiry")
        ):
            contract = option(kind, rebate=7, rebate_timing=timing)
            context = market(spot=contract.barrier_level)
            result = engine.price_with_greeks(contract, context, greeks="all")
            assert result.price == pricing.AnalyticBarrierEngine().price(
                contract, context
            )
            for name in ("delta", "gamma", *OTHER_GREEKS):
                assert getattr(result, name) is None
            later = context.replace(
                valuation_time=datetime(2025, 1, 1) + timedelta(microseconds=1)
            )
            assert_rejection(
                MissingHistoryError,
                engine.price,
                contract,
                later,
            )
            touched = contract.replace(
                touch_state="touched",
            )
            assert_rejection(InvalidInputError, engine.price, touched, context)
            assert engine.price(
                touched, later
            ) == pricing.AnalyticBarrierEngine().price(touched, later)
            for history in ("touched", "untouched"):
                known = contract.replace(
                    touch_state=history,
                )
                expired = market(spot=contract.barrier_level, valuation=END)
                result = engine.price_with_greeks(known, expired, greeks="all")
                assert result.price == pricing.AnalyticBarrierEngine().price(
                    known, expired
                )
                reference = pricing.AnalyticBarrierEngine().price_with_greeks(
                    known, expired, "all"
                )
                assert result.delta == reference.delta
                assert result.gamma == reference.gamma
            observation = START + timedelta(days=90)
            scheduled = option(
                kind,
                rebate=7,
                rebate_timing=timing,
                observation_times=[observation],
            )
            on_date = market(spot=contract.barrier_level, valuation=observation)
            assert engine.price(
                scheduled, on_date
            ) == pricing.AnalyticBarrierEngine().price(scheduled, on_date)
            after = on_date.replace(
                valuation_time=datetime.combine(observation, datetime.min.time())
                + timedelta(microseconds=1)
            )
            untouched = scheduled.replace(
                touch_state="untouched",
            )
            assert engine.price(
                untouched, after
            ) == pricing.AnalyticBarrierEngine().price(untouched, after)
            assert contract.touch_state is None
    near_barrier = market(spot=80.005)
    result = engine.price_with_greeks(option(), near_barrier, greeks="all")
    assert result.delta is None
    assert result.gamma is None


def test_defaults_and_strict_representations():
    factory = pricing.MonteCarloBarrierEngine
    engine, vanilla = factory(), pricing.MonteCarloVanillaEngine()
    for name in (
        "path_count",
        "seed",
        "max_active_workers",
        "sampling",
        "antithetic",
    ):
        assert getattr(engine, name) == getattr(vanilla, name)
        with pytest.raises(AttributeError):
            setattr(engine, name, getattr(engine, name))
    assert "MonteCarloBarrierEngine" in pricing.__all__
    for name in ("path_count", "seed", "max_active_workers"):
        for value in (
            True,
            False,
            1.5,
            2.0,
            "3",
            None if name != "seed" else object(),
        ):
            with pytest.raises(TypeError):
                factory(**{name: value})
        with pytest.raises(OverflowError):
            factory(**{name: 1 << 100})
    for fields in (
        {"antithetic": 1},
        {"unknown": 1},
        {"sampling": True},
    ):
        with pytest.raises(TypeError):
            factory(**fields)
    with pytest.raises(ValueError):
        factory(sampling="unknown")
    for fields in (
        {"path_count": 0},
        {"path_count": 10000001},
        {"max_active_workers": -1},
        {"path_count": 3, "sampling": "sobol"},
    ):
        assert_rejection(InvalidInputError, factory, **fields)
    for seed in (-1, 2**64):
        with pytest.raises(OverflowError):
            factory(seed=seed)
    for seed in (0, 2**64 - 1):
        assert factory(seed=seed).seed == seed
    for fields in (
        {"spot_shift": True},
        {"volatility_shift": True},
        {"rate_shift": True},
        {"time_shift": True},
        {"unknown": 1},
    ):
        with pytest.raises(TypeError):
            numerical_greeks(
                engine,
                option(),
                market(),
                "delta",
                **fields,
            )
    seeded = factory(path_count=32, seed=42)
    expected = seeded.price_with_greeks(option(), market(), "delta")
    controlled = numerical_greeks(
        seeded,
        option(),
        market(),
        "delta",
        volatility_shift=0.001,
        rate_shift=0.001,
        time_shift=timedelta(days=2),
    )
    assert controlled.price == expected.price
    h = (100 + 0.01) - 100
    assert controlled.delta == pytest.approx(
        (
            seeded.price(option(), market(spot=100 + h))
            - seeded.price(option(), market(spot=100 - h))
        )
        / (2 * h),
        rel=0,
        abs=1e-10,
    )
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    for operation, args in (
        (engine.price, (vanilla, market())),
        (engine.price_with_greeks, (vanilla, market(), "delta")),
        (
            partial(pricing.implied_volatility, lower_bound=0.0, upper_bound=4.0),
            (engine, vanilla, market(), 10),
        ),
    ):
        with pytest.raises(UnsupportedOperationError):
            operation(*args)


def test_applicability_errors_validation_order_and_failure_state():
    engine = pricing.MonteCarloBarrierEngine(path_count=32, seed=42)
    contract, context = option(), market()
    quote = engine.price(contract, context)
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[math.exp(-0.03 * 360 / 365), (math.exp(-0.03 * 360 / 365)) * 0.5],
    )
    for changes in ({"rate": curve}, {"dividend": curve}):
        curved = market(**changes)
        assert_rejection(
            UnsupportedOperationError,
            engine.price,
            contract,
            curved,
        )
        assert_rejection(
            UnsupportedOperationError,
            engine.price_with_greeks,
            contract,
            curved,
            [],
        )
        assert_rejection(
            UnsupportedOperationError,
            pricing.implied_volatility,
            engine,
            contract,
            curved,
            math.nan,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    for valuation in (START - timedelta(days=1), END + timedelta(days=1)):
        assert_rejection(
            InvalidInputError,
            engine.price,
            contract,
            market(valuation=valuation),
        )
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        contract,
        context,
        "delta",
        spot_shift=0,
    )
    for sigma, rate in ((50, 0.03), (0.2, -1000)):
        assert_rejection(
            ComputationFailureError,
            engine.price,
            contract,
            market(volatility=sigma, rate=rate),
        )
    assert engine.price(contract, context) == quote
    assert context.model_parameters.volatility == 0.2
    assert contract.touch_state is None


def test_solver_pins_draws_checks_fixed_cashflows_and_keeps_inputs():
    engine = pricing.MonteCarloBarrierEngine(path_count=8192, seed=42, sampling="sobol")
    contract, context = option(), market()
    quote = engine.price(contract, context)

    def solve(index):
        return pricing.implied_volatility(
            engine,
            contract,
            context,
            quote,
            lower_bound=0.15,
            upper_bound=0.25,
        )

    with ThreadPoolExecutor(max_workers=4) as pool:
        for sigma in pool.map(solve, range(8)):
            assert sigma == pytest.approx(0.2, rel=0, abs=1e-5)
    for observed, controls, expected_error in (
        (-1, {}, RootNotBracketedError),
        (math.nan, {}, InvalidInputError),
        (quote, {"max_iterations": 0}, InvalidInputError),
    ):
        assert_rejection(
            expected_error,
            pricing.implied_volatility,
            engine,
            contract,
            context,
            observed,
            **{"lower_bound": 0.0, "upper_bound": 4.0, **controls},
        )
    fixed = contract.replace(
        touch_state="touched",
    )
    later = market(valuation=START + timedelta(days=1))
    assert_rejection(
        UnsupportedOperationError,
        pricing.implied_volatility,
        engine,
        fixed,
        later,
        engine.price(fixed, later),
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert engine.price(contract, context) == quote
    assert context.model_parameters.volatility == 0.2


def test_sobol_dimension_limit_includes_the_unobserved_expiry_leg():
    dates = [START + timedelta(days=i) for i in range(1, 21202)]
    contract = option(
        "up_and_out",
        expiry_time=dates[-1] + timedelta(days=1),
        observation_times=dates,
    )
    engine = pricing.MonteCarloBarrierEngine(path_count=1, seed=42, sampling="sobol")
    assert_rejection(InvalidInputError, engine.price, contract, market())


def test_scaled_rare_hits_and_only_requested_greek_failures():
    engine = pricing.MonteCarloBarrierEngine(path_count=1, seed=42)
    contract = option("down_and_in", strike=1, barrier_level=1e300 * math.exp(-20))
    context = market(spot=1e300, rate=0.5, dividend=0, volatility=1)
    assert engine.price(contract, context) > 0
    contract = option()
    context = market(volatility=1e-200)
    price = engine.price(contract, context)
    assert engine.price_with_greeks(contract, context, []).price == price
    assert_rejection(
        ComputationFailureError,
        engine.price_with_greeks,
        contract,
        context,
        "gamma",
    )
    from kiyosi.market import weekdays_calendar

    with pytest.raises(InvalidInputError):
        option(
            observation_times=[START + timedelta(days=3)],
            calendar=weekdays_calendar(),
        )
