from datetime import datetime

import pytest

"""Heston fixed-grid pricing and Python boundary contracts."""

import inspect
import json
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import timedelta
from pathlib import Path

from kiyosi import ComputationFailureError, InvalidInputError, UnsupportedOperationError
from kiyosi.instruments import AmericanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticHestonEuropeanEngine,
    FiniteDifferenceHestonVanillaEngine,
    implied_volatility,
)
from test_heston import END, START, context, model, option, reference


def test_american_reference_and_exercise_bounds():
    fixture = json.loads(
        (
            Path(__file__).parent
            / "quantlib_reference"
            / "heston_finite_difference.json"
        ).read_text()
    )
    reference_price = fixture["price"]
    budget = 0.001 + 1e-4 * abs(reference_price)
    assert fixture["refinement_uncertainty"] == pytest.approx(
        abs(fixture["extrapolated_prices"][-1] - fixture["extrapolated_prices"][-2]),
        rel=0,
        abs=1e-15,
    )
    assert fixture["refinement_uncertainty"] < budget / 10
    engine = FiniteDifferenceHestonVanillaEngine()
    contract = AmericanOption(
        option_type="put", strike=100, effective_time=START, expiry_time=END
    )
    actual = engine.price(contract, context())
    assert actual == pytest.approx(reference_price, rel=0, abs=budget)
    temporal = [
        FiniteDifferenceHestonVanillaEngine(time_step_count=n).price(
            contract, context()
        )
        for n in (100, 200)
    ] + [actual]
    assert abs(temporal[2] - temporal[1]) < abs(temporal[1] - temporal[0])
    assert actual >= engine.price(option(option_type="put"), context())
    call = AmericanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    snapshot = context(model(dividend_yield=0))
    expected = AnalyticHestonEuropeanEngine().price(option(), snapshot)
    assert engine.price(call, snapshot) == pytest.approx(
        expected, rel=0, abs=0.001 + 1e-4 * expected
    )
    immediate = context(model(risk_free_rate=0.03, dividend_yield=0.1), spot_price=200)
    assert engine.price(call, immediate) == pytest.approx(100, rel=0, abs=1e-10)


def test_defaults_signatures_and_lossless_conversion(subtests):
    engine = FiniteDifferenceHestonVanillaEngine()
    assert (
        (
            engine.asset_step_count,
            engine.variance_step_count,
            engine.time_step_count,
        )
    ) == ((400, 100, 400))
    assert engine.asset_upper_boundary is None
    assert engine.variance_upper_boundary is None
    signature = inspect.signature(FiniteDifferenceHestonVanillaEngine)
    assert tuple(signature.parameters) == (
        (
            "asset_step_count",
            "variance_step_count",
            "time_step_count",
            "asset_upper_boundary",
            "variance_upper_boundary",
        )
    )
    assert all(
        p.kind is inspect.Parameter.KEYWORD_ONLY for p in signature.parameters.values()
    )
    for name in ("asset_step_count", "variance_step_count", "time_step_count"):
        for value, error in (
            (True, TypeError),
            (1.5, TypeError),
            ("10", TypeError),
            (1 << 100, OverflowError),
        ):
            with subtests.test(name=name, value=value), pytest.raises(error):
                FiniteDifferenceHestonVanillaEngine(**{name: value})
    for name in ("asset_upper_boundary", "variance_upper_boundary"):
        with subtests.test(name=name), pytest.raises(TypeError):
            FiniteDifferenceHestonVanillaEngine(**{name: True})
        for value in (0, -1, math.nan, math.inf):
            with (
                subtests.test(name=name, value=value),
                pytest.raises(InvalidInputError),
            ):
                FiniteDifferenceHestonVanillaEngine(**{name: value})
    for kwargs in (
        {"asset_step_count": 2},
        {"variance_step_count": 2},
        {"time_step_count": 0},
    ):
        with subtests.test(kwargs=kwargs), pytest.raises(InvalidInputError):
            FiniteDifferenceHestonVanillaEngine(**kwargs)
    with pytest.raises(AttributeError):
        engine.time_step_count = 1


@pytest.mark.parametrize(
    "changes,expected",
    (
        ({}, 9.01127840754174),
        ({"vol_of_vol": 0.9}, 7.783760588260141),
        (
            {"mean_reversion_speed": 0.2, "vol_of_vol": 1.0, "correlation": -0.9},
            6.256279042682394,
        ),
    ),
)
@pytest.mark.parametrize("side", ("call", "put"))
def test_european_prices_match_shared_independent_anchors(changes, expected, side):
    engine = FiniteDifferenceHestonVanillaEngine()
    parameters = model(**changes)
    snapshot = context(parameters)
    contract = option(option_type=side)
    ref = (
        expected
        if side == "call"
        else expected - 100 * (math.exp(-0.02) - math.exp(-0.05))
    )
    assert engine.price(contract, snapshot) == pytest.approx(
        ref, rel=0, abs=0.001 + 1e-4 * abs(ref)
    )


def test_absorbing_zero_and_node_curves_keep_interior_exercise():
    parameters = model(
        risk_free_rate=0.05,
        dividend_yield=0.2,
        initial_variance=0,
        long_run_variance=0,
        mean_reversion_speed=0,
    )
    snapshot = context(parameters, spot_price=30)
    contract = AmericanOption(
        option_type="put",
        strike=100,
        effective_time=START,
        expiry_time=datetime(2028, 1, 1),
    )
    optimum = math.log(5 / 6) / -0.15
    expected = 100 * math.exp(-0.05 * optimum) - 30 * math.exp(-0.2 * optimum)
    engine = FiniteDifferenceHestonVanillaEngine(time_step_count=1)
    assert engine.price(contract, snapshot) == pytest.approx(expected, rel=0, abs=1e-12)
    assert expected > 70
    for side, rate, dividend in (("call", 1e-18, 0), ("put", 0, 1e-18)):
        parameters = model(
            risk_free_rate=rate,
            dividend_yield=dividend,
            initial_variance=0,
            long_run_variance=0,
        )
        assert engine.price(
            option(option_type=side, strike=1e18),
            context(parameters, spot_price=1e18),
        ) == pytest.approx(1, rel=0, abs=1e-12)
    middle = datetime(2025, 7, 1)
    curve = RateTermStructure(
        reference_time=START, times=[middle, END], factors=[math.exp(0.1), 1]
    )
    parameters = model(
        risk_free_rate=curve,
        dividend_yield=0,
        initial_variance=0,
        long_run_variance=0,
    )
    contract = AmericanOption(
        option_type="put", strike=100, effective_time=START, expiry_time=END
    )
    assert engine.price(
        contract, context(parameters, spot_price=0.01)
    ) == pytest.approx(100 * math.exp(0.1) - 0.01, rel=0, abs=1e-12)


def test_expiry_applicability_errors_and_state(subtests):
    engine = FiniteDifferenceHestonVanillaEngine()
    snapshot = context()
    saved_model = snapshot.model_parameters
    end = snapshot.replace(valuation_time=END, spot_price=110)
    assert engine.price(option(), end) == 10
    for when in (END + timedelta(microseconds=1), END + timedelta(days=1)):
        with subtests.test(when=when), pytest.raises(InvalidInputError):
            engine.price(option(), snapshot.replace(valuation_time=when))
    bsm = end.replace(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
    )
    with pytest.raises(UnsupportedOperationError):
        engine.price(option(), bsm)
    result = engine.price_with_greeks(option(), snapshot, "delta")
    assert result.price == engine.price(option(), snapshot)
    assert result.delta is None
    assert result.status("delta") == "unsupported"
    with pytest.raises(UnsupportedOperationError):
        implied_volatility(
            engine, option(), snapshot, 9, lower_bound=0.0, upper_bound=4.0
        )
    for controls in (
        {"asset_upper_boundary": 100},
        {"variance_upper_boundary": 0.04},
    ):
        with subtests.test(controls=controls), pytest.raises(InvalidInputError):
            FiniteDifferenceHestonVanillaEngine(**controls).price(option(), snapshot)
    broken = FiniteDifferenceHestonVanillaEngine(
        asset_step_count=3,
        variance_step_count=3,
        time_step_count=1,
        asset_upper_boundary=1e308,
    )
    for _ in range(2):
        with pytest.raises(ComputationFailureError):
            broken.price(option(), snapshot)
    extreme = context(
        model(
            risk_free_rate=0,
            dividend_yield=0,
            initial_variance=2000,
            long_run_variance=2000,
            mean_reversion_speed=0,
            vol_of_vol=0,
        ),
        spot_price=1e200,
    )
    put = option(option_type="put", strike=1e-200)
    assert AnalyticHestonEuropeanEngine().price(put, extreme) > 0.9e-200
    with pytest.raises(ComputationFailureError):
        engine.price(put, extreme)
    assert snapshot.model_parameters == saved_model
    assert snapshot.spot_price == 100
    assert engine.time_step_count == 400
    short = RateTermStructure(
        reference_time=START, times=[datetime(2025, 7, 1)], factors=[0.98]
    )
    with pytest.raises(InvalidInputError):
        engine.price(
            option(), end.replace(model_parameters=model(risk_free_rate=short))
        )


def test_shared_engine_concurrency_and_price_scaling(subtests):
    engine = FiniteDifferenceHestonVanillaEngine(
        asset_step_count=100, variance_step_count=25, time_step_count=50
    )
    expected = engine.price(option(), context())
    with ThreadPoolExecutor(max_workers=4) as pool:
        actual = list(pool.map(lambda _: engine.price(option(), context()), range(4)))
    assert actual == [expected] * 4
    for scale in (1e-100, 1e100):
        with subtests.test(scale=scale):
            value = engine.price(
                option(strike=100 * scale), context(spot_price=100 * scale)
            )
            assert value / scale == pytest.approx(expected, rel=0, abs=1e-11)


def test_deterministic_variance_and_nonabsorbing_zero(subtests):
    engine = FiniteDifferenceHestonVanillaEngine()
    for xi in (1e-8, 1e-200):
        with subtests.test(xi=xi):
            assert engine.price(
                option(), context(model(vol_of_vol=xi))
            ) == pytest.approx(9.227005508154036, rel=0, abs=0.002)
    for changes in (
        {"vol_of_vol": 0, "initial_variance": 0.01, "long_run_variance": 0.09},
        {"initial_variance": 0},
        {"correlation": -1},
        {"correlation": 1},
    ):
        snapshot = context(model(**changes))
        ref = AnalyticHestonEuropeanEngine().price(option(), snapshot)
        with subtests.test(changes=changes):
            assert engine.price(option(), snapshot) == pytest.approx(
                ref, rel=0, abs=0.001 + 1e-4 * ref
            )


def test_maturity_moneyness_and_nonflat_rate_prices(subtests):
    engine = FiniteDifferenceHestonVanillaEngine()
    for expiry, strike in (
        (START + timedelta(days=7), 100),
        (datetime(2030, 1, 1), 120),
        (END, 50),
        (END, 150),
    ):
        contract = option(expiry_time=expiry, strike=strike)
        ref = AnalyticHestonEuropeanEngine().price(contract, context())
        with subtests.test(expiry=expiry, strike=strike):
            assert engine.price(contract, context()) == pytest.approx(
                ref, rel=0, abs=1e-5 * strike + 1e-4 * ref
            )
    risk = RateTermStructure(
        reference_time=START,
        times=[datetime(2025, 7, 1), END],
        factors=[1.01, 0.98],
    )
    dividend = RateTermStructure(
        reference_time=START,
        times=[datetime(2025, 4, 1), END],
        factors=[0.99, 0.97],
    )
    snapshot = context(model(risk_free_rate=risk, dividend_yield=dividend))
    ref = AnalyticHestonEuropeanEngine().price(option(), snapshot)
    assert engine.price(option(), snapshot) == pytest.approx(
        ref, rel=0, abs=0.001 + 1e-4 * ref
    )


def test_spatial_time_and_domain_refinement_are_checked_separately(subtests):
    snapshot = context()
    ref = 9.01127840754174
    base = {
        "asset_step_count": 400,
        "variance_step_count": 100,
        "time_step_count": 100,
    }

    def price(**changes):
        return FiniteDifferenceHestonVanillaEngine(**(base | changes)).price(
            option(), snapshot
        )

    spatial = [price(asset_step_count=n) for n in (100, 200, 400)]
    assert abs(spatial[2] - ref) < abs(spatial[1] - ref)
    assert abs(spatial[1] - ref) < abs(spatial[0] - ref)
    variance = [price(variance_step_count=n) for n in (25, 50, 100)]
    assert abs(variance[2] - variance[1]) < abs(variance[1] - variance[0])
    temporal = [price(time_step_count=n) for n in (25, 50, 100)]
    assert abs(temporal[2] - temporal[1]) < 0.4 * abs(temporal[1] - temporal[0])
    for controls in (
        {"asset_upper_boundary": 800},
        {"variance_upper_boundary": 0.6},
    ):
        with subtests.test(controls=controls):
            enlarged = price(**controls)
            assert enlarged == pytest.approx(spatial[-1], rel=0, abs=0.0005)
            assert enlarged == pytest.approx(ref, rel=0, abs=0.001 + 1e-4 * ref)


def test_refined_tail_price_uses_relative_acceptance():
    contract = option(
        option_type="put", strike=70, expiry_time=START + timedelta(days=30)
    )
    expected = 3.105513031411244e-6
    estimates = [
        reference(model(), contract, tolerance) for tolerance in (1e-10, 1e-12)
    ]
    assert abs(estimates[1] - estimates[0]) < expected * 0.001
    assert estimates[1] == pytest.approx(expected, rel=0, abs=expected * 0.001)
    engine = FiniteDifferenceHestonVanillaEngine(
        asset_step_count=1200, variance_step_count=200, time_step_count=100
    )
    actual = engine.price(contract, context())
    assert abs(actual / expected - 1) < 0.01
