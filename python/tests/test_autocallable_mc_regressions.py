"""Autocallable Monte Carlo shared-path and event-grid regressions."""

import math
from datetime import datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import ComputationFailureError, pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    CouponObservation,
    KnockInObservation,
    KnockOutObservation,
    PhoenixOption,
    SnowballObservation,
    SnowballOption,
    TernarySnowballOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    TradingCalendar,
)
from kiyosi.pricing import numerical_greeks
from price_differences import price_differences


def test_scalar_price_survives_confidence_interval_overflow():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    note = BinarySnowballOption(
        calendar=TradingCalendar.weekdays(),
        maturity_coupon_rate=0,
        principal_redemption_ratio=0,
        effective_time=start,
        expiry_time=end,
        autocall_observations=SnowballObservation.from_columns(
            [end], [100], knock_out_coupon_rates=[1.5e308]
        ),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.MonteCarloAutocallableEngine(
        path_count=2, seed=0, max_active_workers=1
    )
    assert engine.price(note, context) / 7.5e307 == pytest.approx(1, rel=0, abs=1e-12)
    with pytest.raises(ComputationFailureError):
        engine.price_with_statistics(note, context)


def test_shared_spot_simulations_match_separate_seeded_prices(subtests):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 13)
    autocall_times = [datetime(2025, 1, 3), datetime(2025, 1, 8), end]
    knock_out_levels = [101, 100, 99]
    terms = {
        "barrier_state": "no_event",
        "effective_time": start,
        "expiry_time": end,
    }
    cases = [
        (
            BinarySnowballOption(
                calendar=TradingCalendar.weekdays(),
                maturity_coupon_rate=0.05,
                **terms,
                autocall_observations=SnowballObservation.from_columns(
                    autocall_times,
                    knock_out_levels,
                    knock_out_coupon_rates=[0.08] * 3,
                ),
            ),
            pricing.MonteCarloAutocallableEngine,
        ),
    ]
    for mode in ("every_trading_day", "at_expiry"):
        common = {
            **terms,
            "knock_in_level": 99,
            "knock_in_observation": (
                KnockInObservation.every_trading_day(clock_time())
                if mode == "every_trading_day"
                else KnockInObservation.at_expiry()
            ),
        }
        cases.extend(
            [
                (
                    SnowballOption(
                        calendar=TradingCalendar.weekdays(),
                        maturity_coupon_rate=0.06,
                        initial_spot=100,
                        upper_strike=100,
                        lower_strike=60,
                        **common,
                        autocall_observations=SnowballObservation.from_columns(
                            autocall_times,
                            knock_out_levels,
                            knock_out_coupon_rates=[0.08] * 3,
                        ),
                    ),
                    pricing.MonteCarloAutocallableEngine,
                ),
                (
                    PhoenixOption(
                        calendar=TradingCalendar.weekdays(),
                        coupon_per_period=0.08,
                        initial_spot=100,
                        upper_strike=100,
                        lower_strike=60,
                        **common,
                        knock_out_observations=[
                            KnockOutObservation(at, level)
                            for at, level in zip(
                                autocall_times, knock_out_levels, strict=True
                            )
                        ],
                        coupon_observations=[
                            CouponObservation(at, level)
                            for at, level in zip(
                                autocall_times, [100, 99, 98], strict=True
                            )
                        ],
                    ),
                    pricing.MonteCarloAutocallableEngine,
                ),
                (
                    TernarySnowballOption(
                        calendar=TradingCalendar.weekdays(),
                        maturity_coupon_rate=0.06,
                        minimum_coupon_rate=0.02,
                        **common,
                        autocall_observations=SnowballObservation.from_columns(
                            autocall_times,
                            knock_out_levels,
                            knock_out_coupon_rates=[0.08] * 3,
                        ),
                    ),
                    pricing.MonteCarloAutocallableEngine,
                ),
            ]
        )
    parameters = BlackScholesMertonParameters(
        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.8
    )
    spot, shift = 100.25, 0.2
    h = (spot + shift) - spot
    contexts = [
        PricingContext(
            model_parameters=parameters,
            spot_price=spot + offset * h,
            valuation_time=datetime(2025, 1, 1, 12),
        )
        for offset in (0, 1, -1, 2, -2)
    ]
    for note, engine_type in cases:
        for paths in (1, 31, 32):
            for seed in (42, 2**64 - 1):
                with subtests.test(note=type(note).__name__, paths=paths, seed=seed):
                    engine = engine_type(path_count=paths, seed=seed)
                    p, up, down, up2, down2 = [
                        engine.price(note, ctx) for ctx in contexts
                    ]
                    result = numerical_greeks(
                        engine,
                        note,
                        contexts[0],
                        ["delta", "gamma", "speed"],
                        spot_shift=shift,
                    )
                    assert result.price == p
                    assert result.delta == (up - down) / (2 * h)
                    assert result.gamma == (up - 2 * p + down) / h / h
                    assert result.speed == (up2 - 2 * up + 2 * down - down2) / h / h / (
                        2 * h
                    )
                    numerical = numerical_greeks(
                        engine,
                        note,
                        contexts[0],
                        greeks="all",
                        spot_shift=shift,
                    )
                    assert (
                        (result.price, result.delta, result.gamma, result.speed)
                    ) == (
                        (
                            numerical.price,
                            numerical.delta,
                            numerical.gamma,
                            numerical.speed,
                        )
                    )
                    assert result.vega is None


def test_irrelevant_knock_in_events_preserve_spot_and_time_greeks(subtests):
    import math

    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    note = SnowballOption(
        calendar=TradingCalendar.weekdays(),
        maturity_coupon_rate=0,
        initial_spot=100,
        knock_in_level=75,
        upper_strike=100,
        lower_strike=100,
        barrier_state="no_event",
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        effective_time=start,
        expiry_time=end,
        autocall_observations=SnowballObservation.from_columns(
            [end], [110], knock_out_coupon_rates=[0]
        ),
    )
    engine = pricing.MonteCarloAutocallableEngine(path_count=64, seed=42)
    for time in (
        datetime(2025, 1, 1),
        datetime(2025, 1, 2, 12),
    ):
        with subtests.test(engine=type(engine).__name__, time=time):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                ),
                spot_price=75,
                valuation_time=time,
            )
            result = engine.price_with_greeks(
                note,
                context,
                ["delta", "gamma", "theta", "charm", "color"],
            )
            years = (datetime(2026, 1, 1) - time).total_seconds() / (365 * 86400)
            assert result.price == pytest.approx(
                math.exp(-0.04 * years), rel=0, abs=1e-12
            )
            assert ((result.delta, result.gamma, result.charm, result.color)) == (
                (0, 0, 0, 0)
            )
            assert result.theta is not None
            assert result.theta == pytest.approx(
                0.04 * result.price, rel=0, abs=0.00000365
            )


@pytest.mark.parametrize(
    "state,reference_step",
    (("no_event", timedelta(hours=6)), ("knocked_in", timedelta(days=1))),
)
def test_default_time_greeks_respect_remaining_knock_in_events(state, reference_step):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=75,
        valuation_time=datetime(2025, 1, 2, 12),
    )
    engine = pricing.MonteCarloAutocallableEngine(path_count=64, seed=42)
    note = SnowballOption(
        calendar=TradingCalendar.weekdays(),
        maturity_coupon_rate=0.06,
        initial_spot=100,
        knock_in_level=75,
        upper_strike=100,
        lower_strike=60,
        barrier_state=state,
        effective_time=start,
        expiry_time=end,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        autocall_observations=SnowballObservation.from_columns(
            [end], [110], knock_out_coupon_rates=[0.08]
        ),
    )
    request = ["theta", "charm", "color"]
    result = engine.price_with_greeks(note, context, request)
    # Live daily fixings constrain the default roll; completed knock-in leaves a full day.
    expected = price_differences(
        engine, note, context, request, time_shift=reference_step
    )
    assert result.price == expected.price
    for greek in request:
        assert result.require(greek) == pytest.approx(
            getattr(expected, greek), rel=0, abs=1e-7
        )


def test_fixed_implied_volatility_preserves_non_finite_pricing_errors(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    note = BinarySnowballOption(
        calendar=TradingCalendar.weekdays(),
        maturity_coupon_rate=1e308,
        principal_redemption_ratio=1e308,
        effective_time=start,
        expiry_time=end,
        autocall_observations=SnowballObservation.from_columns(
            [end], [110], knock_out_coupon_rates=[1e308]
        ),
    )
    for valuation in (start, end):
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=valuation,
        )
        with subtests.test(valuation=valuation):
            with pytest.raises(ComputationFailureError) as error:
                pricing.implied_volatility(
                    pricing.MonteCarloAutocallableEngine(path_count=2, seed=1),
                    note,
                    context,
                    1,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
            assert type(error.value) is ComputationFailureError


def test_contract_calendar_controls_daily_monte_carlo_observations(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0, volatility=0.3
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.MonteCarloAutocallableEngine(path_count=2000, seed=42)
    reference = pricing.QuadratureAutocallableEngine(asset_step_count=1024)
    for observations in ([datetime(2025, 7, 1)], [datetime(2025, 7, 1), end]):
        terms = {
            "maturity_coupon_rate": 0.04,
            "effective_time": start,
            "expiry_time": end,
            "autocall_observations": SnowballObservation.from_columns(
                observations,
                [110] * len(observations),
                knock_out_coupon_rates=[0.08] * len(observations),
            ),
        }
        for mode in ("at_expiry", "every_trading_day"):
            snowball = SnowballOption(
                calendar=TradingCalendar.weekdays(),
                **terms,
                initial_spot=100,
                knock_in_level=90,
                upper_strike=100,
                lower_strike=60,
                knock_in_observation=(
                    KnockInObservation.every_trading_day(clock_time())
                    if mode == "every_trading_day"
                    else KnockInObservation.at_expiry()
                ),
            )
            binary = BinarySnowballOption(calendar=TradingCalendar.weekdays(), **terms)
            for note in (binary, snowball):
                with subtests.test(
                    observations=observations,
                    note=type(note).__name__,
                    mode=mode,
                ):
                    all_days = note.replace(calendar=TradingCalendar.all_days())
                    if isinstance(note, SnowballOption) and mode == "every_trading_day":
                        weekend = datetime(2025, 1, 4)
                        assert weekend not in note.knock_in_observation_schedule
                        assert weekend in all_days.knock_in_observation_schedule
                        estimates = [
                            engine.price_with_statistics(scheduled, context)
                            for scheduled in (note, all_days)
                        ]
                        for scheduled, estimate in zip(
                            (note, all_days), estimates, strict=True
                        ):
                            # Include sampling uncertainty and spatial quadrature error.
                            budget = 5 * estimate.standard_error + 0.002
                            assert estimate.price == pytest.approx(
                                reference.price(scheduled, context), rel=0, abs=budget
                            )
                        assert estimates[0].price != estimates[1].price
                    else:
                        assert engine.price(note, context) == engine.price(
                            all_days, context
                        )


def check_autocallable(subtests, note, context, expected):
    engine = pricing.MonteCarloAutocallableEngine(path_count=32, seed=7)
    with subtests.test(engine=type(engine).__name__, product=type(note).__name__):
        assert engine.price(note, context) == pytest.approx(expected, rel=0, abs=1e-12)


def test_current_knockouts_pay_only_contractual_coupon(subtests):
    common = {
        "calendar": TradingCalendar.all_days(),
        "effective_time": START,
        "expiry_time": END,
    }
    observations = SnowballObservation.from_columns(
        [FIRST, MIDDLE, END],
        [100, 100, 100],
        knock_out_coupon_rates=[-0.365, 100, 100],
    )
    strikes = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
    notes = (
        BinarySnowballOption(
            **common,
            maturity_coupon_rate=5,
            barrier_state="no_event",
            autocall_observations=observations,
        ),
        SnowballOption(
            **common,
            **strikes,
            maturity_coupon_rate=5,
            knock_in_level=80,
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            barrier_state="knocked_in",
            autocall_observations=observations,
        ),
        TernarySnowballOption(
            **common,
            maturity_coupon_rate=5,
            minimum_coupon_rate=-1,
            knock_in_level=80,
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            barrier_state="knocked_in",
            autocall_observations=observations,
        ),
    )
    for note in notes:
        check_autocallable(subtests, note, cashflow_market(100, FIRST), 0.99)
    for barrier, expected in ((100, 0.635), (math.nextafter(100, 200), 1)):
        note = PhoenixOption(
            **common,
            **strikes,
            coupon_per_period=-0.365,
            knock_in_level=80,
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            barrier_state="knocked_in",
            knock_out_observations=[
                KnockOutObservation(at, level)
                for at, level in zip([FIRST, END], [100, 100], strict=True)
            ],
            coupon_observations=[
                CouponObservation(at, level)
                for at, level in zip([FIRST, END], [barrier, 0], strict=True)
            ],
        )
        check_autocallable(subtests, note, cashflow_market(100, FIRST), expected)


@pytest.mark.parametrize("spot,expected", ((50, 0.85), (90, 1.15), (105, 1.25)))
def test_historical_knockin_and_terminal_caps(subtests, spot, expected):
    common = {
        "calendar": TradingCalendar.all_days(),
        "initial_spot": 100,
        "knock_in_level": 80,
        "knock_in_observation": KnockInObservation.every_trading_day(clock_time()),
        "upper_strike": 100,
        "lower_strike": 60,
        "barrier_state": "knocked_in",
        "principal_redemption_ratio": 1.25,
        "effective_time": START,
        "expiry_time": END,
    }
    snowball = SnowballOption(
        **common,
        maturity_coupon_rate=5,
        autocall_observations=SnowballObservation.from_columns(
            [END], [110], knock_out_coupon_rates=[100]
        ),
    )
    phoenix = PhoenixOption(
        **common,
        coupon_per_period=-0.365,
        knock_out_observations=[
            KnockOutObservation(at, level)
            for at, level in zip([FIRST, END], [110, 110], strict=True)
        ],
        coupon_observations=[
            CouponObservation(at, level)
            for at, level in zip([FIRST, END], [90, 90], strict=True)
        ],
    )
    check_autocallable(subtests, snowball, cashflow_market(spot, END), expected)
    check_autocallable(
        subtests,
        phoenix,
        cashflow_market(spot, END),
        expected - (0.365 if spot >= 90 else 0),
    )


START, FIRST, MIDDLE, END = (
    datetime(2025, 1, 1),
    datetime(2025, 1, 11),
    datetime(2025, 1, 21),
    datetime(2025, 1, 31),
)


def cashflow_market(spot, valuation):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=spot,
        valuation_time=valuation,
    )
