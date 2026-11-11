"""Accumulator CPU Monte Carlo through the installed Rust extension."""

import math
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import Accumulator
from kiyosi.market import (
    RateTermStructure,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import MonteCarloAccumulatorEngine as MC
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.history import dated as dated_history
from support.terminal import assert_terminal_greeks
from test_accumulator import START, contract, market, terminal_reference


def test_scalar_price_survives_confidence_interval_overflow():
    note = contract(
        datetime(2025, 1, 2), knock_out=200, acceleration=1, daily=0, quantity=1
    )
    note = note.replace(
        effective_time=START - timedelta(days=1),
        accumulated_quantity=note.accumulated_quantity,
        knock_out_settled=note.knock_out_settled,
        history_time=(note.history_time or note.effective_time)
        if note.accumulated_quantity is not None or note.knock_out_settled is not None
        else None,
    )
    context = market(spot=100, valuation=START, volatility=1, rate=0.01, dividend=0)
    engine = MC(path_count=2, seed=0, max_active_workers=1)
    quantity = 6e307
    expected = (
        engine.price_with_statistics(dated_history(note, context), context).price
        * quantity
    )
    scaled = note.replace_history(
        accumulated_quantity=quantity,
        history_time=(note.history_time or note.effective_time)
        if quantity is not None
        else None,
    )
    assert math.isfinite(expected)
    assert expected > 0
    assert engine.price(
        dated_history(scaled, context), context
    ) / expected == pytest.approx(1, rel=0, abs=1e-12)
    with pytest.raises(ComputationFailureError):
        engine.price_with_statistics(dated_history(scaled, context), context)


@pytest.mark.parametrize("spot,expected", ((90, -50), (100, 0), (105, 20), (110, 30)))
def test_current_events_settle_before_sampling(spot, expected):
    end = datetime(2025, 1, 6)
    engine = MC()
    result = engine.price_with_statistics(
        dated_history(contract(end), market(spot, end)), market(spot, end)
    )
    assert result.price == expected
    assert result.standard_error == 0
    assert result.confidence_interval == ((expected, expected))


def test_terminal_and_multidate_prices_match_independent_references():
    engine = MC(path_count=32768, seed=42)
    for option, context, reference in (
        (
            contract(datetime(2025, 1, 6)),
            market(valuation=datetime(2025, 1, 3, 12)),
            terminal_reference,
        ),
        (
            contract(),
            market(99, START + timedelta(days=1)),
            pricing.QuadratureAccumulatorEngine().price,
        ),
        (contract(), market(111), pricing.QuadratureAccumulatorEngine().price),
    ):
        result = engine.price_with_statistics(dated_history(option, context), context)
        assert result.price == engine.price(dated_history(option, context), context)
        assert result.standard_error > 0
        assert result.price == pytest.approx(
            reference(dated_history(option, context), context),
            rel=0,
            abs=4 * result.standard_error + 0.02,
        )
        lower, upper = result.confidence_interval
        assert upper - lower == pytest.approx(
            2 * 1.959963984540054 * result.standard_error, rel=0, abs=1e-12
        )


def test_strict_settings_protocols_defaults_and_ownership(subtests):
    assert "MonteCarloAccumulatorEngine" in pricing.__all__
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
    with pytest.raises(OverflowError):
        MC(seed=-1)
    for fields in ({"antithetic": 1}, {"sampling": 1}, {"unknown": 1}):
        with pytest.raises(TypeError):
            MC(**fields)
    with pytest.raises(ValueError):
        MC(sampling="invalid")
    for fields in (
        {"path_count": 0},
        {"path_count": 10000001},
        {"max_active_workers": -1},
        {"path_count": 31, "sampling": "sobol"},
    ):
        assert_rejection(InvalidInputError, lambda fields=fields: MC(**fields))
    with pytest.raises(AttributeError):
        engine.seed = 42
    note, context = contract(), market()
    for operation in (
        lambda: engine.price(dated_history(object(), context), context),
        lambda: engine.price_with_statistics(dated_history(object(), context), context),
        lambda: numerical_greeks(
            engine, dated_history(note, context), context, "delta", spot_shift=True
        ),
        lambda: pricing.implied_volatility(
            engine,
            dated_history(object(), context),
            context,
            1,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: pricing.implied_volatility(
            engine,
            dated_history(note, context),
            context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
        lambda: pricing.implied_volatility(
            engine,
            dated_history(note, context),
            context,
            1,
            max_iterations=2.5,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
    ):
        with pytest.raises(TypeError):
            operation()


def test_counts_sobol_antithetic_and_shared_engine_threads():
    note, context = contract(), market()
    for sampling in ("pseudo_random", "sobol"):
        for antithetic in (False, True):
            for paths in (1, 2, 4, 1024):
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
    odd = MC(path_count=31, seed=42, antithetic=True).price_with_statistics(
        dated_history(note, context), context
    )
    assert odd.standard_error is None
    assert odd.confidence_interval is None
    reference = pricing.QuadratureAccumulatorEngine().price(
        dated_history(note, context), context
    )
    for antithetic in (False, True):
        result = MC(
            path_count=16384, seed=42, sampling="sobol", antithetic=antithetic
        ).price_with_statistics(dated_history(note, context), context)
        assert result.price == pytest.approx(reference, rel=0, abs=1)
        assert result.standard_error is None


def test_midnight_intraday_calendar_and_omitted_history():
    engine = MC(path_count=2, seed=42)
    note = contract(datetime(2025, 1, 6))
    holiday = custom_calendar(
        base=weekdays_calendar(), closed_dates=[(datetime(2025, 1, 3)).date()]
    )
    for context, calendar, expected in (
        (market(90, datetime(2025, 1, 3), 1e-20, 0, 0), weekdays_calendar(), -70),
        (
            market(90, datetime(2025, 1, 3, 0, 0, 0, 1), 1e-20, 0, 0),
            weekdays_calendar(),
            -50,
        ),
        (market(90, datetime(2025, 1, 4), 1e-20, 0, 0), weekdays_calendar(), -50),
        (market(90, datetime(2025, 1, 3), 1e-20, 0, 0), all_days_calendar(), -110),
        (
            market(90, datetime(2025, 1, 3), 1e-20, 0, 0),
            holiday,
            -50,
        ),
    ):
        assert (
            engine.price(
                dated_history(
                    note.replace(
                        calendar=calendar,
                        accumulated_quantity=note.accumulated_quantity,
                        knock_out_settled=note.knock_out_settled,
                        history_time=(note.history_time or note.effective_time)
                        if note.accumulated_quantity is not None
                        or note.knock_out_settled is not None
                        else None,
                    ),
                    context,
                ),
                context,
            )
            == expected
        )
    omitted = Accumulator(
        observation_clock_time=clock_time(),
        strike=100,
        knock_out_level=110,
        daily_quantity=1,
        acceleration_factor=2,
        effective_time=START,
        expiry_time=datetime(2025, 1, 6),
    )
    assert_rejection(
        MissingHistoryError,
        lambda: engine.price(
            dated_history(omitted, market(90, omitted.expiry_time)),
            market(90, omitted.expiry_time),
        ),
    )
    assert engine.price(
        dated_history(
            omitted.replace_history(
                accumulated_quantity=0,
                knock_out_settled=False,
                history_time=omitted.history_time or omitted.expiry_time,
            ),
            market(90, omitted.expiry_time),
        ),
        market(90, omitted.expiry_time),
    ) == (-20)
    assert omitted.accumulated_quantity is None


def test_shared_greeks_scalar_bumps_and_implied_volatility():
    note = contract(datetime(2025, 1, 6))
    context = market(valuation=datetime(2025, 1, 3, 12))
    engine = MC(path_count=4096, seed=42)
    result = numerical_greeks(
        engine, dated_history(note, context), context, greeks="all", spot_shift=0.25
    )
    assert result.price == engine.price(dated_history(note, context), context)
    up = engine.price(
        dated_history(note, context.replace(spot_price=100.5)),
        context.replace(spot_price=100.5),
    )
    down = engine.price(
        dated_history(note, context.replace(spot_price=100)),
        context.replace(spot_price=100),
    )
    assert result.delta == (up - down) / 0.5
    # Equivalent second-difference groupings can round differently.
    rounding = (
        4
        * sys.float_info.epsilon
        * (abs(up) + 2 * abs(result.price) + abs(down))
        / 0.25**2
    )
    assert result.gamma == pytest.approx(
        (up - 2 * result.price + down) / 0.25**2, rel=0, abs=rounding
    )
    for greek in (
        "speed",
        "vega",
        "vanna",
        "zomma",
        "rho",
    ):
        assert math.isfinite(result.require(greek))
    for greek in ("theta", "charm", "color"):
        assert math.isfinite(result.require(greek))
    sigma = pricing.implied_volatility(
        engine,
        dated_history(note, context),
        context,
        result.price,
        lower_bound=0.1,
        upper_bound=0.3,
    )
    assert sigma == pytest.approx(0.2, rel=0, abs=1e-6)
    for spot in (100, 110):
        boundary = engine.price_with_greeks(
            dated_history(note, market(spot, START + timedelta(days=1))),
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
    expired = engine.price_with_greeks(
        dated_history(note, market(90, note.expiry_time)),
        market(90, note.expiry_time),
        greeks="all",
    )
    assert_terminal_greeks(expired, 5)
    assert engine.seed == 42
    assert note.accumulated_quantity == 3
    assert context.model_parameters.volatility == 0.2
    unseeded = MC(path_count=1024)
    smooth = contract(datetime(2025, 1, 6), knock_out=1e300, acceleration=1, quantity=0)
    shared = numerical_greeks(
        unseeded,
        dated_history(smooth, context),
        context,
        ["delta", "gamma"],
        spot_shift=0.25,
    )
    assert shared.gamma == pytest.approx(0, rel=0, abs=1e-10)
    assert unseeded.seed is None


def test_validation_fixed_payoffs_and_failure_reuse():
    engine = MC(path_count=32, seed=42)
    note, context = contract(datetime(2025, 1, 6)), market()
    for instrument, invalid, expected_error in (
        (note, market(110, START - timedelta(days=1)), InvalidInputError),
        (
            note,
            market(110, datetime(2025, 1, 6, 0, 0, 0, 1)),
            InvalidInputError,
        ),
        (
            contract(quantity=sys.float_info.max),
            market(111, START + timedelta(days=1)),
            ComputationFailureError,
        ),
    ):
        assert_rejection(
            expected_error,
            lambda instrument=instrument, invalid=invalid: engine.price(
                dated_history(instrument, invalid), invalid
            ),
        )
    assert_rejection(
        InvalidInputError,
        lambda: numerical_greeks(
            engine, dated_history(note, context), context, "delta", spot_shift=0
        ),
    )
    curve = RateTermStructure(
        reference_time=START,
        times=[note.expiry_time, note.expiry_time + (note.expiry_time - START)],
        factors=[0.99, (0.99) * 0.5],
    )
    for field in ("risk_free_rate", "dividend_yield"):
        curved = context.replace(
            model_parameters=context.model_parameters.replace(**{field: curve})
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda curved=curved: engine.price(dated_history(note, curved), curved),
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda curved=curved: engine.price_with_greeks(
                dated_history(note, curved), curved, []
            ),
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda curved=curved: pricing.implied_volatility(
                engine,
                dated_history(note, curved),
                curved,
                math.nan,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
    for instrument, fixed, expected in (
        (note, market(110, START + timedelta(days=1)), 30),
        (note, market(90, note.expiry_time), -50),
        (
            contract(daily=0),
            market(90, START + timedelta(days=1), rate=0, dividend=0),
            -30,
        ),
        (contract(daily=0, quantity=0), context, 0),
        (contract(knock_out=100, acceleration=0, quantity=0), context, 0),
    ):
        result = engine.price_with_statistics(dated_history(instrument, fixed), fixed)
        assert result.price == expected
        assert result.standard_error == 0
        assert_rejection(
            UnsupportedOperationError,
            lambda instrument=instrument, fixed=fixed, expected=expected: (
                pricing.implied_volatility(
                    engine,
                    dated_history(instrument, fixed),
                    fixed,
                    expected,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
            ),
        )
    for quote, expected_error in (
        (math.nan, InvalidInputError),
        (1e100, RootNotBracketedError),
    ):
        assert_rejection(
            expected_error,
            lambda quote=quote: pricing.implied_volatility(
                engine,
                dated_history(note, context),
                context,
                quote,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
    assert (
        engine.price(
            dated_history(note, market(110, START + timedelta(days=1))),
            market(110, START + timedelta(days=1)),
        )
        == 30
    )
    assert note.accumulated_quantity == 3


def test_sobol_dimension_limit_preserves_known_settlements():
    engine = MC(path_count=2, seed=42, sampling="sobol")
    note = contract(datetime(2090, 1, 2))
    context = market(valuation=START + timedelta(days=1))
    note = note.replace(
        calendar=all_days_calendar(),
        accumulated_quantity=note.accumulated_quantity,
        knock_out_settled=note.knock_out_settled,
        history_time=(note.history_time or note.effective_time)
        if note.accumulated_quantity is not None or note.knock_out_settled is not None
        else None,
    )
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(dated_history(note, context), context),
    )
    result = engine.price_with_statistics(
        dated_history(note, context.replace(spot_price=110)),
        context.replace(spot_price=110),
    )
    assert result.price == 30
    assert result.standard_error is None
    assert result.confidence_interval is None
    zero = contract(datetime(2090, 1, 2), daily=0, quantity=0)
    assert engine.price(dated_history(zero, context), context) == 0


def test_rate_volatility_and_mixed_greeks_match_seeded_scalar_repricing(subtests):
    note = contract(datetime(2025, 1, 13))
    context = market(valuation=datetime(2025, 1, 3, 12))
    engine = MC(path_count=1024, seed=42, antithetic=True)
    h = 0.25
    v = (0.2 + 0.002) - 0.2
    r = (0.04 + 0.001) - 0.04

    def price(spot=100.25, volatility=0.2, rate=0.04):
        return engine.price(
            dated_history(
                note,
                context.replace(
                    spot_price=spot,
                    model_parameters=context.model_parameters.replace(
                        volatility=volatility, risk_free_rate=rate
                    ),
                ),
            ),
            context.replace(
                spot_price=spot,
                model_parameters=context.model_parameters.replace(
                    volatility=volatility, risk_free_rate=rate
                ),
            ),
        )

    result = numerical_greeks(
        engine,
        dated_history(note, context),
        context,
        greeks="all",
        spot_shift=h,
        volatility_shift=0.002,
        rate_shift=0.001,
    )
    high, low = price(volatility=0.2 + v), price(volatility=0.2 - v)
    high_up = price(100.5, 0.2 + v)
    high_down = price(100.0, 0.2 + v)
    low_up = price(100.5, 0.2 - v)
    low_down = price(100.0, 0.2 - v)
    expected = {
        "rho": (price(rate=0.04 + r) - price(rate=0.04 - r)) / (2 * r),
        "vega": (high - low) / (2 * v),
        "vanna": (high_up - high_down - low_up + low_down) / (4 * h * v),
        "zomma": ((high_up - 2 * high + high_down) - (low_up - 2 * low + low_down))
        / (2 * h**2 * v),
        "speed": (price(100.75) - 2 * price(100.5) + 2 * price(100) - price(99.75))
        / (2 * h**3),
    }
    for greek, value in expected.items():
        with subtests.test(greek=greek):
            assert result.require(greek) == pytest.approx(value, rel=0, abs=1e-9)
