"""Single-barrier public API, event history and independent QuantLib comparisons."""

import math
from datetime import datetime, timedelta

import pytest
import QuantLib as ql
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import BarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
)
from kiyosi.pricing import (
    AnalyticBarrierEngine,
    AnalyticEuropeanEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_decimal_equal, assert_rejection
from support.terminal import assert_terminal_greeks

KINDS = {
    "up_and_in": ql.Barrier.UpIn,
    "up_and_out": ql.Barrier.UpOut,
    "down_and_in": ql.Barrier.DownIn,
    "down_and_out": ql.Barrier.DownOut,
}


def make_terms(**changes):
    return {
        "option_type": "call",
        "strike": 100,
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
        "barrier_level": 120,
        "barrier_type": "up_and_out",
    } | changes


def make_context(spot=100, time=datetime(2025, 1, 1), **changes):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=spot,
        valuation_time=time,
        **changes,
    )


def test_history_is_prior_and_paid_hit_rebate_does_not_recur():
    engine = AnalyticBarrierEngine()
    option = BarrierOption(**make_terms(rebate=7, rebate_timing="at_hit"))
    assert option.touch_state == None
    assert option.observation_mode == "continuous"
    assert engine.price(option, make_context(120)) == 7
    with pytest.raises(InvalidInputError) as error:
        engine.price(
            option,
            make_context(time=datetime(2025, 1, 2)),
        )
    assert type(error.value) is MissingHistoryError
    touched = option.replace(
        touch_state="touched",
    )
    assert (
        engine.price(
            touched,
            make_context(time=datetime(2025, 1, 2)),
        )
        == 0
    )
    assert option.touch_state is None
    assert touched.replace(touch_state=None) == option


def test_construction_representation_and_replacement_failures_preserve_owner(subtests):
    option = BarrierOption(**make_terms())
    for field in ("strike", "barrier_level", "rebate"):
        for value in (True, "100", None, complex(1, 0)):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                BarrierOption(**make_terms(**{field: value}))
    for field in (
        "option_type",
        "barrier_type",
        "rebate_timing",
        "touch_state",
    ):
        with subtests.test(field=field), pytest.raises(ValueError):
            BarrierOption(**make_terms(**{field: "unknown"}))
        with subtests.test(field=field), pytest.raises(TypeError):
            BarrierOption(**make_terms(**{field: True}))
    for values, expected_error in (
        ({"barrier_level": 0}, InvalidInputError),
        ({"barrier_level": float("nan"), "strike": -1}, InvalidInputError),
        ({"strike": 0}, InvalidInputError),
        ({"rebate": -1}, InvalidInputError),
        ({"rebate": float("inf")}, InvalidInputError),
        ({"expiry_time": datetime(2024, 1, 1)}, InvalidInputError),
        (
            {"barrier_type": "up_and_in", "rebate_timing": "at_hit"},
            InvalidInputError,
        ),
        ({"observation_times": []}, InvalidInputError),
        (
            {
                "observation_times": [datetime(2026, 1, 2)],
            },
            InvalidInputError,
        ),
        (
            {
                "observation_times": [datetime(2025, 1, 2)] * 2,
            },
            InvalidInputError,
        ),
    ):
        with subtests.test(values=values):
            assert_rejection(
                expected_error,
                lambda values=values: BarrierOption(**make_terms(**values)),
            )
    for fields, error in (
        ({"touch_state": "bad"}, ValueError),
        ({"touch_state": 1}, TypeError),
        ({"strike": True}, TypeError),
    ):
        with pytest.raises(error):
            option.replace(**fields)
        assert option == BarrierOption(**make_terms())
    with pytest.raises(AttributeError):
        option.touch_state = "touched"
    assert option.replace() == option


def test_expiry_and_prior_settlement_matrix(subtests):
    engine = AnalyticBarrierEngine()
    for kind in ("up_and_in", "up_and_out", "down_and_in", "down_and_out"):
        for state in ("touched", "untouched"):
            for level in (90, 100, 120):
                for timing in (
                    ("at_expiry",) if kind.endswith("_in") else ("at_hit", "at_expiry")
                ):
                    option = BarrierOption(
                        **make_terms(
                            strike=80,
                            barrier_type=kind,
                            barrier_level=level,
                            touch_state=state,
                            rebate=7,
                            rebate_timing=timing,
                        )
                    )
                    hit = state == "touched" or (
                        100 >= level if kind.startswith("up") else 100 <= level
                    )
                    rebate = 0 if state == "touched" and timing == "at_hit" else 7
                    expected = (
                        (20 if hit else 7)
                        if kind.endswith("_in")
                        else (rebate if hit else 20)
                    )
                    context = make_context(time=option.expiry_time)
                    with subtests.test(
                        kind=kind, state=state, level=level, timing=timing
                    ):
                        assert engine.price(option, context) == expected
                        result = engine.price_with_greeks(option, context, greeks="all")
                        assert_terminal_greeks(
                            result,
                            None
                            if state == "untouched" and level == 100
                            else int(kind.endswith("_in") == hit),
                        )
                        assert_rejection(
                            UnsupportedOperationError,
                            lambda option=option, context=context, expected=expected: (
                                implied_volatility(
                                    engine,
                                    option,
                                    context,
                                    expected,
                                    lower_bound=0.0,
                                    upper_bound=4.0,
                                )
                            ),
                        )
    for timing in ("at_hit", "at_expiry"):
        option = BarrierOption(
            **make_terms(rebate=7, rebate_timing=timing, touch_state="touched")
        )
        context = make_context(time=datetime(2025, 1, 2))
        expected = 0 if timing == "at_hit" else 7 * math.exp(-0.05 * 364 / 365)
        assert engine.price(option, context) == pytest.approx(
            expected, rel=0, abs=1e-12
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda option=option, context=context, expected=expected: (
                implied_volatility(
                    engine,
                    option,
                    context,
                    expected,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
            ),
        )


def test_scheduled_fixing_occurs_only_at_midnight_and_history_is_strict():
    engine = AnalyticBarrierEngine()
    fixing = datetime(2025, 1, 2)
    option = BarrierOption(**make_terms(observation_times=[fixing], rebate=7))
    assert_rejection(
        UnsupportedOperationError,
        lambda: engine.price(option, make_context()),
    )
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(
            option.replace(
                touch_state="touched",
            ),
            make_context(time=fixing),
        ),
    )
    assert engine.price(
        option,
        make_context(120, fixing),
    ) == pytest.approx(7 * math.exp(-0.05 * 364 / 365), rel=0, abs=1e-12)
    noon = datetime(2025, 1, 2, 12)
    assert_rejection(
        MissingHistoryError,
        lambda: engine.price(option, make_context(150, noon)),
    )
    untouched = option.replace(
        touch_state="untouched",
    )
    vanilla = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=option.effective_time,
        expiry_time=option.expiry_time,
    )
    assert engine.price(
        untouched,
        make_context(150, noon),
    ) == AnalyticEuropeanEngine().price(vanilla, make_context(150, noon))
    dates = list(untouched.observation_times)
    dates.clear()
    assert untouched.observation_times == ((fixing,))
    assert untouched.observation_schedule.times == ((fixing,))
    assert_decimal_equal(untouched.mean_observation_year_fraction, 1 / 365)
    with pytest.raises(InvalidInputError):
        BarrierOption(
            **make_terms(
                observation_times=[datetime(2025, 1, 4)],
                touch_state="touched",
            )
        )


def test_scheduled_bgk_domain_and_event_sensitive_greeks():
    engine = AnalyticBarrierEngine()
    for up in (False, True):
        level = 120 if up else 80
        kind = "up_and_out" if up else "down_and_out"
        scheduled = BarrierOption(
            **make_terms(
                barrier_type=kind,
                barrier_level=level,
                observation_times=[datetime(2026, 1, 1)],
                touch_state="untouched",
            )
        )
        shift = (1 if up else -1) * 0.5825971579390107 * 0.2
        shifted = BarrierOption(
            **make_terms(
                barrier_type=kind,
                barrier_level=level * math.exp(shift),
                touch_state="untouched",
            )
        )
        noon = datetime(2025, 1, 1, 12)
        context = make_context(time=noon)
        assert engine.price(scheduled, context) == pytest.approx(
            engine.price(shifted, context), rel=0, abs=1e-11
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda scheduled=scheduled, up=up, noon=noon: engine.price(
                scheduled,
                make_context(150 if up else 50, noon),
            ),
        )
        dense = BarrierOption(
            **make_terms(
                barrier_type=kind,
                barrier_level=level,
                observation_times=[
                    datetime(2025, 1, 1) + timedelta(days=i) for i in range(1, 366)
                ],
                touch_state="untouched",
                calendar=all_days_calendar(),
            )
        )
        result = engine.price_with_greeks(
            dense,
            make_context(time=noon),
            greeks="all",
        )
        for greek in ("theta", "charm", "color"):
            assert math.isfinite(result.require(greek))
        assert result.delta is not None
    option = BarrierOption(**make_terms(touch_state="untouched", rebate=7))
    result = engine.price_with_greeks(option, make_context(120), greeks="all")
    assert result.delta is None
    assert result.vega == 0
    assert result.require("rho") == pytest.approx(-7 * math.exp(-0.05), rel=0, abs=2e-8)
    near = engine.price_with_greeks(
        option,
        make_context(119.995),
        greeks="all",
    )
    assert near.delta is None
    assert near.speed is None
    assert near.vega is not None


def test_quantlib_price_matrix_and_independent_greeks(subtests):
    engine = AnalyticBarrierEngine()
    for kind in KINDS:
        for direction in ("call", "put"):
            for strike in (70, 100, 130):
                for timing in (
                    ("at_expiry",) if kind.endswith("_in") else ("at_hit", "at_expiry")
                ):
                    for rate, dividend, sigma in (
                        (0.05, 0.02, 0.2),
                        (-0.02, 0.07, 0.45),
                    ):
                        level = 120 if kind.startswith("up") else 80
                        terms = make_terms(
                            option_type=direction,
                            strike=strike,
                            barrier_type=kind,
                            barrier_level=level,
                            rebate=7,
                            rebate_timing=timing,
                            touch_state="untouched",
                        )
                        context = make_context().replace(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=rate,
                                dividend_yield=dividend,
                                volatility=sigma,
                            )
                        )
                        expected = reference(
                            terms, rate=rate, dividend=dividend, sigma=sigma
                        )
                        with subtests.test(
                            kind=kind,
                            direction=direction,
                            strike=strike,
                            timing=timing,
                            rate=rate,
                        ):
                            assert engine.price(
                                BarrierOption(**terms),
                                context,
                            ) == pytest.approx(expected, rel=0, abs=1e-8)
    terms = make_terms(
        barrier_type="down_and_out",
        barrier_level=80,
        rebate=7,
        touch_state="untouched",
    )
    option = BarrierOption(**terms)
    result = engine.price_with_greeks(option, make_context(), greeks="all")

    def f(spot=100, sigma=0.2, rate=0.05, days=0):
        return reference(terms, spot=spot, sigma=sigma, rate=rate, days=days)

    h, v, r = 0.03, 0.0001, 0.0001

    def delta(sigma=0.2, days=0):
        return (f(100 + h, sigma, days=days) - f(100 - h, sigma, days=days)) / (2 * h)

    def gamma(sigma=0.2, days=0):
        return (
            f(100 + h, sigma, days=days)
            - 2 * f(sigma=sigma, days=days)
            + f(100 - h, sigma, days=days)
        ) / h**2

    expected = {
        "delta": delta(),
        "gamma": gamma(),
        "speed": (f(100 + 2 * h) - 2 * f(100 + h) + 2 * f(100 - h) - f(100 - 2 * h))
        / (2 * h**3),
        "vega": (f(sigma=0.2 + v) - f(sigma=0.2 - v)) / (2 * v),
        "vanna": (delta(0.2 + v) - delta(0.2 - v)) / (2 * v),
        "zomma": (gamma(0.2 + v) - gamma(0.2 - v)) / (2 * v),
        "rho": (f(rate=0.05 + r) - f(rate=0.05 - r)) / (2 * r),
        "theta": (f(days=1) - f()) * 365,
        "charm": (delta(days=1) - delta()) * 365,
        "color": (gamma(days=1) - gamma()) * 365,
    }
    for greek, value in expected.items():
        with subtests.test(greek=greek):
            assert getattr(result, greek) == pytest.approx(
                value, rel=0, abs=GREEK_TOLERANCES[greek]
            )
    sigma = implied_volatility(
        engine,
        option,
        make_context(),
        result.price,
        lower_bound=0.15,
        upper_bound=0.25,
    )
    assert sigma == pytest.approx(0.2, rel=0, abs=1e-7)


@pytest.mark.parametrize("kind", KINDS)
@pytest.mark.parametrize("direction", ("call", "put"))
def test_tiny_volatility_and_extreme_monetary_scales(kind, direction):
    engine = AnalyticBarrierEngine()
    up = kind.startswith("up")
    knock_in = kind.endswith("_in")
    for spot, sigma in ((1e16, 1e-20), (1e150, 1e-200)):
        option = BarrierOption(
            **make_terms(
                option_type=direction,
                strike=spot,
                barrier_type=kind,
                barrier_level=(1.2 if up else 0.8) * spot,
            )
        )
        context = make_context(spot).replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=sigma
            )
        )
        expected = 0 if knock_in else spot * sigma / math.sqrt(2 * math.pi)
        actual = engine.price(option, context)
        assert (actual / expected if expected else actual) == pytest.approx(
            (1 if expected else 0), rel=0, abs=1e-12
        )
    terms = make_terms(
        option_type=direction,
        barrier_type=kind,
        barrier_level=120 if up else 80,
    )
    unit = engine.price(
        BarrierOption(**terms),
        make_context(),
    )
    for scale in (1e-300, 1e300):
        scaled = terms | {
            "strike": 100 * scale,
            "barrier_level": terms["barrier_level"] * scale,
        }
        actual = engine.price(
            BarrierOption(**scaled),
            make_context(100 * scale),
        )
        assert actual / scale == pytest.approx(unit, rel=0, abs=1e-9)


def test_analysis_representation_errors_validation_and_concurrency():
    from concurrent.futures import ThreadPoolExecutor

    engine = AnalyticBarrierEngine()
    option = BarrierOption(**make_terms(touch_state="untouched"))
    context = make_context()
    for field, value, error in (
        ("spot_shift", True, TypeError),
        ("time_shift", 1.5, TypeError),
        ("time_shift", 2**40, TypeError),
    ):
        with pytest.raises(error):
            numerical_greeks(
                engine,
                option,
                context,
                "delta",
                **{field: value},
            )
    for fields in (
        {"spot_shift": 0},
        {"volatility_shift": float("nan")},
        {"time_shift": timedelta(days=0)},
    ):
        assert_rejection(
            InvalidInputError,
            lambda fields=fields: numerical_greeks(
                engine, option, context, "delta", **fields
            ),
        )
    for changes in (
        {"lower_bound": -0.1},
        {"max_iterations": 0},
        {"price_tolerance": -1},
    ):
        assert_rejection(
            InvalidInputError,
            lambda changes=changes: implied_volatility(
                engine,
                option,
                context,
                1,
                **{"lower_bound": 0.0, "upper_bound": 4.0, **changes},
            ),
        )
    assert_rejection(
        RootNotBracketedError,
        lambda: implied_volatility(
            engine,
            option,
            context,
            -1,
            lower_bound=0.15,
            upper_bound=0.25,
        ),
    )
    curve = RateTermStructure(
        reference_time=datetime(2025, 1, 1),
        times=[
            datetime(2026, 1, 1),
            datetime(2026, 1, 1) + (datetime(2026, 1, 1) - datetime(2025, 1, 1)),
        ],
        factors=[0.95, (0.95) * 0.5],
    )
    curved = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=curve)
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: engine.price_with_greeks(option, curved, []),
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: implied_volatility(
            engine,
            option,
            curved,
            1,
            lower_bound=0,
            upper_bound=4.0,
        ),
    )
    with ThreadPoolExecutor(4) as pool:
        prices = list(
            pool.map(
                lambda _: engine.price(option, context),
                range(16),
            )
        )
    assert prices == [engine.price(option, context)] * 16
    assert option == BarrierOption(**make_terms(touch_state="untouched"))


GREEKS = (
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
GREEK_TOLERANCES = {
    "delta": 1e-6,
    "gamma": 1e-7,
    "speed": 2e-7,
    "theta": 0.0365,
    "charm": 0.00365,
    "color": 0.001095,
    "vega": 0.0001,
    "vanna": 0.00001,
    "zomma": 0.000002,
    "rho": 0.0001,
}


def reference(terms, spot=100, sigma=0.2, rate=0.05, dividend=0.02, days=0):
    with ql.SavedSettings():
        valuation = terms["effective_time"] + timedelta(days=days)
        today = ql.Date(valuation.day, valuation.month, valuation.year)
        end = terms["expiry_time"]
        expiry = ql.Date(end.day, end.month, end.year)
        ql.Settings.instance().evaluationDate = today
        process = ql.BlackScholesMertonProcess(
            ql.QuoteHandle(ql.SimpleQuote(spot)),
            ql.YieldTermStructureHandle(
                ql.FlatForward(today, dividend, ql.Actual365Fixed())
            ),
            ql.YieldTermStructureHandle(
                ql.FlatForward(today, rate, ql.Actual365Fixed())
            ),
            ql.BlackVolTermStructureHandle(
                ql.BlackConstantVol(
                    today, ql.NullCalendar(), sigma, ql.Actual365Fixed()
                )
            ),
        )
        payoff = ql.PlainVanillaPayoff(
            ql.Option.Call if terms["option_type"] == "call" else ql.Option.Put,
            terms["strike"],
        )

        def value(kind, rebate):
            contract = ql.BarrierOption(
                KINDS[kind],
                terms["barrier_level"],
                rebate,
                payoff,
                ql.EuropeanExercise(expiry),
            )
            contract.setPricingEngine(ql.AnalyticBarrierEngine(process))
            return contract.NPV()

        kind = terms["barrier_type"]
        rebate = terms.get("rebate", 0)
        if kind.endswith("_in") or terms.get("rebate_timing", "at_expiry") == "at_hit":
            result = value(kind, rebate)
        else:
            opposite = kind.replace("_out", "_in")
            result = (
                value(kind, 0)
                + rebate * math.exp(-rate * (end - valuation).days / 365)
                - (value(opposite, rebate) - value(opposite, 0))
            )
        return result
