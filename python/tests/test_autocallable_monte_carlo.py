"""Autocallable CPU sampling through the installed Rust extension."""

import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import pytest
from kiyosi import (
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import BinarySnowballOption, autocallable_observations
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
from support.history import dated as dated_history
from test_phoenix import contract as phoenix
from test_snowball import END, START, contract, market


def test_terminal_price_matches_quadrature_with_sampling_error():
    note, context = contract(), market()
    engine = MC(path_count=32768, seed=42)
    result = engine.price_with_statistics(dated_history(note, context), context)
    assert result.price == engine.price(dated_history(note, context), context)
    assert result.price == pytest.approx(
        QuadratureAutocallableEngine().price(dated_history(note, context), context),
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
        lambda: engine.price(dated_history(object(), context), context),
        lambda: engine.price_with_statistics(dated_history(object(), context), context),
        lambda: numerical_greeks(
            engine, dated_history(note, context), context, "delta", spot_shift=True
        ),
        lambda: implied_volatility(
            engine,
            dated_history(note, context),
            context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: implied_volatility(
            engine,
            dated_history(note, context),
            context,
            1,
            max_iterations=2.5,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: implied_phoenix_coupon(
            engine, dated_history(note, context), context, True
        ),
        lambda: implied_phoenix_coupon(
            engine, dated_history(note, context), context, 1, max_iterations=True
        ),
        lambda: implied_phoenix_coupon(
            engine,
            dated_history(note, context),
            context,
            1,
            quote_convention="shift_maturity_coupon",
        ),
    ):
        with pytest.raises(TypeError):
            operation()
    assert_rejection(
        InvalidInputError,
        lambda: numerical_greeks(
            engine, dated_history(note, context), context, "delta", spot_shift=0
        ),
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
        lambda: engine.price(dated_history(note, curved), curved),
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: engine.price_with_greeks(dated_history(note, curved), curved, []),
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
                    dated_history(note, context), context
                )
                engine = MC(**fields, max_active_workers=3)
                with ThreadPoolExecutor(max_workers=2) as pool:
                    results = list(
                        pool.map(
                            lambda _, engine=engine: engine.price_with_statistics(
                                dated_history(note, context), context
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
        .price_with_statistics(dated_history(note, context), context)
        .standard_error
        is None
    )


def test_current_phoenix_coupon_paid_once_and_weekend_does_not_knock_in():
    engine = MC(path_count=32768, seed=42)
    # Tiny volatility makes every future observation coupon deterministic.
    note = phoenix(
        observations=autocallable_observations(
            [datetime(2025, 2, 3), END], [200, 200], coupon_barrier_levels=[90, 90]
        )
    )
    at_event = market(100, datetime(2025, 2, 3), volatility=1e-12, rate=0, dividend=0)
    assert engine.price(dated_history(note, at_event), at_event) == pytest.approx(
        1 + 2 * 0.01, rel=0, abs=1e-14
    )
    after_event = at_event.replace(valuation_time=datetime(2025, 2, 3, 0, 0, 0, 1))
    assert engine.price(dated_history(note, after_event), after_event) == pytest.approx(
        1 + 0.01, rel=0, abs=1e-14
    )
    weekend = datetime(2025, 1, 4)
    snow = contract(
        expiry_time=datetime(2025, 1, 6),
        maturity_coupon_rate=0.12,
        knock_in_observation_mode="every_trading_day",
        barrier_state="no_event",
        observations=autocallable_observations(
            [datetime(2025, 1, 6)], [120], knock_out_coupon_rates=[0.12]
        ),
    )
    first = engine.price(
        dated_history(snow, market(79, weekend, volatility=0.2)),
        market(79, weekend, volatility=0.2),
    )
    later = engine.price(
        dated_history(
            snow, market(79, weekend + timedelta(microseconds=1), volatility=0.2)
        ),
        market(79, weekend + timedelta(microseconds=1), volatility=0.2),
    )
    assert first == pytest.approx(later, rel=0, abs=1e-7)


def test_solvers_reject_fixed_cashflows_and_preserve_state():
    engine, note, context = MC(path_count=4096, seed=42), phoenix(), market()
    snapshot = repr(note), repr(context)
    quote = engine.price(dated_history(note, context), context)
    sigma = implied_volatility(
        engine,
        dated_history(note, context),
        context,
        quote,
        lower_bound=0.2,
        upper_bound=0.4,
    )
    assert sigma == pytest.approx(0.3, rel=0, abs=1e-6)
    coupon = implied_phoenix_coupon(
        engine,
        dated_history(note, context),
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
        observations=autocallable_observations(
            [datetime(2025, 2, 3), END], [101, 105], coupon_barrier_levels=[0, 0]
        ),
    )
    with pytest.raises(InvalidInputError):
        implied_phoenix_coupon(
            unseeded, dated_history(exposed, market(rate=0)), market(rate=0), 1.01
        )
    for _ in range(3):
        replay = unseeded.with_resolved_seed()
        solved = implied_phoenix_coupon(
            replay,
            dated_history(exposed, market(rate=0)),
            market(rate=0),
            1.01,
            lower_bound=0,
            upper_bound=0.5,
            price_tolerance=1e-12,
        )
        assert solved["coupon_per_period"] > 0
        assert solved["coupon_per_period"] < 0.5
        assert replay.price(
            dated_history(exposed.replace(**solved), market(rate=0)),
            market(rate=0),
        ) == pytest.approx(1.01, rel=0, abs=1e-12)
    assert unseeded.seed is None
    fixed = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=START,
        expiry_time=END,
        observations=autocallable_observations(
            [END], [110], knock_out_coupon_rates=[0]
        ),
    )
    estimate = MC(path_count=1).price_with_statistics(
        dated_history(fixed, market(rate=0)), market(rate=0)
    )
    assert (
        (estimate.price, estimate.standard_error, estimate.confidence_interval)
    ) == ((1, 0, (1, 1)))
    assert_rejection(
        UnsupportedOperationError,
        lambda: implied_volatility(
            engine,
            dated_history(fixed, market(rate=0)),
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
            dated_history(fixed, market(100, END)),
            market(100, END),
            1,
            quote_convention="preserve_maturity_coupon",
        ),
    )
    assert_rejection(
        InvalidInputError,
        lambda: implied_phoenix_coupon(
            engine, dated_history(note, context), context, math.nan
        ),
    )
    assert_rejection(
        RootNotBracketedError,
        lambda: implied_phoenix_coupon(
            engine, dated_history(note, context), context, 100
        ),
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
            result = engine.price_with_statistics(
                dated_history(contract(row), market(row)), market(row)
            )
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
        "observations": autocallable_observations(
            [end], [110], knock_out_coupon_rates=[0.08]
        ),
    }
    engine = MC(path_count=2, seed=42, sampling="sobol")
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(
            dated_history(
                contract(**fields, knock_in_observation_mode="every_trading_day"),
                market(),
            ),
            market(),
        ),
    )
    assert math.isfinite(
        engine.price(dated_history(contract(**fields), market()), market())
    )
