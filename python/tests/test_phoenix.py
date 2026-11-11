"""Phoenix contracts and quadrature through the installed Rust public API."""

import inspect
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from datetime import time as clock_time
from functools import partial

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    NonConvergenceError,
    RootNotBracketedError,
    StaleHistoryError,
    UnsupportedOperationError,
)
from kiyosi.instruments import (
    AutocallableObservation,
    PhoenixOption,
    autocallable_observations,
)
from kiyosi.market import (
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    RateTermStructure,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import (
    FiniteDifferenceAutocallableEngine,
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_decimal_equal, assert_rejection
from support.history import dated as dated_history
from support.history import history_fields
from support.terminal import assert_terminal_greeks
from test_snowball import market, structured

GREEKS = (
    "delta",
    "gamma",
    "speed",
    "vega",
    "vanna",
    "zomma",
    "rho",
    "theta",
    "charm",
    "color",
)

START, MIDDLE, END = datetime(2025, 1, 1), datetime(2025, 2, 3), datetime(2025, 4, 1)


def contract(**changes):
    fields = {
        "calendar": weekdays_calendar(),
        "coupon_per_period": 0.01,
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 60,
        "knock_in_observation_mode": "at_expiry",
        "effective_time": START,
        "expiry_time": END,
        "barrier_state": "no_event",
        "observations": autocallable_observations(
            [MIDDLE, END], [110, 110], coupon_barrier_levels=[90, 90]
        ),
    }
    fields.update(changes)
    return PhoenixOption(
        **history_fields(
            observation_clock_time=clock_time()
            if fields.get("knock_in_observation_mode", "every_trading_day")
            == "every_trading_day"
            else None,
            **fields,
        )
    )


def test_lockout_daily_layers_agree_across_engines_and_constant_local_volatility():
    end = datetime(2025, 1, 10)
    note = contract(
        expiry_time=end,
        knock_in_level=99,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        lockout_periods=2,
        observations=autocallable_observations(
            [datetime(2025, 1, 2), datetime(2025, 1, 6), end],
            [100] * 3,
            coupon_barrier_levels=[90] * 3,
        ),
    )
    context = market()
    expected = QuadratureAutocallableEngine().price(note, context)
    fd = FiniteDifferenceAutocallableEngine(asset_step_count=800, time_step_count=800)
    actual = fd.price(note, context)
    assert actual == pytest.approx(expected, rel=0, abs=2e-5)
    local = context.replace(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.04,
            dividend_yield=0.01,
            surface=LocalVolatilitySurface(
                times=[START, end], spots=[50, 150], values=[[0.3, 0.3], [0.3, 0.3]]
            ),
        )
    )
    assert fd.price(note, local) == pytest.approx(actual, rel=0, abs=1e-12)
    estimate = MonteCarloAutocallableEngine(
        path_count=32768, seed=42
    ).price_with_statistics(note, context)
    assert estimate.price == pytest.approx(
        expected, rel=0, abs=5 * estimate.standard_error
    )


def test_lockout_preserves_conditional_coupons_and_continuation_in_all_engines():
    note = contract(
        lower_strike=100,
        lockout_periods=2,
        observations=autocallable_observations(
            [START, MIDDLE, END], [100] * 3, coupon_barrier_levels=[90] * 3
        ),
    )
    expected = math.exp(-0.04 * (END - START).days / 365) + 0.01
    for at in (MIDDLE, END):
        t = (at - START).days / 365
        d2 = (math.log(100 / 90) + (0.03 - 0.3**2 / 2) * t) / (0.3 * math.sqrt(t))
        expected += 0.01 * math.exp(-0.04 * t) * math.erfc(-d2 / math.sqrt(2)) / 2
    context = market()
    for engine, tolerance in (
        (QuadratureAutocallableEngine(), 1e-9),
        (FiniteDifferenceAutocallableEngine(), 2e-5),
    ):
        assert engine.price(note, context) == pytest.approx(
            expected, rel=0, abs=tolerance
        )
    estimate = MonteCarloAutocallableEngine(
        path_count=32768, seed=42
    ).price_with_statistics(note, context)
    assert estimate.price == pytest.approx(
        expected, rel=0, abs=5 * estimate.standard_error + 1e-6
    )


def test_lockout_preserves_daily_knock_in_and_first_eligible_knock_out():
    note = contract(
        lower_strike=0,
        lockout_periods=1,
        knock_in_observation_mode="every_trading_day",
        observations=autocallable_observations(
            [START, END], [50, 200], coupon_barrier_levels=[0, 0]
        ),
    )
    horizon = (END - START).days / 365
    rate = math.log(90 / 70) / horizon
    context = market(70, START, volatility=0, rate=rate, dividend=0)
    for engine in (
        QuadratureAutocallableEngine(),
        FiniteDifferenceAutocallableEngine(),
        MonteCarloAutocallableEngine(),
    ):
        assert engine.price(note, context) == pytest.approx(
            0.01 + 0.91 * math.exp(-rate * horizon), rel=0, abs=1e-12
        )
        context_at_end = market(200, END)
        assert engine.price(
            dated_history(note, context_at_end), context_at_end
        ) == pytest.approx(1.01, rel=0, abs=1e-14)


def test_lockout_history_depends_only_on_eligible_knock_out_observations():
    first_eligible = datetime(2025, 3, 3)
    after_locked = MIDDLE + timedelta(days=1)
    after_eligible = first_eligible + timedelta(days=1)
    note = contract(
        lockout_periods=1,
        barrier_state=None,
        observations=autocallable_observations(
            [MIDDLE, first_eligible, END], [110] * 3, coupon_barrier_levels=[90] * 3
        ),
    )
    engine = QuadratureAutocallableEngine()
    assert math.isfinite(engine.price(note, market(100, first_eligible)))
    with pytest.raises(MissingHistoryError):
        engine.price(note, market(100, after_eligible))
    known = note.replace_history(barrier_state="no_event", history_time=after_locked)
    with pytest.raises(StaleHistoryError):
        known.replace(lockout_periods=0)
    assert known.lockout_periods == 1
    assert known.replace(lockout_periods=2).lockout_periods == 2
    with pytest.raises(InvalidInputError):
        note.replace_history(barrier_state="knocked_out", history_time=after_locked)
    possible = note.replace_history(
        barrier_state="knocked_out", history_time=after_eligible
    )
    assert engine.price(possible, market(100, after_eligible)) == 0
    daily = note.replace(
        knock_in_observation_mode="every_trading_day",
        observation_clock_time=clock_time(),
    )
    with pytest.raises(MissingHistoryError):
        engine.price(daily, market(100, START + timedelta(days=1)))


def test_lockout_counts_use_integer_protocol_and_core_validation(subtests):
    class Count:
        def __index__(self):
            return 1

    note = contract(lockout_periods=Count())
    assert note.lockout_periods == 1
    assert contract().lockout_periods == 0
    assert inspect.signature(PhoenixOption).parameters["lockout_periods"].default == 0
    assert note.replace(lockout_periods=Ellipsis) == note
    for value, error in (
        (True, TypeError),
        (False, TypeError),
        (1.0, TypeError),
        (0.5, TypeError),
        (None, TypeError),
        ("1", TypeError),
        (-1, OverflowError),
        (2**100, OverflowError),
        (2, InvalidInputError),
    ):
        for operation in (contract, note.replace):
            with (
                subtests.test(value=value, operation=operation),
                pytest.raises(error),
            ):
                operation(lockout_periods=value)
    context = market(110, MIDDLE)
    result = QuadratureAutocallableEngine().price_with_greeks(
        dated_history(note, context), context, "delta"
    )
    assert math.isfinite(result.require("delta"))
    assert note.lockout_periods == 1


@pytest.mark.parametrize("coupon", (-0.01, 0, 0.01))
@pytest.mark.parametrize("at", (START, MIDDLE, END))
def test_periodic_coupons_ignore_unequal_and_zero_length_periods_in_all_engines(
    subtests, coupon, at
):
    note = contract(
        coupon_per_period=coupon,
        observations=autocallable_observations(
            [START, MIDDLE, END], [110] * 3, coupon_barrier_levels=[90] * 3
        ),
    )
    context = market(110, at)
    for engine in (
        QuadratureAutocallableEngine(),
        FiniteDifferenceAutocallableEngine(),
        MonteCarloAutocallableEngine(),
    ):
        with subtests.test(coupon=coupon, at=at, engine=engine):
            assert engine.price(dated_history(note, context), context) == pytest.approx(
                1 + coupon, rel=0, abs=1e-14
            )


@pytest.mark.parametrize("coupon", (-0.01, 0, 0.01))
def test_coupon_properties_roundtrip_through_construction_and_replacement(coupon):
    note = contract(coupon_per_period=coupon)
    fields = {
        name: getattr(note, name)
        for name in inspect.signature(PhoenixOption).parameters
    }
    assert PhoenixOption(**history_fields(**fields)) == note
    assert note.replace(coupon_per_period=coupon) == note
    assert note.replace() == note
    assert note.replace(coupon_per_period=Ellipsis) == note
    with pytest.raises(TypeError):
        note.replace(coupon_per_period=None)
    assert PhoenixOption(**history_fields(**fields)) == note


@pytest.mark.parametrize("coupon", (0.365, -0.365))
def test_current_conditional_coupon_and_settlement_boundaries(coupon):
    engine = QuadratureAutocallableEngine()
    note = contract(coupon_per_period=coupon, principal_redemption_ratio=0.25)
    for spot, base, paid in (
        (60, -0.15, False),
        (70, -0.05, False),
        (80, 0.25, False),
        (90, 0.25, True),
        (110, 0.25, True),
    ):
        assert_decimal_equal(
            engine.price(dated_history(note, market(spot, END)), market(spot, END)),
            base + (coupon if paid else 0),
        )
    assert_decimal_equal(
        engine.price(dated_history(note, market(110, MIDDLE)), market(110, MIDDLE)),
        0.25 + coupon,
    )


def test_owned_defaults_replacement_and_strict_representations(subtests):
    note = contract(barrier_state=None)
    assert note.principal_redemption_ratio == 1
    assert note.barrier_state is None
    assert note.coupon_barrier_levels == ((90, 90))
    records = list(note.observations)
    records.clear()
    assert note.observation_times == ((MIDDLE, END))
    assert (
        note.replace_history(
            barrier_state="no_event",
            history_time=note.history_time or note.expiry_time,
        ).replace_history(barrier_state=None)
        == note
    )
    assert "PhoenixOption(coupon_per_period=" in repr(note)
    for fields in (
        {"coupon_per_period": True},
        {"coupon_per_period": "0.1"},
        {"principal_redemption_ratio": None},
        {"initial_spot": True},
        {"observations": [(END, 110)]},
        {"effective_time": datetime(2025, 1, 1).date()},
        {"knock_in_observation_mode": 1},
        {"barrier_state": True},
    ):
        with subtests.test(fields=fields), pytest.raises(TypeError):
            contract(**fields)
    for fields in (
        {"knock_in_observation_mode": "daily"},
        {"barrier_state": "unknown"},
    ):
        with pytest.raises(ValueError):
            contract(**fields)
    for replacement in (
        note.replace_history(),
        note.replace_history(
            barrier_state=Ellipsis,
        ),
    ):
        assert replacement == note
        assert replacement is not note
    for fields in ({"observations": []}, {"barrier_state": True}):
        with pytest.raises(TypeError):
            note.replace_history(**fields)
    with pytest.raises(AttributeError):
        note.coupon_per_period = 1
    engine = QuadratureAutocallableEngine()
    assert ((engine.asset_step_count, engine.integration_standard_deviations)) == (
        (512, 8)
    )
    with pytest.raises(TypeError):
        engine.price(dated_history(object(), market()), market())
    with pytest.raises(TypeError):
        implied_volatility(
            engine,
            dated_history(object(), market()),
            market(),
            1,
            lower_bound=0.0,
            upper_bound=4.0,
        )


def test_core_validation_order_errors_and_history():
    for fields, expected_error in (
        ({"coupon_per_period": math.nan}, InvalidInputError),
        ({"initial_spot": 0}, InvalidInputError),
        ({"lower_strike": 101}, InvalidInputError),
        ({"principal_redemption_ratio": -1}, InvalidInputError),
        ({"observations": []}, InvalidInputError),
        (
            {
                "observations": [
                    AutocallableObservation.phoenix(END, 110, coupon_barrier_level=0)
                ]
                * 2
            },
            InvalidInputError,
        ),
        ({"expiry_time": datetime(2024, 1, 1)}, InvalidInputError),
    ):
        assert_rejection(expected_error, contract, **fields)
    for value in (-1, math.nan, math.inf):
        assert_rejection(
            InvalidInputError,
            AutocallableObservation.phoenix,
            END,
            110,
            coupon_barrier_level=value,
        )
    assert_rejection(
        InvalidInputError,
        contract,
        observations=[AutocallableObservation.snowball(END, 110, coupon_rate=0.1)],
    )
    engine = QuadratureAutocallableEngine()
    unknown = contract(
        knock_in_observation_mode="every_trading_day", barrier_state=None
    )
    assert_rejection(
        MissingHistoryError,
        engine.price,
        dated_history(unknown, market(valuation=MIDDLE)),
        market(valuation=MIDDLE),
    )
    assert_rejection(
        InvalidInputError,
        contract,
        barrier_state="knocked_in",
    )
    assert_rejection(
        InvalidInputError,
        engine.price,
        dated_history(contract(barrier_state="knocked_out"), market()),
        market(),
    )
    with pytest.raises(InvalidInputError):
        contract(
            expiry_time=datetime(2025, 4, 5),
            observations=[
                AutocallableObservation.phoenix(
                    datetime(2025, 2, 1), 110, coupon_barrier_level=0
                )
            ],
        )
    with pytest.raises(InvalidInputError):
        contract(expiry_time=datetime(2025, 4, 5), barrier_state=None)
    for valuation in (datetime(2024, 12, 31), datetime(2025, 4, 1, 0, 0, 0, 1)):
        assert_rejection(
            InvalidInputError,
            engine.price,
            dated_history(contract(), market(valuation=valuation)),
            market(valuation=valuation),
        )
    curve = RateTermStructure(
        reference_time=START,
        times=[END, END + (END - START)],
        factors=[math.exp(-0.04 * 90 / 365), (math.exp(-0.04 * 90 / 365)) * 0.5],
    )
    assert_rejection(
        UnsupportedOperationError,
        engine.price_with_greeks,
        dated_history(contract(), market(rate=curve)),
        market(rate=curve),
        [],
    )
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        contract(),
        market(),
        "delta",
        spot_shift=0,
    )
    assert unknown.barrier_state is None


def test_independent_quantlib_terminal_portfolios(subtests):
    engine = QuadratureAutocallableEngine()
    for name, kind, inputs in structured.scenarios():
        if kind != "PhoenixOption":
            continue
        with subtests.test(name=name):
            start, end, valuation = (
                datetime.fromisoformat(inputs[k])
                for k in ("effective_time", "expiry_time", "valuation")
            )
            note = contract(
                effective_time=start,
                expiry_time=end,
                coupon_per_period=inputs["coupon_per_period"],
                principal_redemption_ratio=inputs["principal_redemption_ratio"],
                barrier_state=inputs["history"],
                knock_in_level=inputs["knock_in_level"],
                knock_in_observation_mode=inputs["knock_in_mode"],
                observations=autocallable_observations(
                    [end],
                    [110],
                    coupon_barrier_levels=[inputs["coupon_barrier_level"]],
                ),
            )
            context = market(
                inputs["spot"], valuation, inputs["volatility"], inputs["rate"]
            )
            assert engine.price(dated_history(note, context), context) == pytest.approx(
                structured.price(dated_history(kind, inputs), inputs),
                rel=0,
                abs=1e-10,
            )


@pytest.mark.parametrize("coupon", (0.08, -0.2, 0))
def test_signed_coupons_are_discounted_on_their_own_dates_and_past_coupons_excluded(
    coupon,
):
    engine = QuadratureAutocallableEngine()
    note = contract(
        coupon_per_period=coupon,
        observations=autocallable_observations(
            [MIDDLE, END], [200, 190], coupon_barrier_levels=[0] * 2
        ),
    )
    for valuation in (START, MIDDLE, datetime(2025, 2, 3, 0, 0, 0, 1)):
        v = (
            valuation
            if isinstance(valuation, datetime)
            else datetime.combine(valuation, datetime.min.time())
        )
        t = lambda d, v=v: (
            (datetime.combine(d, datetime.min.time()) - v).total_seconds()
            / (365 * 86400)
        )
        expected = math.exp(-0.04 * t(END))
        for current in (MIDDLE, END):
            if t(current) >= 0:
                expected += coupon * math.exp(-0.04 * t(current))
        assert engine.price(
            dated_history(note, market(valuation=valuation, volatility=1e-8)),
            market(valuation=valuation, volatility=1e-8),
        ) == pytest.approx(expected, rel=0, abs=1e-9)


def test_unconditional_coupon_tails_and_fixed_cashflow_identifiability():
    middle, end = datetime(2025, 7, 1), datetime(2026, 1, 1)
    note = contract(
        coupon_per_period=-0.5,
        lower_strike=100,
        principal_redemption_ratio=0.25,
        expiry_time=end,
        observations=autocallable_observations(
            [middle, end], [100, 100], coupon_barrier_levels=[0] * 2
        ),
    )
    time = 181 / 365
    survival = math.erfc((0.03 - 2) * math.sqrt(time) / (2 * math.sqrt(2))) / 2
    expected = (-0.5 + 0.25 * (1 - survival)) * math.exp(-0.04 * time) + (
        0.25 - 0.5
    ) * survival * math.exp(-0.04)
    for settings in (
        {},
        {"asset_step_count": 128, "integration_standard_deviations": 0.5},
    ):
        assert QuadratureAutocallableEngine(**settings).price(
            dated_history(note, market(volatility=2)), market(volatility=2)
        ) == pytest.approx(expected, rel=0, abs=1e-10)
    # Only a final unconditional coupon remains, so volatility cannot affect cashflows.
    fixed = contract(
        lower_strike=100,
        observations=autocallable_observations([END], [110], coupon_barrier_levels=[0]),
    )
    engine = QuadratureAutocallableEngine()
    quote = engine.price(dated_history(fixed, market()), market())
    assert_decimal_equal(quote, (1 + 0.01) * math.exp(-0.04 * 90 / 365))
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        dated_history(fixed, market()),
        market(),
        quote,
        lower_bound=0.0,
        upper_bound=4.0,
    )


def test_event_greeks_settled_history_and_solver_failures():
    note = contract()
    engine = QuadratureAutocallableEngine()
    result = engine.price_with_greeks(
        dated_history(note, market(90, MIDDLE)), market(90, MIDDLE), greeks="all"
    )
    assert result.delta is None
    assert result.gamma is None
    assert result.theta is None
    assert result.vega is not None
    assert result.rho is not None
    result = engine.price_with_greeks(
        dated_history(note, market(110, MIDDLE)), market(110, MIDDLE), greeks="all"
    )
    assert ((result.vega, result.rho)) == ((0, 0))
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        dated_history(note, market(110, MIDDLE)),
        market(110, MIDDLE),
        result.price,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    settled = note.replace_history(
        barrier_state="knocked_out",
        history_time=note.expiry_time,
    )
    assert (
        engine.price(
            dated_history(settled, market(valuation=END)), market(valuation=END)
        )
        == 0
    )
    result = engine.price_with_greeks(
        dated_history(note, market(90, END)), market(90, END), greeks="all"
    )
    assert_terminal_greeks(result, None)
    assert_rejection(
        UnsupportedOperationError,
        implied_volatility,
        engine,
        dated_history(note, market(90, END)),
        market(90, END),
        result.price,
        lower_bound=0.0,
        upper_bound=4.0,
    )
    ctx = market(103)
    quote = engine.price(dated_history(note, ctx), ctx)
    solved = implied_volatility(
        engine,
        dated_history(note, ctx),
        ctx,
        quote,
        lower_bound=0.2,
        upper_bound=0.4,
    )
    assert solved == pytest.approx(0.3, rel=0, abs=1e-6)
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        dated_history(note, ctx),
        ctx,
        10,
        lower_bound=0.2,
        upper_bound=0.4,
    )
    assert_rejection(
        NonConvergenceError,
        implied_volatility,
        engine,
        dated_history(note, ctx),
        ctx,
        quote,
        lower_bound=0.2,
        upper_bound=0.4,
        max_iterations=1,
        price_tolerance=1e-15,
        parameter_tolerance=1e-15,
    )
    assert ctx.model_parameters.volatility == 0.3
    assert note.barrier_state == "no_event"


def test_two_event_prices_match_independent_conditional_payoff_integral():
    calendar = custom_calendar(
        base=all_days_calendar(),
        closed_dates=[
            (START + timedelta(days=i)).date()
            for i in range(91)
            if START + timedelta(days=i) not in (START, MIDDLE, END)
        ],
    )
    first, remaining = 33 / 365, 57 / 365
    sigma, rate, dividend = 0.3, 0.04, 0.01
    drift = rate - dividend - sigma**2 / 2
    width = sigma * math.sqrt(first)
    normal = lambda z: math.erfc(-z / math.sqrt(2)) / 2
    z_for = lambda level: (math.log(level / 100) - drift * first) / width

    def terminal_expectation(spot, previously_in, coupon):
        threshold = 100 if previously_in else 95
        # Principal and capped loss are a piecewise affine terminal portfolio.
        pieces = [
            (0, 60, 0.6, 0),
            (60, threshold, 0, 0.01),
            (threshold, math.inf, 1, 0),
        ]
        deviation = sigma * math.sqrt(remaining)
        mean = math.log(spot) + drift * remaining
        value = coupon * normal((mean - math.log(90)) / deviation)
        for lower, upper, cash, asset in pieces:
            low = (math.log(lower) - mean) / deviation if lower else -math.inf
            high = (math.log(upper) - mean) / deviation
            value += cash * (normal(high) - normal(low))
            value += (
                asset
                * spot
                * math.exp((rate - dividend) * remaining)
                * (normal(high - deviation) - normal(low - deviation))
            )
        return value * math.exp(-rate * remaining)

    for mode in ("at_expiry", "every_trading_day"):
        for coupon in (0.08, -0.2):

            def reference(panels, mode=mode, coupon=coupon):
                value = (1 + coupon) * (1 - normal(z_for(110)))
                for left, right in zip(
                    [-12, z_for(90), z_for(95)],
                    [z_for(90), z_for(95), z_for(110)],
                    strict=True,
                ):
                    h = (right - left) / panels
                    points = (
                        [math.nextafter(left, right)]
                        + [left + h * i for i in range(1, panels)]
                        + [math.nextafter(right, left)]
                    )

                    def integrand(z, right=right):
                        spot = 100 * math.exp(drift * first + width * z)
                        payoff = terminal_expectation(
                            spot,
                            mode == "every_trading_day" and right <= z_for(95),
                            coupon,
                        )
                        payoff += coupon if right > z_for(90) else 0
                        return payoff * math.exp(-z * z / 2) / math.sqrt(2 * math.pi)

                    value += (
                        h
                        / 3
                        * math.fsum(
                            integrand(z)
                            * (1 if i in (0, panels) else 4 if i % 2 else 2)
                            for i, z in enumerate(points)
                        )
                    )
                return value * math.exp(-rate * first)

            coarse, fine = reference(1024), reference(2048)
            assert coarse == pytest.approx(fine, rel=0, abs=1e-9)
            note = contract(
                calendar=calendar,
                coupon_per_period=coupon,
                knock_in_level=95,
                knock_in_observation_mode=mode,
            )
            assert QuadratureAutocallableEngine().price(
                dated_history(note, market()), market()
            ) == pytest.approx(fine, rel=0, abs=1e-6)


def test_grid_domain_and_bump_convergence():
    values = []
    for mode in ("at_expiry", "every_trading_day"):
        note = contract(
            knock_in_level=95,
            knock_in_observation_mode=mode,
            observations=autocallable_observations(
                [MIDDLE, datetime(2025, 3, 3), END],
                [110, 108, 105],
                coupon_barrier_levels=[95] * 3,
            ),
        )
        prices = [
            QuadratureAutocallableEngine(
                asset_step_count=n, integration_standard_deviations=e
            ).price(dated_history(note, market()), market())
            for n, e in ((512, 8), (1024, 8), (1024, 10))
        ]
        assert prices[0] == pytest.approx(prices[1], rel=0, abs=1e-4)
        assert prices[1] == pytest.approx(prices[2], rel=0, abs=1e-4)
        values.append(prices[0])
    assert values[1] < values[0] - 0.001
    end = datetime(2026, 1, 1)
    note = contract(
        expiry_time=end,
        observations=autocallable_observations(
            [datetime(2025, 6, 2), end], [110, 105], coupon_barrier_levels=[95, 95]
        ),
    )
    context = market(101.5, datetime(2025, 1, 15), volatility=0.25)
    coarse = {"spot_shift": 0.5, "volatility_shift": 0.002, "rate_shift": 0.001}
    fine = {"spot_shift": 0.25, "volatility_shift": 0.001, "rate_shift": 0.0005}
    standard = numerical_greeks(
        QuadratureAutocallableEngine(),
        dated_history(note, context),
        context,
        greeks="all",
        **coarse,
    )
    engine = QuadratureAutocallableEngine(asset_step_count=1024)
    refined = numerical_greeks(
        engine, dated_history(note, context), context, greeks="all", **coarse
    )
    smaller = numerical_greeks(
        engine, dated_history(note, context), context, greeks="all", **fine
    )
    tolerances = {
        "delta": (3e-6, 0.005),
        "gamma": (3e-7, 0.01),
        "speed": (1e-7, 0.05),
        "vega": (3e-6, 0.005),
        "vanna": (1e-7, 0.05),
        "zomma": (1e-7, 0.05),
        "rho": (3e-6, 0.005),
        "theta": (1e-7, 0.02),
        "charm": (1e-7, 0.05),
        "color": (1e-7, 0.05),
    }
    assert standard.price == pytest.approx(refined.price, rel=0, abs=1e-4)
    for greek, (absolute, relative) in tolerances.items():
        target = getattr(refined, greek)
        assert target is not None
        for actual in (getattr(standard, greek), getattr(smaller, greek)):
            assert actual == pytest.approx(
                target, rel=0, abs=absolute + relative * abs(target)
            )


def test_all_greeks_and_volatility_recovery_against_digital_formula(subtests):
    # Equal terminal strikes remove KI dependence, leaving cash plus a digital.
    end = datetime(2026, 1, 1)
    note = contract(
        expiry_time=end,
        lower_strike=100,
        coupon_per_period=0.1,
        observations=autocallable_observations(
            [end], [100], coupon_barrier_levels=[100]
        ),
    )
    engine = QuadratureAutocallableEngine()
    spot, vol, rate = 103, 0.3, -0.04

    def value(s=spot, sigma=vol, rf=rate, t=1):
        d2 = (math.log(s / 100) + (rf - 0.01 - sigma**2 / 2) * t) / (
            sigma * math.sqrt(t)
        )
        return math.exp(-rf * t) * (1 + 0.1 * math.erfc(-d2 / math.sqrt(2)) / 2)

    h, v, r = 0.25, (vol + 0.002) - vol, (rate + 0.001) - rate

    def delta(sigma=vol, t=1):
        return (value(spot + h, sigma, t=t) - value(spot - h, sigma, t=t)) / (2 * h)

    def gamma(sigma=vol, t=1):
        return (
            value(spot + h, sigma, t=t)
            - 2 * value(sigma=sigma, t=t)
            + value(spot - h, sigma, t=t)
        ) / h**2

    dt = 1 / 365
    # At the effective date the shared contract uses a forward time stencil.
    expected = {
        "delta": delta(),
        "gamma": gamma(),
        "speed": (
            value(spot + 2 * h)
            - 2 * value(spot + h)
            + 2 * value(spot - h)
            - value(spot - 2 * h)
        )
        / (2 * h**3),
        "vega": (value(sigma=vol + v) - value(sigma=vol - v)) / (2 * v),
        "vanna": (delta(vol + v) - delta(vol - v)) / (2 * v),
        "zomma": (gamma(vol + v) - gamma(vol - v)) / (2 * v),
        "rho": (value(rf=rate + r) - value(rf=rate - r)) / (2 * r),
        "theta": (value(t=1 - dt) - value()) / dt,
        "charm": (delta(t=1 - dt) - delta()) / dt,
        "color": (gamma(t=1 - dt) - gamma()) / dt,
    }
    context = market(spot, volatility=vol, rate=rate)
    result = numerical_greeks(
        engine,
        dated_history(note, context),
        context,
        greeks="all",
        spot_shift=h,
        volatility_shift=0.002,
        rate_shift=0.001,
    )
    for greek, target in expected.items():
        with subtests.test(greek=greek):
            assert getattr(result, greek) == pytest.approx(
                target, rel=0, abs=1e-9 + 1e-5 * abs(target)
            )
    quote = value()
    solved = implied_volatility(
        engine,
        dated_history(note, context),
        context,
        quote,
        lower_bound=0.25,
        upper_bound=0.4,
        price_tolerance=1e-11,
    )
    assert solved == pytest.approx(vol, rel=0, abs=1e-7)
    assert_rejection(
        RootNotBracketedError,
        implied_volatility,
        engine,
        dated_history(note, context),
        context,
        10,
        lower_bound=0.1,
        upper_bound=0.5,
    )
    assert context.model_parameters.volatility == vol
    with ThreadPoolExecutor(max_workers=3) as pool:
        prices = list(
            pool.map(
                lambda s: engine.price(
                    dated_history(note, market(s, volatility=vol, rate=rate)),
                    market(s, volatility=vol, rate=rate),
                ),
                [90, 100, 110],
            )
        )
    for s, actual in zip([90, 100, 110], prices, strict=True):
        assert actual == pytest.approx(value(s=s), rel=0, abs=1e-12)


def test_coupon_above_knock_out_zero_coupon_and_expiry_without_observation():
    engine = QuadratureAutocallableEngine()
    note = contract(
        observations=autocallable_observations(
            [MIDDLE, END], [110, 110], coupon_barrier_levels=[120, 120]
        )
    )
    assert_decimal_equal(
        engine.price(dated_history(note, market(110, MIDDLE)), market(110, MIDDLE)), 1
    )
    assert_decimal_equal(
        engine.price(dated_history(note, market(120, MIDDLE)), market(120, MIDDLE)),
        1 + 0.01,
    )
    zero = contract(coupon_per_period=0)
    result = engine.price_with_greeks(
        dated_history(zero, market(90, MIDDLE)), market(90, MIDDLE), "delta"
    )
    assert result.delta is not None
    no_expiry_event = contract(
        observations=autocallable_observations(
            [MIDDLE], [110], coupon_barrier_levels=[90]
        )
    )
    assert (
        engine.price(dated_history(no_expiry_event, market(100, END)), market(100, END))
        == 1
    )
    # An effective-date observation pays the full periodic coupon.
    immediate = contract(
        observations=autocallable_observations(
            [START, END], [110, 110], coupon_barrier_levels=[90, 90]
        )
    )
    result = engine.price_with_greeks(
        dated_history(immediate, market(90)), market(90), "delta"
    )
    assert result.delta is None
    assert_decimal_equal(
        engine.price(dated_history(immediate, market(110)), market(110)), 1.01
    )


def test_dense_deterministic_ladder_and_short_terminal_transition():
    dates = [START + timedelta(days=i) for i in range(1, 33)]
    note = contract(
        calendar=all_days_calendar(),
        expiry_time=dates[-1],
        principal_redemption_ratio=0.5,
        observations=autocallable_observations(
            dates,
            [125 - 15 * i / 32 for i in range(1, 33)],
            coupon_barrier_levels=[90] * 32,
        ),
    )
    engine = QuadratureAutocallableEngine(asset_step_count=1024)
    for spot, expected in (
        (50, 0.1),
        (70, 0.2),
        (95, 0.82),
        (120, 0.61),
        (140, 0.51),
    ):
        assert engine.price(
            dated_history(
                note,
                market(
                    spot,
                    volatility=1e-8,
                    rate=0,
                    dividend=0,
                ),
            ),
            market(
                spot,
                volatility=1e-8,
                rate=0,
                dividend=0,
            ),
        ) == pytest.approx(expected, rel=0, abs=1e-8)
    valuation = datetime(2025, 4, 1) - timedelta(seconds=1)
    note = contract(
        lower_strike=100,
        observations=autocallable_observations(
            [END], [110], coupon_barrier_levels=[100]
        ),
    )
    for sigma in (0.01, 0.2):
        t = 1 / (365 * 86400)
        d2 = (0.03 - sigma * sigma / 2) * math.sqrt(t) / sigma
        expected = math.exp(-0.04 * t) * (1 + 0.01 * math.erfc(-d2 / math.sqrt(2)) / 2)
        assert engine.price(
            dated_history(note, market(100, valuation, volatility=sigma)),
            market(100, valuation, volatility=sigma),
        ) == pytest.approx(expected, rel=0, abs=1e-9)
