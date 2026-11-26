"""Autocallable CPU sampling through the installed Rust extension."""

import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import (
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    BinarySnowballOption,
    CouponObservation,
    KnockInObservation,
    KnockOutObservation,
    autocallable_observations,
)
from kiyosi.market import RateTermStructure, weekdays_calendar
from kiyosi.pricing import MonteCarloAutocallableEngine as MC
from kiyosi.pricing import (
    QuadratureAutocallableEngine,
    implied_phoenix_coupon,
    implied_snowball_coupon,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from test_phoenix import contract as phoenix
from test_snowball import END, START, contract, market


@pytest.mark.parametrize("sampling", ("pseudo_random", "sobol"))
def test_portfolio_prices_replay_scalar_paths(sampling):
    note = contract(
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        autocall_observations=autocallable_observations(
            [datetime(2025, 2, 3), datetime(2025, 3, 3), END],
            [103, 108, 105],
            knock_out_coupon_rates=[0.08] * 3,
        ),
    )
    variation = note.replace(maturity_coupon_rate=0.09, knock_in_level=95)
    context = market()
    engine = MC(path_count=2048, seed=42, sampling=sampling, antithetic=True)
    notes = (variation, note, variation)
    assert engine.price_many(iter(notes), context) == tuple(
        engine.price(item, context) for item in notes
    )
    early = note.replace(
        autocall_observations=autocallable_observations(
            [datetime(2025, 2, 3), datetime(2025, 3, 3), END],
            [85] * 3,
            knock_out_coupon_rates=[0.08] * 3,
        )
    )
    staggered = (note,) + (early,) * 32
    assert engine.price_many(staggered, context) == tuple(
        engine.price(item, context) for item in staggered
    )
    different_grid = note.replace(knock_in_observation=KnockInObservation.at_expiry())
    interleaved = (variation, different_grid, note) * 13
    expected = tuple(engine.price(item, context) for item in interleaved)
    assert engine.price_many(iter(interleaved), context) == expected
    assert (
        engine.replace(max_active_workers=1).price_many(interleaved, context)
        == expected
    )
    assert engine.price_many([], context) == ()
    with pytest.raises(UnsupportedOperationError):
        engine.price_many((note, phoenix()), context)


def test_terminal_price_matches_quadrature_with_sampling_error():
    note, context = contract(), market()
    engine = MC(path_count=32768, seed=42)
    result = engine.price_with_statistics(note, context)
    assert result.price == engine.price(note, context)
    assert result.price == pytest.approx(
        QuadratureAutocallableEngine().price(note, context),
        rel=0,
        abs=4 * result.standard_error,
    )
    lower, upper = result.confidence_interval
    assert lower < result.price
    assert upper > result.price


def test_defaults_strict_representations_and_validation(subtests):
    engine = MC()
    assert (
        (
            engine.path_count,
            engine.seed,
            engine.max_active_workers,
            engine.sampling,
            engine.antithetic,
        )
    ) == ((131072, None, 0, "pseudo_random", False))
    for field in ("path_count", "max_active_workers", "seed"):
        for value, error in (
            (True, TypeError),
            (2.5, TypeError),
            (2**80, OverflowError),
        ):
            with subtests.test(field=field, value=value), pytest.raises(error):
                MC(**{field: value})
    for fields in ({"antithetic": 1}, {"sampling": 1}, {"unknown": 1}):
        with pytest.raises(TypeError):
            MC(**fields)
    with pytest.raises(ValueError):
        MC(sampling="invalid")
    for fields in (
        {"path_count": 0},
        {"max_active_workers": -1},
        {"path_count": 31, "sampling": "sobol"},
    ):
        assert_rejection(InvalidInputError, lambda fields=fields: MC(**fields))
    with pytest.raises(AttributeError):
        engine.seed = 42
    note, context = phoenix(), market()
    for operation in (
        lambda: engine.price(object(), context),
        lambda: engine.price_with_statistics(object(), context),
        lambda: numerical_greeks(engine, note, context, "delta", spot_shift=True),
        lambda: implied_volatility(
            engine,
            note,
            context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: implied_volatility(
            engine,
            note,
            context,
            1,
            max_iterations=2.5,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: implied_phoenix_coupon(engine, note, context, True),
        lambda: implied_phoenix_coupon(engine, note, context, 1, max_iterations=True),
        lambda: implied_phoenix_coupon(
            engine,
            note,
            context,
            1,
            quote_convention="shift_maturity_coupon",
        ),
    ):
        with pytest.raises(TypeError):
            operation()
    assert_rejection(
        InvalidInputError,
        lambda: numerical_greeks(engine, note, context, "delta", spot_shift=0),
    )
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.99, (0.99) * 0.5],
    )
    curved = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=curve)
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: engine.price(note, curved),
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: engine.price_with_greeks(note, curved, []),
    )


def test_statistics_counts_sobol_and_shared_engine_concurrency():
    note, context = phoenix(), market()
    for sampling in ("pseudo_random", "sobol"):
        for antithetic in (False, True):
            for paths in (1, 2, 32, 1024):
                fields = {
                    "path_count": paths,
                    "seed": 2**64 - 1,
                    "sampling": sampling,
                    "antithetic": antithetic,
                }
                serial = MC(**fields, max_active_workers=1).price_with_statistics(
                    note, context
                )
                engine = MC(**fields, max_active_workers=3)
                with ThreadPoolExecutor(max_workers=2) as pool:
                    results = list(
                        pool.map(
                            lambda _, engine=engine: engine.price_with_statistics(
                                note, context
                            ),
                            range(2),
                        )
                    )
                for result in results:
                    assert (
                        (
                            result.price,
                            result.standard_error,
                            result.confidence_interval,
                        )
                    ) == (
                        (
                            serial.price,
                            serial.standard_error,
                            serial.confidence_interval,
                        )
                    )
                assert (serial.standard_error is not None) == (
                    sampling == "pseudo_random" and paths > (2 if antithetic else 1)
                )
    assert (
        MC(path_count=31, antithetic=True, seed=42)
        .price_with_statistics(note, context)
        .standard_error
        is None
    )


def test_current_phoenix_coupon_paid_once_and_weekend_does_not_knock_in():
    engine = MC(path_count=32768, seed=42)
    # Tiny volatility makes every future observation coupon deterministic.
    note = phoenix(
        knock_out_observations=[
            KnockOutObservation(at, level)
            for at, level in zip([datetime(2025, 2, 3), END], [200, 200], strict=True)
        ],
        coupon_observations=[
            CouponObservation(at, level)
            for at, level in zip([datetime(2025, 2, 3), END], [90, 90], strict=True)
        ],
    )
    at_event = market(100, datetime(2025, 2, 3), volatility=1e-12, rate=0, dividend=0)
    assert engine.price(note, at_event) == pytest.approx(1 + 2 * 0.01, rel=0, abs=1e-14)
    after_event = at_event.replace(valuation_time=datetime(2025, 2, 3, 0, 0, 0, 1))
    assert engine.price(note, after_event) == pytest.approx(1 + 0.01, rel=0, abs=1e-14)
    weekend = datetime(2025, 1, 4)
    snow = contract(
        expiry_time=datetime(2025, 1, 6),
        maturity_coupon_rate=0.12,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        barrier_state="no_event",
        autocall_observations=autocallable_observations(
            [datetime(2025, 1, 6)], [120], knock_out_coupon_rates=[0.12]
        ),
    )
    first = engine.price(
        snow,
        market(79, weekend, volatility=0.2),
    )
    later = engine.price(
        snow,
        market(79, weekend + timedelta(microseconds=1), volatility=0.2),
    )
    assert first == pytest.approx(later, rel=0, abs=1e-7)


def test_solvers_reject_fixed_cashflows_and_preserve_state():
    engine, note, context = MC(path_count=4096, seed=42), phoenix(), market()
    snapshot = repr(note), repr(context)
    quote = engine.price(note, context)
    sigma = implied_volatility(
        engine,
        note,
        context,
        quote,
        lower_bound=0.2,
        upper_bound=0.4,
    )
    assert sigma == pytest.approx(0.3, rel=0, abs=1e-6)
    coupon = implied_phoenix_coupon(
        engine,
        note,
        context,
        quote,
        lower_bound=-0.1,
        upper_bound=0.5,
    )
    assert set(coupon) == {"coupon_per_period"}
    assert coupon["coupon_per_period"] == pytest.approx(
        note.coupon_per_period, rel=0, abs=1e-8
    )
    assert ((repr(note), repr(context))) == snapshot
    unseeded = MC(path_count=31)
    exposed = phoenix(
        lower_strike=100,
        upper_strike=100,
        knock_out_observations=[
            KnockOutObservation(at, level)
            for at, level in zip([datetime(2025, 2, 3), END], [101, 105], strict=True)
        ],
        coupon_observations=[
            CouponObservation(at, level)
            for at, level in zip([datetime(2025, 2, 3), END], [0, 0], strict=True)
        ],
    )
    zero_rate = market(rate=0)
    replay = unseeded.with_resolved_seed()
    expected_coupon = 0.01
    quote = replay.price(
        exposed.with_coupon(coupon_per_period=expected_coupon), zero_rate
    )
    with pytest.raises(InvalidInputError, match="with_resolved_seed"):
        implied_phoenix_coupon(unseeded, exposed, zero_rate, quote)
    solved = implied_phoenix_coupon(
        replay,
        exposed,
        zero_rate,
        quote,
        lower_bound=0,
        upper_bound=0.5,
        price_tolerance=1e-12,
    )
    assert solved["coupon_per_period"] == pytest.approx(
        expected_coupon, rel=0, abs=1e-12
    )
    assert replay.price(exposed.replace(**solved), zero_rate) == pytest.approx(
        quote, rel=0, abs=1e-12
    )
    assert unseeded.seed is None
    fixed = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=START,
        expiry_time=END,
        autocall_observations=autocallable_observations(
            [END], [110], knock_out_coupon_rates=[0]
        ),
    )
    estimate = MC(path_count=1).price_with_statistics(fixed, market(rate=0))
    assert (
        (estimate.price, estimate.standard_error, estimate.confidence_interval)
    ) == ((1, 0, (1, 1)))
    assert_rejection(
        UnsupportedOperationError,
        lambda: implied_volatility(
            engine,
            fixed,
            market(rate=0),
            1,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: implied_snowball_coupon(
            engine,
            fixed,
            market(100, END),
            1,
            quote_convention="preserve_maturity_coupon",
        ),
    )
    assert_rejection(
        InvalidInputError,
        lambda: implied_phoenix_coupon(engine, note, context, math.nan),
    )
    assert_rejection(
        RootNotBracketedError,
        lambda: implied_phoenix_coupon(engine, note, context, 100),
    )


def test_quantlib_terminal_payoff_portfolios(subtests):
    from support.structured import contract, market, references

    rows = [
        row for row in references() if row["engine"] == "QuadratureAutocallableEngine"
    ]
    assert ({row["instrument"] for row in rows}) == {
        "SnowballOption",
        "BinarySnowballOption",
        "TernarySnowballOption",
        "PhoenixOption",
    }
    engine = MC(path_count=65536, seed=42, antithetic=True)
    for row in rows:
        with subtests.test(case=row["case_id"]):
            result = engine.price_with_statistics(contract(row), market(row))
            assert result.price == pytest.approx(
                float(row["outputs"]["price"]),
                rel=0,
                abs=4 * result.standard_error
                + float(row["inputs"]["uncertainty_price"])
                + 1e-10,
            )


def test_sobol_dimension_limit_uses_remaining_monitoring_dates():
    end = datetime(2110, 1, 1)
    fields = {
        "expiry_time": end,
        "knock_in_level": 1e-100,
        "autocall_observations": autocallable_observations(
            [end], [1e100], knock_out_coupon_rates=[0.08]
        ),
    }
    engine = MC(path_count=2, seed=42, sampling="sobol")
    with pytest.raises(InvalidInputError, match="Sobol dimension"):
        engine.price(
            contract(
                **fields,
                knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            ),
            market(),
        )

    # Both seeded paths finish between the deliberately distant barriers.
    years = (end - START).days / 365
    expected = (1 + 0.06 * years) * math.exp(-0.04 * years)
    assert engine.price(contract(**fields), market()) == pytest.approx(
        expected, rel=0, abs=1e-12
    )


@pytest.mark.parametrize("sampling", ["pseudo_random", "sobol"])
def test_batched_phoenix_spot_greeks_replay_scalar_stencils(sampling):
    first = datetime(2025, 2, 3)
    note = phoenix(
        coupon_per_period=0.02,
        knock_in_level=90,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        knock_out_observations=[
            KnockOutObservation(first, 101),
            KnockOutObservation(END, 105),
        ],
        coupon_observations=[
            CouponObservation(first, 99),
            CouponObservation(END, 102),
        ],
    )
    engine = MC(
        path_count=2048,
        seed=31,
        sampling=sampling,
        antithetic=True,
        max_active_workers=2,
    )
    context = market(100, START)
    greeks = ["delta", "gamma", "vega"]
    batch = engine.price_with_greeks(note, context, greeks, spot_shift=4)
    scalar = numerical_greeks(engine, note, context, greeks, spot_shift=4)
    assert (batch.price, batch.delta, batch.gamma, batch.vega, batch.seed) == (
        scalar.price,
        scalar.delta,
        scalar.gamma,
        scalar.vega,
        scalar.seed,
    )
    assert all(math.isfinite(value) for value in (batch.delta, batch.gamma, batch.vega))


def test_shared_daily_grid_prices_staggered_cash_flows():
    first = contract(
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        autocall_observations=autocallable_observations(
            [datetime(2025, 2, 3), datetime(2025, 3, 3), END],
            [103, 108, 105],
            knock_out_coupon_rates=[0.08] * 3,
        ),
    )
    second = first.replace(
        autocall_observations=autocallable_observations(
            [datetime(2025, 2, 4), datetime(2025, 3, 4), END],
            [106, 103, 99],
            knock_out_coupon_rates=[0.12, 0.09, 0.06],
        ),
    )
    notes = (second, first, second)
    context = market()
    engine = MC(path_count=16384, seed=42, max_active_workers=1)
    actual = engine.price_many(notes, context, shared_grid=True)
    assert actual[0] == actual[2]
    assert actual != engine.price_many(notes, context)
    assert actual == engine.replace(max_active_workers=2).price_many(
        notes, context, shared_grid=True
    )
    off_clock = first.replace(
        autocall_observations=autocallable_observations(
            [datetime(2025, 2, 3, 1), datetime(2025, 3, 3), END],
            [103, 108, 105],
            knock_out_coupon_rates=[0.08] * 3,
        )
    )
    for sampler in (engine, engine.replace(sampling="sobol", antithetic=True)):
        dense = sampler.price_many((off_clock, off_clock), context, shared_grid=True)
        sparse = sampler.price_many(notes, context, shared_grid=True)
        mixed = (second, off_clock, first, off_clock, second)
        assert sampler.price_many(mixed, context, shared_grid=True) == (
            sparse[0],
            dense[0],
            sparse[1],
            dense[1],
            sparse[2],
        )
        assert sampler.price_many((first,), context, shared_grid=True) == (
            sampler.price(first, context),
        )
    oracle = QuadratureAutocallableEngine()
    for price, note in zip(actual, notes, strict=True):
        estimate = engine.price_with_statistics(note, context)
        assert estimate.standard_error is not None
        assert abs(price - oracle.price(note, context)) < 5 * estimate.standard_error
    assert engine.price_many([], context, shared_grid=True) == ()
    with pytest.raises(TypeError):
        engine.price_many(notes, context, shared_grid=1)
