"""Binary barrier FD prices, monitoring and settlement through the public API."""

import importlib
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from functools import partial

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    FiniteDifferenceBinaryBarrierEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

START = datetime(2025, 1, 1)
END = datetime(2026, 1, 1)


def market(spot=100, time=START, rate=0.04, dividend=0.01, sigma=0.2):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=sigma
        ),
        spot_price=spot,
        valuation_time=time,
    )


def touch(factory=ins.cash_touch_option, **changes):
    terms = {"effective_time": START, "expiry_time": END, "barrier_level": 120.13}
    terms["barrier_type"] = "up_and_in"
    if factory is ins.cash_touch_option:
        terms["payout"] = 10
    terms |= changes
    if terms["barrier_type"] in ("up_and_out", "down_and_out"):
        factory = (
            ins.cash_no_touch_option
            if factory is ins.cash_touch_option
            else ins.asset_no_touch_option
        )
    return factory(**terms)


def test_continuous_touch_settlements_match_analytics():
    engine = FiniteDifferenceBinaryBarrierEngine()
    for factory in (ins.cash_touch_option, ins.asset_touch_option):
        for kind in ("up_and_in", "up_and_out", "down_and_in", "down_and_out"):
            for timing in ("at_expiry", "at_hit"):
                if kind.endswith("_out") and timing == "at_hit":
                    continue
                option = touch(
                    factory,
                    barrier_type=kind,
                    barrier_level=79.87 if kind.startswith("down") else 120.13,
                    settlement_timing=timing,
                )
                assert engine.price(option, market()) == pytest.approx(
                    AnalyticBinaryBarrierEngine().price(option, market()),
                    rel=0,
                    abs=(0.02 if option.payoff_type == "asset" else 0.002),
                )


@pytest.mark.parametrize(
    "factory,amount", ((ins.cash_touch_option, 10), (ins.asset_touch_option, 130))
)
def test_current_and_prior_at_hit_payment(factory, amount):
    engine = FiniteDifferenceBinaryBarrierEngine()
    option = touch(factory, settlement_timing="at_hit")
    assert engine.price(option, market(130)) == amount
    assert (
        engine.price(
            option.replace(
                touch_state="touched",
            ),
            market(time=datetime(2025, 1, 2)),
        )
        == 0
    )


@pytest.mark.parametrize("up", (False, True))
@pytest.mark.parametrize("asset", (False, True))
def test_two_fixing_prices_and_grid_convergence_against_independent_integrals(
    up, asset
):
    coarse = FiniteDifferenceBinaryBarrierEngine(
        asset_step_count=100, time_step_count=100, asset_upper_boundary=400
    )
    fine = FiniteDifferenceBinaryBarrierEngine(
        asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
    )
    for kind, at_hit in (
        ("one_touch", False),
        ("one_touch", True),
        ("no_touch", False),
    ):
        factory = ins.asset_touch_option if asset else ins.cash_touch_option
        fields = {
            "barrier_level": 120.125 if up else 79.875,
            "barrier_type": f"{'up' if up else 'down'}_and_{'in' if kind == 'one_touch' else 'out'}",
            "observation_times": [datetime(2025, 6, 30), END],
        }
        if kind == "one_touch":
            fields["settlement_timing"] = "at_hit" if at_hit else "at_expiry"
        option = touch(factory, **fields)
        expected = two_fixing_price(
            up=up, knock_in=kind == "one_touch", asset=asset, at_hit=at_hit
        )
        value = fine.price(option, market())
        assert value == pytest.approx(expected, rel=0, abs=(0.001 if asset else 0.0001))
        assert abs(value - expected) < abs(coarse.price(option, market()) - expected)
    for knock_in in (False, True):
        for direction in ("call", "put"):
            factory = (
                ins.asset_binary_barrier_option
                if asset
                else ins.cash_binary_barrier_option
            )
            fields = {
                "option_type": direction,
                "strike": 100,
                "effective_time": START,
                "expiry_time": END,
                "barrier_level": 120.125 if up else 79.875,
                "barrier_type": f"{'up' if up else 'down'}_and_{'in' if knock_in else 'out'}",
                "observation_times": [datetime(2025, 6, 30), END],
            }
            if not asset:
                fields["payout"] = 10
            option = factory(**fields)
            expected = two_fixing_price(
                up=up, knock_in=knock_in, asset=asset, direction=direction
            )
            assert fine.price(option, market()) == pytest.approx(
                expected, rel=0, abs=(0.002 if asset else 0.0002)
            )


def test_node_curves_and_early_at_hit_horizon():
    fixing = datetime(2025, 6, 30)
    first = (fixing - START).days / 365
    rate1, rate2, dividend1, dividend2 = -0.02, 0.08, 0.03, -0.01
    rate = RateTermStructure(
        reference_time=START,
        times=[fixing, END],
        factors=[
            math.exp(-rate1 * first),
            math.exp(-rate1 * first - rate2 * (1 - first)),
        ],
    )
    dividend = RateTermStructure(
        reference_time=START,
        times=[fixing, END],
        factors=[
            math.exp(-dividend1 * first),
            math.exp(-dividend1 * first - dividend2 * (1 - first)),
        ],
    )
    ctx = market(rate=rate, dividend=dividend)
    engine = FiniteDifferenceBinaryBarrierEngine(
        asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
    )
    for asset in (False, True):
        for at_hit in (False, True):
            option = touch(
                ins.asset_touch_option if asset else ins.cash_touch_option,
                barrier_level=120.125,
                observation_times=[fixing, END],
                settlement_timing="at_hit" if at_hit else "at_expiry",
            )
            expected = two_fixing_price(
                up=True,
                knock_in=True,
                asset=asset,
                at_hit=at_hit,
                rate1=rate1,
                rate2=rate2,
                dividend1=dividend1,
                dividend2=dividend2,
            )
            assert engine.price(option, ctx) == pytest.approx(
                expected, rel=0, abs=(0.002 if asset else 0.0002)
            )
    short = RateTermStructure(reference_time=START, times=[fixing], factors=[0.97])
    ctx = market(rate=short, dividend=short)
    option = touch(
        observation_times=[fixing],
        settlement_timing="at_hit",
    )
    digital = ins.CashOrNothingOption(
        option_type="call",
        strike=option.barrier_level,
        payout=10,
        effective_time=START,
        expiry_time=fixing,
    )
    assert engine.price(option, ctx) == pytest.approx(
        AnalyticDigitalEngine().price(digital, ctx),
        rel=0,
        abs=0.0004,
    )
    later_payment = touch(observation_times=[fixing])
    assert_rejection(InvalidInputError, engine.price, later_payment, ctx)
    known = option.replace(
        touch_state="untouched",
    )
    after = market(130, datetime(2025, 6, 30, 0, 0, 0, 1))
    assert_rejection(MissingHistoryError, engine.price, option, after)
    assert engine.price(known, after) == 0
    assert engine.price(option, market(130, fixing)) == 10
    no_touch = touch(
        barrier_type="up_and_out",
        observation_times=[fixing],
        touch_state="untouched",
    )
    assert engine.price(no_touch, after) == pytest.approx(
        10 * math.exp(-0.04 * (END - fixing).days / 365), rel=0, abs=1e-8
    )


def test_history_expiry_greeks_and_solvers_preserve_inputs():
    engine = FiniteDifferenceBinaryBarrierEngine(asset_upper_boundary=400)
    option = touch(touch_state="untouched")
    ctx = market(time=datetime(2025, 1, 2))
    result = engine.price_with_greeks(option, ctx, greeks="all")
    greeks = (
        "delta",
        "gamma",
        "speed",
        "theta",
        "charm",
        "color",
        "vega",
        "vanna",
        "zomma",
        "rho",
    )
    assert all(math.isfinite(getattr(result, g)) for g in greeks)
    h = 0.01
    up = engine.price(
        option,
        market(100 + h, datetime(2025, 1, 2)),
    )
    down = engine.price(
        option,
        market(100 - h, datetime(2025, 1, 2)),
    )
    assert result.delta == pytest.approx((up - down) / (2 * h), rel=0, abs=1e-6)
    assert result.gamma == pytest.approx(
        (up - 2 * result.price + down) / h**2, rel=0, abs=1e-6
    )
    assert implied_volatility(
        engine,
        option,
        ctx,
        result.price,
        lower_bound=0.1,
        upper_bound=0.3,
    ) == pytest.approx(0.2, rel=0, abs=1e-7)
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        option,
        ctx,
        -1,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert_rejection(MissingHistoryError, engine.price, touch(), ctx)
    assert_rejection(
        InvalidInputError,
        engine.price,
        option.replace(
            touch_state="touched",
        ),
        market(),
    )
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        option,
        ctx,
        ["delta"],
        spot_shift=0,
    )
    for factory in (ins.cash_touch_option, ins.asset_touch_option):
        for kind in ("up_and_in", "up_and_out"):
            contract = touch(factory, barrier_type=kind, touch_state="untouched")
            expiry = engine.price_with_greeks(
                contract,
                market(130, END),
                greeks="all",
            )
            assert_terminal_greeks(
                expiry,
                int(factory is ins.asset_touch_option and kind == "up_and_in"),
            )
    equality = engine.price_with_greeks(touch(), market(120.13), greeks="all")
    assert equality.delta is None
    assert equality.gamma is None
    paid = touch(settlement_timing="at_hit", touch_state="touched")
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        paid,
        ctx,
        0,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert engine.price(option, ctx) == result.price
    assert option.touch_state == "untouched"
    assert ctx.spot_price == 100
    with ThreadPoolExecutor(4) as pool:
        assert (
            list(
                pool.map(
                    lambda _: engine.price(option, ctx),
                    range(16),
                )
            )
            == [result.price] * 16
        )


def test_continuous_domain_retains_configured_nodes_and_stability():
    option = touch(barrier_level=120)

    def engine(upper, scheme="rannacher"):
        return FiniteDifferenceBinaryBarrierEngine(
            asset_step_count=800,
            time_step_count=2000,
            scheme=scheme,
            asset_upper_boundary=upper,
        )

    expected = engine(400).price(option, market())
    for upper in (10_000, 1e9):
        assert engine(upper).price(option, market()) == expected
    assert_rejection(
        InvalidInputError, engine(480, "explicit_euler").price, option, market()
    )


def test_settings_protocols_defaults_and_errors():
    engine = FiniteDifferenceBinaryBarrierEngine()
    assert (
        (
            engine.asset_step_count,
            engine.time_step_count,
            engine.scheme,
            engine.asset_upper_boundary,
        )
    ) == ((800, 400, "rannacher", None))
    for name in ("asset_step_count", "time_step_count"):
        for value in (True, 2.5, 4.0):
            with pytest.raises(TypeError):
                FiniteDifferenceBinaryBarrierEngine(**{name: value})
        with pytest.raises(OverflowError):
            FiniteDifferenceBinaryBarrierEngine(**{name: 2**40})
    with pytest.raises(TypeError):
        FiniteDifferenceBinaryBarrierEngine(asset_upper_boundary=True)
    with pytest.raises(TypeError):
        FiniteDifferenceBinaryBarrierEngine(unexpected=1)
    with pytest.raises(ValueError):
        FiniteDifferenceBinaryBarrierEngine(scheme="unknown")
    for fields in (
        {"asset_step_count": 2},
        {"time_step_count": 0},
        {"asset_upper_boundary": math.nan},
    ):
        assert_rejection(
            InvalidInputError, FiniteDifferenceBinaryBarrierEngine, **fields
        )
    for fields in (
        {"asset_upper_boundary": 120},
        {"scheme": "explicit_euler", "time_step_count": 1},
    ):
        assert_rejection(
            InvalidInputError,
            FiniteDifferenceBinaryBarrierEngine(**fields).price,
            touch(),
            market(),
        )
    with pytest.raises(InvalidInputError):
        touch(observation_times=[datetime(2025, 1, 4)])
    with pytest.raises(TypeError):
        engine.price(object(), market())
    with pytest.raises(TypeError):
        numerical_greeks(
            engine,
            touch(),
            market(),
            ["delta"],
            spot_shift=True,
        )
    with pytest.raises(TypeError):
        implied_volatility(
            engine,
            touch(),
            market(),
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(AttributeError):
        engine.time_step_count = 1


def test_existing_independent_quantlib_price_scenarios(subtests, quantlib_settings):

    reference = importlib.import_module("quantlib_reference.binary")
    engine = FiniteDifferenceBinaryBarrierEngine(
        asset_step_count=3200, time_step_count=800
    )
    for identifier, inputs in reference.scenarios():
        with subtests.test(identifier=identifier):
            kind, asset = inputs["BarrierType"], inputs["asset_settlement"]
            fields = {
                "effective_time": datetime.fromisoformat(inputs["effective_time"]),
                "expiry_time": datetime.fromisoformat(inputs["expiry_time"]),
                "barrier_level": inputs["barrier"],
                "touch_state": "untouched",
            }
            if not asset:
                fields["payout"] = inputs["payout"]
            if inputs["option"] == "none":
                factory = (
                    (ins.asset_no_touch_option if asset else ins.cash_no_touch_option)
                    if kind.endswith("_out")
                    else (ins.asset_touch_option if asset else ins.cash_touch_option)
                )
                option = factory(
                    **fields, barrier_type=kind, settlement_timing=inputs["settlement"]
                )
            else:
                option = (
                    ins.asset_binary_barrier_option
                    if asset
                    else ins.cash_binary_barrier_option
                )(
                    **fields,
                    option_type=inputs["option"],
                    strike=inputs["strike"],
                    barrier_type=kind,
                )
            ctx = market(
                inputs["spot"],
                datetime.fromisoformat(inputs["valuation"]),
                rate=inputs["rate"],
                dividend=inputs["dividend"],
                sigma=inputs["volatility"],
            )
            assert engine.price(option, ctx) == pytest.approx(
                reference.measure(inputs, "price"),
                rel=0,
                abs=reference.barrier.FD["price"],
            )

            if (
                inputs["spot"],
                inputs["strike"],
                (option.expiry_time - ctx.valuation_time).days,
            ) == (100, 100, 365):
                result = engine.price_with_greeks(option, ctx, greeks="all")
                for greek in (
                    "delta",
                    "gamma",
                    "speed",
                    "theta",
                    "charm",
                    "color",
                    "vega",
                    "vanna",
                    "zomma",
                    "rho",
                ):
                    assert getattr(result, greek) == pytest.approx(
                        reference.measure(inputs, greek),
                        rel=0,
                        abs=reference.barrier.FD[greek],
                    ), f"{identifier}: {greek}"


def two_fixing_price(
    *,
    up,
    knock_in,
    asset,
    direction=None,
    at_hit=False,
    rate1=0.04,
    rate2=0.04,
    dividend1=0.01,
    dividend2=0.01,
):
    """Integrate conditional lognormal payoffs, splitting at the first trigger.

    Parameters
    ----------
    up : bool
        Whether the barrier triggers upward.
    knock_in : bool
        Whether touching activates the terminal payoff.
    asset : bool
        Whether settlement pays the asset rather than fixed cash.
    direction : {'call', 'put'} or None, optional
        Terminal payoff direction; touch contracts have no strike direction.
    at_hit : bool, optional
        Whether a one-touch pays at the hitting time.
    rate1 : float, optional
        Continuously compounded rate before the first fixing.
    rate2 : float, optional
        Continuously compounded rate after the first fixing.
    dividend1 : float, optional
        Dividend yield before the first fixing.
    dividend2 : float, optional
        Dividend yield after the first fixing.

    Returns
    -------
    float
        Independent discounted payoff value.
    """
    first = (datetime(2025, 6, 30) - START).days / 365
    remaining = 1 - first
    level = 120.125 if up else 79.875
    sigma = 0.2
    normal = lambda x: 0.5 * math.erfc(-x / math.sqrt(2))
    cut = (math.log(level / 100) - (rate1 - dividend1 - sigma**2 / 2) * first) / (
        sigma * math.sqrt(first)
    )

    def integrand(z):
        spot = 100 * math.exp(
            (rate1 - dividend1 - sigma**2 / 2) * first + sigma * math.sqrt(first) * z
        )
        hit = spot >= level if up else spot <= level
        if hit and not knock_in:
            return 0.0
        if hit and at_hit:
            value = spot if asset else 10.0
        else:
            lower, upper = (
                (100.0, math.inf)
                if direction == "call"
                else (0.0, 100.0)
                if direction == "put"
                else (0.0, math.inf)
            )
            if not hit:
                if up == knock_in:
                    lower = max(lower, level)
                else:
                    upper = min(upper, level)
            if lower >= upper:
                return 0.0
            drift = (
                rate2 - dividend2 + (0.5 if asset else -0.5) * sigma**2
            ) * remaining
            width = sigma * math.sqrt(remaining)
            low = (
                0.0 if lower == 0 else normal((math.log(lower / spot) - drift) / width)
            )
            high = (
                1.0
                if math.isinf(upper)
                else normal((math.log(upper / spot) - drift) / width)
            )
            value = (
                spot * math.exp(-dividend2 * remaining)
                if asset
                else 10 * math.exp(-rate2 * remaining)
            ) * (high - low)
        return math.exp(-rate1 * first - z * z / 2) / math.sqrt(2 * math.pi) * value

    total = 0.0
    for left, right in ((-10.0, cut), (cut, 10.0)):
        count = 4000
        width = (right - left) / count
        # Midpoints avoid assigning a finite weight to the trigger discontinuity.
        total += width * sum(integrand(left + (i + 0.5) * width) for i in range(count))
    return total
