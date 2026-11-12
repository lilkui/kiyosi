import math
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from functools import partial

import pytest
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    RateTermStructure,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks
from test_accumulator import START, contract, market, terminal_reference


def test_settings_protocols_defaults_exports_and_ownership(subtests):
    assert "FiniteDifferenceAccumulatorEngine" in pricing.__all__
    engine = pricing.FiniteDifferenceAccumulatorEngine()
    assert engine.asset_step_count == 800
    assert engine.time_step_count == 400
    assert engine.scheme == "rannacher"
    assert engine.asset_upper_boundary is None
    for field in ("asset_step_count", "time_step_count"):
        for value in (True, 3.5, "800", None):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                pricing.FiniteDifferenceAccumulatorEngine(**{field: value})
        for value in (2**40, -(2**40)):
            with pytest.raises(OverflowError):
                pricing.FiniteDifferenceAccumulatorEngine(**{field: value})
    for value in (True, "440", object()):
        with pytest.raises(TypeError):
            pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=value)
    with pytest.raises(OverflowError):
        pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=10**400)
    for fields in (
        {"asset_step_count": 2},
        {"time_step_count": 0},
        {"asset_upper_boundary": 0},
        {"asset_upper_boundary": math.nan},
        {"asset_upper_boundary": math.inf},
    ):
        assert_rejection(
            InvalidInputError,
            pricing.FiniteDifferenceAccumulatorEngine,
            **fields,
        )
    with pytest.raises(ValueError):
        pricing.FiniteDifferenceAccumulatorEngine(scheme="unknown")
    with pytest.raises(TypeError):
        pricing.FiniteDifferenceAccumulatorEngine(scheme=True)
    with pytest.raises(TypeError):
        pricing.FiniteDifferenceAccumulatorEngine(extra=1)
    with pytest.raises(AttributeError):
        engine.asset_step_count = 400
    wrong = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=datetime(2025, 2, 3),
    )
    with pytest.raises(UnsupportedOperationError):
        engine.price(wrong, market())
    with pytest.raises(UnsupportedOperationError):
        pricing.implied_volatility(
            engine,
            wrong,
            market(),
            1,
            lower_bound=0.0,
            upper_bound=4.0,
        )


def test_settlement_history_boundaries_and_failure_reuse(subtests):
    engine = pricing.FiniteDifferenceAccumulatorEngine()
    end = datetime(2025, 1, 6)
    option = contract(end)
    for spot, quantity in (
        (90, 5),
        (100, 4),
        (109, 4),
        (110, 3),
        (111, 3),
        (math.nextafter(100, 0), 5),
        (math.nextafter(100, 110), 4),
        (math.nextafter(110, 0), 4),
        (math.nextafter(110, 120), 3),
    ):
        with subtests.test(spot=spot):
            result = engine.price_with_greeks(
                option,
                market(spot, end),
                greeks="all",
            )
            assert result.price == quantity * (spot - 100)
            assert_terminal_greeks(
                result,
                None if min(abs(spot - 100), abs(spot - 110)) <= 0.01 else quantity,
            )
    assert (
        engine.price(
            option,
            market(110, START + timedelta(days=1)),
        )
        == 30
    )
    for value in (-1, math.nan, math.inf):
        assert_rejection(
            InvalidInputError,
            option.replace,
            accumulated_quantity=value,
        )
    for value in (True, "3"):
        with pytest.raises(TypeError):
            option.replace(
                accumulated_quantity=value,
            )
    assert option.accumulated_quantity == 3
    assert (
        option.replace(
            accumulated_quantity=0.25,
        ).accumulated_quantity
        == 0.25
    )
    for instrument, context, expected_error in (
        (
            option,
            market(100, START - timedelta(days=1)),
            InvalidInputError,
        ),
        (
            option,
            market(100, datetime(2025, 1, 6, 0, 0, 0, 1)),
            InvalidInputError,
        ),
        (
            contract(end, quantity=sys.float_info.max),
            market(111, end),
            ComputationFailureError,
        ),
    ):
        assert_rejection(
            expected_error,
            engine.price,
            instrument,
            context,
        )
    assert engine.price(option, market(110, end)) == 30


def test_midnight_intraday_and_calendar_events_are_applied_once():
    engine = pricing.FiniteDifferenceAccumulatorEngine(
        asset_step_count=3200, asset_upper_boundary=440
    )
    option = contract(datetime(2025, 1, 6))
    for spot, quantity in ((90, 5), (100, 4), (109, 4)):
        at = datetime(2025, 1, 3)
        expected = terminal_reference(
            option.replace(
                accumulated_quantity=quantity,
            ),
            market(spot, at),
        )
        assert engine.price(option, market(spot, at)) == pytest.approx(
            expected, rel=0, abs=0.001
        )
    early = market(90, datetime(2025, 1, 3, 0, 0, 0, 1))
    late = market(90, datetime(2025, 1, 3, 12))
    for context in (early, late, market(90, datetime(2025, 1, 4))):
        assert engine.price(option, context) == pytest.approx(
            terminal_reference(option, context), rel=0, abs=0.001
        )
    holiday = custom_calendar(
        base=weekdays_calendar(), closed_dates=[(datetime(2025, 1, 3)).date()]
    )
    context = market(90, datetime(2025, 1, 3))
    option = option.replace(
        calendar=holiday,
        accumulated_quantity=option.accumulated_quantity,
        knock_out_settled=option.knock_out_settled,
    )
    assert engine.price(option, context) == pytest.approx(
        terminal_reference(option, context), rel=0, abs=0.001
    )
    weekend = contract(datetime(2025, 1, 5), calendar=all_days_calendar())
    assert math.isfinite(
        engine.price(
            weekend.replace(calendar=all_days_calendar()),
            market(),
        )
    )


def test_validation_precedes_state_and_bumped_failures_leave_inputs_intact():
    option, context = contract(), market()
    engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=110)
    assert_rejection(InvalidInputError, engine.price, option, context)
    # Immediate settlement needs no spatial domain.
    assert (
        engine.price(
            option,
            market(110, START + timedelta(days=1)),
        )
        == 30
    )
    assert_rejection(
        InvalidInputError,
        engine.price_with_greeks,
        option,
        context,
        [],
    )
    engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=110.1)
    context = market(110.05, valuation=START + timedelta(days=1, hours=12))
    quote = engine.price(option, context)
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        option,
        context,
        "delta",
        spot_shift=-1,
    )
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        option,
        context,
        "speed",
        spot_shift=0.2,
    )
    assert engine.price(option, context) == quote
    assert engine.asset_upper_boundary == 110.1
    assert option.accumulated_quantity == 3
    assert context.spot_price == 110.05
    unstable = pricing.FiniteDifferenceAccumulatorEngine(
        asset_step_count=800, time_step_count=1, scheme="explicit_euler"
    )
    assert_rejection(InvalidInputError, unstable.price, option, market())


def test_native_interpolation_mixed_greeks_and_event_availability():
    option = contract(datetime(2025, 4, 1))
    engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=440)
    context = market(100.25, datetime(2025, 1, 4, 12))
    result = engine.price_with_greeks(option, context, greeks="all")
    h = 0.01
    high, low = (
        engine.price(
            option,
            context.replace(spot_price=100.25 + sign * h),
        )
        for sign in (1, -1)
    )
    assert result.delta == pytest.approx((high - low) / (2 * h), rel=0, abs=1e-6)
    assert result.gamma == pytest.approx(
        (high - 2 * result.price + low) / h**2, rel=0, abs=1e-6
    )
    for greek, dependency, field, bump, scale in (
        ("speed", "gamma", "spot_price", h, 1),
        ("vanna", "delta", "volatility", 0.0001, 1),
        ("zomma", "gamma", "volatility", 0.0001, 1),
    ):
        bumped = []
        center = context.spot_price if field == "spot_price" else 0.2
        actual_bump = (center + bump) - center
        for sign in (1, -1):
            changes = {field: center + sign * actual_bump}
            shifted = (
                context.replace(**changes)
                if field == "spot_price"
                else context.replace(
                    model_parameters=context.model_parameters.replace(**changes)
                )
            )
            bumped.append(
                getattr(
                    engine.price_with_greeks(option, shifted, dependency),
                    dependency,
                )
            )
        assert getattr(result, greek) == pytest.approx(
            (bumped[0] - bumped[1]) / (2 * actual_bump * scale), rel=0, abs=1e-10
        )
    for greek, dependency in (("charm", "delta"), ("color", "gamma")):
        before, after = [
            engine.price_with_greeks(
                option,
                context.replace(
                    valuation_time=context.valuation_time + timedelta(days=offset)
                ),
                dependency,
            )
            for offset in (-1, 1)
        ]
        assert getattr(result, greek) == pytest.approx(
            (getattr(after, dependency) - getattr(before, dependency)) / (2 / 365),
            rel=0,
            abs=1e-10,
        )
    for bump in (200, 1e-300):
        native = numerical_greeks(
            engine,
            option,
            context,
            ["delta", "gamma", "speed"],
            spot_shift=bump,
        )
        assert result.delta is not None
        assert native.delta is None
        assert result.gamma is not None
        assert native.gamma is None
        assert native.speed is None
    for spot in (100, 110):
        boundary = engine.price_with_greeks(
            option,
            market(spot, START + timedelta(days=1)),
            greeks="all",
        )
        for greek in (
            "delta",
            "gamma",
            "speed",
            "vanna",
            "zomma",
            "theta",
            "charm",
            "color",
        ):
            assert getattr(boundary, greek) is None
        assert boundary.vega is not None
        assert boundary.rho is not None
    sunday = engine.price_with_greeks(
        option,
        market(90, datetime(2025, 1, 5, 23, 59, 59)),
        greeks="all",
    )
    assert math.isfinite(sunday.require("theta"))
    small = engine.price_with_greeks(
        option,
        market(90, volatility=0.0001),
        [
            "vega",
            "vanna",
            "zomma",
        ],
        volatility_shift=0.0001,
    )
    assert small.vega is None
    assert small.vanna is None
    assert small.zomma is None


def test_node_curves_terminal_forward_and_coverage_order():
    end = datetime(2026, 1, 1)
    risk = RateTermStructure(
        reference_time=START,
        times=[datetime(2025, 7, 1), end],
        factors=[0.97, 0.94],
    )
    dividend = RateTermStructure(
        reference_time=START,
        times=[datetime(2025, 7, 1), end],
        factors=[0.99, 0.98],
    )
    context = market(100, START).replace(
        model_parameters=market().model_parameters.replace(
            risk_free_rate=risk, dividend_yield=dividend
        )
    )
    option = contract(end, knock_out=399, daily=0, quantity=2).replace(
        effective_time=START - timedelta(days=1),
        accumulated_quantity=2,
        knock_out_settled=False,
    )
    engine = pricing.FiniteDifferenceAccumulatorEngine(
        asset_upper_boundary=400, time_step_count=800
    )
    assert engine.price(option, context) == pytest.approx(
        2 * (98 - 94), rel=0, abs=0.002
    )
    short = RateTermStructure(
        reference_time=START, times=[datetime(2025, 2, 3)], factors=[0.99]
    )
    context = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=short)
    )
    assert_rejection(InvalidInputError, engine.price, option, context)
    assert (
        engine.price(
            option,
            context.replace(spot_price=399),
        )
        == 598
    )


def test_implied_solver_identifiability_protocols_and_failure_state():
    engine = pricing.FiniteDifferenceAccumulatorEngine()
    option = contract(datetime(2025, 1, 6))
    context = market(100, datetime(2025, 1, 3, 12), volatility=0.4)
    quote = engine.price(option, context)
    assert pricing.implied_volatility(
        engine,
        option,
        context,
        quote,
        lower_bound=0.2,
        upper_bound=0.6,
    ) == pytest.approx(0.4, rel=0, abs=1e-6)
    assert_rejection(
        RootNotBracketedError,
        pricing.implied_volatility,
        engine,
        option,
        context,
        1e10,
        lower_bound=0.2,
        upper_bound=0.6,
    )
    assert_rejection(
        NonConvergenceError,
        pricing.implied_volatility,
        engine,
        option,
        context,
        quote,
        lower_bound=0.2,
        upper_bound=0.6,
        max_iterations=1,
        price_tolerance=1e-30,
        parameter_tolerance=1e-30,
    )
    for instrument, scenario in (
        (option, market(110, START + timedelta(days=1))),
        (option, market(90, option.expiry_time)),
        (contract(daily=0, quantity=0), market()),
        (contract(daily=0), market(90, rate=0, dividend=0)),
    ):
        assert_rejection(
            UnsupportedOperationError,
            pricing.implied_volatility,
            engine,
            instrument,
            scenario,
            engine.price(instrument, scenario),
            lower_bound=0.0,
            upper_bound=4.0,
        )
    for value in (None, True, "1"):
        with pytest.raises(TypeError):
            pricing.implied_volatility(
                engine,
                option,
                context,
                value,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    for value in (True, 1.5):
        with pytest.raises(TypeError):
            pricing.implied_volatility(
                engine,
                option,
                context,
                quote,
                max_iterations=value,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    with pytest.raises(OverflowError):
        pricing.implied_volatility(
            engine,
            option,
            context,
            quote,
            max_iterations=2**40,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    assert_rejection(
        InvalidInputError,
        pricing.implied_volatility,
        engine,
        option,
        context,
        math.nan,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert engine.price(option, context) == quote
    assert context.model_parameters.volatility == 0.4


def test_speed_propagates_nonfinite_shifted_prices_and_preserves_inputs():
    engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=440)
    option = contract(datetime(2025, 1, 6), daily=0, quantity=sys.float_info.max)
    context = market(100, datetime(2025, 1, 3, 12))
    quote = engine.price(option, context)
    for spot in (98, 102):
        assert_rejection(
            ComputationFailureError,
            engine.price,
            option,
            context.replace(spot_price=spot),
        )
    assert_rejection(
        ComputationFailureError,
        partial(numerical_greeks, engine),
        option,
        context,
        "speed",
        spot_shift=2,
    )
    assert engine.price(option, context) == quote
    assert option.accumulated_quantity == sys.float_info.max
    assert context.spot_price == 100


def test_quantity_affinity_and_concurrent_shared_engine():
    engine, context = pricing.FiniteDifferenceAccumulatorEngine(), market(90)
    values = [engine.price(contract(quantity=q), context) for q in (0, 3, 6)]
    assert values[2] - values[1] == pytest.approx(
        values[1] - values[0], rel=0, abs=1e-10
    )
    assert engine.price(contract(daily=0, quantity=0), context) == 0
    assert engine.price(
        contract(daily=0),
        market(90, rate=0, dividend=0),
    ) == (-30)
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(
            pool.map(
                lambda _: engine.price(contract(), context),
                range(2),
            )
        )
    assert results == [values[1]] * 2


def test_quantlib_terminal_portfolios_and_separate_grid_convergence(subtests):

    from support.structured import contract, market, references, settings

    rows = [
        row
        for row in references()
        if row["engine"] == "FiniteDifferenceAccumulatorEngine"
    ]
    assert len(rows) == 18
    for row in rows:
        option, context, fine = contract(row), market(row), settings(row)
        budget = float(row["tolerances"]["price"]) + float(
            row["inputs"]["uncertainty_price"]
        )
        with subtests.test(case=row["case_id"]):
            refined = pricing.FiniteDifferenceAccumulatorEngine(**fine).price(
                option, context
            )
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
                    assert pricing.FiniteDifferenceAccumulatorEngine(
                        **configuration
                    ).price(option, context) == pytest.approx(
                        refined, rel=0, abs=budget
                    )
