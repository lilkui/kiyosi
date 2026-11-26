import math
from datetime import datetime, timedelta
from itertools import product

import pytest
from kiyosi import ComputationFailureError, pricing
from kiyosi.instruments import BarrierOption, TouchOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    all_days_calendar,
)


def test_scheduled_barrier_rejects_drift_that_overwhelms_grid_diffusion():
    start = datetime(2025, 1, 1)
    end = start + timedelta(days=365)
    option = BarrierOption(
        option_type="put",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=102,
        barrier_type="up_and_in",
        rebate=2,
        observation_times=[start + timedelta(days=day) for day in (30, 100, 365)],
        calendar=all_days_calendar(),
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.001
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.FiniteDifferenceBarrierEngine()
    with pytest.raises(ComputationFailureError, match="drift"):
        engine.price(option, context)
    with pytest.raises(ComputationFailureError, match="drift"):
        engine.solve(option, context, spot_range=(99.9, 100.1))


@pytest.mark.parametrize("kind", ("vanilla", "touch"))
def test_continuous_upper_edge_uses_barrier_for_scalar_and_saved_prices(kind):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    terms = {"effective_time": start, "expiry_time": end, "barrier_level": 120}
    if kind == "vanilla":
        option = BarrierOption(
            option_type="call", strike=100, barrier_type="up_and_out", **terms
        )
        engine_type = pricing.FiniteDifferenceBarrierEngine
    else:
        option = TouchOption(
            payoff_type="cash", payout=2, barrier_type="up_and_out", **terms
        )
        engine_type = pricing.FiniteDifferenceBinaryBarrierEngine
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = engine_type(asset_upper_boundary=110)
    scalar = engine.price(option, context)
    assert scalar == engine_type().price(option, context)
    saved = engine.solve(option, context, spot_range=(90, 130))
    assert saved.price(100) == pytest.approx(scalar, rel=0, abs=1e-4)
    assert saved.price(130) == 0


@pytest.mark.parametrize("asset_steps", (799, 800))
def test_nearby_grid_sizes_resolve_narrow_up_and_out_payoff(asset_steps):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 31)
    option = BarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=100.15,
        barrier_type="up_and_out",
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0.01, volatility=0.005
        ),
        spot_price=99.9,
        valuation_time=start,
    )
    expected = pricing.AnalyticBarrierEngine().price(option, context)
    engine = pricing.FiniteDifferenceBarrierEngine(asset_step_count=asset_steps)
    saved = engine.solve(option, context, spot_range=(99.89, 99.91))
    assert engine.price(option, context) == pytest.approx(expected, rel=0.005)
    assert saved.price(99.9) == pytest.approx(expected, rel=0.005)


@pytest.mark.parametrize("rebate", (0, 3))
def test_short_continuous_barrier_resolves_diffusion_and_strike(rebate):
    start, end = datetime(2025, 1, 6), datetime(2025, 1, 7)
    option = BarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=99,
        barrier_type="down_and_out",
        rebate=rebate,
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.05
        ),
        spot_price=99.5,
        valuation_time=start,
    )
    expected = pricing.AnalyticBarrierEngine().price(option, context)
    engine = pricing.FiniteDifferenceBarrierEngine()
    saved = engine.solve(option, context, spot_range=(99.4, 99.6))
    assert engine.price(option, context) == pytest.approx(expected, rel=0.001)
    assert saved.price(99.5) == pytest.approx(expected, rel=0.001)


def test_continuous_barrier_resolves_low_volatility_near_the_edge():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = BarrierOption(
        option_type="call",
        strike=95,
        effective_time=start,
        expiry_time=end,
        barrier_level=99.9,
        barrier_type="down_and_out",
    )
    context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05,
            dividend_yield=0,
            volatility=0.01,
        ),
        spot_price=100,
        valuation_time=start,
    )
    expected = pricing.AnalyticBarrierEngine().price(option, context)
    engine = pricing.FiniteDifferenceBarrierEngine()
    saved = engine.solve(option, context, spot_range=(99.95, 100.05))
    assert engine.price(option, context) == pytest.approx(expected, rel=0.005)
    assert saved.price(100) == pytest.approx(expected, rel=0.005)


def test_constant_local_volatility_resolves_continuous_barriers():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    life = {"effective_time": start, "expiry_time": end}
    barrier = {"barrier_level": 99.9, "barrier_type": "down_and_out"}
    vanilla = BarrierOption(option_type="call", strike=95, **life, **barrier)
    touch = TouchOption(payoff_type="cash", payout=1, **life, **barrier)
    bsm = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0, volatility=0.01
        ),
        spot_price=100,
        valuation_time=start,
    )
    local = bsm.replace(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0,
            surface=LocalVolatilitySurface(
                times=[start, end], spots=[80, 120], values=[[0.01, 0.01]] * 2
            ),
        )
    )
    for option, analytic, engine in (
        (
            vanilla,
            pricing.AnalyticBarrierEngine(),
            pricing.FiniteDifferenceBarrierEngine(),
        ),
        (
            touch,
            pricing.AnalyticBinaryBarrierEngine(),
            pricing.FiniteDifferenceBinaryBarrierEngine(),
        ),
    ):
        expected = analytic.price(option, bsm)
        saved = engine.solve(option, local, spot_range=(99.95, 100.05))
        assert engine.price(option, local) == pytest.approx(expected, rel=0.005)
        assert saved.price(100) == pytest.approx(expected, rel=0.005)


def test_continuous_single_barriers_retain_exact_levels_near_knock_out(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    parameters = BlackScholesMertonParameters(
        risk_free_rate=0.04, dividend_yield=0, volatility=0.2
    )
    for barrier, knock_in, rebate, distance in product(
        (120, 120.13, 80, 79.87), (False, True), (0, 2), (0.01, 1e-6, 0)
    ):
        with subtests.test(
            barrier=barrier, knock_in=knock_in, rebate=rebate, distance=distance
        ):
            up = barrier > 100
            direction = "up" if up else "down"
            activation = "in" if knock_in else "out"
            option = BarrierOption(
                option_type="call" if up else "put",
                strike=100,
                effective_time=start,
                expiry_time=end,
                barrier_level=barrier,
                barrier_type=f"{direction}_and_{activation}",
                rebate=rebate,
            )
            context = PricingContext(
                model_parameters=parameters,
                spot_price=barrier + (-distance if up else distance),
                valuation_time=start,
            )
            expected = pricing.AnalyticBarrierEngine().price(option, context)
            actual = pricing.FiniteDifferenceBarrierEngine().price(option, context)
            assert actual == pytest.approx(
                expected, rel=0, abs=1e-9 + 0.005 * abs(expected)
            )


def test_scheduled_edges_retain_next_observation_settlement(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    valuation = datetime(2025, 1, 1, 12)
    for scheme, up, timing, rebate, observes_expiry in product(
        ("explicit_euler", "implicit_euler", "crank_nicolson", "rannacher"),
        (False, True),
        ("at_hit", "at_expiry"),
        (0, 2),
        (False, True),
    ):
        with subtests.test(
            scheme=scheme,
            up=up,
            timing=timing,
            rebate=rebate,
            observes_expiry=observes_expiry,
        ):
            dates = [datetime(2025, 1, 2)]
            if observes_expiry:
                dates.append(end)
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                ),
                spot_price=150 if up else 1,
                valuation_time=valuation,
            )
            option = BarrierOption(
                option_type="call" if up else "put",
                strike=100,
                effective_time=start,
                expiry_time=end,
                barrier_level=110 if up else 90,
                barrier_type="up_and_out" if up else "down_and_out",
                rebate=rebate,
                rebate_timing=timing,
                observation_times=dates,
            )
            engine = pricing.FiniteDifferenceBarrierEngine(
                asset_step_count=200,
                time_step_count=2000 if scheme == "explicit_euler" else 200,
                scheme=scheme,
                asset_upper_boundary=151 if up else None,
            )
            payment = dates[0] if timing == "at_hit" else end
            elapsed = (
                datetime.combine(payment, datetime.min.time()) - valuation
            ).total_seconds() / (365 * 86400)
            assert engine.price(option, context) == pytest.approx(
                rebate * math.exp(-0.04 * elapsed), rel=0, abs=1e-6
            )
