"""Range coupon contracts and independent marginal/payoff references."""

import math
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, time, timedelta
from datetime import time as clock_time
from functools import partial

import pytest
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    MissingHistoryError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import RangeAccrualOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import (
    AnalyticRangeAccrualEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_rejection
from support.history import dated as dated_history
from support.history import history_fields
from support.terminal import assert_terminal_greeks

START = datetime(2025, 1, 1)


def instant(value):
    return (
        datetime.combine(value, time())
        if isinstance(value, date) and not isinstance(value, datetime)
        else value
    )


def contract(**changes):
    values = {
        "calendar": all_days_calendar(),
        "notional": 1000,
        "annual_coupon_rate": 0.12,
        "lower_bound": 95,
        "upper_bound": 110,
        "effective_time": START,
        "expiry_time": START + timedelta(days=16),
    }
    values.update(changes)
    return RangeAccrualOption(
        **history_fields(observation_clock_time=clock_time(), **values)
    )


def market(valuation=START, spot=100, rate=0.04, dividend=0.01, volatility=0.2):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
    )


def reference(option, context):
    hits = option.realized_hit_count or 0
    total = 0
    model = context.model_parameters
    valuation = context.valuation_time
    current = option.effective_time + timedelta(days=1)
    while current <= option.expiry_time:
        if option.calendar.is_trading_day(current.date()):
            total += 1
            observed = instant(current)
            if observed == valuation:
                hits += option.lower_bound <= context.spot_price <= option.upper_bound
            elif observed > valuation:
                elapsed = (observed - valuation).total_seconds() / (365 * 86400)
                mean = (
                    math.log(context.spot_price)
                    - model.risk_free_curve.log_factor(valuation, observed)
                    + model.dividend_curve.log_factor(valuation, observed)
                    - 0.5 * model.volatility**2 * elapsed
                )
                width = model.volatility * math.sqrt(2 * elapsed)
                upper = math.erfc((mean - math.log(option.upper_bound)) / width) / 2
                lower = (
                    0
                    if option.lower_bound == 0
                    else math.erfc((mean - math.log(option.lower_bound)) / width) / 2
                )
                hits += upper - lower
        current += timedelta(days=1)
    return (
        option.notional
        * option.annual_coupon_rate
        * (option.expiry_time - option.effective_time).days
        / 365
        * hits
        / total
        * model.risk_free_curve.factor(valuation, option.expiry_time)
    )


@pytest.mark.parametrize("lower", (110.0, 120.0))
def test_deterministic_flat_curves_include_exact_bounds_after_many_observations(lower):
    rate = math.log1p((lower - 100.0) / 100.0)
    option = contract(
        annual_coupon_rate=0.1,
        lower_bound=lower,
        upper_bound=lower + 10.0,
        expiry_time=START + timedelta(days=365),
    )
    assert AnalyticRangeAccrualEngine().price(
        option, market(rate=rate, dividend=0.0, volatility=0.0)
    ) == pytest.approx(100.0 / 365.0 * math.exp(-rate), rel=0, abs=1e-14)


def test_dense_shifted_curves_match_independent_intraday_marginals(subtests):
    nodes = [START + timedelta(days=n, hours=12) for n in range(731)]
    reference_time = START - timedelta(days=2)

    def curve(rate, amplitude, shift):
        years = [
            (node - reference_time).total_seconds() / (365 * 86400) for node in nodes
        ]
        return RateTermStructure(
            reference_time=reference_time,
            times=nodes,
            factors=[math.exp(-rate * t + amplitude * math.sin(7 * t)) for t in years],
        ).shifted(shift)

    risk = curve(0.03, 0.005, 0.002)
    dividend = curve(-0.01, 0.003, -0.001)
    for valuation in (START, START + timedelta(days=5, hours=9)):
        option = contract(expiry_time=START + timedelta(days=730))
        if valuation != START:
            option = option.replace_history(
                realized_hit_count=3, history_time=valuation
            )
        for volatility in (0.2, 1e-10):
            context = market(
                valuation=valuation,
                rate=risk,
                dividend=dividend,
                volatility=volatility,
            )
            with subtests.test(valuation=valuation, volatility=volatility):
                assert AnalyticRangeAccrualEngine().price(
                    option, context
                ) == pytest.approx(reference(option, context), rel=0, abs=1e-10)


def test_schedule_snapshots_survive_calendar_replacement_and_owner_deletion():
    original = contract(expiry_time=START + timedelta(days=3))
    snapshot = original.observation_schedule
    closed = (START + timedelta(days=1)).date()
    calendar = custom_calendar(base=original.calendar, closed_dates=[closed])
    updated = original.replace(calendar=calendar)
    assert tuple(updated.observation_schedule) == tuple(snapshot)[1:]
    assert updated.observation_schedule.calendar == calendar
    del original, updated
    assert tuple(snapshot) == tuple(START + timedelta(days=n) for n in range(1, 4))
    assert snapshot.calendar.is_trading_day(closed)


def test_automatic_time_bumps_recover_daily_risk_without_crossing_observations():
    option = RangeAccrualOption(
        notional=1000,
        annual_coupon_rate=0.12,
        lower_bound=90,
        upper_bound=110,
        effective_time=START,
        expiry_time=datetime(2025, 1, 5),
        observation_clock_time=clock_time(15),
        realized_hit_count=1,
        history_time=START + timedelta(days=1),
    )
    context = market(datetime(2025, 1, 2, 12), rate=0.04, dividend=0, volatility=0)
    engine = AnalyticRangeAccrualEngine()
    request = ["theta", "charm", "color"]
    auto = engine.price_with_greeks(dated_history(option, context), context, request)
    assert auto.require("theta") == pytest.approx(0.04 * auto.price, rel=0, abs=1e-9)
    assert auto.require("charm") == 0
    assert auto.require("color") == 0
    assert auto == numerical_greeks(
        engine,
        dated_history(option, context),
        context,
        request,
        time_shift=None,
    )
    explicit = numerical_greeks(
        engine,
        dated_history(option, context),
        context,
        request,
        time_shift=timedelta(days=1),
    )
    assert explicit.status("theta") == "stencil_unavailable"
    at_observation = context.replace(valuation_time=datetime(2025, 1, 2, 15))
    assert (
        engine.price_with_greeks(
            dated_history(option, at_observation), at_observation, request
        ).status("theta")
        == "stencil_unavailable"
    )


def test_dividend_coverage_ends_at_the_last_observation(subtests):
    start = datetime(2025, 1, 2, 16)
    last = datetime(2025, 1, 3, 9)
    expiry = datetime(2025, 1, 3, 17)
    option = RangeAccrualOption(
        notional=1000,
        annual_coupon_rate=0.12,
        lower_bound=90,
        upper_bound=110,
        effective_time=start,
        expiry_time=expiry,
        observation_clock_time=time(9),
    )

    def price(risk_end, dividend_end):
        return AnalyticRangeAccrualEngine().price(
            dated_history(
                option,
                market(
                    valuation=start,
                    rate=RateTermStructure.bounded_flat(
                        0.04, reference_time=start, max_time=risk_end
                    ),
                    dividend=RateTermStructure.bounded_flat(
                        0.01, reference_time=start, max_time=dividend_end
                    ),
                ),
            ),
            market(
                valuation=start,
                rate=RateTermStructure.bounded_flat(
                    0.04, reference_time=start, max_time=risk_end
                ),
                dividend=RateTermStructure.bounded_flat(
                    0.01, reference_time=start, max_time=dividend_end
                ),
            ),
        )

    assert price(expiry, last) == price(expiry, expiry)
    for risk_end, dividend_end in ((last, last), (expiry, start)):
        with (
            subtests.test(risk_end=risk_end, dividend_end=dividend_end),
            pytest.raises(InvalidInputError),
        ):
            price(risk_end, dividend_end)


def test_unresolvable_curve_knots_fail_before_future_pricing():
    knot, expiry = datetime(2500, 1, 1), datetime(2500, 1, 2)
    curve = RateTermStructure(
        reference_time=START,
        times=[knot, instant(knot) + timedelta(microseconds=1), expiry],
        factors=[1, 1, 1],
    )
    option = contract(expiry_time=expiry)
    engine = AnalyticRangeAccrualEngine()
    for rate, dividend in ((curve, 0), (0, curve)):
        context = market(rate=rate, dividend=dividend)
        assert_rejection(
            ComputationFailureError,
            engine.price,
            dated_history(option, context),
            context,
        )
        assert (
            engine.price(
                dated_history(
                    contract(
                        effective_time=option.effective_time,
                        expiry_time=expiry,
                        annual_coupon_rate=0,
                    ),
                    context,
                ),
                context,
            )
            == 0
        )


def test_native_representation_errors_and_unsupported_pairs(subtests):
    from kiyosi.instruments import EuropeanOption
    from kiyosi.pricing import AnalyticEuropeanEngine

    option, context = contract(), market()
    engine = AnalyticRangeAccrualEngine()
    vanilla = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=option.expiry_time,
    )
    for field in ("effective_time", "expiry_time"):
        for value in ("2025-01-01", START.date()):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                contract(**{field: value})
    for value in (2**31, -(2**31) - 1):
        with pytest.raises(OverflowError):
            option.replace_history(
                realized_hit_count=value,
                history_time=(option.history_time or option.effective_time)
                if value is not None
                else None,
            )
    with pytest.raises(TypeError):
        option.replace_history(notional=2000)
    with pytest.raises(UnsupportedOperationError):
        engine.price(dated_history(vanilla, context), context)
    with pytest.raises(UnsupportedOperationError):
        implied_volatility(
            engine,
            dated_history(vanilla, context),
            context,
            1,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    with pytest.raises(UnsupportedOperationError):
        AnalyticEuropeanEngine().price(dated_history(option, context), context)
    with pytest.raises(TypeError):
        numerical_greeks(
            engine,
            dated_history(option, context),
            context,
            "delta",
            spot_shift=True,
        )
    with pytest.raises(TypeError):
        implied_volatility(
            engine,
            dated_history(option, context),
            context,
            True,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    assert option.replace_history() == option
    assert option == contract()


def test_solver_failures_and_no_mutation():
    engine, option, context = AnalyticRangeAccrualEngine(), contract(), market()
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        dated_history(option, context),
        context,
        -1,
        lower_bound=0.1,
        upper_bound=0.4,
    )
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        dated_history(contract(annual_coupon_rate=0), context),
        context,
        0,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        dated_history(
            option.replace_history(
                realized_hit_count=15,
                history_time=option.history_time or option.expiry_time,
            ),
            market(option.expiry_time),
        ),
        market(option.expiry_time),
        0,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert_rejection(
        MissingHistoryError,
        implied_volatility,
        engine,
        dated_history(option, market(START + timedelta(days=3))),
        market(START + timedelta(days=3)),
        0,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    assert option == contract()
    assert context.model_parameters.volatility == 0.2


def test_daily_coupon_matches_exact_marginal_sum():
    option = contract(expiry_time=START + timedelta(days=365))
    context = market()
    assert AnalyticRangeAccrualEngine().price(
        dated_history(option, context), context
    ) == pytest.approx(
        reference(dated_history(option, context), context), rel=0, abs=1e-10
    )
    assert implied_volatility(
        AnalyticRangeAccrualEngine(),
        dated_history(option, context),
        context,
        reference(dated_history(option, context), context),
        lower_bound=0.1,
        upper_bound=0.4,
    ) == pytest.approx(context.model_parameters.volatility, rel=0, abs=1e-8)


def test_terms_representation_and_owning_replacement(subtests):
    option = contract()
    assert option.realized_hit_count is None
    assert option.notional == 1000
    assert option.annual_coupon_rate == 0.12
    assert option.lower_bound == 95
    assert option.upper_bound == 110
    changed = option.replace_history(
        realized_hit_count=3,
        history_time=option.history_time or option.expiry_time,
    )
    assert changed.realized_hit_count == 3
    assert changed.replace_history(realized_hit_count=None) == option
    for value in (True, 1.0, 1.5, "1"):
        with subtests.test(value=value):
            with pytest.raises(TypeError):
                contract(realized_hit_count=value)
            with pytest.raises(TypeError):
                option.replace_history(
                    realized_hit_count=value,
                    history_time=(option.history_time or option.effective_time)
                    if value is not None
                    else None,
                )
    with pytest.raises(OverflowError):
        contract(realized_hit_count=2**100)
    for name in ("notional", "annual_coupon_rate", "lower_bound", "upper_bound"):
        for value in (True, "100"):
            with subtests.test(name=name, value=value), pytest.raises(TypeError):
                contract(**{name: value})
        for value in (-1, math.inf, math.nan):
            with pytest.raises(InvalidInputError, match=name):
                contract(**{name: value})
    for values in (
        {"notional": 0},
        {"lower_bound": 110},
        {"upper_bound": 90},
        {"realized_hit_count": -1},
    ):
        with pytest.raises(InvalidInputError, match=next(iter(values))):
            contract(**values)
    assert_rejection(InvalidInputError, contract, expiry_time=START)
    with pytest.raises(InvalidInputError, match="realized_hit_count"):
        option.replace_history(
            realized_hit_count=-1,
            history_time=(option.history_time or option.effective_time)
            if -1 is not None
            else None,
        )
    assert option == contract()
    assert "realized_hit_count" in repr(changed)
    with pytest.raises(AttributeError):
        option.notional = 2


def test_closed_bounds_and_expiry_coupon(subtests):
    option = contract(expiry_time=START + timedelta(days=2), realized_hit_count=1)
    engine = AnalyticRangeAccrualEngine()
    for spot, hit in (
        (math.nextafter(95, 0), False),
        (95, True),
        (math.nextafter(95, 100), True),
        (math.nextafter(110, 100), True),
        (110, True),
        (math.nextafter(110, 120), False),
    ):
        with subtests.test(spot=spot):
            result = engine.price_with_greeks(
                dated_history(option, market(option.expiry_time, spot)),
                market(option.expiry_time, spot),
                greeks="all",
            )
            assert result.price == pytest.approx(
                1000 * 0.12 / 365 * (1 + hit), rel=0, abs=1e-12
            )
            assert_terminal_greeks(result, None)


def test_history_midnight_and_intraday_within_contract_life(subtests):
    engine = AnalyticRangeAccrualEngine()
    for valuation, hits in (
        (instant(START), None),
        (instant(START) + timedelta(hours=12), None),
        (instant(START) + timedelta(days=1), None),
        (instant(START) + timedelta(days=1, hours=12), 1),
        (instant(START) + timedelta(days=8), 5),
    ):
        with subtests.test(valuation=valuation):
            option = contract(realized_hit_count=hits)
            context = market(valuation)
            assert engine.price(
                dated_history(option, context), context
            ) == pytest.approx(
                reference(dated_history(option, context), context), rel=0, abs=1e-10
            )
    for valuation, count in (
        (START, 1),
        (START + timedelta(days=1), 1),
        (instant(START) + timedelta(days=1, hours=12), 2),
    ):
        assert_rejection(
            InvalidInputError,
            contract,
            realized_hit_count=count,
            history_time=valuation,
        )
    assert_rejection(
        MissingHistoryError,
        engine.price,
        dated_history(contract(), market(START + timedelta(days=2))),
        market(START + timedelta(days=2)),
    )
    assert_rejection(
        InvalidInputError,
        contract,
        realized_hit_count=16,
    )


def test_weekend_calendar_full_term_and_zero_coupon():
    option = contract(
        calendar=weekdays_calendar(),
        effective_time=datetime(2025, 1, 3),
        expiry_time=datetime(2025, 1, 6),
    )
    context = market(datetime(2025, 1, 3))
    engine = AnalyticRangeAccrualEngine()
    assert engine.price(dated_history(option, context), context) == pytest.approx(
        reference(dated_history(option, context), context), rel=0, abs=1e-12
    )
    sunday = contract(calendar=weekdays_calendar(), expiry_time=datetime(2025, 1, 5))
    assert engine.price(dated_history(sunday, market()), market()) == pytest.approx(
        reference(dated_history(sunday, market()), market()), rel=0, abs=1e-12
    )
    short_curve = RateTermStructure(
        reference_time=START, times=[START + timedelta(days=1)], factors=[0.999]
    )
    short = market(rate=short_curve)
    assert_rejection(
        InvalidInputError,
        engine.price,
        dated_history(
            contract(
                calendar=weekdays_calendar(),
            ),
            short,
        ),
        short,
    )
    assert (
        engine.price(
            dated_history(
                contract(calendar=weekdays_calendar(), annual_coupon_rate=0), short
            ),
            short,
        )
        == 0
    )
    assert_rejection(
        InvalidInputError,
        engine.price,
        dated_history(
            contract(
                calendar=weekdays_calendar(),
            ),
            market(dividend=short_curve),
        ),
        market(dividend=short_curve),
    )
    weekend = market(datetime(2025, 1, 4, 12))
    result = engine.price_with_greeks(
        dated_history(option, weekend), weekend, ["theta", "charm", "color"]
    )
    before = market(datetime(2025, 1, 3, 12))
    after = market(datetime(2025, 1, 5, 12))
    assert result.theta == pytest.approx(
        (
            reference(dated_history(option, after), after)
            - reference(dated_history(option, before), before)
        )
        / (2 / 365),
        rel=0,
        abs=3.65e-10,
    )
    assert result.charm is not None
    assert result.color is not None
    assert_rejection(
        MissingHistoryError,
        engine.price,
        dated_history(
            contract(calendar=weekdays_calendar(), annual_coupon_rate=0),
            market(START + timedelta(days=3)),
        ),
        market(START + timedelta(days=3)),
    )


def test_coupon_is_weighted_over_full_contract_and_discounted_only_at_expiry():
    option = contract()
    engine = AnalyticRangeAccrualEngine()
    context = market(rate=0.8, dividend=0.8, volatility=1e-9)
    assert engine.price(dated_history(option, context), context) == pytest.approx(
        120 * 16 / 365 * math.exp(-0.8 * 16 / 365), rel=0, abs=1e-12
    )
    live = option.replace_history(
        realized_hit_count=3,
        history_time=option.history_time or option.expiry_time,
    )
    current = market(START + timedelta(days=7), rate=0.8, dividend=0.8, volatility=1e-9)
    assert engine.price(dated_history(live, current), current) == pytest.approx(
        120 * 13 / 365 * math.exp(-0.8 * 9 / 365), rel=0, abs=1e-12
    )
    assert (
        engine.price(
            dated_history(
                option, market(spot=80, rate=0.8, dividend=0.8, volatility=1e-9)
            ),
            market(spot=80, rate=0.8, dividend=0.8, volatility=1e-9),
        )
        == 0
    )


def test_flat_nonflat_and_negative_rates_against_independent_probabilities():
    option = contract()
    rate = RateTermStructure(
        reference_time=START,
        times=[START + timedelta(days=5), option.expiry_time],
        factors=[1.001, 0.997],
    )
    dividend = RateTermStructure(
        reference_time=START - timedelta(days=1),
        times=[START + timedelta(days=7), option.expiry_time],
        factors=[0.998, 1.002],
    )
    engine = AnalyticRangeAccrualEngine()
    for context in (
        market(),
        market(rate=-0.03),
        market(rate=rate, dividend=dividend),
    ):
        assert engine.price(dated_history(option, context), context) == pytest.approx(
            reference(dated_history(option, context), context), rel=0, abs=1e-10
        )
    lower_zero = contract(lower_bound=0)
    assert engine.price(dated_history(lower_zero, market()), market()) == pytest.approx(
        reference(dated_history(lower_zero, market()), market()), rel=0, abs=1e-10
    )


def test_extreme_coupon_tail_scaling_and_overflow():
    mean = math.log(100) + 0.01 / 365
    width = 0.2 / math.sqrt(365)
    option = contract(
        notional=1e300,
        annual_coupon_rate=1e100,
        lower_bound=math.exp(mean + 40 * width),
        upper_bound=math.exp(mean + 41 * width),
        expiry_time=START + timedelta(days=1),
    )
    expected = math.exp(
        math.log(1e300)
        + math.log(1e100)
        - math.log(365)
        - 0.04 / 365
        - 804.608442013753788
    )
    engine = AnalyticRangeAccrualEngine()
    assert engine.price(
        dated_history(option, market()), market()
    ) / expected == pytest.approx(1, rel=0, abs=2e-11)
    overflow = contract(notional=1e300, annual_coupon_rate=1e100)
    assert_rejection(
        ComputationFailureError,
        engine.price,
        dated_history(overflow, market()),
        market(),
    )


def test_adjacent_price_bounds_at_tiny_volatility(subtests):
    below = math.nextafter(100.0, 0.0)
    above = math.nextafter(100.0, math.inf)
    engine = AnalyticRangeAccrualEngine()
    for lower, upper, volatility, expected, days in (
        (100, above, 1e-16, 0.5, 1),
        (below, 100, 1e-16, 0.5, 1),
        (below, above, 1e-16, 1.0, 1),
        (100, above, 1e-16, 0.5, 2),
        (100, above, math.ulp(0.0), 0.5, 1),
        (below, 100, math.ulp(0.0), 0.5, 1),
        (below, above, math.ulp(0.0), 1.0, 1),
        (below, 110, 1e-14, 0.6069959804126283, 1),
        (90, above, 1e-14, 0.6069959804126285, 1),
    ):
        with subtests.test(lower=lower, upper=upper, days=days):
            option = contract(
                notional=365 / days,
                annual_coupon_rate=1,
                lower_bound=lower,
                upper_bound=upper,
                expiry_time=START + timedelta(days=days),
            )
            context = market(rate=0, dividend=0, volatility=volatility)
            assert engine.price(
                dated_history(option, context), context
            ) == pytest.approx(expected, rel=0, abs=2e-14)


def test_exact_prices_and_greeks_support_concurrent_reads():
    option, context = contract(), market()
    engine = AnalyticRangeAccrualEngine()
    expected = reference(dated_history(option, context), context)
    with ThreadPoolExecutor(2) as pool:
        for value in pool.map(
            lambda _: engine.price(dated_history(option, context), context),
            range(4),
        ):
            assert value == pytest.approx(expected, rel=0, abs=1e-10)
    result = numerical_greeks(
        engine,
        dated_history(option, context),
        context,
        ["delta", "gamma"],
        spot_shift=0.25,
    )
    up = engine.price(dated_history(option, market(spot=100.25)), market(spot=100.25))
    down = engine.price(dated_history(option, market(spot=99.75)), market(spot=99.75))
    assert result.price == engine.price(dated_history(option, context), context)
    assert result.delta == pytest.approx((up - down) / 0.5, rel=0, abs=1e-12)
    assert result.gamma == pytest.approx(
        (up - 2 * result.price + down) / 0.25**2, rel=0, abs=1e-12
    )


def test_smoothed_greeks_against_independent_reference(subtests):
    valuation = START + timedelta(days=3)
    curve = RateTermStructure(
        reference_time=START - timedelta(days=5),
        times=[START + timedelta(days=7), START + timedelta(days=15)],
        factors=[1.001, 0.997],
    )
    h, v, r = 0.1, 0.0001, 0.0001
    for days, rate, dividend in ((8, 0.04, 0.01), (15, curve, -0.01)):
        option = contract(
            calendar=weekdays_calendar(),
            effective_time=START + timedelta(days=2),
            expiry_time=START + timedelta(days=days),
        )
        context = market(
            valuation,
            spot=99,
            rate=rate,
            dividend=dividend,
        )
        engine = AnalyticRangeAccrualEngine()
        result = numerical_greeks(
            engine,
            dated_history(option, context),
            context,
            greeks="all",
            spot_shift=0.1,
        )

        def price(
            spot=99,
            vol=0.2,
            rate=0.0,
            when=valuation,
            context=context,
            option=option,
        ):
            model = context.model_parameters.replace(
                risk_free_rate=context.model_parameters.risk_free_curve.shifted(rate),
                volatility=vol,
            )
            return reference(
                dated_history(
                    option,
                    PricingContext(
                        model_parameters=model,
                        spot_price=spot,
                        valuation_time=when,
                    ),
                ),
                PricingContext(
                    model_parameters=model,
                    spot_price=spot,
                    valuation_time=when,
                ),
            )

        def delta(**args):
            return (price(spot=99 + h, **args) - price(spot=99 - h, **args)) / (2 * h)

        def gamma(**args):
            return (
                price(spot=99 + h, **args)
                - 2 * price(**args)
                + price(spot=99 - h, **args)
            ) / h**2

        expected = {
            "delta": delta(),
            "gamma": gamma(),
            "speed": (
                price(spot=99 + 2 * h)
                - 2 * price(spot=99 + h)
                + 2 * price(spot=99 - h)
                - price(spot=99 - 2 * h)
            )
            / (2 * h**3),
            "vega": (price(vol=0.2 + v) - price(vol=0.2 - v)) / (2 * v),
            "vanna": (delta(vol=0.2 + v) - delta(vol=0.2 - v)) / (2 * v),
            "zomma": (gamma(vol=0.2 + v) - gamma(vol=0.2 - v)) / (2 * v),
            "rho": (price(rate=r) - price(rate=-r)) / (2 * r),
            "theta": (
                price(when=valuation + timedelta(days=1))
                - price(when=valuation - timedelta(days=1))
            )
            / (2 / 365),
            "charm": (
                delta(when=valuation + timedelta(days=1))
                - delta(when=valuation - timedelta(days=1))
            )
            / (2 / 365),
            "color": (
                gamma(when=valuation + timedelta(days=1))
                - gamma(when=valuation - timedelta(days=1))
            )
            / (2 / 365),
        }
        for name, value in expected.items():
            with subtests.test(greek=name):
                assert getattr(result, name) is not None
                assert getattr(result, name) == pytest.approx(value, rel=0, abs=2e-8)


def test_curve_knot_uses_right_hand_time_greek_between_observations():
    valuation = START + timedelta(days=3)
    option = contract(
        calendar=weekdays_calendar(),
        effective_time=START + timedelta(days=2),
        expiry_time=START + timedelta(days=5),
    )
    rate = RateTermStructure(
        reference_time=START - timedelta(days=5),
        times=[valuation, option.expiry_time],
        factors=[0.999, 1.002],
    )
    context = market(valuation, rate=rate)
    after = market(valuation + timedelta(days=1), rate=rate)
    result = AnalyticRangeAccrualEngine().price_with_greeks(
        dated_history(option, context), context, "theta"
    )
    assert result.theta == pytest.approx(
        (
            reference(dated_history(option, after), after)
            - reference(dated_history(option, context), context)
        )
        * 365,
        rel=0,
        abs=3.65e-10,
    )


def test_long_contract_and_current_spot_stencil():
    engine = AnalyticRangeAccrualEngine()
    option = contract(expiry_time=START + timedelta(days=21204))
    assert engine.price(dated_history(option, market()), market()) == pytest.approx(
        reference(dated_history(option, market()), market()), rel=0, abs=1e-8
    )
    seasoned = option.replace_history(
        realized_hit_count=20000,
        history_time=option.history_time or option.expiry_time,
    )
    context = market(START + timedelta(days=20001))
    assert engine.price(dated_history(seasoned, context), context) == pytest.approx(
        reference(dated_history(seasoned, context), context), rel=0, abs=1e-8
    )
    current = contract(realized_hit_count=0)
    for spot, available in ((95.005, False), (95.1, True)):
        result = engine.price_with_greeks(
            dated_history(current, market(START + timedelta(days=1), spot=spot)),
            market(START + timedelta(days=1), spot=spot),
            "delta",
        )
        assert (result.delta is not None) == available


def test_event_availability_and_shift_validation():
    engine = AnalyticRangeAccrualEngine()
    option = contract(realized_hit_count=0)
    result = engine.price_with_greeks(
        dated_history(option, market(START + timedelta(days=1), spot=95)),
        market(START + timedelta(days=1), spot=95),
        greeks="all",
    )
    for name in (
        "delta",
        "gamma",
        "speed",
        "vanna",
        "zomma",
        "theta",
        "charm",
        "color",
    ):
        assert getattr(result, name) is None
    assert result.vega is not None
    assert result.rho is not None
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        option,
        market(),
        "delta",
        volatility_shift=0,
    )
    with pytest.raises(TypeError):
        AnalyticRangeAccrualEngine(unknown=1)
