"""Rust FD defaults, representation boundaries, history and solver contracts."""

import math
from datetime import datetime
from datetime import time as clock_time

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    BinarySnowballOption,
    KnockInObservation,
    autocallable_observations,
)
from kiyosi.market import RateTermStructure, weekdays_calendar
from kiyosi.pricing import FiniteDifferenceAutocallableEngine as FD
from kiyosi.pricing import (
    implied_phoenix_coupon,
    implied_snowball_coupon,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from test_phoenix import contract as phoenix
from test_snowball import END, START, market
from test_snowball import contract as snowball


def test_engine_defaults_and_representation_errors(subtests):
    engine = FD()
    assert ((engine.asset_step_count, engine.time_step_count)) == ((800, 400))
    assert engine.scheme == "rannacher"
    assert engine.asset_upper_boundary is None
    for field in ("asset_step_count", "time_step_count"):
        for value, error in (
            (True, TypeError),
            (2.5, TypeError),
            (2**40, OverflowError),
        ):
            with subtests.test(field=field, value=value), pytest.raises(error):
                FD(**{field: value})
    for fields in ({"scheme": 1}, {"asset_upper_boundary": True}, {"unknown": 1}):
        with pytest.raises(TypeError):
            FD(**fields)
    with pytest.raises(ValueError):
        FD(scheme="unknown")
    for fields in (
        {"time_step_count": 0},
        {"asset_step_count": 2},
        {"asset_upper_boundary": math.nan},
    ):
        assert_rejection(InvalidInputError, lambda fields=fields: FD(**fields))
    with pytest.raises(AttributeError):
        engine.time_step_count = 1
    with pytest.raises(TypeError):
        engine.price(object(), market())
    note = phoenix()
    for operation in (
        lambda: numerical_greeks(
            engine,
            note,
            market(),
            "delta",
            spot_shift=True,
        ),
        lambda: implied_volatility(
            engine,
            note,
            market(),
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: implied_volatility(
            engine,
            note,
            market(),
            1,
            max_iterations=2.5,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: implied_phoenix_coupon(engine, note, market(), True),
        lambda: implied_phoenix_coupon(engine, note, market(), 1, max_iterations=True),
        lambda: implied_phoenix_coupon(
            engine,
            note,
            market(),
            1,
            quote_convention="shift_maturity_coupon",
        ),
    ):
        with pytest.raises(TypeError):
            operation()
    with pytest.raises(OverflowError):
        implied_phoenix_coupon(engine, note, market(), 1, max_iterations=2**40)


@pytest.mark.parametrize("coupon", (0, 0.1))
@pytest.mark.parametrize("shift", (200, 1e-300))
def test_native_spot_greeks_survive_infeasible_scalar_bumps(coupon, shift):
    note = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=START,
        expiry_time=END,
        observations=autocallable_observations(
            [END], [100], knock_out_coupon_rates=[coupon]
        ),
    )
    engine, context = FD(), market()
    reference = engine.price_with_greeks(note, context, ["delta", "gamma"])
    result = numerical_greeks(
        engine,
        note,
        context,
        ["delta", "gamma", "speed"],
        spot_shift=shift,
    )
    assert reference.delta is not None
    assert reference.gamma is not None
    assert result.delta is None
    assert result.gamma is None
    assert result.speed is None


def test_explicit_uniform_stability_uses_the_top_node_bound():
    end = datetime(2026, 1, 1)
    note = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=START,
        expiry_time=end,
        observations=autocallable_observations(
            [datetime(2025, 2, 3), end],
            [100, 100],
            knock_out_coupon_rates=[0.1, 0.1],
        ),
    )
    engine = FD(
        asset_step_count=11,
        time_step_count=10,
        scheme="explicit_euler",
        asset_upper_boundary=400,
    )
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(note, market()),
    )


def test_event_greeks_node_curves_and_failed_replacement():
    engine = FD()
    note = phoenix()
    context = market(90, datetime(2025, 2, 3))
    result = engine.price_with_greeks(note, context, greeks="all")
    assert result.delta is None
    assert result.gamma is None
    assert result.speed is None
    assert result.theta is None
    assert result.vega is not None
    assert result.rho is not None
    snapshot = note.observations
    with pytest.raises(ValueError):
        note.replace(
            barrier_state="invalid",
        )
    assert note.observations == tuple(snapshot)
    context = market()
    curve = RateTermStructure(
        reference_time=START, times=[END], factors=[math.exp(-0.04 * 90 / 365)]
    )
    curved = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=curve)
    )
    assert engine.price(note, curved) == pytest.approx(
        engine.price(note, context), rel=0, abs=1e-12
    )
    short = RateTermStructure(
        reference_time=START, times=[datetime(2025, 2, 3)], factors=[0.99]
    )
    original_price = engine.price(note, context)
    invalid = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=short)
    )
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(note, invalid),
    )
    assert engine.price(note, context) == original_price


def test_fixed_prices_and_unidentifiable_parameters_validate_first():
    note = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=START,
        expiry_time=END,
        observations=autocallable_observations(
            [END], [110], knock_out_coupon_rates=[0]
        ),
    )
    context = market(rate=0)
    engine = FD(asset_upper_boundary=1)
    result = engine.price_with_greeks(note, context, ["delta", "gamma", "vega"])
    assert (
        (
            result.price,
            result.delta,
            result.gamma,
            result.vega,
        )
    ) == ((1, 0, 0, 0))
    assert_rejection(
        UnsupportedOperationError,
        lambda: implied_volatility(
            engine,
            note,
            context,
            1,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: implied_snowball_coupon(
            engine,
            note,
            market(100, END),
            1,
            quote_convention="preserve_maturity_coupon",
        ),
    )
    assert_rejection(
        InvalidInputError,
        lambda: implied_snowball_coupon(
            engine,
            note,
            context,
            math.nan,
            quote_convention="preserve_maturity_coupon",
        ),
    )
    missing = snowball(
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        barrier_state=None,
    )
    later = market(100, datetime(2025, 2, 3))
    assert_rejection(
        MissingHistoryError,
        lambda: engine.price(missing, later),
    )
    assert missing.barrier_state is None


def test_terminal_portfolios_and_separate_grid_convergence(subtests):
    from support.structured import contract, market, references, settings

    rows = [
        row
        for row in references()
        if row["engine"] == "FiniteDifferenceAutocallableEngine"
    ]
    assert ({row["instrument"] for row in rows}) == {
        "SnowballOption",
        "BinarySnowballOption",
        "TernarySnowballOption",
        "PhoenixOption",
    }
    for row in rows:
        note, context, fine = contract(row), market(row), settings(row)
        budget = float(row["tolerances"]["price"]) + float(
            row["inputs"]["uncertainty_price"]
        )
        with subtests.test(case=row["case_id"]):
            refined = FD(**fine).price(note, context)
            assert refined == pytest.approx(
                float(row["outputs"]["price"]), rel=0, abs=budget
            )
            if row["inputs"]["convergence"]:
                for configuration in (
                    {**fine, "asset_step_count": fine["asset_step_count"] // 2},
                    {**fine, "time_step_count": fine["time_step_count"] // 2},
                    {
                        **fine,
                        "asset_step_count": fine["asset_step_count"] * 2,
                        "asset_upper_boundary": fine["asset_upper_boundary"] * 2,
                    },
                ):
                    with subtests.test(settings=configuration):
                        assert FD(**configuration).price(
                            note, context
                        ) == pytest.approx(refined, rel=0, abs=budget)
