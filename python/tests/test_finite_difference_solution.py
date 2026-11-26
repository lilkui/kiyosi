"""Reusable FD solutions, event scenarios and Python value/boundary contracts."""

import copy
import gc
import inspect
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, time, timedelta
from decimal import Decimal

import pytest
from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
)
from support.assertions import assert_rejection
from test_heston import context as heston_context
from test_local_volatility import option
from test_phoenix import MIDDLE
from test_phoenix import contract as phoenix
from test_support_matrix import terminal_cases

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
SPOTS = (70.0, 80.0, 95.25, 100.0, 105.75, 120.0, 130.0, 100.0)
PAIRS = [
    (instrument, name)
    for instrument, names, _ in terminal_cases()
    for name in names
    if name.startswith("FiniteDifference")
    and name != "FiniteDifferenceHestonVanillaEngine"
]


def context(kind="bsm", spot=150.0, valuation=START):
    model = BlackScholesMertonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        volatility=0 if kind == "deterministic" else 0.2,
    )
    if kind == "local":
        model = LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            surface=LocalVolatilitySurface(
                times=[START, END], spots=[50, 150], values=[[0.2, 0.2]] * 2
            ),
        )
    return PricingContext(
        model_parameters=model, spot_price=spot, valuation_time=valuation
    )


@pytest.mark.parametrize(("direction", "barrier"), (("call", 115), ("put", 85)))
@pytest.mark.parametrize("rate", (-0.12, 0.0, 0.12))
def test_saved_accumulator_matches_scalar_deterministic_fixings(
    direction, barrier, rate
):
    option = ins.Accumulator(
        option_type=direction,
        strike=100,
        knock_out_level=barrier,
        daily_quantity=0.1,
        acceleration_factor=2.5,
        effective_time=START,
        expiry_time=END,
        observation_clock_time=time(),
        calendar=all_days_calendar(),
        knock_out_observation_times=[
            START + timedelta(days=120),
            START + timedelta(days=240),
            END,
        ],
    )
    snapshot = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=0, volatility=0
        ),
        spot_price=100,
        valuation_time=START,
    )
    engine = pricing.FiniteDifferenceAccumulatorEngine()
    solution = engine.solve(option, snapshot, spot_range=(75, 125))
    spots = (125, 75, 85, 100, 115, 100, 93, 108)
    expected = [
        engine.price(option, snapshot.replace(spot_price=spot)) for spot in spots
    ]
    assert solution.price_many(spots) == pytest.approx(expected, rel=0, abs=1e-9)


def test_log_grid_covers_low_saved_put_endpoint():
    put = ins.EuropeanOption(
        option_type="put", strike=100, effective_time=START, expiry_time=END
    )
    low = 0.0001
    engine = pricing.FiniteDifferenceVanillaEngine()
    solution = engine.solve(put, context(), spot_range=(low, 120.0))
    expected = pricing.AnalyticEuropeanEngine().price(put, context(spot=low))
    assert solution.price(low) == pytest.approx(expected, abs=0.001, rel=0)
    assert engine.price(put, context(spot=low)) == pytest.approx(
        expected, abs=0.001, rel=0
    )
    assert solution.price(120.0) < solution.price(low)


def test_log_grid_keeps_continuous_barrier_equality():
    barrier = 80.1
    contract = ins.BarrierOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
        barrier_level=barrier,
        barrier_type="down_and_out",
        rebate=7.0,
    )
    below = math.nextafter(barrier, 0.0)
    engine = pricing.FiniteDifferenceBarrierEngine()
    solution = engine.solve(contract, context(), spot_range=(below, 100.0))
    analytic = pricing.AnalyticBarrierEngine()
    rebate = analytic.price(contract, context(spot=barrier))
    assert solution.price(below) == pytest.approx(rebate, abs=1e-12, rel=0)
    assert solution.price(barrier) == pytest.approx(rebate, abs=1e-12, rel=0)
    above = barrier + 0.01
    assert solution.price(above) == pytest.approx(
        analytic.price(contract, context(spot=above)), abs=0.001, rel=0
    )


def test_log_grid_respects_discounted_asset_digital_cap_with_carry():
    contract = ins.AssetOrNothingOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    snapshot = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.11, dividend_yield=0.04, volatility=0.08
        ),
        spot_price=250,
        valuation_time=START,
    )
    value = pricing.FiniteDifferenceDigitalEngine().price(contract, snapshot)
    assert value <= 250 * math.exp(-0.04)
    assert value == pytest.approx(
        pricing.AnalyticDigitalEngine().price(contract, snapshot), abs=0.00001, rel=0
    )


@pytest.mark.parametrize("volatility", (0.01, 0.012_720, 0.012_735))
def test_automatic_european_grid_resolves_low_volatility_with_strong_carry(volatility):
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "expiry_time": START + timedelta(days=730),
    }
    snapshot = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.15, dividend_yield=-0.03, volatility=volatility
        ),
        spot_price=70,
        valuation_time=START,
    )
    for contract, engine, analytic in (
        (
            ins.EuropeanOption(**terms),
            pricing.FiniteDifferenceVanillaEngine(),
            pricing.AnalyticEuropeanEngine(),
        ),
        (
            ins.CashOrNothingOption(payout=100, **terms),
            pricing.FiniteDifferenceDigitalEngine(),
            pricing.AnalyticDigitalEngine(),
        ),
        (
            ins.AssetOrNothingOption(**terms),
            pricing.FiniteDifferenceDigitalEngine(),
            pricing.AnalyticDigitalEngine(),
        ),
    ):
        expected = analytic.price(contract, snapshot)
        assert engine.price(contract, snapshot) == pytest.approx(expected, abs=0.001)
        solution = engine.solve(contract, snapshot, spot_range=(68.0, 72.0))
        assert solution.price(70) == pytest.approx(expected, abs=0.001)


@pytest.mark.parametrize("kind", ("bsm", "local", "deterministic"))
@pytest.mark.parametrize(
    "instrument,name",
    PAIRS,
    ids=[
        f"{name}.{type(instrument).__name__}.{i}"
        for i, (instrument, name) in enumerate(PAIRS)
    ],
)
def test_all_one_dimensional_families_reuse_saved_prices(instrument, name, kind):
    engine = getattr(pricing, name)(asset_upper_boundary=400)
    snapshot = context(kind)
    solution = engine.solve(instrument, snapshot, spot_range=(70, 130))
    assert type(solution) is pricing.FiniteDifferenceSolution
    expected_support = (
        ("delta", "gamma") if kind == "local" else ("delta", "gamma", "speed")
    )
    assert solution.supported == expected_support
    prices = solution.price_many(iter(SPOTS))
    results = solution.price_many_with_greeks(SPOTS, "all")
    assert type(prices) is tuple
    assert type(results) is tuple
    for spot, price, result in zip(SPOTS, prices, results, strict=True):
        expected = engine.price(instrument, snapshot.replace(spot_price=spot))
        assert price == pytest.approx(expected, rel=0, abs=1e-6 * max(1, abs(expected)))
        assert price == solution.price(spot) == result.price
        assert result == solution.price_with_greeks(spot, "all")
        assert result.supported == expected_support
        assert result.status("vega") == "unsupported"
    assert prices[3] == prices[-1]


@pytest.mark.parametrize("american", (False, True), ids=("european", "american"))
def test_heston_queries_freeze_normalization_and_preserve_unsupported_greeks(american):
    instrument = option()
    if american:
        instrument = ins.AmericanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
    engine = pricing.FiniteDifferenceHestonVanillaEngine()
    snapshot = heston_context(spot_price=150)
    solution = engine.solve(instrument, snapshot, spot_range=(80, 120))
    assert solution.supported == ()
    spots = (80, 100, 105, 120)
    exercise_anchors = (80, 100, 120) if american else ()
    for spot, result in zip(
        spots,
        solution.price_many_with_greeks(spots, "all"),
        strict=True,
    ):
        assert result.price == solution.price(spot)
        if spot in exercise_anchors:
            expected = engine.price(instrument, snapshot.replace(spot_price=spot))
            assert result.price == pytest.approx(expected, rel=0, abs=0.005)
        elif not american:
            reference = pricing.AnalyticHestonEuropeanEngine().price(
                instrument, snapshot.replace(spot_price=spot)
            )
            assert result.price == pytest.approx(reference, rel=0, abs=0.003)
        assert result.supported == ()
        assert result.status("delta") == "unsupported"


def test_owned_metadata_signatures_equality_copy_and_concurrent_reads():
    engine = pricing.FiniteDifferenceVanillaEngine()
    instrument, snapshot = option(), context()
    solution = engine.solve(instrument, snapshot, spot_range=(80, 120))
    assert solution.spot_range == (80, 120)
    assert solution.valuation_time == START
    assert "FiniteDifferenceSolution" in pricing.__all__
    assert copy.copy(solution) is solution
    assert copy.deepcopy(solution) is solution
    assert solution == engine.solve(instrument, snapshot, spot_range=(80, 120))
    assert solution != engine.solve(instrument, snapshot, spot_range=(81, 120))
    assert solution != engine.replace(asset_step_count=400).solve(
        instrument, snapshot, spot_range=(80, 120)
    )
    changed_contract = instrument.replace(strike=110)
    assert solution != engine.solve(changed_contract, snapshot, spot_range=(80, 120))
    text = repr(solution)
    assert text.startswith("FiniteDifferenceSolution(spot_range=(80.0, 120.0)")
    assert "valuation_time=" in text and "supported=" in text
    assert len(text) < 250
    for field in ("spot_range", "valuation_time", "supported"):
        with pytest.raises(AttributeError):
            setattr(solution, field, None)
    with pytest.raises(TypeError):
        pricing.FiniteDifferenceSolution()
    with pytest.raises(TypeError):
        hash(solution)
    assert not hasattr(solution, "replace")
    assert (
        inspect.signature(engine.solve).parameters["spot_range"].kind
        is inspect.Parameter.KEYWORD_ONLY
    )
    assert (
        inspect.signature(solution.price_with_greeks).parameters["greeks"].default
        is inspect.Parameter.empty
    )
    assert tuple(inspect.signature(solution.price_with_greeks).parameters) == (
        "spot_price",
        "greeks",
        "spot_shift",
    )
    expected = solution.price_many([80, 95, 100, 105, 120])
    snapshot = snapshot.replace(spot_price=200)
    del engine, instrument, snapshot
    gc.collect()
    with ThreadPoolExecutor(4) as pool:
        assert tuple(pool.map(solution.price, [80, 95, 100, 105, 120])) == expected
        batches = tuple(pool.map(solution.price_many, ([80, 95, 100, 105, 120],) * 4))
        assert batches == (expected,) * 4


@pytest.mark.parametrize(
    "bounds",
    ((100, 100), (120, 80), (-1, 120), (0, 120), (math.nan, 120), (80, math.inf)),
    ids=("equal", "reversed", "negative", "zero", "nan", "infinite"),
)
def test_invalid_intervals_fail_before_publishing_a_solution(bounds):
    with pytest.raises(InvalidInputError):
        pricing.FiniteDifferenceVanillaEngine().solve(
            option(), context(), spot_range=bounds
        )


def test_conversion_empty_batches_iterator_errors_and_failure_state():
    engine = pricing.FiniteDifferenceVanillaEngine()
    solution = engine.solve(option(), context(), spot_range=(Decimal(80), Decimal(120)))
    expected = solution.price(Decimal(100))
    for value in (True, "100", object()):
        with pytest.raises(TypeError):
            solution.price(value)
        with pytest.raises(TypeError):
            solution.price_many([100, value])
    for spot in (0, -1, 79.99, 120.01, math.nan, math.inf):
        with pytest.raises(InvalidInputError):
            solution.price(spot)
        with pytest.raises(InvalidInputError):
            solution.price_many_with_greeks([100, spot], "all")
    for bounds in ([80, 120], (True, 120), (80, False)):
        with pytest.raises(TypeError):
            engine.solve(option(), context(), spot_range=bounds)
    for bounds in ((), (80,), (80, 100, 120)):
        assert_rejection(
            ValueError, engine.solve, option(), context(), spot_range=bounds
        )
    assert solution.price_many([]) == ()
    assert solution.price_many_with_greeks([], []) == ()
    assert solution.price_with_greeks(100, []).requested == ()
    assert_rejection(ValueError, solution.price_many_with_greeks, [], "unknown")
    for shift in (0, -1, math.inf, math.nan):
        with pytest.raises(InvalidInputError):
            solution.price_many_with_greeks([], [], spot_shift=shift)
    with pytest.raises(TypeError):
        solution.price_with_greeks(100, "delta", spot_shift=True)

    class IteratorFailure(Exception):
        pass

    def broken():
        yield 100
        raise IteratorFailure("iteration failed")

    for operation in (
        lambda: solution.price_many(broken()),
        lambda: solution.price_many_with_greeks(broken(), "delta"),
    ):
        with pytest.raises(IteratorFailure, match="iteration failed"):
            operation()
    assert solution.price(100) == expected
    with pytest.raises(UnsupportedOperationError):
        pricing.FiniteDifferenceVanillaEngine().solve(
            option(), heston_context(), spot_range=(80, 120)
        )
    with pytest.raises(UnsupportedOperationError):
        pricing.FiniteDifferenceHestonVanillaEngine().solve(
            option(), context(), spot_range=(80, 120)
        )


def test_native_spot_greeks_stencils_and_default_controls_match_the_saved_price():
    solution = pricing.FiniteDifferenceVanillaEngine().solve(
        option(), context(), spot_range=(80, 120)
    )
    assert solution.price_with_greeks(100, "all") == solution.price_with_greeks(
        100, "all", spot_shift=None
    )
    assert solution.price_with_greeks(100, "all") == solution.price_with_greeks(
        100, "all", spot_shift=...
    )
    result = solution.price_with_greeks(
        100, ["delta", "gamma", "speed"], spot_shift=0.1
    )
    up, center, down = solution.price_many([100.1, 100, 99.9])
    assert result.delta == pytest.approx((up - down) / 0.2, rel=0, abs=1e-5)
    assert result.gamma == pytest.approx(
        (up - 2 * center + down) / 0.1**2, rel=0, abs=1e-5
    )
    up_gamma = solution.price_with_greeks(100.1, "gamma").gamma
    down_gamma = solution.price_with_greeks(99.9, "gamma").gamma
    assert result.speed == pytest.approx(
        (up_gamma - down_gamma) / 0.2, rel=0, abs=1e-10
    )
    for spot in (80, 120):
        endpoint = solution.price_with_greeks(spot, "all")
        assert endpoint.status("delta") == endpoint.status("gamma") == "available"
        assert endpoint.status("speed") == "stencil_unavailable"
    too_wide = solution.price_with_greeks(100, "all", spot_shift=100)
    assert too_wide.status("speed") == "stencil_unavailable"
    assert too_wide.status("delta") == "available"


@pytest.mark.parametrize("kind,level", (("up_and_in", 110), ("down_and_in", 90)))
@pytest.mark.parametrize("binary", (False, True), ids=("vanilla", "binary"))
def test_local_knockin_uses_saved_continuation_across_current_touch(
    kind, level, binary
):
    fields = {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "expiry_time": END,
        "barrier_type": kind,
        "barrier_level": level,
        "touch_state": "untouched",
    }
    instrument = (
        ins.BinaryBarrierOption(**fields, payoff_type="cash", payout=10)
        if binary
        else ins.BarrierOption(**fields)
    )
    factory = (
        pricing.FiniteDifferenceBinaryBarrierEngine
        if binary
        else pricing.FiniteDifferenceBarrierEngine
    )
    engine = factory(asset_upper_boundary=400)
    snapshot = context("local").replace(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            surface=LocalVolatilitySurface(
                times=[START, END], spots=[50, 150], values=[[0.18, 0.28], [0.24, 0.32]]
            ),
        )
    )
    solution = engine.solve(instrument, snapshot, spot_range=(70, 130))
    for spot in (70, 90, 100, 110, 130):
        expected = engine.price(instrument, snapshot.replace(spot_price=spot))
        assert solution.price(spot) == pytest.approx(expected, rel=0, abs=1e-8)


@pytest.mark.parametrize("kind", ("bsm", "local"))
def test_current_autocall_events_use_each_scenario_spot_with_fixed_history(kind):
    instrument = phoenix(
        knock_in_observation=ins.KnockInObservation.every_trading_day(time())
    )
    snapshot = context(kind, spot=115, valuation=MIDDLE)
    engine = pricing.FiniteDifferenceAutocallableEngine(asset_upper_boundary=400)
    solution = engine.solve(instrument, snapshot, spot_range=(70, 115))
    spots = [70, 80, 90, 100, 110, 115]
    for level in (80, 90, 110):
        spots.extend([math.nextafter(level, 0), math.nextafter(level, math.inf)])
    for spot in spots:
        expected = engine.price(instrument, snapshot.replace(spot_price=spot))
        assert solution.price(spot) == pytest.approx(expected, rel=0, abs=1e-4)
    for threshold in (80, 90, 110):
        assert (
            solution.price_with_greeks(threshold, "all").status("delta") == "undefined"
        )
    assert solution.price(110) == solution.price(115) == 1.01
    if kind == "bsm":
        reference = pricing.QuadratureAutocallableEngine(asset_step_count=1024)
        for spot in (70, 85, 95, 105):
            assert solution.price(spot) == pytest.approx(
                reference.price(instrument, snapshot.replace(spot_price=spot)),
                rel=0,
                abs=1e-4,
            )


def test_saved_deterministic_american_uses_dense_exercise_intervals():
    times = [START + timedelta(days=day) for day in range(1, 1201)]
    curve = RateTermStructure(
        reference_time=START, times=times, factors=[1.0] * len(times)
    )
    model = BlackScholesMertonParameters(
        risk_free_rate=curve, dividend_yield=0, volatility=0
    )
    snapshot = PricingContext(
        model_parameters=model, spot_price=100, valuation_time=START
    )
    instrument = ins.AmericanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=times[-1]
    )
    solution = pricing.FiniteDifferenceVanillaEngine().solve(
        instrument, snapshot, spot_range=(80, 120)
    )
    assert solution.price(110) == pytest.approx(10.0, abs=1e-12)
    assert solution.price_with_greeks(110, "delta").delta == pytest.approx(
        1.0, abs=1e-9
    )
    assert solution.price_with_greeks(100, "delta").status("delta") == "undefined"


def test_deterministic_american_exercise_tie_has_undefined_spot_greeks():
    nodes = [START + timedelta(days=180), END]
    market = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=RateTermStructure(
                reference_time=START, times=nodes, factors=[1.0, 61 / 64]
            ),
            dividend_yield=RateTermStructure(
                reference_time=START, times=nodes, factors=[62 / 64] * 2
            ),
            volatility=0,
        ),
        spot_price=150,
        valuation_time=START,
    )
    instrument = ins.AmericanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    engine = pricing.FiniteDifferenceVanillaEngine()
    solution = engine.solve(instrument, market, spot_range=(145, 155))
    greeks = ["delta", "gamma"]
    for result in (
        engine.price_with_greeks(instrument, market, greeks),
        solution.price_with_greeks(150, greeks),
    ):
        assert result.price == pytest.approx(50, rel=0, abs=1e-12)
        assert result.status("delta") == result.status("gamma") == "undefined"
    for spot, delta in ((149, 62 / 64), (151, 1)):
        result = solution.price_with_greeks(spot, greeks)
        assert result.delta == pytest.approx(delta, rel=0, abs=1e-9)
        assert result.gamma == pytest.approx(0, rel=0, abs=1e-8)
    for result in (
        engine.price_with_greeks(
            instrument, market.replace(spot_price=149.5), greeks, spot_shift=1
        ),
        solution.price_with_greeks(149.5, greeks, spot_shift=1),
    ):
        assert result.price == pytest.approx(49.515625, rel=0, abs=1e-12)
        assert result.status("delta") == "stencil_unavailable"
        assert result.status("gamma") == "stencil_unavailable"


def test_expiry_deterministic_and_settlement_paths_ignore_unused_grid_bounds():
    engine = pricing.FiniteDifferenceDigitalEngine(asset_upper_boundary=1)
    instrument = ins.CashOrNothingOption(
        option_type="call", strike=100, payout=10, effective_time=START, expiry_time=END
    )
    solution = engine.solve(instrument, context(valuation=END), spot_range=(80, 120))
    assert solution.price_many([80, 100, math.nextafter(100, math.inf), 120]) == (
        0,
        0,
        10,
        10,
    )
    assert solution.price_with_greeks(100, "all").status("gamma") == "undefined"
    deterministic = context().replace(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0
        )
    )
    vanilla = pricing.FiniteDifferenceVanillaEngine(asset_upper_boundary=1).solve(
        option(), deterministic, spot_range=(80, 120)
    )
    for spot in (80, 100, 120):
        expected = pricing.AnalyticEuropeanEngine().price(
            option(), deterministic.replace(spot_price=spot)
        )
        assert vanilla.price(spot) == pytest.approx(expected, rel=0, abs=1e-12)


def test_fixed_mesh_converges_over_an_interval_against_analytic_prices():
    spots = (80, 93.7, 100, 108.3, 120)
    expected = [
        pricing.AnalyticEuropeanEngine().price(option(), context(spot=spot))
        for spot in spots
    ]
    solutions = [
        pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=n, time_step_count=n, asset_upper_boundary=400
        ).solve(option(), context(), spot_range=(80, 120))
        for n in (100, 200, 400)
    ]
    errors = [
        max(
            abs(value - ref)
            for value, ref in zip(saved.price_many(spots), expected, strict=True)
        )
        for saved in solutions
    ]
    assert errors[2] < errors[1] < errors[0]
    assert errors[-1] < 0.001


@pytest.mark.parametrize("binary", (False, True), ids=("vanilla", "binary"))
def test_finished_scheduled_monitoring_uses_saved_local_continuation(binary):
    valuation = datetime(2025, 1, 5)
    fields = {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "expiry_time": END,
        "barrier_type": "up_and_out",
        "barrier_level": 110,
        "observation_times": [datetime(2025, 1, 2)],
        "touch_state": "untouched",
    }
    instrument = (
        ins.BinaryBarrierOption(**fields, payoff_type="cash", payout=10)
        if binary
        else ins.BarrierOption(**fields)
    )
    factory = (
        pricing.FiniteDifferenceBinaryBarrierEngine
        if binary
        else pricing.FiniteDifferenceBarrierEngine
    )
    engine = factory(asset_upper_boundary=400)
    snapshot = context("local", valuation=valuation)
    solution = engine.solve(instrument, snapshot, spot_range=(80, 120))
    for spot in (80, 100, 110, 120):
        expected = engine.price(instrument, snapshot.replace(spot_price=spot))
        assert solution.price(spot) == pytest.approx(expected, rel=0, abs=1e-10)
    assert solution.price(120) > 1


def test_fixed_rebate_after_monitoring_ignores_unused_surface_and_grid_coverage():
    valuation = datetime(2025, 1, 5)
    instrument = ins.BarrierOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
        barrier_type="up_and_in",
        barrier_level=110,
        rebate=2,
        observation_times=[datetime(2025, 1, 2)],
        touch_state="untouched",
    )
    snapshot = context("local", valuation=valuation).replace(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            surface=LocalVolatilitySurface(
                times=[START, valuation], spots=[80, 120], values=[[0.2, 0.2]] * 2
            ),
        )
    )
    engine = pricing.FiniteDifferenceBarrierEngine(asset_upper_boundary=1)
    solution = engine.solve(instrument, snapshot, spot_range=(80, 120))
    expected = 2 * math.exp(-0.05 * (END - valuation).total_seconds() / (365 * 86400))
    assert solution.price_many([80, 110, 120]) == pytest.approx(
        (expected,) * 3, rel=0, abs=1e-14
    )


@pytest.mark.parametrize(
    "absorbing", (False, True), ids=("deterministic_variance", "absorbing_zero")
)
def test_heston_degenerate_solutions_preserve_direct_and_one_dimensional_paths(
    absorbing,
):
    from test_heston import model

    snapshot = heston_context(
        model(
            vol_of_vol=0,
            initial_variance=0 if absorbing else 0.04,
            long_run_variance=0 if absorbing else 0.04,
        )
    )
    engine = pricing.FiniteDifferenceHestonVanillaEngine(
        asset_upper_boundary=1 if absorbing else 400, variance_upper_boundary=0.001
    )
    solution = engine.solve(option(), snapshot, spot_range=(80, 120))
    reference_model = BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0 if absorbing else 0.2
    )
    for spot in (80, 100, 120):
        shifted = snapshot.replace(spot_price=spot, model_parameters=reference_model)
        expected = pricing.AnalyticEuropeanEngine().price(option(), shifted)
        assert solution.price(spot) == pytest.approx(
            expected, rel=0, abs=1e-12 if absorbing else 0.003
        )
    terminal = engine.solve(
        option(), snapshot.replace(valuation_time=END), spot_range=(80, 120)
    )
    assert terminal.price_many([80, 100, 120]) == (0, 0, 20)
    assert terminal.price_with_greeks(100, "all").status("delta") == "unsupported"


def test_time_refinement_and_domain_enlargement_preserve_interval_accuracy():
    spots = (80, 93.7, 100, 108.3, 120)
    expected = [
        pricing.AnalyticEuropeanEngine().price(option(), context(spot=spot))
        for spot in spots
    ]
    temporal = [
        pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=1600, time_step_count=t, asset_upper_boundary=400
        ).solve(option(), context(), spot_range=(80, 120))
        for t in (25, 50, 100)
    ]
    errors = [
        max(
            abs(value - ref)
            for value, ref in zip(saved.price_many(spots), expected, strict=True)
        )
        for saved in temporal
    ]
    assert errors[-1] < errors[0]
    assert errors[-1] < 0.001
    domains = [
        pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=n, time_step_count=400, asset_upper_boundary=upper
        ).solve(option(), context(), spot_range=(80, 120))
        for n, upper in ((1600, 400), (2400, 600), (3200, 800))
    ]
    for saved in domains:
        assert saved.price_many(spots) == pytest.approx(expected, rel=0, abs=0.0001)


def test_heston_near_strike_mesh_and_dense_saved_queries_match_analytic_prices():
    snapshot = heston_context()
    engine = pricing.FiniteDifferenceHestonVanillaEngine()
    reference = pricing.AnalyticHestonEuropeanEngine()
    instrument = option()
    witness = snapshot.replace(spot_price=99.79797979797979)
    assert engine.price(instrument, witness) == pytest.approx(
        reference.price(instrument, witness), rel=0, abs=0.005
    )
    solution = engine.solve(instrument, snapshot, spot_range=(80, 120))
    for i in range(100):
        spot = 80 + 40 * i / 99
        expected = reference.price(instrument, snapshot.replace(spot_price=spot))
        assert solution.price(spot) == pytest.approx(expected, rel=0, abs=0.005)


@pytest.mark.parametrize("kind", ("bsm", "local"))
@pytest.mark.parametrize("binary", (False, True), ids=("vanilla", "binary"))
def test_current_scheduled_barrier_events_precede_saved_interpolation(kind, binary):
    valuation, end = datetime(2025, 1, 10), datetime(2025, 1, 20)
    fields = {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "expiry_time": end,
        "barrier_type": "up_and_out",
        "barrier_level": 110,
        "observation_times": [valuation, end],
        "touch_state": "untouched",
    }
    instrument = (
        ins.BinaryBarrierOption(**fields, payoff_type="cash", payout=10)
        if binary
        else ins.BarrierOption(**fields)
    )
    factory = (
        pricing.FiniteDifferenceBinaryBarrierEngine
        if binary
        else pricing.FiniteDifferenceBarrierEngine
    )
    engine = factory(
        asset_step_count=3200, time_step_count=800, asset_upper_boundary=400
    )
    snapshot = context(kind, valuation=valuation)
    solution = engine.solve(instrument, snapshot, spot_range=(90, 120))
    vanilla = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=end
    )
    cash = ins.CashOrNothingOption(
        option_type="call",
        strike=100,
        payout=10 if binary else 1,
        effective_time=START,
        expiry_time=end,
    )
    for spot in (
        90,
        100,
        109.9,
        math.nextafter(110, 0),
        110,
        math.nextafter(110, math.inf),
        120,
    ):
        if spot >= 110:
            assert solution.price(spot) == 0
        else:
            analytic = context(valuation=valuation, spot=spot)
            digital_upper = pricing.AnalyticDigitalEngine().price(
                cash.replace(strike=110), analytic
            )
            if binary:
                expected = (
                    pricing.AnalyticDigitalEngine().price(cash, analytic)
                    - digital_upper
                )
            else:
                reference = pricing.AnalyticEuropeanEngine()
                expected = (
                    reference.price(vanilla, analytic)
                    - reference.price(vanilla.replace(strike=110), analytic)
                    - 10 * digital_upper
                )
            assert solution.price(spot) == pytest.approx(expected, rel=0, abs=0.003)
        assert solution.price(spot) == pytest.approx(
            engine.price(instrument, snapshot.replace(spot_price=spot)),
            rel=0,
            abs=1e-10,
        )
    assert solution.price_with_greeks(110, "all").status("delta") == "undefined"


def test_american_exercise_floor_greeks_follow_the_quoted_price():
    contract = ins.AmericanOption(
        option_type="put", strike=100, effective_time=START, expiry_time=END
    )
    market = context(spot=73.47)
    market = market.replace(
        model_parameters=market.model_parameters.replace(dividend_yield=0)
    )
    engine = pricing.FiniteDifferenceVanillaEngine(
        asset_step_count=40, asset_upper_boundary=400
    )
    solution = engine.solve(contract, market, spot_range=(70, 100))
    h = 0.001
    down, center, up = solution.price_many([73.47 - h, 73.47, 73.47 + h])
    assert center == pytest.approx(26.53, abs=1e-12)
    assert (up - down) / (2 * h) == pytest.approx(-1, abs=1e-9)
    assert (up - 2 * center + down) / h**2 == pytest.approx(0, abs=1e-8)
    for result in (
        engine.price_with_greeks(contract, market, ["delta", "gamma"]),
        solution.price_with_greeks(73.47, ["delta", "gamma"]),
    ):
        assert result.price == center
        assert result.delta == -1
        assert result.gamma == 0
