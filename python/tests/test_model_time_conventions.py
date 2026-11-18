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
        inspect.signature(factory).parameters["time_convention"].default
        == "actual_365_fixed"
    )
    assert base.time_convention == "actual_365_fixed"
    assert base.calendar is None
    fields = {"risk_free_rate": 0.07, "dividend_yield": 0.02}
    fields["volatility" if kind == "bsm" else "surface"] = (
        base.volatility if kind == "bsm" else base.surface
    )
    with pytest.raises(InvalidInputError):
        factory(**fields, time_convention="business")
    with pytest.raises(InvalidInputError):
        factory(**fields, calendar=weekdays_calendar())
    with pytest.raises(TypeError):
        factory(**fields, time_convention=None)
    calendar = weekdays_calendar()
    changed = base.replace(time_convention="business", calendar=calendar)
    assert changed.time_convention == "business"
    assert changed.calendar == calendar
    assert "calendar=" in repr(changed)
    assert base.calendar is None
    assert changed.replace(time_convention="actual_365_fixed", calendar=None) == base
    field = "volatility" if kind == "bsm" else "surface"
    assert getattr(changed, field) == getattr(base, field)
    for operation in [
        lambda: base.replace(time_convention="business"),
        lambda: base.replace(calendar=calendar),
        lambda: changed.replace(time_convention="actual_365_fixed"),
        lambda: changed.replace(calendar=None),
    ]:
        with pytest.raises(InvalidInputError):
            operation()
    for invalid in [True, 1, None]:
        with pytest.raises(TypeError):
            base.replace(time_convention=invalid)
        with pytest.raises(TypeError):
            base.replace(calendar=invalid if invalid is not None else object())
    with pytest.raises(ValueError) as error:
        base.replace(time_convention="unknown")
    assert type(error.value) is ValueError
    assert base == changed.replace(time_convention="actual_365_fixed", calendar=None)


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
    assert base.time_convention == "actual_365_fixed"
    assert base.calendar is None
    with pytest.raises(UnsupportedOperationError):
        HestonParameters(
            **fields, time_convention="business", calendar=weekdays_calendar()
        )
    with pytest.raises(UnsupportedOperationError):
        base.replace(time_convention="business", calendar=weekdays_calendar())
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
    model = bsm(time_convention="business", calendar=calendar)
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
        FRIDAY, MONDAY, time_convention="business", calendar=all_days_calendar()
    )
    natural = business.replace(time_convention="actual_365_fixed", calendar=None)
    engine = fd()
    assert engine.price(european(), context(business)) == pytest.approx(
        engine.price(european(), context(natural)), rel=0, abs=1e-10
    )
    assert business.surface.times == (FRIDAY, MONDAY)
    assert business.surface.volatility(FRIDAY + timedelta(days=1), 100) == 0.3


def test_changing_local_volatility_uses_natural_dates_and_business_variance():
    surface = LocalVolatilitySurface(
        times=[FRIDAY, MONDAY], spots=[50, 150], values=[[0.2, 0.2], [0.5, 0.5]]
    )
    model = local(
        FRIDAY,
        MONDAY,
        surface=surface,
        time_convention="business",
        calendar=weekdays_calendar(),
    )
    # Sigma is 0.2 + 0.1*d in natural days. Only [0, .5] and [2.5, 3] are open.
    integral = lambda d: 0.2**2 * d + 0.2 * 0.1 * d**2 + 0.1**2 * d**3 / 3
    variance = (integral(0.5) + integral(3) - integral(2.5)) / 252
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
    constant = context(bsm(time_convention="business", calendar=calendar), start)
    surface = context(
        local(start, end, time_convention="business", calendar=calendar), start
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
    constant = context(bsm(time_convention="business", calendar=calendar), start)
    surface = context(
        local(start, end, time_convention="business", calendar=calendar), start
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
                                time_convention="business",
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
        bsm(time_convention="business", calendar=weekdays_calendar()), start
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
        local(start, end, time_convention="business", calendar=weekdays_calendar()),
        start,
    )
    assert engine.price(barrier, local_market) == 0
    assert fd().price(european(start, end, 90), local_market) == pytest.approx(
        expected, rel=0, abs=1e-10
    )


@pytest.mark.parametrize("lower", [0, 0.1])
def test_implied_volatility_rejects_closed_horizon(lower):
    start = datetime(2026, 11, 14, 12)
    end = start + timedelta(days=1)
    market = context(
        bsm(
            risk_free_rate=0,
            dividend_yield=0,
            time_convention="business",
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
            time_convention="business",
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
        bsm(time_convention="business", calendar=weekdays_calendar()), start
    )
    if kind == "local":
        market = context(
            local(start, end, time_convention="business", calendar=weekdays_calendar()),
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
        "time_convention": "business",
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
        bsm(time_convention="business", calendar=weekdays_calendar()), start
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
