"""Shared FD model contracts through the public Python interface."""

from datetime import datetime, timedelta

import pytest
from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi.instruments import BarrierOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilitySurface,
    RateTermStructure,
)
from support.history import dated as dated_history
from test_local_volatility import END, START, context, surface
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
    controlled = pricing.FiniteDifferenceBarrierEngine(spot_shift=0.001, **settings)
    result = controlled.price_with_greeks(option, local, ["delta", "gamma"])
    assert result.price == unavailable.price
    assert result.status("gamma") == "available"
    delta = (
        controlled.price(option, local.replace(spot_price=119.996))
        - controlled.price(option, local.replace(spot_price=119.994))
    ) / 0.002
    assert result.require("delta") == pytest.approx(delta, rel=0, abs=1e-6)
    boundary = controlled.price_with_greeks(
        option, local.replace(spot_price=120), "delta"
    )
    assert boundary.status("delta") == "undefined"


def test_fd_native_spot_controls_validate_and_survive_reconstruction(subtests):
    import pickle

    for _, name in representatives():
        factory = getattr(pricing, name)
        with subtests.test(engine=name):
            engine = factory(spot_shift=0.001)
            assert engine.spot_shift == 0.001
            assert pickle.loads(pickle.dumps(engine)) == engine
            for invalid in (0, -0.001, float("nan"), float("inf")):
                with pytest.raises(InvalidInputError):
                    factory(spot_shift=invalid)
            with pytest.raises(TypeError):
                factory(spot_shift=True)


def test_every_fd_scheme_preserves_constant_surface_bsm_results(subtests):
    local = context()
    bsm = local.replace(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
    )
    for contract, name in representatives():
        for scheme in [
            "explicit_euler",
            "implicit_euler",
            "crank_nicolson",
            "rannacher",
        ]:
            with subtests.test(engine=name, scheme=scheme):
                engine = getattr(pricing, name)(
                    asset_step_count=80,
                    time_step_count=16000,
                    asset_upper_boundary=400,
                    scheme=scheme,
                )
                actual, expected = (
                    engine.price(dated_history(contract, local), local),
                    engine.price(dated_history(contract, bsm), bsm),
                )
                assert actual == pytest.approx(expected, rel=0, abs=1e-6)


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
                getattr(pricing, name)().price(dated_history(contract, short), short)
            with pytest.raises(InvalidInputError):
                getattr(pricing, name)(
                    asset_step_count=800,
                    time_step_count=1,
                    scheme="explicit_euler",
                ).price(dated_history(contract, context()), context())


def test_digital_rejects_node_rates_even_at_expiry(subtests):
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.95, (0.95) * 0.5],
    )
    for contract, name in representatives():
        if name != "FiniteDifferenceDigitalEngine":
            continue
        for field in ["risk_free_rate", "dividend_yield"]:
            for time in [START, END]:
                local = context(surface()).replace(valuation_time=time)
                local = local.replace(
                    model_parameters=local.model_parameters.replace(**{field: curve})
                )
                with (
                    subtests.test(field=field, time=time),
                    pytest.raises(UnsupportedOperationError),
                ):
                    getattr(pricing, name)().price(
                        dated_history(contract, local), local
                    )
