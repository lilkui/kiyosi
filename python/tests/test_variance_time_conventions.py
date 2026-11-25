"""Model variance clocks and natural-day interest through the public API."""

import inspect
import math
from datetime import datetime, time, timedelta

import pytest
from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    HestonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from test_local_volatility_fd_contracts import representatives
from test_support_matrix import HESTON_ENGINES, terminal_cases

FRIDAY = datetime(2026, 11, 13, 12)
MONDAY = FRIDAY + timedelta(days=3)


def bsm(**changes):
    return BlackScholesMertonParameters(
        **{"risk_free_rate": 0.07, "dividend_yield": 0.02, "volatility": 0.3} | changes
    )


def context(model, start=FRIDAY, spot=100):
    return PricingContext(model_parameters=model, valuation_time=start, spot_price=spot)


def european(start=FRIDAY, end=MONDAY, strike=100):
    return ins.EuropeanOption(
        option_type="call", strike=strike, effective_time=start, expiry_time=end
    )


def fd(**settings):
    return pricing.FiniteDifferenceVanillaEngine(
        **{"asset_step_count": 600, "time_step_count": 300} | settings
    )


def local(start, end, **changes):
    return LocalVolatilityParameters(
        **{
            "risk_free_rate": 0.07,
            "dividend_yield": 0.02,
            "surface": LocalVolatilitySurface(
                times=[start, end], spots=[50, 150], values=[[0.3] * 2] * 2
            ),
        }
        | changes
    )


@pytest.mark.parametrize("kind", ["bsm", "local"])
def test_model_clock_defaults_replacement_and_validation(kind):
    base = bsm() if kind == "bsm" else local(FRIDAY, MONDAY)
    factory = type(base)
    assert (
        inspect.signature(factory).parameters["variance_time_convention"].default
        == "actual_365_fixed"
    )
    assert base.variance_time_convention == "actual_365_fixed"
    assert base.calendar is None
    fields = {"risk_free_rate": 0.07, "dividend_yield": 0.02}
    fields["volatility" if kind == "bsm" else "surface"] = (
        base.volatility if kind == "bsm" else base.surface
    )
    with pytest.raises(InvalidInputError):
        factory(**fields, variance_time_convention="business")
    with pytest.raises(InvalidInputError):
        factory(**fields, calendar=weekdays_calendar())
    with pytest.raises(TypeError):
        factory(**fields, variance_time_convention=None)
    calendar = weekdays_calendar()
    changed = base.replace(variance_time_convention="business", calendar=calendar)
    assert changed.variance_time_convention == "business"
    assert changed.calendar == calendar
    assert "calendar=" in repr(changed)
    assert base.calendar is None
    assert (
        changed.replace(variance_time_convention="actual_365_fixed", calendar=None)
        == base
    )
    field = "volatility" if kind == "bsm" else "surface"
    assert getattr(changed, field) == getattr(base, field)
    for operation in [
        lambda: base.replace(variance_time_convention="business"),
        lambda: base.replace(calendar=calendar),
        lambda: changed.replace(variance_time_convention="actual_365_fixed"),
        lambda: changed.replace(calendar=None),
    ]:
        with pytest.raises(InvalidInputError):
            operation()
    for invalid in [True, 1, None]:
        with pytest.raises(TypeError):
            base.replace(variance_time_convention=invalid)
        with pytest.raises(TypeError):
            base.replace(calendar=invalid if invalid is not None else object())
    with pytest.raises(ValueError) as error:
        base.replace(variance_time_convention="unknown")
    assert type(error.value) is ValueError
    assert base == changed.replace(
        variance_time_convention="actual_365_fixed", calendar=None
    )


def test_heston_rejects_business_clock_at_model_boundary():
    fields = {
        "risk_free_rate": 0.07,
        "dividend_yield": 0.02,
        "initial_variance": 0.04,
        "long_run_variance": 0.04,
        "mean_reversion_speed": 1.5,
        "vol_of_vol": 0.3,
        "correlation": -0.7,
    }
    base = HestonParameters(**fields)
    assert base.variance_time_convention == "actual_365_fixed"
    assert base.calendar is None
    with pytest.raises(UnsupportedOperationError):
        HestonParameters(
            **fields, variance_time_convention="business", calendar=weekdays_calendar()
        )
    with pytest.raises(UnsupportedOperationError):
        base.replace(variance_time_convention="business", calendar=weekdays_calendar())
    assert base == HestonParameters(**fields)


@pytest.mark.parametrize(
    ("basis", "closed_friday", "trading_days"),
    [(243, False, 1.0), (252, False, 1.0), (365, False, 1.0), (252, True, 0.5)],
    ids=["basis243", "basis252", "basis365", "holiday_override"],
)
def test_european_matches_independent_two_clock_formula(
    basis, closed_friday, trading_days
):
    calendar = custom_calendar(
        base=weekdays_calendar(),
        trading_days_per_year=basis,
        closed_dates=[FRIDAY.date()] if closed_friday else [],
    )
    model = bsm(variance_time_convention="business", calendar=calendar)
    tau_c, tau_v = 3 / 365, trading_days / basis  # Friday noon to Monday noon.
    sd = 0.3 * math.sqrt(tau_v)
    d1 = ((0.07 - 0.02) * tau_c + 0.5 * sd**2) / sd
    normal = lambda x: 0.5 * math.erfc(-x / math.sqrt(2))
    expected = 100 * math.exp(-0.02 * tau_c) * normal(d1) - 100 * math.exp(
        -0.07 * tau_c
    ) * normal(d1 - sd)
    engine = fd()
    actual = engine.price(european(), context(model))
    assert actual == pytest.approx(expected, rel=0, abs=0.002)
    natural = engine.price(european(), context(bsm()))
    assert abs(actual - natural) > 0.05


def test_all_days_365_matches_natural_clock_and_keeps_surface_dates():
    business = local(
        FRIDAY,
        MONDAY,
        variance_time_convention="business",
        calendar=all_days_calendar(),
    )
    natural = business.replace(
        variance_time_convention="actual_365_fixed", calendar=None
    )
    engine = fd()
    assert engine.price(european(), context(business)) == pytest.approx(
        engine.price(european(), context(natural)), rel=0, abs=1e-10
    )
    assert business.surface.times == (FRIDAY, MONDAY)
    assert business.surface.volatility(FRIDAY + timedelta(days=1), 100) == 0.3


@pytest.mark.parametrize("product", ["vanilla", "digital"])
def test_all_days_365_matches_natural_clock_for_finite_difference_theta(product):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    natural = bsm(risk_free_rate=0.05, volatility=0.04)
    business = natural.replace(
        variance_time_convention="business", calendar=all_days_calendar()
    )
    settings = {"asset_step_count": 80, "time_step_count": 80}
    if product == "vanilla":
        option = european(start, end)
        engine = fd(**settings)
    else:
        option = ins.CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=10,
            effective_time=start,
            expiry_time=end,
        )
        engine = pricing.FiniteDifferenceDigitalEngine(**settings)
    expected = engine.price_with_greeks(option, context(natural, start), "theta")
    actual = engine.price_with_greeks(option, context(business, start), "theta")
    assert actual.theta == pytest.approx(expected.theta, rel=0, abs=1e-10)


def test_all_days_365_matches_natural_clock_for_barriers_and_continuations():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    natural = bsm(risk_free_rate=0.05, volatility=1.0)
    business = natural.replace(
        variance_time_convention="business", calendar=all_days_calendar()
    )
    natural, business = context(natural, start), context(business, start)
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
    }
    option = ins.BarrierOption(**terms, barrier_level=80, barrier_type="down_and_out")
    barrier = pricing.FiniteDifferenceBarrierEngine()
    assert barrier.price(option, business) == pytest.approx(
        barrier.price(option, natural), rel=0, abs=1e-10
    )

    binary = ins.BinaryBarrierOption(
        **terms,
        barrier_level=110,
        barrier_type="down_and_in",
        payoff_type="cash",
        payout=10,
    )
    digital = ins.CashOrNothingOption(**terms, payout=10)
    expected = pricing.AnalyticDigitalEngine().price(digital, natural)
    engine = pricing.FiniteDifferenceBinaryBarrierEngine()
    assert engine.price(binary, business) == pytest.approx(expected, rel=0, abs=1e-10)
    assert engine.solve(binary, business, spot_range=(99, 101)).price(
        100
    ) == pytest.approx(expected, rel=0, abs=1e-10)


def test_changing_local_volatility_uses_natural_dates_and_business_variance():
    surface = LocalVolatilitySurface(
        times=[FRIDAY, MONDAY], spots=[50, 150], values=[[0.2, 0.2], [0.5, 0.5]]
    )
    model = local(
        FRIDAY,
        MONDAY,
        surface=surface,
        variance_time_convention="business",
        calendar=weekdays_calendar(),
    )
    # Sigma is 0.2 + 0.1*d in natural days. Only [0, .5] and [2.5, 3] are open.
    integral = lambda d: 0.2**2 * d + 0.2 * 0.1 * d**2 + 0.1**2 * d**3 / 3
    variance = (integral(0.5) + integral(3) - integral(2.5)) / 261
    sd = math.sqrt(variance)
    tau_c = 3 / 365
    d1 = ((0.07 - 0.02) * tau_c + variance / 2) / sd
    normal = lambda x: 0.5 * math.erfc(-x / math.sqrt(2))
    expected = 100 * math.exp(-0.02 * tau_c) * normal(d1) - 100 * math.exp(
        -0.07 * tau_c
    ) * normal(d1 - sd)
    assert fd().price(european(), context(model)) == pytest.approx(
        expected, rel=0, abs=0.002
    )


def test_business_clock_every_fd_product_and_scheme(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    calendar = weekdays_calendar()
    constant = context(
        bsm(variance_time_convention="business", calendar=calendar), start
    )
    surface = context(
        local(start, end, variance_time_convention="business", calendar=calendar), start
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
                actual = engine.price(contract, constant)
                assert math.isfinite(actual)
                assert actual == pytest.approx(
                    engine.price(contract, surface), rel=0, abs=1e-6
                )
                if scheme == "rannacher":
                    saved = engine.solve(contract, constant, spot_range=(90, 110))
                    assert saved.price(100) == pytest.approx(actual, rel=0, abs=1e-6)


def test_business_clock_each_supported_contract(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    calendar = weekdays_calendar()
    constant = context(
        bsm(variance_time_convention="business", calendar=calendar), start
    )
    surface = context(
        local(start, end, variance_time_convention="business", calendar=calendar), start
    )
    for contract, names, _ in terminal_cases():
        for name in names:
            if not name.startswith("FiniteDifference") or name in HESTON_ENGINES:
                continue
            with subtests.test(contract=type(contract).__name__, engine=name):
                engine = getattr(pricing, name)(
                    asset_step_count=100, time_step_count=500
                )
                assert engine.price(contract, constant) == pytest.approx(
                    engine.price(contract, surface), rel=0, abs=1e-6
                )


def test_unsupported_engines_reject_business_time_before_shortcuts(subtests):
    for contract, names, _ in terminal_cases():
        for name in names:
            if name.startswith("FiniteDifference") and name not in HESTON_ENGINES:
                continue
            engine = getattr(pricing, name)()
            for volatility in [0.0, 0.3]:
                for at in [contract.effective_time, contract.expiry_time]:
                    with subtests.test(engine=name, volatility=volatility, at=at):
                        market = context(
                            bsm(
                                volatility=volatility,
                                variance_time_convention="business",
                                calendar=weekdays_calendar(),
                            ),
                            at,
                            105,
                        )
                        with pytest.raises(UnsupportedOperationError):
                            engine.price(contract, market)
                        with pytest.raises(UnsupportedOperationError):
                            engine.price_with_greeks(contract, market, "delta")


def test_closed_horizon_keeps_carry_discount_and_continuous_monitoring():
    start = datetime(2026, 11, 14)
    end = start + timedelta(days=1)
    market = context(
        bsm(variance_time_convention="business", calendar=weekdays_calendar()), start
    )
    expected = 100 * math.exp(-0.02 / 365) - 90 * math.exp(-0.07 / 365)
    assert fd().price(european(start, end, 90), market) == pytest.approx(
        expected, rel=0, abs=0.0001
    )
    barrier = ins.BarrierOption(
        option_type="call",
        strike=90,
        effective_time=start,
        expiry_time=end,
        barrier_type="up_and_out",
        barrier_level=100.01,
    )
    # Deterministic carry grows spot by about 0.014, crossing the barrier on Saturday.
    engine = pricing.FiniteDifferenceBarrierEngine(
        asset_step_count=800, time_step_count=500
    )
    assert engine.price(barrier, market) < 0.002
    scheduled = barrier.replace(observation_times=[start], calendar=all_days_calendar())
    assert engine.price(scheduled, market) == pytest.approx(expected, rel=0, abs=0.001)
    local_market = context(
        local(
            start,
            end,
            variance_time_convention="business",
            calendar=weekdays_calendar(),
        ),
        start,
    )
    assert engine.price(barrier, local_market) == 0
    assert fd().price(european(start, end, 90), local_market) == pytest.approx(
        expected, rel=0, abs=1e-10
    )


@pytest.mark.parametrize("kind", ["bsm", "local"])
def test_barriers_settle_exact_touches_before_variance_resumes(kind):
    start = datetime(2026, 11, 14)
    end = start + timedelta(days=3)
    clock = {"variance_time_convention": "business", "calendar": weekdays_calendar()}
    model = bsm(**clock) if kind == "bsm" else local(start, end, **clock)
    market = context(model, start)
    terms = {"effective_time": start, "expiry_time": end, "barrier_level": 100.01}
    barrier = ins.BarrierOption(
        option_type="call", strike=90, barrier_type="up_and_out", **terms
    )
    touch = ins.TouchOption(
        barrier_type="up_and_in",
        payoff_type="cash",
        payout=10,
        settlement_timing="at_hit",
        **terms,
    )
    hit_time = math.log1p(0.01 / 100) / (0.07 - 0.02)
    for option, engine, expected in (
        (barrier, pricing.FiniteDifferenceBarrierEngine(), 0.0),
        (
            touch,
            pricing.FiniteDifferenceBinaryBarrierEngine(),
            10 * math.exp(-0.07 * hit_time),
        ),
    ):
        saved = engine.solve(option, market, spot_range=(99, 101))
        assert engine.price(option, market) == pytest.approx(expected, abs=1e-12)
        assert saved.price(100) == pytest.approx(expected, abs=1e-12)


def test_missed_closed_date_fixing_rolls_pending_barrier_to_reopen():
    start = datetime(2026, 11, 14)
    sunday, monday, tuesday = [start + timedelta(days=n) for n in (1, 2, 3)]
    market = context(
        bsm(variance_time_convention="business", calendar=weekdays_calendar()), start
    )
    barrier = ins.BarrierOption(
        option_type="call",
        strike=90,
        effective_time=start,
        expiry_time=tuesday,
        barrier_type="up_and_out",
        barrier_level=100.02,
        calendar=all_days_calendar(),
        observation_times=[sunday, tuesday],
    )
    engine = pricing.FiniteDifferenceBarrierEngine(asset_upper_boundary=400)
    rolled = market.replace(
        valuation_time=monday, spot_price=100 * math.exp((0.07 - 0.02) * 2 / 365)
    )
    remaining = barrier.replace(observation_times=[tuesday])
    expected = math.exp(-0.07 * 2 / 365) * engine.price(remaining, rolled)
    saved = engine.solve(barrier, market, spot_range=(99, 101))
    assert engine.price(barrier, market) == pytest.approx(expected, abs=1e-12)
    assert saved.price(100) == pytest.approx(expected, abs=1e-12)
    assert saved.price(100.01) == 0.0


@pytest.mark.parametrize("lower", [0, 0.1])
def test_implied_volatility_rejects_closed_horizon(lower):
    start = datetime(2026, 11, 14, 12)
    end = start + timedelta(days=1)
    market = context(
        bsm(
            risk_free_rate=0,
            dividend_yield=0,
            variance_time_convention="business",
            calendar=weekdays_calendar(),
        ),
        start,
    )
    option = european(start, end, 90)
    engine = fd()
    with pytest.raises(UnsupportedOperationError):
        pricing.implied_volatility(
            engine,
            option,
            market,
            engine.price(option, market),
            lower_bound=lower,
            upper_bound=1,
        )


def test_closed_horizon_coupon_jump_has_no_spot_greeks():
    from test_phoenix import contract

    start = datetime(2026, 11, 14, 12)
    end = start + timedelta(days=1)
    note = contract(
        effective_time=start,
        expiry_time=end,
        calendar=all_days_calendar(),
        knock_out_observations=[ins.KnockOutObservation(end, 200)],
        coupon_observations=[ins.CouponObservation(end, 100)],
    )
    market = context(
        bsm(
            risk_free_rate=0,
            dividend_yield=0,
            variance_time_convention="business",
            calendar=weekdays_calendar(),
        ),
        start,
    )
    engine = pricing.FiniteDifferenceAutocallableEngine()
    solution = engine.solve(note, market, spot_range=(99, 101))
    controls = {"greeks": ("delta", "gamma"), "spot_shift": 0.125}
    for spot, status in [(100, "undefined"), (100.0625, "stencil_unavailable")]:
        for result in (
            engine.price_with_greeks(note, market.replace(spot_price=spot), **controls),
            solution.price_with_greeks(spot, **controls),
        ):
            assert result.status("delta") == status
            assert result.status("gamma") == status


@pytest.mark.parametrize("kind", ["bsm", "local"])
def test_closed_prefix_pays_coupon_and_knock_out_exactly(kind):
    from test_phoenix import contract

    start = datetime(2026, 11, 14)
    sunday = start + timedelta(days=1)
    end = start + timedelta(days=2, hours=12)
    note = contract(
        effective_time=start,
        expiry_time=end,
        calendar=all_days_calendar(),
        rebate_rate=0.02,
        knock_out_observations=[ins.KnockOutObservation(sunday, 110)],
        coupon_observations=[ins.CouponObservation(sunday, 100)],
    )
    plain = note.replace(coupon_per_period=0)
    inputs = {
        "risk_free_rate": 0.04,
        "dividend_yield": 0.04,
        "variance_time_convention": "business",
        "calendar": weekdays_calendar(),
    }
    market = context(
        bsm(**inputs) if kind == "bsm" else local(start, end, **inputs), start
    )
    engine = pricing.FiniteDifferenceAutocallableEngine()
    saved = engine.solve(note, market, spot_range=(99, 111))
    saved_plain = engine.solve(plain, market, spot_range=(99, 111))
    discount = math.exp(-0.04 / 365)
    for spot, expected in ((99, 0), (100, 0.01 * discount)):
        at_spot = market.replace(spot_price=spot)
        assert engine.price(note, at_spot) - engine.price(
            plain, at_spot
        ) == pytest.approx(expected, abs=1e-12)
        assert saved.price(spot) - saved_plain.price(spot) == pytest.approx(
            expected, abs=1e-12
        )
    assert engine.price(note, market.replace(spot_price=110)) == pytest.approx(
        1.03 * discount, abs=1e-12
    )
    assert saved.price(110) == pytest.approx(1.03 * discount, abs=1e-12)


def test_closed_weekend_needs_no_surface_coverage_at_expiry():
    start = datetime(2026, 11, 14)
    model = local(
        start,
        start + timedelta(hours=12),
        risk_free_rate=0,
        dividend_yield=0,
        variance_time_convention="business",
        calendar=weekdays_calendar(),
    )
    option = european(start, start + timedelta(days=1))
    market = context(model, start, 110)
    engine = fd()
    assert engine.price(option, market) == pytest.approx(10, abs=1e-12)
    solution = engine.solve(option, market, spot_range=(90, 120))
    assert solution.price_many([90, 110]) == pytest.approx((0, 10), abs=1e-12)


def test_closed_prefix_prices_fixed_cashflows_without_surface_at_reopen():
    from test_phoenix import contract

    start = datetime(2026, 11, 14)
    end = start + timedelta(days=2, hours=12)
    note = contract(
        effective_time=start,
        expiry_time=end,
        calendar=all_days_calendar(),
        lower_strike=100,
        coupon_per_period=0,
        knock_out_observations=[ins.KnockOutObservation(end, 110)],
        coupon_observations=[ins.CouponObservation(end, 100)],
    )
    model = local(
        start,
        start + timedelta(days=1),
        variance_time_convention="business",
        calendar=weekdays_calendar(),
    )
    market = context(model, start)
    engine = pricing.FiniteDifferenceAutocallableEngine()
    expected = math.exp(-0.07 * 2.5 / 365)
    assert engine.price(note, market) == pytest.approx(expected, abs=1e-12)
    assert engine.solve(note, market, spot_range=(90, 110)).price(100) == pytest.approx(
        expected, abs=1e-12
    )


def test_implied_volatility_rejects_fixed_cashflows_after_closed_prefix():
    from test_phoenix import contract

    start = datetime(2026, 11, 14)
    fixing = start + timedelta(days=1)
    end = start + timedelta(days=2, hours=12)
    note = contract(
        effective_time=start,
        expiry_time=end,
        calendar=all_days_calendar(),
        lower_strike=100,
        coupon_per_period=0,
        knock_out_observations=[ins.KnockOutObservation(fixing, 110)],
        coupon_observations=[ins.CouponObservation(fixing, 100)],
    )
    market = context(
        bsm(variance_time_convention="business", calendar=weekdays_calendar()), start
    )
    engine = pricing.FiniteDifferenceAutocallableEngine()
    observed = math.exp(-0.07 * 2.5 / 365)
    assert engine.price(note, market) == pytest.approx(observed, abs=1e-12)
    for lower in (0, 0.1):
        with pytest.raises(UnsupportedOperationError):
            pricing.implied_volatility(
                engine, note, market, observed, lower_bound=lower, upper_bound=1
            )


def test_closed_prefix_carries_knock_in_state_and_spot_into_the_grid():
    from test_phoenix import contract

    start = datetime(2026, 11, 14)
    opens = start + timedelta(days=2)
    end = opens + timedelta(hours=12)
    note = contract(
        effective_time=start,
        expiry_time=end,
        calendar=all_days_calendar(),
        knock_in_level=95,
        knock_in_observation=ins.KnockInObservation.every_trading_day(time(12)),
        knock_out_observations=[ins.KnockOutObservation(end, 110)],
        coupon_observations=[ins.CouponObservation(end, 90)],
    )
    model = bsm(
        risk_free_rate=0.04,
        dividend_yield=0,
        variance_time_convention="business",
        calendar=weekdays_calendar(),
    )
    market = context(model, start, 94)
    engine = pricing.FiniteDifferenceAutocallableEngine(asset_upper_boundary=400)
    saved = engine.solve(note, market, spot_range=(92, 102))
    for spot, state in [(94, "knocked_in"), (97, "no_event")]:
        factor = math.exp(0.04 * 2 / 365)
        opened = market.replace(valuation_time=opens, spot_price=spot * factor)
        expected = engine.price(note.replace(barrier_state=state), opened) / factor
        actual = engine.price(note, market.replace(spot_price=spot))
        assert actual == pytest.approx(expected, abs=1e-12)
        assert saved.price(spot) == pytest.approx(expected, abs=1e-12)


def test_saved_snowball_keeps_a_single_surviving_unknocked_spot():
    start = datetime(2026, 11, 14)
    fixing = start + timedelta(hours=12)
    end = start + timedelta(days=2, hours=12)
    knock_out = math.nextafter(100, math.inf)
    note = ins.SnowballOption(
        calendar=all_days_calendar(),
        effective_time=start,
        expiry_time=end,
        maturity_coupon_rate=0.08,
        initial_spot=100,
        knock_in_level=100,
        upper_strike=100,
        lower_strike=50,
        knock_in_observation=ins.KnockInObservation.every_trading_day(time(12)),
        autocall_observations=ins.autocallable_observations(
            [fixing], [knock_out], knock_out_coupon_rates=[0.05]
        ),
    )
    model = bsm(
        risk_free_rate=0,
        dividend_yield=0,
        variance_time_convention="business",
        calendar=weekdays_calendar(),
    )
    market = context(model, start)
    engine = pricing.FiniteDifferenceAutocallableEngine(asset_upper_boundary=400)
    saved = engine.solve(note, market, spot_range=(90, 110))
    spots = (math.nextafter(100, 0), 100, knock_out)
    expected = tuple(
        engine.price(note, market.replace(spot_price=spot)) for spot in spots
    )
    assert abs(expected[0] - expected[1]) > 1e-5
    assert saved.price_many(spots) == pytest.approx(expected, rel=0, abs=1e-12)


def test_weekend_events_keep_real_timestamp_order():
    start = datetime(2026, 11, 14)
    end = datetime(2026, 11, 16, 12)
    contract = ins.Accumulator(
        strike=100,
        knock_out_level=105,
        daily_quantity=1,
        acceleration_factor=1,
        effective_time=start,
        expiry_time=end,
        observation_clock_time=time(10),
        accumulated_quantity=0,
        knock_out_settled=False,
        knock_out_observation_times=[start + timedelta(hours=15)],
        calendar=all_days_calendar(),
    )
    market = context(
        bsm(
            risk_free_rate=0,
            dividend_yield=0,
            variance_time_convention="business",
            calendar=weekdays_calendar(),
        ),
        start,
        110,
    )
    engine = pricing.FiniteDifferenceAccumulatorEngine(
        asset_step_count=300, time_step_count=300
    )
    assert engine.price(contract, market) == pytest.approx(10, rel=0, abs=1e-6)
    simultaneous = contract.replace(
        knock_out_observation_times=[start + timedelta(hours=10)]
    )
    assert engine.price(simultaneous, market) == pytest.approx(0, rel=0, abs=1e-6)


@pytest.mark.parametrize("kind", ["bsm", "local"])
def test_snowball_coupon_keeps_natural_day_accrual_on_closed_dates(kind):
    start = datetime(2026, 11, 14)
    end = start + timedelta(days=1)
    note = ins.SnowballOption(
        effective_time=start,
        expiry_time=end,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=60,
        knock_in_observation=ins.KnockInObservation.at_expiry(),
        maturity_coupon_rate=0.06,
        autocall_observations=ins.autocallable_observations(
            [end], [120], knock_out_coupon_rates=[0.08]
        ),
        calendar=all_days_calendar(),
    )
    market = context(
        bsm(variance_time_convention="business", calendar=weekdays_calendar()), start
    )
    if kind == "local":
        market = context(
            local(
                start,
                end,
                variance_time_convention="business",
                calendar=weekdays_calendar(),
            ),
            start,
        )
    actual = pricing.FiniteDifferenceAutocallableEngine().price(note, market)
    assert actual == pytest.approx(
        (1 + 0.06 / 365) * math.exp(-0.07 / 365), rel=0, abs=1e-12
    )


@pytest.mark.parametrize("kind", ["bsm", "local"])
def test_closed_horizon_payoff_kinks_keep_undefined_spot_greeks(kind):
    start = datetime(2026, 11, 14)
    end = start + timedelta(days=1)
    inputs = {
        "risk_free_rate": 0,
        "dividend_yield": 0,
        "variance_time_convention": "business",
        "calendar": weekdays_calendar(),
    }
    model = bsm(**inputs) if kind == "bsm" else local(start, end, **inputs)
    result = fd().price_with_greeks(
        european(start, end), context(model, start), ["delta", "gamma"]
    )
    assert result.price == 0
    assert result.status("delta") == "undefined"
    assert result.status("gamma") == "undefined"


@pytest.mark.parametrize(
    ("start", "before", "after"),
    [
        (datetime(2026, 11, 14), datetime(2026, 11, 14), datetime(2026, 11, 14, 6)),
        (datetime(2026, 11, 16), datetime(2026, 11, 16), datetime(2026, 11, 16, 6)),
        (
            datetime(2026, 11, 13, 21),
            datetime(2026, 11, 13, 18),
            datetime(2026, 11, 14),
        ),
        (datetime(2026, 11, 16, 3), datetime(2026, 11, 16), datetime(2026, 11, 16, 6)),
    ],
    ids=["close_boundary", "open_boundary", "before_close", "after_open"],
)
def test_time_greeks_use_one_sided_natural_time_at_calendar_transition(
    start, before, after
):
    contract = european(datetime(2026, 11, 12), datetime(2026, 11, 20))
    market = context(
        bsm(variance_time_convention="business", calendar=weekdays_calendar()), start
    )
    engine = fd(asset_step_count=300, time_step_count=400)
    result = engine.price_with_greeks(
        contract, market, "theta", time_shift=timedelta(hours=6)
    )
    first = engine.price(contract, market.replace(valuation_time=before))
    last = engine.price(contract, market.replace(valuation_time=after))
    years = (after - before).total_seconds() / (365 * 86400)
    assert result.require("theta") == pytest.approx(
        (last - first) / years, rel=0, abs=1e-9
    )
