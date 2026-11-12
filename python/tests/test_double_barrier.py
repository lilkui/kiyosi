"""Double-barrier contracts and independent analytic references."""

import math
from datetime import datetime
from itertools import product

import pytest
import QuantLib as ql
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    UnsupportedOperationError,
)
from kiyosi.instruments import DoubleBarrierOption, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
)
from kiyosi.pricing import (
    AnalyticDoubleBarrierEngine,
    AnalyticEuropeanEngine,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


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


def market(spot=100, time=START, rate=0.04, dividend=0.01, volatility=0.2):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=time,
    )


def reference(direction, kind, spot=100, volatility=0.2, series=20):
    saved = ql.SavedSettings()
    try:
        start, end = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
        ql.Settings.instance().evaluationDate = start
        dc = ql.Actual365Fixed()
        process = ql.BlackScholesMertonProcess(
            ql.QuoteHandle(ql.SimpleQuote(spot)),
            ql.YieldTermStructureHandle(ql.FlatForward(start, 0.01, dc)),
            ql.YieldTermStructureHandle(ql.FlatForward(start, 0.04, dc)),
            ql.BlackVolTermStructureHandle(
                ql.BlackConstantVol(start, ql.NullCalendar(), volatility, dc)
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
            ql.EuropeanExercise(end),
        )
        option.setPricingEngine(ql.AnalyticDoubleBarrierEngine(process, series))
        return option.NPV()
    finally:
        del saved


def test_prices_greeks_and_parity_match_live_quantlib():
    engine = AnalyticDoubleBarrierEngine()
    for direction, kind in product(("call", "put"), ("knock_in", "knock_out")):
        option = contract(option_type=direction, barrier_type=kind)
        expected = reference(direction, kind)
        assert expected == pytest.approx(
            reference(direction, kind, series=10),
            rel=0,
            abs=1e-10,
        )
        result = engine.price_with_greeks(option, market(), ["delta", "gamma"])
        assert result.price == pytest.approx(expected, rel=0, abs=1e-9)
        low, high = [reference(direction, kind, spot=s) for s in (99.99, 100.01)]
        assert result.delta == pytest.approx((high - low) / 0.02, rel=0, abs=1e-7)
        assert result.gamma == pytest.approx(
            (high - 2 * expected + low) / 0.01**2, rel=0, abs=1e-6
        )
        assert result.vega is None
        other = contract(
            option_type=direction,
            barrier_type="knock_in" if kind == "knock_out" else "knock_out",
        )
        vanilla = EuropeanOption(
            option_type=direction, strike=100, effective_time=START, expiry_time=END
        )
        assert result.price + engine.price(other, market()) == pytest.approx(
            AnalyticEuropeanEngine().price(vanilla, market()),
            rel=0,
            abs=1e-9,
        )


def test_defaults_representations_core_validation_and_owned_replacement(subtests):
    from kiyosi.instruments import BarrierOption

    option = contract()
    single = BarrierOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
        barrier_level=80,
        barrier_type="down_and_out",
    )
    for field in (
        "rebate",
        "rebate_timing",
        "observation_times",
        "touch_state",
    ):
        assert getattr(option, field) == getattr(single, field)
    assert ((option.lower_barrier, option.upper_barrier, option.barrier_type)) == (
        (80, 120, "knock_out")
    )
    assert (
        (
            option.option_type,
            option.strike,
            option.effective_time,
            option.expiry_time,
        )
    ) == (("call", 100, START, END))
    for field in ("strike", "lower_barrier", "upper_barrier", "rebate"):
        for value in (True, "100", None, 1 + 0j):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                contract(**{field: value})
        with pytest.raises(OverflowError):
            contract(**{field: 10**1000})
    for field in (
        "option_type",
        "barrier_type",
        "rebate_timing",
        "touch_state",
    ):
        with subtests.test(field=field), pytest.raises(ValueError):
            contract(**{field: "unknown"})
        with pytest.raises(TypeError):
            contract(**{field: True})
    for field in ("effective_time", "expiry_time"):
        with pytest.raises(TypeError):
            contract(**{field: "2025-01-01"})
    for values, expected_error in [
        ({"lower_barrier": 0}, InvalidInputError),
        ({"lower_barrier": math.nan}, InvalidInputError),
        ({"upper_barrier": math.inf}, InvalidInputError),
        ({"upper_barrier": 80}, InvalidInputError),
        ({"lower_barrier": 130}, InvalidInputError),
        ({"strike": 0}, InvalidInputError),
        ({"expiry_time": datetime(2024, 1, 1)}, InvalidInputError),
        ({"rebate": -1}, InvalidInputError),
        ({"rebate": math.nan}, InvalidInputError),
        (
            {"barrier_type": "knock_in", "rebate_timing": "at_hit"},
            InvalidInputError,
        ),
        ({"observation_times": []}, InvalidInputError),
        (
            {"observation_times": [END, END]},
            InvalidInputError,
        ),
        (
            {"observation_times": [END, START]},
            InvalidInputError,
        ),
        (
            {
                "observation_times": [datetime(2026, 1, 2)],
            },
            InvalidInputError,
        ),
    ]:
        with subtests.test(values=values):
            assert_rejection(expected_error, lambda values=values: contract(**values))
    for changes, error in [
        ({"touch_state": "unknown"}, ValueError),
        ({"touch_state": False}, TypeError),
        ({"strike": True}, TypeError),
    ]:
        with pytest.raises(error):
            option.replace(**changes)
        assert option == contract()
    known = option.replace(
        touch_state="untouched",
    )
    assert known.replace() == known
    assert known.replace(touch_state=None) == option
    assert option.touch_state is None
    scheduled = contract(observation_times=(d for d in [START, END]))
    dates = list(scheduled.observation_times)
    dates.clear()
    assert scheduled.observation_times == ((START, END))
    assert scheduled.observation_schedule.times == ((START, END))
    with pytest.raises(AttributeError):
        option.upper_barrier = 130


def test_current_touch_history_rebates_expiry_and_native_time_boundaries():
    from datetime import datetime, timedelta

    engine = AnalyticDoubleBarrierEngine()
    later = market(time=START + timedelta(days=1))
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
        assert engine.price(past, later) == pytest.approx(expected, rel=0, abs=1e-12)
        assert_rejection(
            InvalidInputError,
            lambda past=past: engine.price(past, market()),
        )
        if spot in (80, 120):
            result = engine.price_with_greeks(option, market(spot), greeks="all")
            for name in (
                "delta",
                "gamma",
                "speed",
                "charm",
                "color",
                "vanna",
                "zomma",
            ):
                assert getattr(result, name) is None
    assert_rejection(
        MissingHistoryError,
        lambda: engine.price(contract(), later),
    )
    microsecond = market(time=datetime(2025, 1, 1, microsecond=1))
    assert_rejection(
        MissingHistoryError,
        lambda: engine.price(contract(), microsecond),
    )
    for kind, spot in product(("knock_in", "knock_out"), (80, 110, 120)):
        option = contract(barrier_type=kind, rebate=2, touch_state="untouched")
        touched = spot != 110
        expected = max(spot - 100, 0) if ((kind == "knock_in") == touched) else 2
        result = engine.price_with_greeks(
            option,
            market(spot, time=END),
            greeks="all",
        )
        assert result.price == expected
        assert_terminal_greeks(result, None if touched else int(kind == "knock_out"))
    option = contract(touch_state="untouched")
    after = market(time=datetime(2026, 1, 1, microsecond=1))
    assert_rejection(InvalidInputError, lambda: engine.price(option, after))
    assert_rejection(
        InvalidInputError,
        lambda: engine.price(
            option,
            market(time=datetime(2024, 12, 31)),
        ),
    )
    inside = contract(barrier_type="knock_in", touch_state="touched")
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    assert engine.price(inside, later) == AnalyticEuropeanEngine().price(vanilla, later)


def test_schedules_validate_then_resolve_before_analytic_restrictions():
    from datetime import datetime

    engine = AnalyticDoubleBarrierEngine()
    first = datetime(2025, 6, 30)
    for dates in ([first], [first, END]):
        option = contract(observation_times=dates)
        assert_rejection(
            UnsupportedOperationError,
            lambda option=option: engine.price(option, market()),
        )
        touched = option.replace(
            touch_state="touched",
        )
        assert_rejection(
            InvalidInputError,
            lambda touched=touched: engine.price(touched, market()),
        )
    for kind in ("knock_in", "knock_out"):
        option = contract(
            barrier_type=kind,
            rebate=3,
            observation_times=[first],
        )
        assert engine.price(
            option,
            market(120, time=first),
        ) == pytest.approx(
            (
                AnalyticEuropeanEngine().price(
                    EuropeanOption(
                        option_type="call",
                        strike=100,
                        effective_time=START,
                        expiry_time=END,
                    ),
                    market(120, time=first),
                )
                if kind == "knock_in"
                else 3 * math.exp(-0.04 * 185 / 365)
            ),
            rel=0,
            abs=1e-12,
        )
        intraday = market(130, time=datetime(2025, 6, 30, 12))
        assert_rejection(
            MissingHistoryError,
            lambda option=option, intraday=intraday: engine.price(option, intraday),
        )
        known = option.replace(
            touch_state="untouched",
        )
        expected = (
            3 * math.exp(-0.04 * 184.5 / 365)
            if kind == "knock_in"
            else AnalyticEuropeanEngine().price(
                EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_time=START,
                    expiry_time=END,
                ),
                intraday,
            )
        )
        assert engine.price(known, intraday) == pytest.approx(
            expected, rel=0, abs=1e-12
        )
    with pytest.raises(InvalidInputError):
        contract(observation_times=[datetime(2025, 7, 5)])


def test_image_spectral_switch_boundary_limits_and_tiny_volatility():
    engine = AnalyticDoubleBarrierEngine()
    for sigma in (0.20273255202675663, 0.20273255608140772, 0.3040988310811233):
        assert engine.price(
            contract(),
            market(volatility=sigma),
        ) == pytest.approx(
            reference("call", "knock_out", volatility=sigma),
            rel=0,
            abs=1e-9,
        )
    for spot in (
        80.001,
        119.999,
        math.nextafter(80, math.inf),
        math.nextafter(120, 0),
    ):
        actual = engine.price(contract(), market(spot))
        assert actual >= 0
        assert actual == pytest.approx(
            reference("call", "knock_out", spot=spot),
            rel=0,
            abs=1e-9,
        )
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    assert engine.price(
        contract(lower_barrier=0.01, upper_barrier=10000),
        market(),
    ) == pytest.approx(
        AnalyticEuropeanEngine().price(vanilla, market()),
        rel=0,
        abs=1e-9,
    )
    assert (
        engine.price(
            contract(lower_barrier=99.99, upper_barrier=100.01),
            market(),
        )
        == 0
    )
    for direction, strike in (("call", 125), ("put", 75)):
        assert (
            engine.price(
                contract(option_type=direction, strike=strike),
                market(),
            )
            == 0
        )
    for up, timing in product((False, True), ("at_hit", "at_expiry")):
        hit = math.log(102 / 100 if up else 100 / 98) / 0.03
        expected = 3 * math.exp(-0.04 * (hit if timing == "at_hit" else 1))
        option = contract(
            lower_barrier=80 if up else 98,
            upper_barrier=102 if up else 120,
            rebate=3,
            rebate_timing=timing,
        )
        assert engine.price(
            option,
            market(volatility=1e-12, dividend=0.01 if up else 0.07),
        ) == pytest.approx(expected, rel=0, abs=1e-7)
    assert engine.price(
        contract(lower_barrier=99.5, upper_barrier=100.5, strike=90),
        market(volatility=1e-12, dividend=0.04),
    ) == pytest.approx(10 * math.exp(-0.04), rel=0, abs=1e-10)


def test_rebates_match_independent_exit_series_including_negative_rates():
    def rebate_reference(rate, count):
        width = math.log(125 / 80)
        survival, integrated = [], []
        for index in range(count):
            frequency = 2 * index + 1
            coefficient = 4 * (-1 if index % 2 else 1) / (frequency * math.pi)
            decay = 0.5 * 0.2**2 * (frequency * math.pi / width) ** 2
            survival.append(coefficient * math.exp(-decay))
            integrated.append(
                coefficient * -math.expm1(-(rate + decay)) / (rate + decay)
            )
        discounted = math.exp(-rate) * math.fsum(survival)
        return [
            3 * (math.exp(-rate) - discounted),
            3 * (1 - discounted - rate * math.fsum(integrated)),
            3 * discounted,
        ]

    engine = AnalyticDoubleBarrierEngine()
    for rate in (-0.02, 0, 0.04):
        refs, refined = rebate_reference(rate, 4097), rebate_reference(rate, 8193)
        for a, b in zip(refs, refined, strict=True):
            assert a == pytest.approx(b, rel=0, abs=1e-11)
        context = market(rate=rate, dividend=rate - 0.02)
        for index, kind, timing in (
            (0, "knock_out", "at_expiry"),
            (1, "knock_out", "at_hit"),
            (2, "knock_in", "at_expiry"),
        ):
            terms = {
                "upper_barrier": 125,
                "barrier_type": kind,
                "rebate_timing": timing,
            }
            actual = engine.price(contract(**terms, rebate=3), context) - engine.price(
                contract(**terms), context
            )
            assert actual == pytest.approx(refs[index], rel=0, abs=1e-9)


def test_shared_analysis_solving_errors_and_concurrent_reads_preserve_inputs():
    from concurrent.futures import ThreadPoolExecutor
    from datetime import timedelta

    from kiyosi.market import RateTermStructure
    from kiyosi.pricing import implied_volatility

    option = contract(lower_barrier=50, upper_barrier=200)
    context = market()
    engine = AnalyticDoubleBarrierEngine()
    quote = engine.price(option, context)
    solved = implied_volatility(
        engine,
        option,
        context,
        quote,
        lower_bound=0.15,
        upper_bound=0.25,
    )
    assert solved == pytest.approx(0.2, rel=0, abs=1e-7)
    known = option.replace(
        touch_state="untouched",
    )
    later = market(time=START + timedelta(days=1))
    result = engine.price_with_greeks(known, later, greeks="all")
    for name in (
        "delta",
        "gamma",
        "vega",
        "rho",
        "theta",
        "speed",
        "charm",
        "color",
        "vanna",
        "zomma",
    ):
        assert math.isfinite(getattr(result, name))
    for field in (
        "spot_shift",
        "volatility_shift",
        "rate_shift",
        "time_shift",
    ):
        assert_rejection(
            InvalidInputError,
            lambda field=field: numerical_greeks(
                engine,
                option,
                context,
                ["delta"],
                **{field: timedelta() if field == "time_shift" else 0},
            ),
        )
        with pytest.raises(TypeError):
            numerical_greeks(
                engine,
                option,
                context,
                ["delta"],
                **{field: True},
            )
    with pytest.raises(TypeError):
        numerical_greeks(
            engine,
            option,
            context,
            ["delta"],
            time_shift=1.5,
        )
    with pytest.raises(OverflowError):
        implied_volatility(
            engine,
            option,
            context,
            quote,
            max_iterations=2**80,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    fixed = contract(rebate=3, touch_state="touched")
    assert_rejection(
        UnsupportedOperationError,
        lambda: implied_volatility(
            engine,
            fixed,
            later,
            engine.price(fixed, later),
            lower_bound=0.0,
            upper_bound=4.0,
        ),
    )
    node = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[math.exp(-0.04), (math.exp(-0.04)) * 0.5],
    )
    curved = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=node)
    )
    assert_rejection(
        UnsupportedOperationError,
        lambda: engine.price(option, curved),
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
            math.nan,
            lower_bound=0.0,
            upper_bound=4.0,
        ),
    )
    near = market(80.005)
    guarded = engine.price_with_greeks(contract(), near, ["delta", "gamma", "vega"])
    assert guarded.delta is None
    assert guarded.gamma is None
    assert guarded.vega is not None
    with ThreadPoolExecutor(max_workers=2) as pool:
        assert (
            list(
                pool.map(
                    lambda _: engine.price(option, context),
                    range(4),
                )
            )
            == [quote] * 4
        )
    assert option == contract(lower_barrier=50, upper_barrier=200)
    assert context == market()


def test_settled_zero_rebates_do_not_require_representable_discount():
    engine = AnalyticDoubleBarrierEngine()
    outside = contract(expiry_time=datetime(2027, 1, 1))
    assert (
        engine.price(
            outside,
            market(120, rate=1e308, dividend=0),
        )
        == 0
    )
    inside = contract(
        expiry_time=datetime(2027, 1, 1),
        barrier_type="knock_in",
        observation_times=[START],
        touch_state="untouched",
    )
    assert (
        engine.price(
            inside,
            market(time=datetime(2025, 1, 2), rate=1e308, dividend=0),
        )
        == 0
    )
