"""Independent local-volatility finite-difference references and discretization checks."""

import itertools
from datetime import datetime
from functools import cache

import pytest
import QuantLib as ql
from kiyosi import pricing
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    BarrierOption,
    BinaryBarrierOption,
    CashOrNothingOption,
)
from kiyosi.market import (
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
)

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


def market(spot=100, dividend=0.02):
    return PricingContext(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=dividend,
            surface=LocalVolatilitySurface(
                times=[START, END],
                spots=[80, 120],
                values=[[0.3, 0.15], [0.35, 0.2]],
            ),
        ),
        spot_price=spot,
        valuation_time=START,
    )


def instrument(kind, direction):
    terms = {
        "option_type": direction,
        "strike": 100,
        "effective_time": START,
        "expiry_time": END,
    }
    if kind == "american":
        return AmericanOption(**terms)
    if kind == "cash":
        return CashOrNothingOption(**terms, payout=10)
    if kind == "asset":
        return AssetOrNothingOption(**terms)
    factory = (
        BinaryBarrierOption if kind in ("binary", "asset_barrier") else BarrierOption
    )
    if kind in ("binary", "asset_barrier"):
        terms["payoff_type"] = "cash" if kind == "binary" else "asset"
    return factory(
        **terms,
        barrier_level=120 if direction == "call" else 80,
        barrier_type="up_and_out" if direction == "call" else "down_and_out",
        **({"payout": 10} if kind == "binary" else {}),
    )


def engine(kind, space=1600, time=1600, upper=400):
    factory = {
        "american": pricing.FiniteDifferenceVanillaEngine,
        "cash": pricing.FiniteDifferenceDigitalEngine,
        "asset": pricing.FiniteDifferenceDigitalEngine,
        "barrier": pricing.FiniteDifferenceBarrierEngine,
        "binary": pricing.FiniteDifferenceBinaryBarrierEngine,
        "asset_barrier": pricing.FiniteDifferenceBinaryBarrierEngine,
    }[kind]
    return factory(
        asset_step_count=space, time_step_count=time, asset_upper_boundary=upper
    )


def values(result):
    return result.price, result.delta, result.gamma


@cache
def reference(kind, direction, time, space, spot=100, dividend=0.02, mesher_vol=0.5):
    """Price the fixed local-volatility surface with QuantLib.

    Parameters
    ----------
    kind : str
        Contract family recognized by the reference.
    direction : {'call', 'put'}
        Payoff direction.
    time, space : int
        Time and spatial step counts.
    spot : float, optional
        Valuation spot.
    dividend : float, optional
        Annual continuously compounded dividend yield.
    mesher_vol : float, optional
        Black volatility used only to size the independent mesh.

    Returns
    -------
    tuple of float
        Independent price, delta and gamma.
    """
    start, end = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
    ql.Settings.instance().evaluationDate = start
    dc = ql.Actual365Fixed()
    matrix = ql.Matrix(2, 2)
    matrix[0][0], matrix[1][0] = 0.3, 0.15
    matrix[0][1], matrix[1][1] = 0.35, 0.2
    local = ql.FixedLocalVolSurface(start, [0.0, 1.0], [80.0, 120.0], matrix, dc)
    process = ql.GeneralizedBlackScholesProcess(
        ql.QuoteHandle(ql.SimpleQuote(spot)),
        ql.YieldTermStructureHandle(ql.FlatForward(start, dividend, dc)),
        ql.YieldTermStructureHandle(ql.FlatForward(start, 0.05, dc)),
        ql.BlackVolTermStructureHandle(
            ql.BlackConstantVol(start, ql.NullCalendar(), mesher_vol, dc)
        ),
        ql.LocalVolTermStructureHandle(local),
    )
    option_type = ql.Option.Call if direction == "call" else ql.Option.Put
    payoff = (
        ql.CashOrNothingPayoff(option_type, 100, 10)
        if kind in ("cash", "binary")
        else ql.AssetOrNothingPayoff(option_type, 100)
        if kind in ("asset", "asset_barrier")
        else ql.PlainVanillaPayoff(option_type, 100)
    )
    if kind in ("barrier", "binary", "asset_barrier"):
        contract = ql.BarrierOption(
            ql.Barrier.UpOut if direction == "call" else ql.Barrier.DownOut,
            120 if direction == "call" else 80,
            0,
            payoff,
            ql.EuropeanExercise(end),
        )
        factory = ql.FdBlackScholesBarrierEngine
    else:
        contract = ql.VanillaOption(
            payoff,
            ql.AmericanExercise(start, end)
            if kind == "american"
            else ql.EuropeanExercise(end),
        )
        factory = ql.FdBlackScholesVanillaEngine
    contract.setPricingEngine(
        factory(process, time, space, 2, ql.FdmSchemeDesc.CrankNicolson(), True)
    )
    return contract.NPV(), contract.delta(), contract.gamma()


@pytest.fixture(autouse=True)
def restore_evaluation_date():
    previous = ql.Settings.instance().evaluationDate
    try:
        yield
    finally:
        ql.Settings.instance().evaluationDate = previous


def assert_measures_close(actual, expected, budgets):
    for name, a, b, budget in zip(
        ("price", "delta", "gamma"), actual, expected, budgets, strict=True
    ):
        assert a == pytest.approx(b, rel=0, abs=budget), name


@pytest.mark.parametrize("kind", ["american", "cash", "asset"])
@pytest.mark.parametrize("direction", ["call", "put"])
def test_american_and_digital_match_refined_independent_local_volatility(
    kind, direction
):
    coarse = reference(kind, direction, 1600, 1600)
    fine = reference(kind, direction, 3200, 3200)
    # Measured reference changes: price <= 0.00070, delta <= 4.7e-6,
    # gamma <= 1.1e-6; these budgets include American time error.
    assert_measures_close(coarse, fine, [0.001, 1e-5, 2e-6])
    actual = values(
        engine(kind).price_with_greeks(
            instrument(kind, direction), market(), ["delta", "gamma"]
        )
    )
    assert_measures_close(actual, fine, [0.002, 0.0002, 0.00003])


@pytest.mark.parametrize("direction", ["call", "put"])
def test_single_barriers_match_time_extrapolated_quantlib_reference(direction):
    refs = [
        reference("barrier", direction, steps, 3200) for steps in [3200, 6400, 12800]
    ]
    estimates = [
        tuple(2 * b - a for a, b in zip(x, y, strict=True))
        for x, y in itertools.pairwise(refs)
    ]
    # QuantLib's absorbing-boundary time error is first order here.
    assert_measures_close(*estimates, [1e-5, 3e-7, 2e-8])
    finer_space = reference("barrier", direction, 12800, 6400)
    assert_measures_close(refs[-1], finer_space, [0.0001, 1e-5, 1e-6])
    actual = values(
        engine("barrier").price_with_greeks(
            instrument("barrier", direction), market(), ["delta", "gamma"]
        )
    )
    assert_measures_close(actual, estimates[-1], [0.0005, 0.00003, 0.000003])


@pytest.mark.parametrize("kind", ["american", "cash", "asset", "barrier"])
def test_skew_time_space_domain_refinement_and_fixed_surface_derivatives(
    subtests, kind
):
    contract = instrument(kind, "put")
    central = engine(kind).price_with_greeks(contract, market(), ["delta", "gamma"])
    for other in [
        engine(kind, time=800),
        engine(kind, space=800),
        engine(kind, space=2400, upper=600),
    ]:
        with subtests.test(kind=kind, grid=other):
            assert other.price(contract, market()) == pytest.approx(
                central.price, rel=0, abs=0.005
            )
    bumped = [engine(kind).price(contract, market(spot)) for spot in [100.1, 99.9]]
    assert central.delta == pytest.approx(
        (bumped[0] - bumped[1]) / 0.2, rel=0, abs=0.0001
    )
    assert central.gamma == pytest.approx(
        (bumped[0] + bumped[1] - 2 * central.price) / 0.01, rel=0, abs=0.00003
    )


@pytest.mark.parametrize("kind", ["binary", "asset_barrier"])
@pytest.mark.parametrize("direction", ["call", "put"])
def test_binary_barriers_match_independent_time_refined_reference(kind, direction):
    refs = [reference(kind, direction, steps, 3200) for steps in [3200, 6400, 12800]]
    estimates = [
        tuple(2 * b - a for a, b in zip(x, y, strict=True))
        for x, y in itertools.pairwise(refs)
    ]
    assert_measures_close(*estimates, [0.0002, 1e-5, 1e-6])
    actual = values(
        engine(kind).price_with_greeks(
            instrument(kind, direction), market(), ["delta", "gamma"]
        )
    )
    assert_measures_close(actual, estimates[-1], [0.003, 0.0003, 0.00003])
