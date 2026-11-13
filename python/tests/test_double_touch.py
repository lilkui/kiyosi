"""Double-touch cash contracts and references independent of the PDE."""

import inspect
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from types import SimpleNamespace

import pytest
import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import DoubleTouchOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import numerical_greeks
from price_differences import price_differences
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
FIRST = START + timedelta(days=180)


def contract(barrier_type="knock_in", **changes):
    return DoubleTouchOption(
        barrier_type=barrier_type,
        **(
            {
                "effective_time": START,
                "expiry_time": END,
                "lower_barrier": 80,
                "upper_barrier": 120,
                "payout": 1,
            }
            | changes
        ),
    )


def market(spot=100, *, time=START, volatility=0.2, rate=0.03, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=time,
    )


def continuous_reference(
    spot=100,
    *,
    lower=80,
    upper=120,
    volatility=0.2,
    rate=0.03,
    dividend=0.01,
    count=1024,
):
    """Integrate log-Brownian survival and discounted exit probabilities.

    Parameters
    ----------
    spot : float, optional
        Underlying spot price.
    lower : float, optional
        Lower boundary of the surviving interval.
    upper : float, optional
        Upper boundary of the surviving interval.
    volatility : float, optional
        Annualized volatility in decimal units.
    rate : float, optional
        Annual continuously compounded risk-free rate.
    dividend : float, optional
        Annual continuously compounded dividend yield.
    count : int, optional
        Integration panel count or series truncation budget.

    Returns
    -------
    tuple of float
        Expiry one-touch, at-hit one-touch and no-touch values, in that
        order.

    Notes
    -----
    The sine expansion solves the absorbing heat equation between log
    boundaries. An exponential tilt restores log-price drift.
    """
    width, x = math.log(upper / lower), math.log(spot / lower)
    drift = rate - dividend - volatility**2 / 2
    alpha = drift / volatility**2
    survival_terms, integral_terms = [], []
    for n in range(1, count + 1):
        frequency = n * math.pi / width
        coefficient = (
            2
            / width
            * math.exp(-alpha * x)
            * math.sin(frequency * x)
            * frequency
            * (1 - (-1) ** n * math.exp(alpha * width))
            / (alpha**2 + frequency**2)
        )
        decay = (volatility * frequency) ** 2 / 2 + drift**2 / (2 * volatility**2)
        survival_terms.append(coefficient * math.exp(-decay))
        integral_terms.append(
            coefficient * -math.expm1(-(rate + decay)) / (rate + decay)
        )
    untouched = math.exp(-rate) * math.fsum(survival_terms)
    return (
        math.exp(-rate) - untouched,
        1 - untouched - rate * math.fsum(integral_terms),
        untouched,
    )


def cash_reference(spot, *, volatility=0.2, rate=0.03, time=1, count=64):
    """Compute analytic spot derivatives of the killed log-Brownian solution.

    Parameters
    ----------
    spot : float
        Underlying spot price.
    volatility : float, optional
        Annualized volatility in decimal units.
    rate : float, optional
        Annual continuously compounded risk-free rate.
    time : float, optional
        Remaining Actual/365 Fixed years.
    count : int, optional
        Integration panel count or series truncation budget.

    Returns
    -------
    list of types.SimpleNamespace
        Expiry one-touch, at-hit one-touch and no-touch prices with spot
        Greeks.
    """
    width, x = math.log(120 / 80), math.log(spot / 80)
    alpha = (rate - 0.01 - volatility**2 / 2) / volatility**2
    beta = math.sqrt(alpha**2 + 2 * rate / volatility**2)
    plus, minus = -alpha + beta, -alpha - beta
    weight = -math.expm1(minus * width) / (
        math.exp(plus * width) - math.exp(minus * width)
    )
    survival, transient = [0.0] * 4, [0.0] * 4
    for n in range(1, count + 1):
        k = n * math.pi / width
        coefficient = 2 * k / width * (1 - (-1) ** n * math.exp(alpha * width))
        decay = volatility**2 / 2 * (k * k + alpha * alpha) + rate
        factor = coefficient * math.exp(-alpha * x - decay * time)
        sine, cosine = math.sin(k * x), math.cos(k * x)
        derivatives = (
            sine,
            k * cosine - alpha * sine,
            (alpha * alpha - k * k) * sine - 2 * alpha * k * cosine,
            (3 * alpha * k * k - alpha**3) * sine
            + (3 * alpha * alpha * k - k**3) * cosine,
        )
        for order, derivative in enumerate(derivatives):
            survival[order] += factor * derivative / (k * k + alpha * alpha)
            transient[order] += factor * derivative / (k * k + beta * beta)
    # Subtract the transient from the stationary exit transform so third derivatives converge.
    at_hit = [
        weight * plus**order * math.exp(plus * x)
        + (1 - weight) * minus**order * math.exp(minus * x)
        - transient[order]
        for order in range(4)
    ]
    at_expiry = [math.exp(-rate * time) - survival[0]] + [
        -value for value in survival[1:]
    ]
    return [
        SimpleNamespace(
            price=value[0],
            delta=value[1] / spot,
            gamma=(value[2] - value[1]) / spot**2,
            speed=(value[3] - 3 * value[2] + 2 * value[1]) / spot**3,
        )
        for value in (at_expiry, at_hit, survival)
    ]


def reference_greeks(spot):
    shift = 0.0001
    base = cash_reference(spot)
    vol_up, vol_down = (
        cash_reference(spot, volatility=0.2 + shift),
        cash_reference(spot, volatility=0.2 - shift),
    )
    rate_up, rate_down = (
        cash_reference(spot, rate=0.03 + shift),
        cash_reference(spot, rate=0.03 - shift),
    )
    after = cash_reference(spot, time=1 - 1 / 365)
    return [
        {
            "delta": b.delta,
            "gamma": b.gamma,
            "speed": b.speed,
            "theta": (t.price - b.price) * 365,
            "charm": (t.delta - b.delta) * 365,
            "color": (t.gamma - b.gamma) * 365,
            "vega": (u.price - d.price) / (2 * shift),
            "vanna": (u.delta - d.delta) / (2 * shift),
            "zomma": (u.gamma - d.gamma) / (2 * shift),
            "rho": (r.price - s.price) / (2 * shift),
        }
        for b, u, d, r, s, t in zip(
            base, vol_up, vol_down, rate_up, rate_down, after, strict=True
        )
    ]


def normal(x):
    return math.erfc(-x / math.sqrt(2)) / 2


def survival_at_fixing(spot, time):
    deviation = 0.2 * math.sqrt(time)
    return normal(math.log(120.125 / spot) / deviation) - normal(
        math.log(80.125 / spot) / deviation
    )


def scheduled_reference(dates, count=1024):
    """Integrate survival through one or two independent normal increments.

    Parameters
    ----------
    dates : sequence of datetime.date
        One or two monitoring dates.
    count : int, optional
        Integration panel count or series truncation budget.

    Returns
    -------
    tuple of float
        Expiry one-touch, at-hit one-touch and no-touch values, in that
        order.
    """
    time = (dates[0] - START).days / 365
    survived_first = survival_at_fixing(100.25, time)
    survival = survived_first
    if len(dates) == 2:
        deviation = 0.2 * math.sqrt(time)
        left, right = (
            math.log(80.125 / 100.25) / deviation,
            math.log(120.125 / 100.25) / deviation,
        )
        step = (right - left) / count

        def integrand(z):
            return (
                survival_at_fixing(100.25 * math.exp(deviation * z), 1 - time)
                * math.exp(-z * z / 2)
                / math.sqrt(2 * math.pi)
            )

        total = integrand(left) + integrand(right)
        total += math.fsum(
            (4 if i % 2 else 2) * integrand(left + i * step) for i in range(1, count)
        )
        survival = step * total / 3
    return (
        math.exp(-0.03) * (1 - survival),
        math.exp(-0.03 * time) * (1 - survived_first)
        + math.exp(-0.03 * (1 if len(dates) == 2 else time))
        * (survived_first - survival),
        math.exp(-0.03) * survival,
    )


def test_constructor_defaults_owning_schedules_and_history():
    option = contract()
    assert isinstance(option, DoubleTouchOption)
    assert option.is_one_touch
    assert option.settlement_timing == "at_expiry"
    assert option.observation_mode == "continuous"
    assert option.touch_state is None
    assert option.observation_times is None
    assert option.payout == 1
    replaced = option.replace(
        touch_state="untouched",
    )
    assert replaced.touch_state == "untouched"
    assert option.touch_state is None
    assert replaced.replace(touch_state=None) == option
    dates = [FIRST, END]
    scheduled = contract("knock_out", observation_times=dates)
    dates.clear()
    copy = list(scheduled.observation_times)
    copy.clear()
    assert scheduled.observation_times == ((FIRST, END))
    assert not (scheduled.is_one_touch)
    assert scheduled.settlement_timing == "at_expiry"
    assert "DoubleTouchOption" in repr(scheduled)
    with pytest.raises(AttributeError):
        option.payout = 2


def test_protocols_domain_errors_and_failed_replace_preserve_inputs(subtests):
    for barrier_type in ("knock_in", "knock_out"):
        for key in ("payout", "lower_barrier", "upper_barrier"):
            for value in (True, "100", object()):
                with (
                    subtests.test(barrier_type=barrier_type, key=key, value=value),
                    pytest.raises(TypeError),
                ):
                    contract(barrier_type, **{key: value})
            for value in (0, -1, math.inf, math.nan):
                assert_rejection(
                    InvalidInputError,
                    lambda barrier_type=barrier_type, key=key, value=value: contract(
                        barrier_type, **{key: value}
                    ),
                )
        for changes, expected_error in (
            ({"lower_barrier": 120}, InvalidInputError),
            ({"lower_barrier": 121}, InvalidInputError),
            (
                {"expiry_time": START - timedelta(days=1)},
                InvalidInputError,
            ),
            ({"observation_times": []}, InvalidInputError),
            (
                {
                    "observation_times": [END, FIRST],
                },
                InvalidInputError,
            ),
        ):
            assert_rejection(
                expected_error,
                lambda barrier_type=barrier_type, changes=changes: contract(
                    barrier_type, **changes
                ),
            )
    with pytest.raises(InvalidInputError):
        contract("knock_out", settlement_timing="at_hit")
    with pytest.raises(ValueError):
        contract(settlement_timing="later")
    option = contract()
    with pytest.raises(TypeError):
        option.replace(
            touch_state=True,
        )
    assert option == contract()


def test_continuous_prices_match_independent_series_and_converge():
    options = (
        contract(),
        contract(settlement_timing="at_hit"),
        contract("knock_out"),
    )
    reference = continuous_reference()
    for left, right in zip(reference, continuous_reference(count=2048), strict=True):
        assert left == pytest.approx(right, rel=0, abs=1e-9)
    coarse = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=80, time_step_count=100
    )
    fine = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=800, time_step_count=1600
    )
    for option, expected in zip(options, reference, strict=True):
        actual = fine.price(option, market())
        assert abs(actual - expected) < abs(coarse.price(option, market()) - expected)
        assert actual == pytest.approx(expected, rel=0, abs=1e-5)
    scaled = (
        contract(payout=7.5),
        contract(payout=7.5, settlement_timing="at_hit"),
        contract("knock_out", payout=7.5),
    )
    for option, expected in zip(scaled, reference, strict=True):
        assert fine.price(option, market()) == pytest.approx(
            7.5 * expected, rel=0, abs=7.5e-5
        )
    assert fine.price(options[0], market()) + fine.price(
        options[2], market()
    ) == pytest.approx(math.exp(-0.03), rel=0, abs=1e-8)
    # Nonzero log drift and a negative rate exercise the tilted series separately.
    for rate, dividend in ((0.04, 0.0), (-0.01, 0.02)):
        expected = continuous_reference(rate=rate, dividend=dividend)
        for option, value in zip(options, expected, strict=True):
            assert fine.price(
                option,
                market(rate=rate, dividend=dividend),
            ) == pytest.approx(value, rel=0, abs=1e-5)


def test_scheduled_one_two_fixings_early_window_and_outside_continuation():
    engine = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
    )
    for dates in ([END], [FIRST, END], [FIRST]):
        changes = {
            "lower_barrier": 80.125,
            "upper_barrier": 120.125,
            "observation_times": dates,
        }
        options = (
            contract(**changes),
            contract(**changes, settlement_timing="at_hit"),
            contract("knock_out", **changes),
        )
        expected = scheduled_reference(dates)
        for a, b in zip(expected, scheduled_reference(dates, 2048), strict=True):
            assert a == pytest.approx(b, rel=0, abs=1e-10)
        for option, value in zip(options, expected, strict=True):
            assert engine.price(option, market(100.25)) == pytest.approx(
                value, rel=0, abs=3e-5
            )
    future = contract(
        "knock_out",
        lower_barrier=80.125,
        upper_barrier=120.125,
        observation_times=[END],
    )
    assert engine.price(future, market(125)) == pytest.approx(
        math.exp(-0.03) * survival_at_fixing(125, 1), rel=0, abs=3e-5
    )


def test_history_current_touch_past_payments_and_expiry():
    engine = pricing.FiniteDifferenceDoubleTouchEngine()
    later = START + timedelta(days=1)
    for timing in ("at_hit", "at_expiry"):
        option = contract(payout=3, settlement_timing=timing)
        for spot in (79, 80, 120, 121):
            assert engine.price(option, market(spot)) == pytest.approx(
                (3 if timing == "at_hit" else 3 * math.exp(-0.03)), rel=0, abs=1e-12
            )
            assert_rejection(
                MissingHistoryError,
                lambda spot=spot, option=option: engine.price(
                    option,
                    market(spot, time=later),
                ),
            )
        known = option.replace(
            touch_state="touched",
        )
        assert_rejection(
            InvalidInputError,
            lambda known=known: engine.price(known, market()),
        )
        expected = 0 if timing == "at_hit" else 3 * math.exp(-0.03 * 364 / 365)
        assert engine.price(known, market(time=later)) == pytest.approx(
            expected, rel=0, abs=1e-12
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda known=known, expected=expected: pricing.implied_volatility(
                engine,
                known,
                market(time=later),
                expected,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
    no_touch = contract("knock_out", payout=3)
    for spot in (80, 120):
        assert engine.price(no_touch, market(spot)) == 0
    for option in (
        no_touch,
        contract(payout=3),
        contract(payout=3, settlement_timing="at_hit"),
    ):
        known = option.replace(
            touch_state="untouched",
        )
        result = engine.price_with_greeks(known, market(time=END), greeks="all")
        assert result.price == (0 if known.is_one_touch else 3)
        assert_terminal_greeks(result, 0)
        for spot in (80, 120):
            assert engine.price(
                known,
                market(spot, time=END),
            ) == (3 if known.is_one_touch else 0)


def test_early_window_utc_fixing_calendar_and_fixed_cash_greeks():
    engine = pricing.FiniteDifferenceDoubleTouchEngine()
    option = contract(
        "knock_out",
        payout=2,
        observation_times=[FIRST],
    )
    event = datetime.combine(FIRST, datetime.min.time())
    assert engine.price(option, market(120, time=event)) == 0
    after = event + timedelta(seconds=1)
    assert_rejection(
        MissingHistoryError,
        lambda: engine.price(option, market(130, time=after)),
    )
    known = option.replace(
        touch_state="untouched",
    )
    for offset in (timedelta(seconds=1), timedelta(hours=12)):
        time = event + offset
        remaining = (
            datetime.combine(END, datetime.min.time()) - time
        ).total_seconds() / (86400 * 365)
        expected = 2 * math.exp(-0.03 * remaining)
        context = market(130, time=time)
        result = engine.price_with_greeks(known, context, greeks="all")
        assert result.price == pytest.approx(expected, rel=0, abs=1e-12)
        assert result.delta == 0
        assert result.vega == 0
        assert result.rho == pytest.approx(-remaining * expected, rel=0, abs=0.000001)
        assert_rejection(
            UnsupportedOperationError,
            lambda context=context, expected=expected: pricing.implied_volatility(
                engine,
                known,
                context,
                expected,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
    with pytest.raises(InvalidInputError):
        contract(
            "knock_out",
            observation_times=[datetime(2025, 1, 4)],
        )


def test_greeks_iv_boundary_and_time_stencils():
    engine = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=800, time_step_count=1600
    )
    for option in (
        contract(touch_state="untouched"),
        contract(settlement_timing="at_hit", touch_state="untouched"),
        contract("knock_out", touch_state="untouched"),
    ):
        result = engine.price_with_greeks(option, market(), greeks="all")
        bumped = price_differences(engine, option, market())
        for name in (
            "delta",
            "gamma",
            "vega",
            "rho",
            "theta",
            "charm",
            "color",
            "speed",
            "vanna",
            "zomma",
        ):
            assert getattr(result, name) == pytest.approx(
                getattr(bumped, name), rel=0, abs=1e-5
            )
        solved = pricing.implied_volatility(
            engine,
            option,
            market(),
            result.price,
            lower_bound=0.15,
            upper_bound=0.25,
        )
        assert solved == pytest.approx(0.2, rel=0, abs=1e-6)
        assert_rejection(
            RootNotBracketedError,
            lambda option=option: pricing.implied_volatility(
                engine,
                option,
                market(),
                2,
                lower_bound=0.15,
                upper_bound=0.25,
            ),
        )
        for spot in (80, 120):
            boundary = engine.price_with_greeks(option, market(spot), greeks="all")
            assert boundary.delta is None
            assert boundary.gamma is None
            assert boundary.vega == 0
    scheduled = contract(
        "knock_out",
        observation_times=[FIRST, END],
        touch_state="untouched",
    )
    result = engine.price_with_greeks(
        scheduled,
        market(time=FIRST),
        ["theta", "charm", "color"],
    )
    assert result.theta is None
    assert result.charm is None
    assert result.color is None


@pytest.mark.parametrize("spot", (80.02, 119.98))
def test_near_barrier_greeks_converge_to_independent_series(subtests, spot):
    options = (
        contract(touch_state="untouched"),
        contract(settlement_timing="at_hit", touch_state="untouched"),
        contract("knock_out", touch_state="untouched"),
    )
    # Absolute budgets use the library's per-unit, per-percent and per-day Greek units.
    budgets = {
        "delta": 1e-7,
        "gamma": 1e-7,
        "speed": 1e-5,
        "theta": 0.00000365,
        "charm": 0.00000365,
        "color": 0.00000365,
        "vega": 0.000001,
        "vanna": 0.00001,
        "zomma": 0.0001,
        "rho": 0.000001,
    }
    coarse = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=800, time_step_count=200
    )
    fine = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=19200, time_step_count=1600
    )
    expected = reference_greeks(spot)
    more_terms = cash_reference(spot, count=128)
    integrated = continuous_reference(spot, count=16384)
    for index, option in enumerate(options):
        with subtests.test(spot=spot, option=option):
            reference = expected[index]
            price = cash_reference(spot)[index].price
            assert price == pytest.approx(integrated[index], rel=0, abs=1e-9)
            for name in ("gamma", "speed"):
                assert reference[name] == pytest.approx(
                    getattr(more_terms[index], name), rel=0, abs=1e-14
                )
            rough = coarse.price_with_greeks(option, market(spot), greeks="all")
            refined = fine.price_with_greeks(option, market(spot), greeks="all")
            assert refined.price == pytest.approx(price, rel=0, abs=1e-7)
            for name, budget in budgets.items():
                with subtests.test(greek=name, budget=budget):
                    rough_error = abs(getattr(rough, name) - reference[name])
                    refined_error = abs(getattr(refined, name) - reference[name])
                    assert refined_error < rough_error
                    assert refined_error < budget
                    if name in ("gamma", "speed"):
                        assert rough_error > budget


@pytest.mark.parametrize("early", (False, True))
def test_node_curves_and_early_at_hit_curve_horizon(early):
    from kiyosi.instruments import CashOrNothingOption

    engine = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
    )
    horizon = FIRST if early else END
    rate = RateTermStructure(
        reference_time=START,
        times=[FIRST] if early else [FIRST, END],
        factors=[0.97] if early else [0.97, 0.99],
    )
    context = market(125, rate=rate)
    fields = {
        "lower_barrier": 80.125,
        "upper_barrier": 120.125,
        "observation_times": [horizon],
    }
    touched = sum(
        pricing.AnalyticDigitalEngine().price(
            CashOrNothingOption(
                option_type=direction,
                strike=level,
                payout=1,
                effective_time=START,
                expiry_time=horizon,
            ),
            context,
        )
        for direction, level in (("put", 80.125), ("call", 120.125))
    )
    option = contract(settlement_timing="at_hit", **fields)
    assert engine.price(option, context) == pytest.approx(touched, rel=0, abs=3e-5)
    if early:
        assert_rejection(
            InvalidInputError,
            lambda fields=fields, context=context: engine.price(
                contract(**fields), context
            ),
        )
    else:
        no = contract("knock_out", **fields)
        assert engine.price(no, context) == pytest.approx(
            0.99 - touched, rel=0, abs=3e-5
        )


def test_narrow_band_tiny_volatility_settings_and_concurrency():
    engine = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=160, time_step_count=800
    )
    option = contract("knock_out", lower_barrier=99.5, upper_barrier=100.5)
    assert engine.price(option, market()) < 1e-8
    assert engine.price(
        option,
        market(volatility=1e-12, dividend=0.03),
    ) == pytest.approx(math.exp(-0.03), rel=0, abs=1e-8)
    assert_rejection(
        InvalidInputError,
        lambda: pricing.FiniteDifferenceDoubleTouchEngine(asset_step_count=2),
    )
    with pytest.raises(TypeError):
        pricing.FiniteDifferenceDoubleTouchEngine(asset_step_count=True)
    with pytest.raises(TypeError):
        pricing.FiniteDifferenceDoubleTouchEngine(time_step_count=1.5)
    unstable = pricing.FiniteDifferenceDoubleTouchEngine(
        asset_step_count=400, time_step_count=1, scheme="explicit_euler"
    )
    assert_rejection(
        InvalidInputError,
        lambda: unstable.price(option, market()),
    )
    with ThreadPoolExecutor(max_workers=2) as executor:
        values = list(
            executor.map(
                lambda _: engine.price(option, market()),
                range(4),
            )
        )
    assert values == [engine.price(option, market())] * 4


def test_analytic_exit_density_series_and_discounted_peaks():
    engine = pricing.AnalyticDoubleTouchEngine()
    for sigma in (0.25, 0.6):
        for rate in (-0.03, 0.03):
            expected = continuous_reference(volatility=sigma, rate=rate, count=8192)[1]
            actual = engine.price(
                contract(settlement_timing="at_hit"),
                market(rate=rate, volatility=sigma),
            )
            assert actual == pytest.approx(expected, rel=0, abs=1e-11)
    for payout in (1, 1e300):
        band, x = math.log(120 / 80), math.log(100 / 80)
        expected = math.exp(
            math.log(payout)
            + math.log(math.cosh(200 * (x - band / 2)))
            - math.log(math.cosh(100 * band))
        )
        actual = engine.price(
            contract(payout=payout, settlement_timing="at_hit"),
            market(rate=800, dividend=799.98),
        )
        assert actual / expected == pytest.approx(1, rel=0, abs=1e-10)
    for rate in (-0.03, 0.03):
        k = math.sqrt(2 * abs(rate)) / 0.2
        half_band = math.log(100.5 / 99.5) / 2
        offset = math.log(100 / 99.5) - half_band
        transform = math.cosh if rate > 0 else math.cos
        expected = transform(k * offset) / transform(k * half_band)
        actual = engine.price(
            contract(
                lower_barrier=99.5,
                upper_barrier=100.5,
                settlement_timing="at_hit",
            ),
            market(rate=rate, dividend=rate - 0.02),
        )
        assert actual == pytest.approx(expected, rel=0, abs=1e-11)
    for payout in (1, 1e300):
        option = contract(
            lower_barrier=70,
            upper_barrier=140,
            payout=payout,
            expiry_time=START + timedelta(days=1),
            settlement_timing="at_hit",
        )
        assert (
            engine.price(
                option,
                market(rate=1e-10, dividend=0.03, volatility=0.01),
            )
            == 0
        )


@pytest.mark.parametrize("up", (True, False))
@pytest.mark.parametrize("sigma", (0.001, 1e-06, 1e-12))
def test_analytic_early_at_hit_payments_avoid_negative_rate_cancellation(up, sigma):
    engine = pricing.AnalyticDoubleTouchEngine()
    rate = -40
    dividend = rate - 0.03 if up else rate + 0.03 - sigma**2
    drift = rate - dividend - sigma**2 / 2
    towards = drift if up else -drift
    option = contract(
        lower_barrier=80 if up else 100 / 1.003,
        upper_barrier=100.3 if up else 125,
        settlement_timing="at_hit",
    )
    # The far barrier and hits after expiry contribute below 1e-140.
    expected = math.exp(
        -2
        * rate
        * math.log(1.003)
        / (towards + math.sqrt(towards**2 + 2 * rate * sigma**2))
    )
    actual = engine.price(
        option,
        market(rate=rate, dividend=dividend, volatility=sigma),
    )
    assert actual / expected == pytest.approx(1, rel=0, abs=1e-9)


def test_analytic_discount_and_payout_scaling_preserve_representable_prices(subtests):
    engine = pricing.AnalyticDoubleTouchEngine()
    # Independent driftless Fourier survival/exit series with cash in the exponent.
    for rate, payout, one, no, hit in (
        (
            -800,
            1e-300,
            1.6945880848984272e47,
            1.0317864872142041e47,
            1.5506138528255889e44,
        ),
        (800, 1e300, 2.2797808602039585e-48, 1.3880937239736419e-48, None),
    ):
        context = market(rate=rate, dividend=rate - 0.02)
        for barrier_type, timing, expected in (
            ("knock_in", "at_expiry", one),
            ("knock_out", "at_expiry", no),
            ("knock_in", "at_hit", hit),
        ):
            if expected is None:
                continue
            changes = (
                {"settlement_timing": timing} if barrier_type == "knock_in" else {}
            )
            option = contract(barrier_type, payout=payout, **changes)
            with subtests.test(rate=rate, barrier_type=barrier_type, timing=timing):
                actual = engine.price(option, context)
                assert actual / expected == pytest.approx(
                    1, rel=0, abs=(1e-8 if timing == "at_hit" else 1e-11)
                )
                if rate < 0:
                    with pytest.raises(ComputationFailureError):
                        engine.price(
                            contract(barrier_type, payout=1, **changes),
                            context,
                        )


def test_analytic_expiry_payments_match_live_quantlib(subtests):
    saved = ql.SavedSettings()
    try:
        start = ql.Date(1, 1, 2025)
        ql.Settings.instance().evaluationDate = start
        dc = ql.Actual365Fixed()
        for days, rate, dividend, volatility in (
            (365, 0.03, 0.01, 0.2),
            (365, -0.01, 0.02, 0.5),
            (7, 0.04, 0.0, 0.2),
        ):
            for spot in (80.02, 100, 119.98):
                process = ql.BlackScholesMertonProcess(
                    ql.QuoteHandle(ql.SimpleQuote(spot)),
                    ql.YieldTermStructureHandle(ql.FlatForward(start, dividend, dc)),
                    ql.YieldTermStructureHandle(ql.FlatForward(start, rate, dc)),
                    ql.BlackVolTermStructureHandle(
                        ql.BlackConstantVol(start, ql.NullCalendar(), volatility, dc)
                    ),
                )
                for barrier_type, kind in (
                    ("knock_in", ql.DoubleBarrier.KnockIn),
                    ("knock_out", ql.DoubleBarrier.KnockOut),
                ):
                    reference = ql.DoubleBarrierOption(
                        kind,
                        80,
                        120,
                        0,
                        ql.CashOrNothingPayoff(ql.Option.Call, 0, 7.5),
                        ql.EuropeanExercise(start + days),
                    )
                    reference.setPricingEngine(
                        ql.AnalyticDoubleBarrierBinaryEngine(process)
                    )
                    option = contract(
                        barrier_type,
                        payout=7.5,
                        expiry_time=START + timedelta(days=days),
                    )
                    actual = pricing.AnalyticDoubleTouchEngine().price(
                        option,
                        market(
                            spot,
                            rate=rate,
                            dividend=dividend,
                            volatility=volatility,
                        ),
                    )
                    with subtests.test(
                        days=days, rate=rate, spot=spot, barrier_type=barrier_type
                    ):
                        assert actual == pytest.approx(
                            reference.NPV(), rel=0, abs=1e-10
                        )
    finally:
        del saved


@pytest.mark.parametrize(
    "rate,dividend", ((0.03, 0.01), (0.04, 0), (-0.01, 0.02), (0, 0.01))
)
def test_analytic_three_cashflows_match_independent_series_and_scale(
    subtests, rate, dividend
):
    engine = pricing.AnalyticDoubleTouchEngine()
    reference = continuous_reference(rate=rate, dividend=dividend, count=8192)
    for barrier_type, timing, expected in zip(
        ("knock_in", "knock_in", "knock_out"),
        ("at_expiry", "at_hit", "at_expiry"),
        reference,
        strict=True,
    ):
        changes = {"settlement_timing": timing} if barrier_type == "knock_in" else {}
        for payout in (1, 7.5):
            option = contract(barrier_type, payout=payout, **changes)
            with subtests.test(rate=rate, option=option):
                assert engine.price(
                    option,
                    market(rate=rate, dividend=dividend),
                ) == pytest.approx(payout * expected, rel=0, abs=payout * 1e-10)


def test_analytic_history_current_boundaries_and_expiry():
    engine = pricing.AnalyticDoubleTouchEngine()
    for option in (
        contract(payout=3),
        contract(payout=3, settlement_timing="at_hit"),
        contract("knock_out", payout=3),
    ):
        at_hit = option.settlement_timing == "at_hit"
        for spot in (79, 80, 120, 121):
            expected = (
                (3 if at_hit else 3 * math.exp(-0.03)) if option.is_one_touch else 0
            )
            assert engine.price(option, market(spot)) == pytest.approx(
                expected, rel=0, abs=1e-12
            )
            with pytest.raises(InvalidInputError):
                engine.price(
                    option,
                    market(spot, time=START + timedelta(days=1)),
                )
        prior = option.replace(
            touch_state="touched",
        )
        with pytest.raises(InvalidInputError):
            engine.price(prior, market())
        later = market(time=START + timedelta(days=1))
        expected = (
            0 if at_hit or not option.is_one_touch else 3 * math.exp(-0.03 * 364 / 365)
        )
        assert engine.price(prior, later) == pytest.approx(expected, rel=0, abs=1e-12)
        with pytest.raises(UnsupportedOperationError):
            pricing.implied_volatility(
                engine,
                prior,
                later,
                expected,
                lower_bound=0.0,
                upper_bound=4.0,
            )
        known = option.replace(
            touch_state="untouched",
        )
        for spot in (80, 100, 120):
            terminal = engine.price_with_greeks(
                known,
                market(spot, time=END),
                greeks="all",
            )
            assert terminal.price == (3 if option.is_one_touch == (spot != 100) else 0)
            assert_terminal_greeks(terminal, None if spot in (80, 120) else 0)
        assert option.touch_state is None


def test_analytic_schedules_validate_then_resolve_or_reject():
    engine = pricing.AnalyticDoubleTouchEngine()
    for option in (
        contract(observation_times=[FIRST]),
        contract(
            observation_times=[FIRST],
            settlement_timing="at_hit",
        ),
        contract(
            "knock_out",
            observation_times=[FIRST],
        ),
    ):
        with pytest.raises(UnsupportedOperationError):
            engine.price(option, market(125))
        after = FIRST + timedelta(days=1)
        with pytest.raises(InvalidInputError):
            engine.price(option, market(time=after))
        known = option.replace(
            touch_state="untouched",
        )
        for spot in (80, 120):
            expected = (
                0
                if not option.is_one_touch
                else (
                    1
                    if option.settlement_timing == "at_hit"
                    else math.exp(-0.03 * (END - FIRST).days / 365)
                )
            )
            assert engine.price(
                known,
                market(spot, time=FIRST),
            ) == pytest.approx(expected, rel=0, abs=1e-12)
        expected = (
            0 if option.is_one_touch else math.exp(-0.03 * (END - after).days / 365)
        )
        assert engine.price(
            known,
            market(125, time=after),
        ) == pytest.approx(expected, rel=0, abs=1e-12)
    with pytest.raises(InvalidInputError):
        contract(observation_times=[datetime(2025, 1, 4)])


def test_analytic_node_curves_and_numerical_failures():
    engine = pricing.AnalyticDoubleTouchEngine()
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[0.97, (0.97) * 0.5],
    )
    for rates in ({"rate": curve}, {"dividend": curve}):
        for spot in (100, 120):
            context = market(spot, **rates)
            option = contract()
            with pytest.raises(UnsupportedOperationError):
                engine.price(option, context)
            with pytest.raises(UnsupportedOperationError):
                engine.price_with_greeks(option, context, greeks="all")
            with pytest.raises(UnsupportedOperationError):
                pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    math.nan,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
    with pytest.raises(ComputationFailureError):
        engine.price(
            contract(),
            market(volatility=1e308),
        )
    for day in (START - timedelta(days=1), END + timedelta(days=1)):
        with pytest.raises(InvalidInputError):
            engine.price(contract(), market(time=day))


def test_analytic_greeks_iv_signatures_and_concurrent_pricing(subtests):
    engine = pricing.AnalyticDoubleTouchEngine()
    budgets = {
        "delta": 1e-7,
        "gamma": 1e-7,
        "speed": 1e-5,
        "theta": 0.00000365,
        "charm": 0.00000365,
        "color": 0.00000365,
        "vega": 0.000001,
        "vanna": 0.00001,
        "zomma": 0.0001,
        "rho": 0.000001,
    }
    assert str(inspect.signature(type(engine))) == "()"
    signature = inspect.signature(numerical_greeks)
    for name in (
        "spot_shift",
        "volatility_shift",
        "rate_shift",
        "time_shift",
    ):
        assert signature.parameters[name].kind is inspect.Parameter.KEYWORD_ONLY
        assert signature.parameters[name].default is Ellipsis
    options = (
        contract(touch_state="untouched"),
        contract(settlement_timing="at_hit", touch_state="untouched"),
        contract("knock_out", touch_state="untouched"),
    )
    for spot in (80.02, 100, 119.98):
        for option, expected in zip(options, reference_greeks(spot), strict=True):
            result = engine.price_with_greeks(option, market(spot), greeks="all")
            for name, value in expected.items():
                with subtests.test(spot=spot, option=option, greek=name):
                    if name == "speed" and spot != 100:
                        assert result.speed is None
                        continue
                    assert getattr(result, name) == pytest.approx(
                        value, rel=0, abs=budgets[name]
                    )
    for option in options:
        price = engine.price(option, market())
        default = engine.price_with_greeks(option, market(), greeks="all")
        explicit = numerical_greeks(
            engine,
            option,
            market(),
            greeks="all",
            spot_shift=0.01,
            volatility_shift=0.0001,
            rate_shift=0.0001,
            time_shift=timedelta(days=1),
        )
        for name in reference_greeks(100)[0]:
            assert getattr(default, name) == getattr(explicit, name)
        assert pricing.implied_volatility(
            engine,
            option,
            market(),
            price,
            lower_bound=0.15,
            upper_bound=0.25,
        ) == pytest.approx(0.2, rel=0, abs=1e-7)
        for spot in (80, 120):
            result = engine.price_with_greeks(option, market(spot), greeks="all")
            assert result.delta is None
            assert result.vega == 0
    with ThreadPoolExecutor(max_workers=2) as executor:
        values = list(
            executor.map(
                lambda _: engine.price(options[0], market()),
                range(4),
            )
        )
    assert values == [values[0]] * 4
    assert values[0] == pytest.approx(0.6031839700294364, rel=0, abs=1e-12)
