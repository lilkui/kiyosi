"""Double barriers: public contracts and independent pricing references."""

import importlib
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from itertools import product

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
from kiyosi.instruments import DoubleBarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
)
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)
FIRST = START + timedelta(days=180)


def contract(**changes):
    return DoubleBarrierOption(
        **(
            {
                "option_type": "call",
                "strike": 100,
                "effective_time": START,
                "expiry_time": END,
                "lower_barrier": 80,
                "upper_barrier": 120,
                "barrier_type": "knock_out",
            }
            | changes
        )
    )


def market(spot=100, *, time=START, volatility=0.2, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=time,
    )


def normal(value):
    return 0.5 * math.erfc(-value / math.sqrt(2))


def terminal_reference(spot, direction, lower, upper, time, rebate=2):
    """Price the surviving interval with closed-form lognormal moments.

    Parameters
    ----------
    spot : float
        Underlying spot price.
    direction : {'call', 'put'} or None
        Terminal payoff direction; touch contracts have no strike direction.
    lower : float
        Lower boundary of the surviving interval.
    upper : float
        Upper boundary of the surviving interval.
    time : float
        Remaining Actual/365 Fixed years.
    rebate : float, optional
        Cash rebate on knock-out.

    Returns
    -------
    float
        Independent discounted payoff value.
    """
    deviation = 0.2 * math.sqrt(time)

    def moments(left, right):
        d1_left = (math.log(spot / left) + 0.05 * time) / deviation
        d1_right = (math.log(spot / right) + 0.05 * time) / deviation
        return (
            spot * math.exp(-0.01 * time) * (normal(d1_left) - normal(d1_right)),
            math.exp(-0.04 * time)
            * (normal(d1_left - deviation) - normal(d1_right - deviation)),
        )

    left, right = (
        (max(lower, 100), upper) if direction == "call" else (lower, min(upper, 100))
    )
    asset, cash = moments(left, right)
    _, survival = moments(lower, upper)
    sign = 1 if direction == "call" else -1
    return sign * (asset - 100 * cash) + rebate * (math.exp(-0.04 * time) - survival)


def vanilla_reference(spot, direction, time, *, strike=100):
    sign = 1 if direction == "call" else -1
    deviation = 0.2 * math.sqrt(time)
    d1 = (math.log(spot / strike) + 0.05 * time) / deviation
    return sign * (
        spot * math.exp(-0.01 * time) * normal(sign * d1)
        - strike * math.exp(-0.04 * time) * normal(sign * (d1 - deviation))
    )


def first_fixing_reference(spot, direction, timing, *, final_fixing=True, count=1024):
    """Integrate continuation over states surviving the first fixing.

    Parameters
    ----------
    spot : float
        Underlying spot price.
    direction : {'call', 'put'} or None
        Terminal payoff direction; touch contracts have no strike direction.
    timing : {'at_hit', 'at_expiry'}
        Rebate payment timing.
    final_fixing : bool, optional
        Whether expiry is also a monitored fixing.
    count : int, optional
        Integration panel count or series truncation budget.

    Returns
    -------
    float
        Independent discounted payoff value.
    """
    first = 180 / 365
    deviation = 0.2 * math.sqrt(first)
    lower = (math.log(80.125 / spot) - 0.01 * first) / deviation
    upper = (math.log(120.125 / spot) - 0.01 * first) / deviation
    width = (upper - lower) / count

    def integrand(z):
        asset = spot * math.exp(0.01 * first + deviation * z)
        continuation = (
            terminal_reference(asset, direction, 80.125, 120.125, 1 - first)
            if final_fixing
            else vanilla_reference(asset, direction, 1 - first)
        )
        return continuation * math.exp(-0.5 * z * z) / math.sqrt(2 * math.pi)

    integral = integrand(lower) + integrand(upper)
    for index in range(1, count):
        integral += (4 if index % 2 else 2) * integrand(lower + index * width)
    touched = normal(lower) + normal(-upper)
    return (
        math.exp(-0.04 * first) * width * integral / 3
        + 2 * math.exp(-0.04 * (first if timing == "at_hit" else 1)) * touched
    )


def continuous_reference(direction, kind, *, spot=100, series=10):
    """Price a zero-rebate double barrier with QuantLib analytics.

    Parameters
    ----------
    direction : {'call', 'put'} or None
        Terminal payoff direction; touch contracts have no strike direction.
    kind : type or str
        Contract class or barrier activation choice used by the test.
    spot : float, optional
        Underlying spot price.
    series : int, optional
        QuantLib image-series truncation budget.

    Returns
    -------
    float
        Independent discounted payoff value.

    Notes
    -----
    Only zero rebates are supported: the QuantLib analytic engine ignores
    the rebate argument.
    """
    with ql.SavedSettings():
        beginning, expiry = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
        ql.Settings.instance().evaluationDate = beginning
        day_count = ql.Actual365Fixed()
        process = ql.BlackScholesMertonProcess(
            ql.QuoteHandle(ql.SimpleQuote(spot)),
            ql.YieldTermStructureHandle(ql.FlatForward(beginning, 0.01, day_count)),
            ql.YieldTermStructureHandle(ql.FlatForward(beginning, 0.04, day_count)),
            ql.BlackVolTermStructureHandle(
                ql.BlackConstantVol(beginning, ql.NullCalendar(), 0.2, day_count)
            ),
        )
        option = ql.DoubleBarrierOption(
            ql.DoubleBarrier.KnockIn
            if kind == "knock_in"
            else ql.DoubleBarrier.KnockOut,
            80,
            120,
            0,
            ql.PlainVanillaPayoff(
                ql.Option.Call if direction == "call" else ql.Option.Put, 100
            ),
            ql.EuropeanExercise(expiry),
        )
        option.setPricingEngine(ql.AnalyticDoubleBarrierEngine(process, series))
        return option.NPV()


def continuous_rebate_references(count):
    # Integrate the killed-Brownian kernel in y, then discount -d(survival).
    # Source: https://www.stat.uchicago.edu/~lalley/Courses/312/BrownianMotion312.pdf, exercise 6.
    width = math.log(125 / 80)
    survival_terms, integral_terms = [], []
    for index in range(count):
        frequency = 2 * index + 1
        coefficient = 4 * (-1 if index % 2 else 1) / (frequency * math.pi)
        decay = 0.5 * 0.2**2 * (frequency * math.pi / width) ** 2
        survival_terms.append(coefficient * math.exp(-decay))
        integral_terms.append(
            coefficient * -math.expm1(-(0.04 + decay)) / (0.04 + decay)
        )
    discounted_survival = math.exp(-0.04) * math.fsum(survival_terms)
    return {
        "at_expiry": 3 * (math.exp(-0.04) - discounted_survival),
        "at_hit": 3 * (1 - discounted_survival - 0.04 * math.fsum(integral_terms)),
        "untouched": 3 * discounted_survival,
    }


def test_continuous_prices_and_greeks_match_stable_independent_references(subtests):
    for direction, kind in product(("call", "put"), ("knock_in", "knock_out")):
        expected = continuous_reference(direction, kind)
        assert expected == pytest.approx(
            continuous_reference(direction, kind, series=20), rel=0, abs=1e-11
        )
        option = contract(option_type=direction, barrier_type=kind)
        errors = []
        for count in (80, 320):
            actual = pricing.FiniteDifferenceDoubleBarrierEngine(
                asset_step_count=count, time_step_count=count
            ).price(option, market())
            errors.append(abs(actual - expected))
        with subtests.test(direction=direction, kind=kind):
            assert errors[1] < errors[0]
            assert errors[1] < 0.01
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=640, time_step_count=800
        )
        actual = engine.price_with_greeks(option, market(), ["delta", "gamma"])
        h = 0.025
        derivatives = []
        for shift in (h, h / 2):
            high = continuous_reference(direction, kind, spot=100 + shift)
            low = continuous_reference(direction, kind, spot=100 - shift)
            derivatives.append(
                ((high - low) / (2 * shift), (high - 2 * expected + low) / shift**2)
            )
        assert derivatives[0][0] == pytest.approx(derivatives[1][0], rel=0, abs=1e-6)
        assert derivatives[0][1] == pytest.approx(derivatives[1][1], rel=0, abs=1e-6)
        assert actual.delta == pytest.approx(derivatives[1][0], rel=0, abs=0.003)
        assert actual.gamma == pytest.approx(derivatives[1][1], rel=0, abs=0.0005)


def test_current_and_past_touches_preserve_rebate_timing():
    engine = pricing.FiniteDifferenceDoubleBarrierEngine()
    for spot, timing in product((79, 80, 120, 121), ("at_hit", "at_expiry")):
        option = contract(rebate=3, rebate_timing=timing)
        expected = 3 if timing == "at_hit" else 3 * math.exp(-0.04)
        assert engine.price(option, market(spot)) == pytest.approx(
            expected, rel=0, abs=1e-12
        )
        past = option.replace(
            touch_state="touched",
        )
        expected = 0 if timing == "at_hit" else 3 * math.exp(-0.04 * 364 / 365)
        assert engine.price(
            past,
            market(time=START + timedelta(days=1)),
        ) == pytest.approx(expected, rel=0, abs=1e-12)
    assert_rejection(
        MissingHistoryError,
        lambda: engine.price(
            contract(),
            market(time=datetime(2025, 1, 1, 0, 0, 1)),
        ),
    )
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(contract(touch_state="touched"), market()),
    )
    for bound, inside in ((80, math.inf), (120, 0)):
        observed = contract(observation_times=[START, END], rebate=3)
        actual = engine.price_with_greeks(
            observed,
            market(bound),
            ["delta", "gamma", "vega"],
        )
        assert actual.price == pytest.approx(3 * math.exp(-0.04), rel=0, abs=1e-12)
        assert actual.delta is None
        assert actual.gamma is None
        assert actual.vega == 0
        future = contract(observation_times=[END], rebate=3)
        just_inside = market(math.nextafter(bound, inside))
        assert engine.price(observed, just_inside) == engine.price(future, just_inside)


def test_discrete_observation_is_only_at_midnight_and_history_can_end_early():
    engine = pricing.FiniteDifferenceDoubleBarrierEngine()
    time = datetime.combine(FIRST, datetime.min.time()) + timedelta(hours=12)
    for kind in ("knock_in", "knock_out"):
        option = contract(
            barrier_type=kind,
            rebate=3,
            observation_times=[FIRST],
            touch_state="untouched",
        )
        context = market(130, time=time)
        expected = (
            3 * math.exp(-0.04 * 184.5 / 365)
            if kind == "knock_in"
            else pricing.AnalyticEuropeanEngine().price(
                EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_time=START,
                    expiry_time=END,
                ),
                context,
            )
        )
        assert engine.price(option, context) == pytest.approx(
            expected, rel=0, abs=1e-12
        )
        if kind == "knock_in":
            assert_rejection(
                UnsupportedOperationError,
                lambda option=option, context=context, expected=expected: (
                    pricing.implied_volatility(
                        engine,
                        option,
                        context,
                        expected,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )
                ),
            )
    future = contract(observation_times=[END])
    assert engine.price(future, market(125)) == pytest.approx(
        terminal_reference(125, "call", 80, 120, 1, rebate=0), rel=0, abs=0.002
    )


def test_one_and_two_fixings_and_early_windows_match_independent_integration(subtests):
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=800, time_step_count=2000, asset_upper_boundary=400
    )
    for direction, timing in product(("call", "put"), ("at_hit", "at_expiry")):
        for dates in ([END], [FIRST, END], [FIRST]):
            option = contract(
                option_type=direction,
                lower_barrier=80.125,
                upper_barrier=120.125,
                rebate=2,
                rebate_timing=timing,
                observation_times=dates,
            )
            if dates == [END]:
                expected = terminal_reference(100.25, direction, 80.125, 120.125, 1)
            else:
                expected = first_fixing_reference(
                    100.25, direction, timing, final_fixing=dates[-1] == END
                )
                assert expected == pytest.approx(
                    first_fixing_reference(
                        100.25,
                        direction,
                        timing,
                        final_fixing=dates[-1] == END,
                        count=2048,
                    ),
                    rel=0,
                    abs=1e-9,
                )
            with subtests.test(direction=direction, timing=timing, dates=dates):
                assert engine.price(option, market(100.25)) == pytest.approx(
                    expected, rel=0, abs=0.002
                )


def test_in_out_parity_preserves_expiry_rebates_and_native_greeks():
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=800, time_step_count=800, asset_upper_boundary=400
    )
    for mode, direction in product(("continuous", "scheduled"), ("call", "put")):
        terms = {
            "option_type": direction,
            "rebate": 2,
            "observation_times": [FIRST, END] if mode == "scheduled" else None,
        }
        values = [
            engine.price_with_greeks(
                contract(**terms, barrier_type=kind),
                market(),
                ["delta", "gamma"],
            )
            for kind in ("knock_in", "knock_out")
        ]
        vanilla = pricing.AnalyticEuropeanEngine().price_with_greeks(
            EuropeanOption(
                option_type=direction,
                strike=100,
                effective_time=START,
                expiry_time=END,
            ),
            market(),
            ["delta", "gamma"],
        )
        for measure, budget in (
            ("price", 0.01),
            ("delta", 0.003),
            ("gamma", 0.0005),
        ):
            expected = getattr(vanilla, measure) + (
                2 * math.exp(-0.04) if measure == "price" else 0
            )
            assert sum(getattr(value, measure) for value in values) == pytest.approx(
                expected, rel=0, abs=budget
            )


def test_greek_stencils_and_implied_volatility_follow_existing_contracts():
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=400, time_step_count=800
    )
    option = contract()
    native = engine.price_with_greeks(
        option,
        market(),
        ["delta", "gamma", "vega", "rho"],
    )

    differences = importlib.import_module("price_differences").price_differences
    bumped = differences(
        engine,
        option,
        market(),
        greeks=(
            "delta",
            "gamma",
            "vega",
            "rho",
        ),
    )
    assert native.price == engine.price(option, market())
    for measure, budget in (
        ("delta", 1e-4),
        ("gamma", 2e-4),
        ("vega", 1e-7),
        ("rho", 1e-7),
    ):
        assert getattr(native, measure) == pytest.approx(
            getattr(bumped, measure), rel=0, abs=budget
        )
    solved = pricing.implied_volatility(
        engine,
        option,
        market(),
        native.price,
        lower_bound=0.15,
        upper_bound=0.25,
    )
    assert solved == pytest.approx(0.2, rel=0, abs=1e-6)
    assert_rejection(
        UnsupportedOperationError,
        lambda: pricing.implied_volatility(
            engine,
            option.replace(
                touch_state="touched",
            ),
            market(time=START + timedelta(days=1)),
            0,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
    )
    scheduled = contract(
        observation_times=[FIRST, END],
        touch_state="untouched",
    )
    event = engine.price_with_greeks(
        scheduled,
        market(time=FIRST),
        ["theta", "charm", "color"],
    )
    assert event.theta is None
    assert event.charm is None
    assert event.color is None
    at_expiry = engine.price_with_greeks(
        option.replace(
            touch_state="untouched",
        ),
        market(110, time=END),
        greeks="all",
    )
    assert at_expiry.price == 10
    assert_terminal_greeks(at_expiry, 1)


def test_narrow_band_and_tiny_volatility_do_not_lose_surviving_payoff():
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=160, time_step_count=800
    )
    option = contract(lower_barrier=99.5, upper_barrier=100.5, strike=90)
    assert engine.price(option, market()) < 1e-8
    actual = engine.price(
        option,
        market(volatility=1e-12, dividend=0.04),
    )
    assert actual == pytest.approx(10 * math.exp(-0.04), rel=0, abs=1e-6)
    for direction, strike in (("call", 140), ("put", 60)):
        excluded = contract(option_type=direction, strike=strike)
        assert engine.price(excluded, market()) == 0
        activated = contract(
            option_type=direction, strike=strike, barrier_type="knock_in"
        )
        expected = pricing.AnalyticEuropeanEngine().price(
            EuropeanOption(
                option_type=direction,
                strike=strike,
                effective_time=START,
                expiry_time=END,
            ),
            market(),
        )
        assert engine.price(activated, market()) == expected


def test_edge_grid_refinement_preserves_independent_reference_limits():
    for spot in (80.125, 119.875):
        expected = continuous_reference("call", "knock_out", spot=spot, series=20)
        assert expected == pytest.approx(
            continuous_reference("call", "knock_out", spot=spot, series=10),
            rel=0,
            abs=1e-10,
        )
        errors = []
        for count in (80, 160, 320):
            engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                asset_step_count=count, time_step_count=count
            )
            errors.append(abs(engine.price(contract(), market(spot)) - expected))
        assert errors[-1] < errors[0]
        assert errors[-1] < 0.0001
    option = contract(lower_barrier=99.5, upper_barrier=100.5, strike=90)
    errors = []
    for count in (80, 160, 320):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=count, time_step_count=5 * count
        )
        errors.append(
            abs(
                engine.price(
                    option,
                    market(volatility=1e-12, dividend=0.04),
                )
                - 10 * math.exp(-0.04)
            )
        )
    assert errors[-1] < errors[0]
    assert errors[-1] < 1e-6


def test_negative_narrow_band_price_requires_a_stable_scheme():
    option = contract(lower_barrier=99.5, upper_barrier=100.5, strike=90)
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=80,
        time_step_count=400,
    )
    with pytest.raises(ComputationFailureError):
        engine.price(option, market())
    stable = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=80,
        time_step_count=400,
        scheme="implicit_euler",
    )
    assert stable.price(option, market()) >= 0
    assert stable.price(option, market()) < 1e-8


def test_grid_validation_calendar_and_shared_instance_concurrency():
    unstable = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=400, time_step_count=1, scheme="explicit_euler"
    )
    assert_rejection(
        InvalidInputError,
        lambda: unstable.price(contract(), market()),
    )
    clipped = pricing.FiniteDifferenceDoubleBarrierEngine(asset_upper_boundary=110)
    assert_rejection(
        InvalidInputError,
        lambda: clipped.price(contract(), market()),
    )
    weekend = datetime(2025, 7, 5)
    with pytest.raises(InvalidInputError):
        contract(observation_times=[weekend])
    engine = pricing.FiniteDifferenceDoubleBarrierEngine()
    option, context = contract(), market()
    expected = engine.price(option, context)
    with ThreadPoolExecutor(max_workers=2) as pool:
        actual = list(
            pool.map(
                lambda _: engine.price(option, context),
                range(4),
            )
        )
    assert actual == [expected] * 4


@pytest.mark.parametrize("lower,upper,dividend", ((80, 102, 0), (98, 120, 0.08)))
@pytest.mark.parametrize("timing", ("at_hit", "at_expiry"))
def test_continuous_future_hits_discount_rebates_at_the_correct_time(
    lower, upper, dividend, timing
):
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=800, time_step_count=2000
    )
    hit_time = (
        math.log(upper / 100) / 0.04 if dividend == 0 else math.log(100 / lower) / 0.04
    )
    option = contract(
        lower_barrier=lower,
        upper_barrier=upper,
        rebate=3,
        rebate_timing=timing,
    )
    expected = 3 * math.exp(-0.04 * (hit_time if timing == "at_hit" else 1))
    actual = engine.price(
        option,
        market(volatility=1e-12, dividend=dividend),
    )
    assert actual == pytest.approx(expected, rel=0, abs=1e-4)


@pytest.mark.parametrize(
    "scheme,count", (("implicit_euler", 320), ("explicit_euler", 40))
)
def test_implicit_and_explicit_schemes_price_unresolved_double_barriers(scheme, count):
    expected = continuous_reference("call", "knock_out")
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=count, time_step_count=2000, scheme=scheme
    )
    assert engine.price(contract(), market()) == pytest.approx(
        expected, rel=0, abs=0.01
    )


def test_overlapping_event_cells_preserve_surviving_payoff():
    lower, upper = 80.05, 80.125
    option = contract(
        option_type="put",
        lower_barrier=lower,
        upper_barrier=upper,
        rebate=2,
        observation_times=[END],
    )
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=800, time_step_count=2000, asset_upper_boundary=400
    )
    expected = terminal_reference(100.25, "put", lower, upper, 1)
    assert engine.price(option, market(100.25)) == pytest.approx(
        expected, rel=0, abs=0.0002
    )


def test_interim_observations_preserve_nonnegative_continuation(subtests):
    strike = 80
    first = (FIRST - START).days / 365
    deviation = 0.2 * math.sqrt(first)

    def reference(spot, lower, upper, count):
        left = (math.log(lower / spot) - 0.01 * first) / deviation
        right = (math.log(upper / spot) - 0.01 * first) / deviation
        width = (right - left) / count

        def integrand(z):
            asset = spot * math.exp(0.01 * first + deviation * z)
            continuation = vanilla_reference(asset, "call", 1 - first, strike=strike)
            return continuation * math.exp(-0.5 * z * z) / math.sqrt(2 * math.pi)

        total = integrand(left) + integrand(right)
        for index in range(1, count):
            total += (4 if index % 2 else 2) * integrand(left + index * width)
        return math.exp(-0.04 * first) * width * total / 3

    worst = [0.0] * 3
    for lower, upper in (
        (80.05, 80.0501),
        (80.24995, 80.25005),
        (80.74995, 80.75005),
    ):
        expected = reference(100.25, lower, upper, 128)
        assert expected > 0
        assert expected == pytest.approx(
            reference(100.25, lower, upper, 64), rel=0, abs=1e-14
        )
        option = contract(
            strike=strike,
            lower_barrier=lower,
            upper_barrier=upper,
            observation_times=[FIRST],
        )
        for scheme in ("crank_nicolson", "rannacher", "implicit_euler"):
            for index, count in enumerate((800, 1600, 3200)):
                engine = pricing.FiniteDifferenceDoubleBarrierEngine(
                    asset_step_count=count,
                    time_step_count=2000,
                    asset_upper_boundary=400,
                    scheme=scheme,
                )
                actual = engine.price(option, market(100.25))
                with subtests.test(
                    lower=lower, upper=upper, scheme=scheme, count=count
                ):
                    assert actual > 0
                    assert actual == pytest.approx(expected, rel=0, abs=expected * 0.02)
                worst[index] = max(worst[index], abs(actual - expected) / expected)
    assert worst[-1] < worst[0]
    assert worst[-1] < 0.01

    option = contract(
        strike=strike,
        lower_barrier=80.24995,
        upper_barrier=120,
        observation_times=[FIRST],
    )
    before = datetime.combine(FIRST, datetime.min.time()) - timedelta(seconds=1)
    for scheme in ("crank_nicolson", "rannacher", "implicit_euler"):
        engine = pricing.FiniteDifferenceDoubleBarrierEngine(
            asset_step_count=800,
            time_step_count=2000,
            asset_upper_boundary=400,
            scheme=scheme,
        )
        assert (
            engine.price(
                option,
                market(80, time=before),
            )
            >= 0
        )


def test_continuous_stochastic_rebates_match_independent_exit_time_series(subtests):
    references = continuous_rebate_references(129)
    refined = continuous_rebate_references(257)
    for timing, value in references.items():
        assert value == pytest.approx(refined[timing], rel=0, abs=2e-8)
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_step_count=640, time_step_count=800
    )
    # q = r - sigma^2/2 makes log spot driftless and S the log-interval midpoint.
    context = market(dividend=0.02)
    for kind, timing in (
        ("knock_out", "at_hit"),
        ("knock_out", "at_expiry"),
        ("knock_in", "at_expiry"),
    ):
        terms = {
            "upper_barrier": 125,
            "barrier_type": kind,
            "rebate_timing": timing,
        }
        base = engine.price(contract(**terms), context)
        actual = engine.price(contract(**terms, rebate=3), context) - base
        expected = references["untouched" if kind == "knock_in" else timing]
        with subtests.test(kind=kind, timing=timing):
            assert actual == pytest.approx(expected, rel=0, abs=0.001)


def test_requests_representations_and_failures_preserve_inputs():
    engine = pricing.FiniteDifferenceDoubleBarrierEngine(asset_upper_boundary=400)
    instrument = contract(touch_state="untouched")
    context = market()
    result = engine.price_with_greeks(instrument, context, greeks="all")
    for name in (
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
        assert math.isfinite(getattr(result, name)), name
        selected = engine.price_with_greeks(instrument, context, name)
        assert getattr(result, name) == getattr(selected, name)
    for name, value in (
        ("spot_shift", True),
        ("volatility_shift", "0.1"),
        ("rate_shift", object()),
        ("time_shift", 1.5),
    ):
        with pytest.raises(TypeError):
            numerical_greeks(
                engine,
                instrument,
                context,
                "delta",
                **{name: value},
            )
    for request, controls in (
        ([], {"spot_shift": 0}),
        (["delta"], {"spot_shift": 0}),
    ):
        assert_rejection(
            InvalidInputError,
            lambda request=request, controls=controls: numerical_greeks(
                engine,
                instrument,
                context,
                request,
                **controls,
            ),
        )
    collapsed = numerical_greeks(
        engine,
        instrument,
        context,
        ["delta", "gamma", "speed"],
        spot_shift=1e-300,
    )
    assert collapsed.delta is None
    assert collapsed.gamma is None
    assert collapsed.speed is None
    for bound in (80, 120):
        discontinuous = engine.price_with_greeks(
            contract(), market(bound), greeks="all"
        )
        assert all(
            getattr(discontinuous, name) is None
            for name in (
                "delta",
                "gamma",
                "speed",
                "charm",
                "color",
                "vanna",
                "zomma",
            )
        )
    assert_rejection(
        RootNotBracketedError,
        lambda: pricing.implied_volatility(
            engine,
            instrument,
            context,
            -1,
            lower_bound=0.15,
            upper_bound=0.25,
        ),
    )
    with pytest.raises(TypeError):
        pricing.implied_volatility(
            engine,
            instrument,
            context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(TypeError):
        pricing.implied_volatility(
            engine,
            instrument,
            context,
            result.price,
            max_iterations=1.5,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    with pytest.raises(UnsupportedOperationError):
        engine.price(vanilla, context)
    assert engine.price(instrument, context) == result.price
    assert context.model_parameters.volatility == 0.2


def test_node_curves_preserve_payment_discounts_and_coverage_order():
    from kiyosi.market import RateTermStructure

    engine = pricing.FiniteDifferenceDoubleBarrierEngine(
        asset_upper_boundary=400, time_step_count=800
    )
    first = START + timedelta(days=91)
    curve = RateTermStructure(
        reference_time=START,
        times=[first, END],
        factors=[math.exp(-0.08 * 91 / 365), math.exp(-0.01)],
    )
    context = market(150, volatility=1e-12)
    context = context.replace(
        model_parameters=context.model_parameters.replace(
            risk_free_rate=curve, dividend_yield=curve
        )
    )
    for timing in ("at_hit", "at_expiry"):
        instrument = contract(
            rebate=7,
            rebate_timing=timing,
            observation_times=[first, END],
        )
        expected = 7 * math.exp(-0.08 * 91 / 365 if timing == "at_hit" else -0.01)
        assert engine.price(instrument, context) == pytest.approx(
            expected, rel=0, abs=1e-5
        )
    short = RateTermStructure(reference_time=START, times=[first], factors=[0.99])
    uncovered = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=short)
    )
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(instrument, uncovered),
    )
    assert (
        engine.price(
            contract(rebate=7, rebate_timing="at_hit"),
            uncovered,
        )
        == 7
    )
    clipped = pricing.FiniteDifferenceDoubleBarrierEngine(asset_upper_boundary=1)
    assert (
        clipped.price(
            contract(rebate=7, rebate_timing="at_hit"),
            uncovered,
        )
        == 7
    )
    for time in (START - timedelta(days=1), END + timedelta(days=1)):
        assert_rejection(
            InvalidInputError,
            lambda time=time: engine.price(contract(), market(time=time)),
        )
