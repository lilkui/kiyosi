"""Shared FD model contracts through the public Python interface."""

from datetime import datetime, timedelta
from math import sqrt

import pytest
from kiyosi import InvalidInputError, pricing
from kiyosi.instruments import (
    AssetOrNothingOption,
    BarrierOption,
    BinaryBarrierOption,
    CashOrNothingOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    RateTermStructure,
)
from test_local_volatility import END, START, context
from test_support_matrix import HESTON_ENGINES, terminal_cases


def representatives():
    found = set()
    for contract, names, _ in terminal_cases():
        for name in names:
            if (
                name.startswith("FiniteDifference")
                and name not in HESTON_ENGINES
                and name not in found
            ):
                found.add(name)
                yield contract, name


def test_wide_local_knock_in_grids_resolve_the_continuation_payoff():
    expiry = START + timedelta(days=5 * 365)
    bsm = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.03, volatility=0.85
        ),
        spot_price=100,
        valuation_time=START,
    )
    local = bsm.replace(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0.03,
            surface=LocalVolatilitySurface(
                times=[START, expiry], spots=[80, 120], values=[[0.85, 0.85]] * 2
            ),
        )
    )
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": START,
        "expiry_time": expiry,
        "barrier_level": 101,
        "barrier_type": "up_and_in",
    }
    barrier = BarrierOption(**terms)
    binary = BinaryBarrierOption(**terms, payoff_type="cash", payout=100)
    for instrument, engine, analytic in (
        (
            barrier,
            pricing.FiniteDifferenceBarrierEngine(),
            pricing.AnalyticBarrierEngine(),
        ),
        (
            binary,
            pricing.FiniteDifferenceBinaryBarrierEngine(),
            pricing.AnalyticBinaryBarrierEngine(),
        ),
    ):
        expected = pytest.approx(analytic.price(instrument, bsm), rel=0, abs=0.02)
        assert engine.price(instrument, local) == expected
        assert (
            engine.solve(instrument, local, spot_range=(99, 100.5)).price(100)
            == expected
        )


def test_native_spot_bump_recovers_local_barrier_greeks_without_changing_price():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = BarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_out",
    )
    local = context(
        LocalVolatilitySurface(
            times=[start, end],
            spots=[80, 120],
            values=[[0.2, 0.2], [0.2, 0.2]],
        )
    ).replace(spot_price=119.995, valuation_time=start)
    settings = {"asset_step_count": 200, "time_step_count": 120}
    automatic = pricing.FiniteDifferenceBarrierEngine(**settings)
    unavailable = automatic.price_with_greeks(option, local, ["delta", "gamma"])
    assert unavailable.status("delta") == "stencil_unavailable"
    result = automatic.price_with_greeks(
        option, local, ["delta", "gamma"], spot_shift=0.001
    )
    assert result.price == unavailable.price
    assert result.status("gamma") == "available"
    delta = (
        automatic.price(option, local.replace(spot_price=119.996))
        - automatic.price(option, local.replace(spot_price=119.994))
    ) / 0.002
    assert result.require("delta") == pytest.approx(delta, rel=0, abs=1e-6)
    boundary = automatic.price_with_greeks(
        option, local.replace(spot_price=120), "delta"
    )
    assert boundary.status("delta") == "undefined"


def test_fd_native_spot_controls_validate_per_request(subtests):
    local = context()
    for contract, name in representatives():
        factory = getattr(pricing, name)
        with subtests.test(engine=name):
            engine = factory()
            for invalid in (0, -0.001, float("nan"), float("inf")):
                with pytest.raises(InvalidInputError):
                    engine.price_with_greeks(
                        contract, local, "delta", spot_shift=invalid
                    )
            with pytest.raises(TypeError):
                engine.price_with_greeks(contract, local, "delta", spot_shift=True)
            with pytest.raises(TypeError):
                factory(spot_shift=0.001)


def test_active_products_reject_missing_surface_coverage_and_unstable_steps(subtests):
    short = context(
        LocalVolatilitySurface(
            times=[START, START + timedelta(days=2)],
            spots=[80, 120],
            values=[[0.2, 0.2], [0.2, 0.2]],
        )
    )
    for contract, name in representatives():
        with subtests.test(engine=name):
            with pytest.raises(InvalidInputError):
                getattr(pricing, name)().price(contract, short)
            with pytest.raises(InvalidInputError):
                getattr(pricing, name)(
                    asset_step_count=800,
                    time_step_count=1,
                    scheme="explicit_euler",
                ).price(contract, context())


def test_digital_term_structure_prices_and_greeks_match_integrated_variance(subtests):
    times = [START + timedelta(days=180), END]
    rates = {
        "risk_free_rate": RateTermStructure(
            reference_time=START, times=times, factors=[0.99, 0.95]
        ),
        "dividend_yield": RateTermStructure(
            reference_time=START, times=times, factors=[0.999, 0.97]
        ),
    }
    first, last = 0.19, 0.23
    bsm = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            **rates, volatility=sqrt((first**2 + first * last + last**2) / 3)
        ),
        spot_price=100,
        valuation_time=START,
    )
    local = bsm.replace(
        model_parameters=LocalVolatilityParameters(
            **rates,
            surface=LocalVolatilitySurface(
                times=[START, END],
                spots=[80, 120],
                values=[[first, first], [last, last]],
            ),
        )
    )
    life = {"effective_time": START, "expiry_time": END}
    request = ["delta", "gamma", "rho", "dividend_rho"]
    for option, payoff in (
        (CashOrNothingOption(option_type="call", strike=100, payout=10, **life), 10),
        (AssetOrNothingOption(option_type="put", strike=105, **life), 0),
    ):
        expected = pricing.AnalyticDigitalEngine().price_with_greeks(
            option, bsm, request
        )
        for snapshot in (bsm, local):
            for upper in (None, 400):
                with subtests.test(
                    option=type(option).__name__,
                    model=type(snapshot.model_parameters).__name__,
                    upper=upper,
                ):
                    engine = pricing.FiniteDifferenceDigitalEngine(
                        asset_step_count=1600,
                        time_step_count=800,
                        asset_upper_boundary=upper,
                    )
                    solution = engine.solve(option, snapshot, spot_range=(95, 105))
                    actual = engine.price_with_greeks(option, snapshot, request)
                    for price in (
                        engine.price(option, snapshot),
                        solution.price(100),
                        actual.price,
                    ):
                        assert price == pytest.approx(expected.price, rel=0, abs=0.002)
                    assert actual.delta == pytest.approx(
                        expected.delta, rel=0, abs=0.0002
                    )
                    assert actual.gamma == pytest.approx(
                        expected.gamma, rel=0, abs=0.00003
                    )
                    for greek in ("rho", "dividend_rho"):
                        if snapshot is bsm:
                            assert actual.require(greek) == pytest.approx(
                                expected.require(greek), rel=0, abs=0.03
                            )
                        else:
                            assert actual.status(greek) == "unsupported"
                    terminal = snapshot.replace(valuation_time=END, spot_price=110)
                    assert engine.price(option, terminal) == payoff
