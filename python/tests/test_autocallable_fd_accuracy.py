"""Autocallable finite-difference accuracy and convergence budgets."""

import math
from datetime import datetime, timedelta
from datetime import time as clock_time

import pytest
from kiyosi import pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    CashOrNothingOption,
    KnockInObservation,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)


def test_nearby_barriers_preserve_prices_and_greeks(subtests):
    start, middle, end = (
        datetime(2025, 1, 1),
        datetime(2025, 7, 1),
        datetime(2026, 1, 1),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.FiniteDifferenceAutocallableEngine()

    def price(barrier):
        option = BinarySnowballOption(
            calendar=weekdays_calendar(),
            maturity_coupon_rate=0.02,
            effective_time=start,
            expiry_time=end,
            autocall_observations=autocallable_observations(
                [middle, end],
                [100, barrier],
                knock_out_coupon_rates=[0.08, 0.08],
            ),
        )
        return engine.price_with_greeks(option, context, ["delta", "gamma"])

    expected = price(100)
    for barrier in (math.nextafter(100, 200), 100 + 1e-12, 100 + 1e-8, 100 + 1e-4):
        with subtests.test(barrier=barrier):
            actual = price(barrier)
            assert actual.price <= 1.08 * math.exp(-0.04)
            for measure in ("price", "delta", "gamma"):
                assert getattr(actual, measure) == pytest.approx(
                    getattr(expected, measure), rel=0, abs=1e-7
                )


@pytest.mark.parametrize("volatility", (0.01, 0.2))
@pytest.mark.parametrize("seconds", (3600, 600, 60, 1))
def test_near_expiry_greeks_match_an_analytic_digital(subtests, volatility, seconds):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=start,
        expiry_time=end,
        autocall_observations=autocallable_observations(
            [end], [100], knock_out_coupon_rates=[0.1]
        ),
    )
    digital = CashOrNothingOption(
        option_type="call",
        strike=100,
        payout=0.1,
        effective_time=start,
        expiry_time=end,
    )
    engine = pricing.FiniteDifferenceAutocallableEngine()
    valuation = datetime(2026, 1, 1) - timedelta(seconds=seconds)
    time = seconds / (365 * 86400)
    width = 100 * volatility * math.sqrt(time)
    for spot in (100, 100 + 0.5 * width):
        with subtests.test(volatility=volatility, seconds=seconds, spot=spot):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04,
                    dividend_yield=0.01,
                    volatility=volatility,
                ),
                spot_price=spot,
                valuation_time=valuation,
            )
            actual = engine.price_with_greeks(option, context, ["delta", "gamma"])
            expected = pricing.AnalyticDigitalEngine().price_with_greeks(
                digital, context, ["delta", "gamma"]
            )
            assert actual.price == pytest.approx(
                math.exp(-0.04 * time) + expected.price, rel=0, abs=1e-5
            )
            for measure, absolute in (("delta", 1e-6), ("gamma", 1e-7)):
                reference = getattr(expected, measure)
                assert getattr(actual, measure) == pytest.approx(
                    reference, rel=0, abs=absolute + 0.005 * abs(reference)
                )


def test_snowball_greeks_preserve_smooth_price_maximum_between_grid_nodes(subtests):
    start, end = datetime(2025, 10, 23), datetime(2026, 10, 23)
    calendar = sse_calendar()
    dates = list(
        monthly_schedule(
            observation_clock_time=clock_time(),
            start_time=start,
            end_time=end,
            first_observation_month=1,
            calendar=calendar,
        ).times
    )
    if dates[-1] != end:
        dates.append(end)
    note = SnowballOption(
        calendar=calendar,
        maturity_coupon_rate=0.2,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        barrier_state="no_event",
        effective_time=start,
        expiry_time=end,
        autocall_observations=autocallable_observations(
            dates, [103] * len(dates), knock_out_coupon_rates=[0.2] * len(dates)
        ),
    )
    model = BlackScholesMertonParameters(
        risk_free_rate=0.02, dividend_yield=0, volatility=0.2
    )
    engine = pricing.FiniteDifferenceAutocallableEngine(
        asset_step_count=1600, time_step_count=1600, asset_upper_boundary=300
    )

    def context(spot):
        return PricingContext(
            model_parameters=model,
            spot_price=spot,
            valuation_time=start,
        )

    # Dealer-normalized Gamma references from an independent quadrature solve.
    for spot, expected, tolerance in (
        (82.0, 2.61633362, 0.03),
        (107.49, 0.93813253, 0.01),
        (107.5, 0.93813253, 0.01),
        (107.51, 0.93813253, 0.01),
    ):
        with subtests.test(spot=spot):
            result = engine.price_with_greeks(
                note,
                context(spot),
                ["delta", "gamma"],
            )
            assert -10000 * result.gamma == pytest.approx(
                expected, rel=0, abs=tolerance
            )
            bump = 0.005
            low = engine.price(note, context(spot - bump))
            high = engine.price(note, context(spot + bump))
            assert 100 * result.delta == pytest.approx(
                100 * (high - low) / (2 * bump), rel=0, abs=1e-5
            )
            assert 10000 * result.gamma == pytest.approx(
                10000 * (high - 2 * result.price + low) / bump**2, rel=0, abs=0.002
            )


@pytest.mark.parametrize("seconds", (1, 3600))
@pytest.mark.parametrize("count", (3, 4, 5, 10, 20, 40))
def test_coarse_grids_preserve_positive_payoff_bounds_and_flat_greeks(seconds, count):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = BinarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=0,
        effective_time=start,
        expiry_time=end,
        autocall_observations=autocallable_observations(
            [end], [100], knock_out_coupon_rates=[0.1]
        ),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        ),
        spot_price=350,
        valuation_time=datetime(2026, 1, 1) - timedelta(seconds=seconds),
    )
    engine = pricing.FiniteDifferenceAutocallableEngine(
        asset_step_count=count, time_step_count=2000
    )
    result = engine.price_with_greeks(option, context, ["delta", "gamma"])
    assert result.price >= 1 - 1e-10
    assert result.price <= 1.1 + 1e-10
    assert result.price == pytest.approx(1.1, rel=0, abs=1e-6)
    assert engine.price(option, context) == result.price
    assert result.delta == pytest.approx(0, rel=0, abs=1e-8)
    assert result.gamma == pytest.approx(0, rel=0, abs=1e-8)


def test_terminal_digital_separates_space_time_domain_and_barrier_placement_errors(
    subtests,
):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )

    def error(assets, times, upper, barrier=100):
        option = BinarySnowballOption(
            calendar=weekdays_calendar(),
            maturity_coupon_rate=0,
            effective_time=start,
            expiry_time=end,
            autocall_observations=autocallable_observations(
                [end], [barrier], knock_out_coupon_rates=[0.1]
            ),
        )
        digital = CashOrNothingOption(
            option_type="call",
            strike=barrier,
            payout=0.1,
            effective_time=start,
            expiry_time=end,
        )
        engine = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=assets,
            time_step_count=times,
            asset_upper_boundary=upper,
        )
        expected = math.exp(-0.05) + pricing.AnalyticDigitalEngine().price(
            digital, context
        )
        return abs(engine.price(option, context) - expected)

    coarse = error(200, 2000, 400)
    for assets in (400, 800, 1600):
        with subtests.test(axis="space", assets=assets):
            refined = error(assets, 2000, 400)
            assert refined <= coarse + 1e-7
            if assets == 1600:
                assert refined <= 1e-5
    for times in (100, 400, 1600):
        with subtests.test(axis="time", times=times):
            assert error(1600, times, 400) <= 1e-5
    for upper in (500, 700, 900):
        with subtests.test(axis="domain", upper=upper):
            assert error(1600, 2000, upper) <= 1e-5
    for barrier in (99.75, 99.875, 100, 100.125, 100.25):
        with subtests.test(axis="barrier_placement", barrier=barrier):
            assert error(800, 2000, 400, barrier) <= 1e-5


def test_projection_preserves_exact_current_event_barrier_comparisons(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = PhoenixOption(
        calendar=weekdays_calendar(),
        coupon_per_period=0.08,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=60,
        barrier_state="no_event",
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        effective_time=start,
        expiry_time=end,
        autocall_observations=autocallable_observations(
            [end], [100], coupon_barrier_levels=[100]
        ),
    )
    engine = pricing.FiniteDifferenceAutocallableEngine()
    for spot, expected in (
        (100, 1.08),
        (math.nextafter(100, 0), 1),
        (80, 1),
        (math.nextafter(80, 0), math.nextafter(80, 0) / 100),
        (math.nextafter(100, 200), 1.08),
    ):
        with subtests.test(spot=spot):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                ),
                spot_price=spot,
                valuation_time=end,
            )
            result = engine.price_with_greeks(option, context, ["delta", "gamma"])
            assert result.price == expected
            assert result.delta is None
            assert result.gamma is None


def test_cash_carry_portfolio_is_preserved_across_event_restarts(subtests):
    start, end = datetime(2024, 1, 1), datetime(2025, 1, 1)
    dates = [datetime(2024, 4, 1), datetime(2024, 7, 1), datetime(2024, 10, 1), end]

    def carry_rate(day):
        time = (day - start).days / 365
        return math.expm1(0.04 * time) / time

    option = TernarySnowballOption(
        calendar=weekdays_calendar(),
        maturity_coupon_rate=carry_rate(end),
        minimum_coupon_rate=carry_rate(end),
        knock_in_level=75,
        knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
        barrier_state="no_event",
        effective_time=start,
        expiry_time=end,
        autocall_observations=autocallable_observations(
            dates,
            [110] * 4,
            knock_out_coupon_rates=[carry_rate(day) for day in dates],
        ),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100.25,
        valuation_time=start,
    )
    for scheme in (
        "crank_nicolson",
        "rannacher",
        "implicit_euler",
        "explicit_euler",
    ):
        with subtests.test(scheme=scheme):
            engine = pricing.FiniteDifferenceAutocallableEngine(
                asset_step_count=80,
                time_step_count=2000,
                scheme=scheme,
                asset_upper_boundary=400,
            )
            result = engine.price_with_greeks(option, context, ["delta", "gamma"])
            assert result.price == pytest.approx(1, rel=0, abs=1e-5)
            assert result.delta == pytest.approx(0, rel=0, abs=1e-6)
            assert result.gamma == pytest.approx(0, rel=0, abs=1e-7)
            assert engine.price(option, context) == result.price
