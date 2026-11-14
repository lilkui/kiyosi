"""Local-volatility contracts and independent numerical references."""

import inspect
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import pytest
import QuantLib as ql
from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    RateTermStructure,
)
from support.assertions import assert_decimal_equal
from support.terminal import assert_terminal_greeks
from test_support_matrix import HESTON_ENGINES, terminal_cases, terminal_delta

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


def surface(values=((0.2, 0.2), (0.2, 0.2))):
    return LocalVolatilitySurface(times=[START, END], spots=[80, 120], values=values)


def context(surf=None, spot=100):
    return PricingContext(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05, dividend_yield=0.02, surface=surf or surface()
        ),
        spot_price=spot,
        valuation_time=START,
    )


def option(direction="call", end=END):
    return EuropeanOption(
        option_type=direction, strike=100, effective_time=START, expiry_time=end
    )


def engine(n=800, t=800, upper=400):
    return pricing.FiniteDifferenceVanillaEngine(
        asset_step_count=n, time_step_count=t, asset_upper_boundary=upper
    )


def quantlib_skew(n, spot, direction):
    """Price the supplied local-volatility skew with an independent PDE.

    Parameters
    ----------
    n : int
        Number of time and spatial steps.
    spot : float
        Valuation spot.
    direction : {'call', 'put'}
        Payoff direction.

    Returns
    -------
    tuple
        Price, delta, gamma and the QuantLib local-volatility surface.

    Notes
    -----
    Uses a log-space mesher without fitting an implied-volatility surface.
    """
    start, end = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
    ql.Settings.instance().evaluationDate = start
    # QuantLib uses price rows and time columns, the transpose of our API.
    matrix = ql.Matrix(2, 2)
    matrix[0][0], matrix[1][0] = 0.3, 0.15
    matrix[0][1], matrix[1][1] = 0.35, 0.2
    local = ql.FixedLocalVolSurface(
        start, [0.0, 1.0], [80.0, 120.0], matrix, ql.Actual365Fixed()
    )
    risk = ql.YieldTermStructureHandle(ql.FlatForward(start, 0.05, ql.Actual365Fixed()))
    dividend = ql.YieldTermStructureHandle(
        ql.FlatForward(start, 0.02, ql.Actual365Fixed())
    )
    # This Black vol sizes the independent mesher, not the PDE's diffusion.
    black = ql.BlackVolTermStructureHandle(
        ql.BlackConstantVol(start, ql.NullCalendar(), 0.5, ql.Actual365Fixed())
    )
    process = ql.GeneralizedBlackScholesProcess(
        ql.QuoteHandle(ql.SimpleQuote(spot)),
        dividend,
        risk,
        black,
        ql.LocalVolTermStructureHandle(local),
    )
    payoff = ql.PlainVanillaPayoff(
        ql.Option.Call if direction == "call" else ql.Option.Put, 100
    )
    contract = ql.VanillaOption(payoff, ql.EuropeanExercise(end))
    contract.setPricingEngine(
        ql.FdBlackScholesVanillaEngine(
            process, n, n, 2, ql.FdmSchemeDesc.CrankNicolson(), True
        )
    )
    return contract.NPV(), contract.delta(), contract.gamma(), local


def test_interpolation_preserves_extreme_positive_constants():
    for value in [
        math.ulp(0.0),
        float.fromhex("0x1p-1022"),
        float.fromhex("0x1.fffffffffffffp1023"),
    ]:
        s = surface(((value, value), (value, value)))
        for spot in [80, 90, 100, 110, 140]:
            assert s.volatility(START + timedelta(days=182), spot) == value


def test_sigma_interpolation_time_boundaries_and_flat_price_tails():
    s = surface(((0.2, 0.4), (0.4, 0.8)))
    mid = START + timedelta(days=182)
    assert_decimal_equal(s.volatility(mid, 100), 0.3 + 0.3 * 182 / 365)
    assert s.volatility(START, 1) == 0.2
    assert s.volatility(END, 140) == 0.8
    assert s.volatility(END, 80) == 0.4
    assert s.maximum_volatility == 0.8
    for time in [START - timedelta(days=1), END + timedelta(days=1)]:
        with pytest.raises(InvalidInputError):
            s.volatility(time, 100)
    for spot in [0, -1, math.nan, math.inf]:
        with pytest.raises(InvalidInputError):
            s.volatility(START, spot)


def test_shape_node_value_and_native_conversion_errors(subtests):
    base = {"times": [START, END], "spots": [80, 120], "values": [[0.2, 0.2]] * 2}
    for change in [
        {"times": []},
        {"times": [START]},
        {"times": [END, START]},
        {"times": [START, START]},
        {"spots": []},
        {"spots": [80]},
        {"spots": [120, 80]},
        {"spots": [80, 80]},
        {"values": []},
        {"values": [[0.2]] * 2},
        {"values": [[0.2, 0.2]]},
        *[{"spots": [v, 120]} for v in [0, -1, math.nan, math.inf]],
        *[{"values": [[0.2, v]] * 2} for v in [0, -1, math.nan, math.inf]],
    ]:
        with subtests.test(change=change), pytest.raises(InvalidInputError):
            LocalVolatilitySurface(**(base | change))
    for change in [
        {"spots": [True, 120]},
        {"values": [[0.2, False]] * 2},
        {"values": [0.2, 0.3]},
        {"times": [datetime(2025, 1, 1).date(), END]},
    ]:
        with subtests.test(change=change), pytest.raises(TypeError):
            LocalVolatilitySurface(**(base | change))
    with pytest.raises(TypeError):
        surface().volatility(START, True)
    with pytest.raises(TypeError):
        LocalVolatilityParameters(
            risk_free_rate=True, dividend_yield=0, surface=surface()
        )
    with pytest.raises(TypeError):
        PricingContext(model_parameters=object(), spot_price=100, valuation_time=START)


def test_owned_inputs_properties_replacement_and_fixed_coordinates():
    rows = [[0.2, 0.4], [0.4, 0.8]]
    s = LocalVolatilitySurface(
        times=iter([START, END]), spots=iter([80, 120]), values=iter(rows)
    )
    rows[0][0] = 99
    copied = [list(row) for row in s.values]
    copied[0][0] = 98
    copied_spots = list(s.spots)
    copied_spots[0] = 1
    assert s.values == (((0.2, 0.4), (0.4, 0.8)))
    assert s.spots == ((80, 120))
    assert s.times[0] == datetime(2025, 1, 1)
    c = context(s)
    m = c.model_parameters
    assert isinstance(m, LocalVolatilityParameters)
    moved = c.replace(spot_price=140, valuation_time=START + timedelta(days=182))
    assert moved.model_parameters.surface == s
    assert c.spot_price == 100
    replacement = m.replace(risk_free_rate=0.07, surface=surface())
    assert replacement.flat_risk_free_rate == 0.07
    assert m.flat_risk_free_rate == 0.05
    assert m.surface == s
    for change in [{"risk_free_rate": math.nan}, {"dividend_yield": math.inf}]:
        with pytest.raises(InvalidInputError):
            m.replace(**change)
        assert m.surface == s
    with pytest.raises(InvalidInputError):
        c.replace(valuation_time=END + timedelta(days=1))
    with pytest.raises(AttributeError):
        s.spots = [1, 2]
    with pytest.raises(TypeError):
        m.replace(surface=None)
    assert m.replace(surface=Ellipsis) == m


def test_automatic_domains_ignore_surface_outside_remaining_horizon(subtests):
    e = pricing.FiniteDifferenceVanillaEngine()
    baseline = e.price_with_greeks(option(), context(), ["delta", "gamma"])
    assert baseline.price == pytest.approx(9.227005508154036, rel=0, abs=0.002)
    for past, future in [(1.5, 0.2), (0.2, 1.5), (1.5, 1.5)]:
        with subtests.test(past=past, future=future):
            extended = LocalVolatilitySurface(
                times=[datetime(2024, 1, 1), START, END, datetime(2030, 1, 1)],
                spots=[80, 120],
                values=[[past] * 2, [0.2] * 2, [0.2] * 2, [future] * 2],
            )
            actual = e.price_with_greeks(
                option(),
                context(extended),
                ["delta", "gamma"],
            )
            for measure in ["price", "delta", "gamma"]:
                assert getattr(actual, measure) == pytest.approx(
                    getattr(baseline, measure), rel=0, abs=1e-12
                )


def test_automatic_domains_preserve_interpolated_endpoint_volatility():
    extended = LocalVolatilitySurface(
        times=[START - timedelta(days=365), END + timedelta(days=365)],
        spots=[80, 120],
        values=[[0.2] * 2, [0.8] * 2],
    )
    clipped = surface(((0.4, 0.4), (0.6, 0.6)))
    e = pricing.FiniteDifferenceVanillaEngine()
    assert e.price(option(), context(extended)) == pytest.approx(
        e.price(option(), context(clipped)),
        rel=0,
        abs=1e-10,
    )


@pytest.mark.parametrize("direction", ["call", "put"])
def test_constant_surface_prices_and_greeks_match_bsm(direction):
    bsm = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=START,
    )
    actual = engine().price_with_greeks(
        option(direction),
        context(),
        ["delta", "gamma"],
    )
    expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
        option(direction), bsm, ["delta", "gamma"]
    )
    for value, ref, tolerance in [
        (actual.price, expected.price, 0.002),
        (actual.delta, expected.delta, 0.0002),
        (actual.gamma, expected.gamma, 0.00003),
    ]:
        assert value == pytest.approx(ref, rel=0, abs=tolerance)


def test_time_variation_matches_independent_integrated_variance():
    mid = START + timedelta(days=182)
    sigmas = [0.15, 0.35, 0.25]
    variance = (
        sum(
            days * (a * a + a * b + b * b) / 3
            for a, b, days in [(0.15, 0.35, 182), (0.35, 0.25, 183)]
        )
        / 365
    )
    s = LocalVolatilitySurface(
        times=[START, mid, END], spots=[80, 120], values=[[v, v] for v in sigmas]
    )
    bsm = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=math.sqrt(variance)
        ),
        spot_price=100,
        valuation_time=START,
    )
    for direction in ["call", "put"]:
        assert engine().price(option(direction), context(s)) == pytest.approx(
            pricing.AnalyticEuropeanEngine().price(option(direction), bsm),
            rel=0,
            abs=0.002,
        )
    remaining_sigma = math.sqrt((0.35**2 + 0.35 * 0.25 + 0.25**2) / 3)
    rolled = context(s).replace(valuation_time=mid)
    reference = bsm.replace(
        valuation_time=mid,
        model_parameters=bsm.model_parameters.replace(volatility=remaining_sigma),
    )
    assert engine().price(option(), rolled) == pytest.approx(
        pricing.AnalyticEuropeanEngine().price(option(), reference),
        rel=0,
        abs=0.002,
    )


def test_skew_refines_time_space_domain_and_fixed_surface_greeks():
    c = context(surface(((0.3, 0.15), (0.35, 0.2))))
    fine = engine(1600, 1600).price_with_greeks(option(), c, ["delta", "gamma"])
    for e, budget in [
        (engine(800, 800), 0.002),
        (engine(1600, 3200), 0.0001),
        (engine(2400, 1600, 600), 0.0001),
        (engine(1600, 1600, None), 0.002),
    ]:
        assert e.price(option(), c) == pytest.approx(fine.price, rel=0, abs=budget)
    up = engine(1600, 1600).price(
        option(),
        c.replace(spot_price=100.1),
    )
    down = engine(1600, 1600).price(
        option(),
        c.replace(spot_price=99.9),
    )
    assert fine.delta == pytest.approx((up - down) / 0.2, rel=0, abs=0.00001)
    assert fine.gamma == pytest.approx(
        (up + down - 2 * fine.price) / 0.01, rel=0, abs=0.00001
    )


def test_short_maturity_matches_bsm():
    o = option(end=START + timedelta(days=1))
    bsm = context().replace(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
    )
    actual = engine(12800, 1600).price_with_greeks(o, context(), ["delta", "gamma"])
    expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
        o, bsm, ["delta", "gamma"]
    )
    for value, ref, budget in [
        (actual.price, expected.price, 0.001),
        (actual.delta, expected.delta, 0.0001),
        (actual.gamma, expected.gamma, 0.0003),
    ]:
        assert value == pytest.approx(ref, rel=0, abs=budget)


def test_existing_schemes_and_explicit_stability():
    for scheme in [
        "explicit_euler",
        "implicit_euler",
        "crank_nicolson",
        "rannacher",
    ]:
        e = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=200,
            time_step_count=4000,
            scheme=scheme,
            asset_upper_boundary=400,
        )
        assert e.price(option(), context()) == pytest.approx(
            9.227005508154036, rel=0, abs=0.015
        )
    with pytest.raises(InvalidInputError):
        pricing.FiniteDifferenceVanillaEngine(
            scheme="explicit_euler", time_step_count=1
        ).price(option(), context())


def test_skew_prices_and_greeks_match_converged_quantlib_reference(subtests):
    previous_date = ql.Settings.instance().evaluationDate
    try:
        for direction in ["call", "put"]:
            for spot in [70.0, 100.0, 140.0]:
                with subtests.test(direction=direction, spot=spot):
                    coarse = quantlib_skew(1600, spot, direction)
                    fine = quantlib_skew(3200, spot, direction)
                    for a, b, budget in zip(
                        coarse[:3],
                        fine[:3],
                        [0.0002, 0.00001, 0.000001],
                        strict=True,
                    ):
                        assert a == pytest.approx(b, rel=0, abs=budget)
                    s = surface(((0.3, 0.15), (0.35, 0.2)))
                    # Check oracle interpolation and both price tails away from time knots.
                    for price in [70, 80, 100, 120, 140]:
                        assert_decimal_equal(
                            s.volatility(START + timedelta(days=182), price),
                            fine[3].localVol(182 / 365, price, True),
                        )
                    result = engine(1600, 1600).price_with_greeks(
                        option(direction),
                        context(s, spot),
                        ["delta", "gamma"],
                    )
                    for value, ref, budget in zip(
                        [result.price, result.delta, result.gamma],
                        fine[:3],
                        [0.002, 0.0002, 0.00003],
                        strict=True,
                    ):
                        assert value == pytest.approx(ref, rel=0, abs=budget)
    finally:
        ql.Settings.instance().evaluationDate = previous_date


def test_product_engine_matrix_including_terminal_shortcuts(subtests):
    for contract, names, terminal_price in terminal_cases():
        for name in names:
            for time in [START, END]:
                with subtests.test(
                    product=type(contract).__name__, engine=name, time=time
                ):
                    e = getattr(pricing, name)()
                    c = context().replace(valuation_time=time, spot_price=105)
                    if (
                        not name.startswith("FiniteDifference")
                        or name in HESTON_ENGINES
                    ):
                        with pytest.raises(UnsupportedOperationError):
                            e.price(contract, c)
                        continue
                    bsm = c.replace(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
                        )
                    )
                    result = e.price_with_greeks(contract, c, ["delta", "gamma"])
                    expected = e.price_with_greeks(contract, bsm, ["delta", "gamma"])
                    assert result.price == pytest.approx(
                        expected.price, rel=0, abs=0.002
                    )
                    for greek in ["delta", "gamma"]:
                        a, b = getattr(result, greek), getattr(expected, greek)
                        if b is None:
                            assert a is None
                        else:
                            assert a == pytest.approx(b, rel=0, abs=0.0002)
                    if time == END:
                        assert result.price == pytest.approx(
                            terminal_price, rel=0, abs=1e-10
                        )
                        assert result.require("delta") == pytest.approx(
                            terminal_delta(contract), rel=0, abs=1e-10
                        )
                        assert result.require("gamma") == 0
                    for request in [
                        "all",
                        ["delta", "vega"],
                    ]:
                        result = e.price_with_greeks(contract, c, request)
                        assert result.vega is None


def test_unsupported_greeks_solvers_expiry_and_coverage():
    c = context()
    for time in [START, END]:
        rolled = c.replace(valuation_time=time)
        baseline = engine().price_with_greeks(option(), rolled, ["delta", "gamma"])
        for greeks in ["all", ["delta", "vega"]]:
            result = engine().price_with_greeks(option(), rolled, greeks)
            assert result.price == baseline.price
            assert result.vega is None
            assert result.theta is None
            assert result.speed is None
            assert result.delta == baseline.delta
        result = engine().price_with_greeks(option(), rolled, "theta")
        assert result.price == baseline.price
        assert result.status("theta") == "unsupported"
        with pytest.raises(UnsupportedOperationError):
            pricing.implied_volatility(
                engine(),
                option(),
                rolled,
                10,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    terminal = engine().price_with_greeks(
        option(),
        c.replace(valuation_time=END, spot_price=110),
        ["delta", "gamma"],
    )
    assert terminal.price == 10
    assert_terminal_greeks(terminal, 1)
    with pytest.raises(InvalidInputError):
        engine().price(option(end=END + timedelta(days=1)), c)
    dated = RateTermStructure(
        reference_time=START, times=[START + timedelta(days=182)], factors=[0.98]
    )
    short = c.replace(model_parameters=c.model_parameters.replace(risk_free_rate=dated))
    with pytest.raises(InvalidInputError):
        engine().price(option(), short)


def test_nonflat_rates_and_shared_concurrent_pricing():
    risk = RateTermStructure(
        reference_time=START, times=[END], factors=[math.exp(-0.05)]
    )
    dividend = RateTermStructure(
        reference_time=START, times=[END], factors=[math.exp(-0.02)]
    )
    c = context().replace(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=risk, dividend_yield=dividend, surface=surface()
        )
    )
    assert c.model_parameters.risk_free_curve == risk
    assert c.model_parameters.dividend_curve == dividend
    e = engine(200, 200)
    expected = e.price(option(), c)
    assert expected == pytest.approx(9.227005508154036, rel=0, abs=0.02)
    with ThreadPoolExecutor(max_workers=4) as pool:
        assert (
            list(pool.map(lambda _: e.price(option(), c), range(8))) == [expected] * 8
        )


def test_signatures_and_example_are_discoverable():
    for cls in [LocalVolatilitySurface, LocalVolatilityParameters]:
        for parameter in inspect.signature(cls).parameters.values():
            assert parameter.kind == inspect.Parameter.KEYWORD_ONLY
    assert list(inspect.signature(LocalVolatilitySurface).parameters) == [
        "times",
        "spots",
        "values",
    ]
