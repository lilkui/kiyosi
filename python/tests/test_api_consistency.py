"""Public API contracts shared by Rust and Python callers."""

import math
import pickle
from datetime import datetime, timedelta
from datetime import time as clock_time
from decimal import Decimal, FloatOperation, Inexact, Rounded, localcontext
from fractions import Fraction

import pytest
from kiyosi import InvalidInputError, UnsupportedOperationError, market, pricing
from kiyosi import instruments as ins
from kiyosi.instruments import KnockInObservation
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_decimal_equal


def test_greek_requests_report_disjoint_support_and_preserve_partial_results(subtests):
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    vanilla = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    bsm = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=105,
        valuation_time=start,
    )
    heston = bsm.replace(
        model_parameters=market.HestonParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            initial_variance=0.04,
            long_run_variance=0.04,
            mean_reversion_speed=1.5,
            vol_of_vol=0.3,
            correlation=-0.7,
        )
    )
    local = bsm.replace(
        model_parameters=market.LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            surface=market.LocalVolatilitySurface(
                times=[start, end], spots=[80, 120], values=[[0.2, 0.2], [0.2, 0.2]]
            ),
        )
    )
    fields = {
        "option_type": "call",
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
    }
    barrier = ins.BarrierOption(
        **fields,
        barrier_level=120,
        barrier_type="up_and_out",
        touch_state="untouched",
    )
    double = ins.DoubleBarrierOption(
        **fields,
        lower_barrier=80,
        upper_barrier=120,
        barrier_type="knock_out",
        touch_state="untouched",
    )
    sampling = {"path_count": 128, "seed": 42, "max_active_workers": 1}
    for engine, instrument, snapshot, partial in (
        (pricing.AnalyticHestonEuropeanEngine(), vanilla, heston, False),
        (
            pricing.FiniteDifferenceHestonVanillaEngine(
                asset_step_count=12, variance_step_count=8, time_step_count=8
            ),
            vanilla,
            heston,
            False,
        ),
        (
            pricing.FiniteDifferenceVanillaEngine(
                asset_step_count=40, time_step_count=40
            ),
            vanilla,
            local,
            True,
        ),
        (
            pricing.CoxRossRubinsteinDigitalEngine(),
            ins.CashOrNothingOption(**fields, payout=10),
            bsm,
            True,
        ),
        (pricing.CoxRossRubinsteinBarrierEngine(), barrier, bsm, True),
        *(
            (
                engine,
                option,
                bsm.replace(
                    model_parameters=market.BlackScholesMertonParameters(
                        risk_free_rate=0.05,
                        dividend_yield=0.02,
                        volatility=volatility,
                    )
                ),
                True,
            )
            for volatility in (0.0, 0.2)
            for engine, option in (
                (
                    pricing.MonteCarloDigitalEngine(**sampling),
                    ins.CashOrNothingOption(**fields, payout=10),
                ),
                (
                    pricing.MonteCarloAsianEngine(**sampling),
                    ins.GeometricAveragePriceOption(**fields, averaging_start_time=end),
                ),
                (
                    pricing.MonteCarloAsianEngine(**sampling),
                    ins.ArithmeticAveragePriceOption(
                        **fields, averaging_start_time=end
                    ),
                ),
            )
        ),
        (pricing.MonteCarloBarrierEngine(**sampling), barrier, bsm, True),
        (pricing.MonteCarloBarrierEngine(**sampling), double, bsm, True),
    ):
        for at in (start, end):
            with subtests.test(engine=engine, at=at):
                context = snapshot.replace(valuation_time=at)
                option = instrument
                result = engine.price_with_greeks(option, context, "vega")
                assert result.price == engine.price(option, context)
                assert result.vega is None
                assert result.status("vega") == "unsupported"
                assert result.status("delta") == "not_requested"
                result = engine.price_with_greeks(option, context, [])
                assert result.price == engine.price(option, context)
                result = engine.price_with_greeks(option, context, ["delta", "vega"])
                assert result.price == engine.price(option, context)
                assert result.status("delta") == (
                    "available" if partial else "unsupported"
                )
                assert result.status("vega") == "unsupported"


def test_autocallable_constructors_require_an_explicit_observation_clock(subtests):
    import inspect
    from datetime import time

    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    common = {
        "effective_time": start,
        "expiry_time": end,
        "knock_in_level": 80,
        "calendar": market.weekdays_calendar(),
    }
    snowball = ins.AutocallableObservation.snowball(end, 110, coupon_rate=0.1)
    phoenix = ins.AutocallableObservation.phoenix(end, 110, coupon_barrier_level=90)
    for cls, extra in (
        (
            ins.SnowballOption,
            {
                "maturity_coupon_rate": 0.1,
                "initial_spot": 100,
                "upper_strike": 100,
                "lower_strike": 0,
                "autocall_observations": [snowball],
            },
        ),
        (
            ins.TernarySnowballOption,
            {
                "maturity_coupon_rate": 0.1,
                "minimum_coupon_rate": 0,
                "autocall_observations": [snowball],
            },
        ),
        (
            ins.PhoenixOption,
            {
                "coupon_per_period": 0.1,
                "initial_spot": 100,
                "upper_strike": 100,
                "lower_strike": 0,
                "autocall_observations": [phoenix],
            },
        ),
    ):
        with subtests.test(contract=cls.__name__):
            assert (
                inspect.signature(cls).parameters["knock_in_observation"].default
                is inspect.Parameter.empty
            )
            with pytest.raises(TypeError):
                cls(**common, **extra)
            observation = KnockInObservation.every_trading_day(time(15))
            daily = cls(**common, **extra, knock_in_observation=observation)
            assert daily.knock_in_observation == observation
            expiry = daily.replace(knock_in_observation=KnockInObservation.at_expiry())
            assert expiry.knock_in_observation.observation_clock_time is None
            assert daily.knock_in_observation.observation_clock_time == time(15)
            with pytest.raises(TypeError):
                daily.replace(knock_in_observation=None)


def test_autocallable_constructors_require_an_explicit_calendar(subtests):
    import inspect

    from test_contract_replacements import contract_inputs

    for cls, fields in contract_inputs():
        if cls not in (
            ins.SnowballOption,
            ins.PhoenixOption,
            ins.BinarySnowballOption,
            ins.TernarySnowballOption,
        ):
            continue
        calendar = fields.pop("calendar")
        with subtests.test(contract=cls.__name__):
            assert (
                inspect.signature(cls).parameters["calendar"].default
                is inspect.Parameter.empty
            )
            with pytest.raises(TypeError):
                cls(**fields)
            for invalid in (None, Ellipsis):
                with pytest.raises(TypeError):
                    cls(**fields, calendar=invalid)
            note = cls(**fields, calendar=calendar)
            assert note.calendar == calendar
            assert "calendar=weekdays_calendar()" in repr(note)


def test_autocallable_calendars_control_knock_in_after_record_conversion():
    start, at, end = (
        datetime(2025, 1, 2, 15),
        datetime(2025, 1, 6, 15),
        datetime(2025, 1, 10, 15),
    )
    calendar = market.custom_calendar(
        base=market.weekdays_calendar(), closed_dates=[at.date()]
    )
    schedule = market.ObservationSchedule(
        times=[end], start_time=start, end_time=end, calendar=calendar
    )
    records = ins.autocallable_observations(
        autocall_times=schedule, knock_out_levels=[110], knock_out_coupon_rates=[0.1]
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0
        ),
        spot_price=79.999,
        valuation_time=at,
    )
    fields = {
        "maturity_coupon_rate": 0.1,
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 0,
        "effective_time": start,
        "expiry_time": end,
        "knock_in_observation": KnockInObservation.every_trading_day(clock_time(15)),
        "autocall_observations": records,
        "barrier_state": "no_event",
    }
    note = ins.SnowballOption(**fields, calendar=schedule.calendar)
    assert note.calendar == calendar
    assert at not in note.knock_in_observation_schedule
    engine = pricing.QuadratureAutocallableEngine()
    coupon = 0.1 * (end - start).days / 365
    assert engine.price(note, context) == pytest.approx(
        (1 + coupon) * math.exp(-0.05 * 4 / 365), rel=0, abs=1e-12
    )
    weekday_note = ins.SnowballOption(**fields, calendar=market.weekdays_calendar())
    assert at in weekday_note.knock_in_observation_schedule
    assert engine.price(weekday_note, context) == pytest.approx(
        79.999 * math.exp(-0.02 * 4 / 365) / 100, rel=0, abs=1e-12
    )
    preset = ins.standard_snowball(
        coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        knock_out_level=110,
        effective_time=start,
        expiry_time=end,
        knock_in_observation_clock_time=clock_time(15),
        autocall_times=schedule,
        barrier_state="no_event",
    )
    assert preset == note


def test_observation_coupon_choices_are_explicit_and_control_cashflows():
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    with pytest.raises(TypeError):
        ins.AutocallableObservation(end, 110)
    for choices in (
        {},
        {"knock_out_coupon_rates": [0], "coupon_barrier_levels": [0]},
    ):
        with pytest.raises(InvalidInputError):
            ins.autocallable_observations([end], [110], **choices)
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0,
            dividend_yield=0,
            volatility=0.2,
        ),
        spot_price=110,
        valuation_time=end,
    )
    engine = pricing.QuadratureAutocallableEngine()
    for coupon, expected in ((0, 1), (0.12, 1.12)):
        note = ins.BinarySnowballOption(
            calendar=market.weekdays_calendar(),
            maturity_coupon_rate=0.12,
            effective_time=start,
            expiry_time=end,
            autocall_observations=[
                ins.AutocallableObservation.snowball(end, 110, coupon_rate=coupon)
            ],
            barrier_state="no_event",
        )
        assert engine.price(note, context) == pytest.approx(expected, rel=0, abs=1e-12)
    for barrier, expected in ((0, 1.03), (90, 1)):
        note = ins.PhoenixOption(
            calendar=market.weekdays_calendar(),
            coupon_per_period=0.03,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=100,
            effective_time=start,
            expiry_time=end,
            knock_in_observation=KnockInObservation.at_expiry(),
            autocall_observations=[
                ins.AutocallableObservation.phoenix(
                    end, 200, coupon_barrier_level=barrier
                )
            ],
            barrier_state="no_event",
        )
        assert engine.price(note, context.replace(spot_price=50)) == pytest.approx(
            expected, rel=0, abs=1e-12
        )


@pytest.mark.parametrize("volatility", (0, 0.2))
@pytest.mark.parametrize("timing", ("at_hit", "at_expiry"))
def test_current_barrier_touches_retain_cashflow_parameter_greeks(volatility, timing):
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    option = ins.BarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_out",
        rebate=10,
        rebate_timing=timing,
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            volatility=volatility,
        ),
        spot_price=120,
        valuation_time=start,
    )
    result = pricing.AnalyticBarrierEngine().price_with_greeks(
        option,
        context,
        ["delta", "rho", "dividend_rho", "vega"],
    )
    expected = -10 * math.exp(-0.05) if timing == "at_expiry" else 0
    assert result.require("rho") == pytest.approx(expected, rel=0, abs=2e-8)
    assert result.require("dividend_rho") == 0
    assert result.status("delta") == "undefined"
    if volatility:
        assert result.require("vega") == 0
    else:
        assert result.status("vega") == "undefined"


@pytest.mark.parametrize("kind", (ins.CashOrNothingOption, ins.AssetOrNothingOption))
@pytest.mark.parametrize("direction", ("call", "put"))
def test_european_digitals_allow_pre_effective_pricing_greeks_and_solves(
    subtests, kind, direction
):
    start = datetime(2025, 1, 1)
    effective, expiry = start + timedelta(days=30), start + timedelta(days=365)
    mc = pricing.MonteCarloDigitalEngine(path_count=128, seed=42, max_active_workers=1)
    fd = pricing.FiniteDifferenceDigitalEngine(
        asset_step_count=200, time_step_count=100
    )
    engines = [
        pricing.AnalyticDigitalEngine(),
        pricing.CoxRossRubinsteinDigitalEngine(),
        fd,
        pricing.QuadratureDigitalEngine(),
        mc,
    ]
    future = kind(
        option_type=direction,
        strike=100,
        effective_time=effective,
        expiry_time=expiry,
        **({"payout": 10} if kind is ins.CashOrNothingOption else {}),
    )
    existing = future.replace(effective_time=start)
    for volatility in (0, 0.2):
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=volatility
        )
        for at in (start, effective - timedelta(microseconds=1), effective):
            context = market.PricingContext(
                model_parameters=model, spot_price=105, valuation_time=at
            )
            for engine in engines:
                with subtests.test(
                    kind=kind.__name__,
                    direction=direction,
                    volatility=volatility,
                    at=at,
                    engine=type(engine).__name__,
                ):
                    price = engine.price(future, context)
                    assert price == engine.price(existing, context)
                    native = engine.price_with_greeks(future, context, "delta")
                    assert native == engine.price_with_greeks(
                        existing,
                        context,
                        "delta",
                    )
                    assert math.isfinite(native.require("delta"))
                    numerical = numerical_greeks(
                        engine,
                        future,
                        context,
                        "theta",
                    )
                    assert math.isfinite(numerical.require("theta"))
                    assert numerical == numerical_greeks(
                        engine,
                        existing,
                        context,
                        "theta",
                    )
                    if volatility:
                        assert pricing.implied_volatility(
                            engine,
                            future,
                            context,
                            price,
                            lower_bound=0.2,
                            upper_bound=0.3,
                        ) == pytest.approx(0.2, rel=0, abs=1e-12)
            assert mc.price_with_statistics(
                future, context
            ) == mc.price_with_statistics(existing, context)
        short = market.RateTermStructure.bounded_flat(
            0.05, reference_time=start, max_time=expiry - timedelta(days=1)
        )
        uncovered = context.replace(
            model_parameters=model.replace(risk_free_rate=short),
            valuation_time=start,
        )
        expired = context.replace(valuation_time=expiry + timedelta(microseconds=1))
        for engine in engines:
            for invalid in (uncovered, expired):
                with pytest.raises(InvalidInputError):
                    engine.price(future, invalid)
    surface = market.LocalVolatilitySurface(
        times=[start, expiry],
        spots=[50, 150],
        values=[[0.2, 0.2], [0.2, 0.2]],
    )
    local = context.replace(
        model_parameters=market.LocalVolatilityParameters(
            risk_free_rate=0.05, dividend_yield=0.02, surface=surface
        ),
        valuation_time=start,
    )
    assert fd.price(future, local) == fd.price(existing, local)
    assert future.effective_time == effective


def test_european_pre_effective_pricing_preserves_contract_and_model_coverage(subtests):
    valuation = datetime(2025, 1, 1)
    effective, expiry = (
        valuation + timedelta(days=30),
        valuation + timedelta(days=365),
    )
    future = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=effective, expiry_time=expiry
    )
    existing = future.replace(effective_time=valuation)
    engines = [
        pricing.AnalyticEuropeanEngine(),
        pricing.CoxRossRubinsteinVanillaEngine(),
        pricing.FiniteDifferenceVanillaEngine(),
        pricing.QuadratureVanillaEngine(),
        pricing.MonteCarloVanillaEngine(path_count=128, seed=42, max_active_workers=1),
    ]
    for volatility in (0, 0.2):
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=volatility
        )
        context = market.PricingContext(
            model_parameters=model, spot_price=100, valuation_time=valuation
        )
        for engine in engines:
            with subtests.test(engine=type(engine).__name__, volatility=volatility):
                assert engine.price(future, context) == engine.price(existing, context)
                assert math.isfinite(
                    engine.price_with_greeks(future, context, "delta").require("delta")
                )
        if volatility:
            engine = engines[0]
            assert pricing.implied_volatility(
                engine,
                future,
                context,
                engine.price(future, context),
                lower_bound=0.0,
                upper_bound=4.0,
            ) == pytest.approx(volatility, rel=0, abs=1e-7)
    heston = market.HestonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        initial_variance=0.04,
        long_run_variance=0.04,
        mean_reversion_speed=1.5,
        vol_of_vol=0,
        correlation=-0.7,
    )
    context = context.replace(model_parameters=heston)
    for engine in (
        pricing.AnalyticHestonEuropeanEngine(),
        pricing.FiniteDifferenceHestonVanillaEngine(
            asset_step_count=40, variance_step_count=10, time_step_count=20
        ),
    ):
        assert engine.price(future, context) == engine.price(existing, context)
    surface = market.LocalVolatilitySurface(
        times=[valuation, expiry], spots=[50, 150], values=[[0.2, 0.2], [0.2, 0.2]]
    )
    local = market.LocalVolatilityParameters(
        risk_free_rate=0.05, dividend_yield=0.02, surface=surface
    )
    context = context.replace(model_parameters=local)
    assert engines[2].price(future, context) == engines[2].price(existing, context)
    bounded = market.RateTermStructure.bounded_flat(
        0.05, reference_time=effective, max_time=expiry
    )
    with pytest.raises(InvalidInputError):
        engines[0].price(
            future,
            context.replace(model_parameters=model.replace(risk_free_rate=bounded)),
        )
    assert future.effective_time == effective


@pytest.mark.parametrize("invalid", (math.nan, math.inf, -math.inf))
def test_snowball_coupon_diagnostics_identify_the_supplied_field(subtests, invalid):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    common = {
        "calendar": market.weekdays_calendar(),
        "effective_time": start,
        "expiry_time": end,
        "autocall_observations": ins.autocallable_observations(
            [end], [110], knock_out_coupon_rates=[0]
        ),
    }
    for factory, fields, field in (
        (
            ins.BinarySnowballOption,
            {"maturity_coupon_rate": invalid},
            "maturity_coupon_rate",
        ),
        (
            ins.TernarySnowballOption,
            {
                "maturity_coupon_rate": invalid,
                "minimum_coupon_rate": 0,
                "knock_in_level": 80,
                "knock_in_observation": KnockInObservation.every_trading_day(
                    clock_time()
                ),
            },
            "maturity_coupon_rate",
        ),
        (
            ins.TernarySnowballOption,
            {
                "maturity_coupon_rate": 0.1,
                "minimum_coupon_rate": invalid,
                "knock_in_level": 80,
                "knock_in_observation": KnockInObservation.every_trading_day(
                    clock_time()
                ),
            },
            "minimum_coupon_rate",
        ),
    ):
        with subtests.test(contract=factory.__name__, field=field):
            with pytest.raises(InvalidInputError) as error:
                factory(**common, **fields)
            assert field in str(error.value)


def test_structured_engines_require_curve_coverage_only_for_remaining_cashflows(
    subtests,
):
    start, valuation, end = (
        datetime(2025, 1, 1),
        datetime(2025, 1, 3),
        datetime(2026, 1, 1),
    )
    curve = market.RateTermStructure(
        reference_time=start, times=[datetime(2025, 2, 1)], factors=[1]
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=curve, dividend_yield=curve, volatility=0.2
        ),
        spot_price=125,
        valuation_time=valuation,
    )
    accumulator = ins.Accumulator(
        knock_out_settled=False,
        observation_clock_time=clock_time(),
        strike=100,
        knock_out_level=120,
        daily_quantity=1,
        acceleration_factor=2,
        accumulated_quantity=3,
        effective_time=start,
        expiry_time=end,
    )
    note = ins.BinarySnowballOption(
        calendar=market.weekdays_calendar(),
        maturity_coupon_rate=0.1,
        effective_time=start,
        expiry_time=end,
        barrier_state="no_event",
        autocall_observations=ins.autocallable_observations(
            [datetime(2025, 1, 2), valuation, end],
            [120, 120, 120],
            knock_out_coupon_rates=[0] * 3,
        ),
    )
    for kind, contract, settled, active, current_value in (
        (
            "Accumulator",
            accumulator,
            accumulator.replace(
                knock_out_settled=True,
            ),
            accumulator.replace(
                knock_out_level=150,
                accumulated_quantity=accumulator.accumulated_quantity,
                knock_out_settled=accumulator.knock_out_settled,
            ),
            75,
        ),
        (
            "Autocallable",
            note,
            note.replace(
                barrier_state="knocked_out",
            ),
            note.replace(
                barrier_state=note.barrier_state,
                autocall_observations=ins.autocallable_observations(
                    [datetime(2025, 1, 2), valuation, end],
                    [150, 150, 150],
                    knock_out_coupon_rates=[0] * 3,
                ),
            ),
            1,
        ),
    ):
        for family in ("FiniteDifference", "Quadrature", "MonteCarlo"):
            engine = getattr(pricing, family + kind + "Engine")()
            with subtests.test(kind=kind, engine=family):
                assert engine.price(settled, context) == 0
                assert engine.price(contract, context) == current_value
                assert engine.price_with_greeks(settled, context, "delta").delta == 0
                with pytest.raises(InvalidInputError):
                    engine.price(active, context)
                if family == "MonteCarlo":
                    estimate = engine.price_with_statistics(settled, context)
                    assert estimate.price == 0
                    assert estimate.standard_error == 0


def test_accumulator_settlement_requires_a_strictly_prior_knock_out_observation(
    subtests,
):
    start, first_explicit, end = (datetime(2025, 1, day) for day in (6, 8, 10))
    model = market.BlackScholesMertonParameters(
        risk_free_rate=0, dividend_yield=0, volatility=0.2
    )
    for times, first in (
        (None, start),
        ([first_explicit], first_explicit),
        ([], None),
    ):
        original = ins.Accumulator(
            observation_clock_time=clock_time(),
            strike=100,
            knock_out_level=120,
            daily_quantity=1,
            acceleration_factor=2,
            effective_time=start,
            expiry_time=end,
            knock_out_observation_times=times,
        )
        for valuation in (
            start,
            first_explicit,
            first_explicit + timedelta(microseconds=1),
            end,
        ):
            settled = original.replace(knock_out_settled=True)
            context = market.PricingContext(
                model_parameters=model, spot_price=100, valuation_time=valuation
            )
            if first is None or first >= valuation:
                with pytest.raises(InvalidInputError):
                    pricing.QuadratureAccumulatorEngine().price(settled, context)
                continue
            for family in ("FiniteDifference", "Quadrature", "MonteCarlo"):
                engine = getattr(pricing, family + "AccumulatorEngine")()
                with subtests.test(schedule=times, valuation=valuation, engine=family):
                    assert engine.price(settled, context) == 0
                    assert (
                        engine.price_with_greeks(settled, context, "delta").delta == 0
                    )
                    if family == "MonteCarlo":
                        assert engine.price_with_statistics(settled, context).price == 0
        assert not (original.knock_out_settled)
        assert original.accumulated_quantity is None
        assert original.knock_out_observation_times == (
            None if times is None else tuple(times)
        )


def test_numeric_rejections_identify_the_failing_field(subtests):
    defaults = {"risk_free_rate": 0.02, "dividend_yield": 0, "volatility": 0.2}
    for field, invalid in (
        ("risk_free_rate", math.nan),
        ("dividend_yield", math.inf),
        ("volatility", -0.2),
    ):
        with (
            subtests.test(field=field),
            pytest.raises(InvalidInputError) as error,
        ):
            market.BlackScholesMertonParameters(**(defaults | {field: invalid}))
        assert field in str(error.value)
    model = market.BlackScholesMertonParameters(**defaults)
    with pytest.raises(InvalidInputError) as error:
        model.replace(volatility=-0.1)
    assert "volatility" in str(error.value)
    assert model.volatility == 0.2


def test_contractual_calendars_are_validated_before_pricing(subtests):
    start, closed, end = (datetime(2025, 1, day) for day in (3, 4, 6))
    life = {"effective_time": start, "expiry_time": end}
    monitoring = {"observation_times": [closed]}
    single = {"barrier_level": 120, "barrier_type": "up_and_out"}
    vanilla = {"option_type": "call", "strike": 100}
    factories = [
        (ins.BarrierOption, dict(life, **vanilla, **single, **monitoring)),
        (
            ins.DoubleBarrierOption,
            dict(
                life,
                **vanilla,
                lower_barrier=80,
                upper_barrier=120,
                barrier_type="knock_out",
                **monitoring,
            ),
        ),
        (
            ins.BinaryBarrierOption,
            dict(
                life, **vanilla, **single, payout=10, **monitoring, payoff_type="cash"
            ),
        ),
        (
            ins.BinaryBarrierOption,
            dict(life, **vanilla, **single, **monitoring, payoff_type="asset"),
        ),
        (
            ins.TouchOption,
            dict(life, **single, payout=10, **monitoring, payoff_type="cash"),
        ),
        (ins.TouchOption, dict(life, **single, **monitoring, payoff_type="asset")),
        (
            ins.DoubleTouchOption,
            dict(
                life,
                lower_barrier=80,
                upper_barrier=120,
                payout=10,
                **monitoring,
                barrier_type="knock_in",
            ),
        ),
        (
            ins.Accumulator,
            dict(
                life,
                strike=90,
                knock_out_level=110,
                daily_quantity=1,
                acceleration_factor=2,
                knock_out_observation_times=[closed],
                observation_clock_time=clock_time(),
            ),
        ),
        (
            ins.BinarySnowballOption,
            dict(
                life,
                maturity_coupon_rate=0.1,
                autocall_observations=[
                    ins.AutocallableObservation.snowball(closed, 110, coupon_rate=0)
                ],
            ),
        ),
        (
            ins.standard_snowball,
            dict(
                life,
                initial_spot=100,
                knock_in_level=80,
                knock_out_level=110,
                coupon_rate=0.1,
                autocall_times=[closed],
                knock_in_observation_clock_time=clock_time(),
            ),
        ),
        (
            ins.RangeAccrualOption,
            {
                "effective_time": start,
                "expiry_time": closed,
                "notional": 100,
                "annual_coupon_rate": 0.1,
                "lower_bound": 80,
                "upper_bound": 120,
                "observation_clock_time": clock_time(),
            },
        ),
    ]
    for factory, fields in factories:
        with subtests.test(contract=factory.__name__):
            with pytest.raises(InvalidInputError):
                factory(**fields, calendar=market.weekdays_calendar())
            option = factory(**fields, calendar=market.all_days_calendar())
            with pytest.raises(InvalidInputError):
                option.replace(calendar=market.weekdays_calendar())
            assert option.calendar == market.all_days_calendar()
            assert option.replace() == option
            assert pickle.loads(pickle.dumps(option)) == option


@pytest.mark.parametrize("direction", ("call", "put"))
@pytest.mark.parametrize("upper", (121, 400, 10000))
def test_bounded_barrier_grids_keep_resolution_when_unused_upper_bound_changes(
    direction, upper
):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.02, dividend_yield=0, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    terms = {
        "option_type": direction,
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
    }
    single = ins.BarrierOption(**terms, barrier_level=120, barrier_type="up_and_out")
    double = ins.DoubleBarrierOption(
        **terms, lower_barrier=80, upper_barrier=120, barrier_type="knock_out"
    )
    expected = pricing.AnalyticBarrierEngine().price(single, context)
    previous = None
    settings = {
        "asset_step_count": 200,
        "time_step_count": 200,
        "asset_upper_boundary": upper,
    }
    actual = (
        pricing.FiniteDifferenceBarrierEngine(**settings).price(single, context),
        pricing.FiniteDifferenceDoubleBarrierEngine(**settings).price(double, context),
    )
    assert actual[0] == pytest.approx(expected, rel=0, abs=0.003)
    if previous is not None:
        assert actual == previous
    previous = actual


@pytest.mark.parametrize(
    "factory",
    (
        pricing.AnalyticBarrierEngine,
        pricing.CoxRossRubinsteinBarrierEngine,
        pricing.QuadratureBarrierEngine,
        pricing.FiniteDifferenceBarrierEngine,
        pricing.MonteCarloBarrierEngine,
    ),
)
def test_barrier_engines_require_curve_coverage_only_for_remaining_cashflows(factory):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    curve = market.RateTermStructure(
        reference_time=start, times=[datetime(2025, 2, 1)], factors=[1]
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=curve, dividend_yield=curve, volatility=0.2
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 2),
    )
    settled = ins.BarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_out",
        touch_state="touched",
        rebate_timing="at_hit",
    )
    engine = factory()
    assert engine.price(settled, context) == 0
    assert engine.price_with_greeks(settled, context, "delta").delta == 0
    for changes in (
        {"touch_state": "untouched"},
        {"rebate": 10, "rebate_timing": "at_expiry"},
        {"barrier_type": "up_and_in", "rebate_timing": "at_expiry"},
        {"touch_state": None},
    ):
        with pytest.raises(InvalidInputError):
            engine.price(settled.replace(**changes), context)


def test_structured_contract_calendars_control_observations_and_survive_reconstruction():
    start, end = datetime(2025, 1, 3), datetime(2025, 1, 6)
    model = market.BlackScholesMertonParameters(
        risk_free_rate=0, dividend_yield=0, volatility=1e-12
    )
    context = market.PricingContext(
        model_parameters=model, spot_price=100, valuation_time=start
    )
    accrual = ins.RangeAccrualOption(
        observation_clock_time=clock_time(),
        notional=1000,
        annual_coupon_rate=0.12,
        lower_bound=90,
        upper_bound=110,
        effective_time=start,
        expiry_time=end,
        realized_hit_count=0,
    )
    expiry = context.replace(valuation_time=end)
    engine = pricing.AnalyticRangeAccrualEngine()
    price = engine.price(accrual, expiry)
    assert price == pytest.approx(360 / 365, rel=0, abs=1e-12)
    daily = accrual.replace(
        calendar=market.all_days_calendar(),
        realized_hit_count=accrual.realized_hit_count,
    )
    assert engine.price(daily, expiry) == pytest.approx(price / 3, rel=0, abs=1e-12)
    accumulator = ins.Accumulator(
        observation_clock_time=clock_time(),
        strike=90,
        knock_out_level=200,
        daily_quantity=1,
        acceleration_factor=1,
        effective_time=start,
        expiry_time=end,
    )
    note = ins.standard_snowball(
        knock_in_observation_clock_time=clock_time(),
        coupon_rate=0.1,
        initial_spot=100,
        knock_in_level=80,
        knock_out_level=110,
        autocall_times=[end],
        effective_time=start,
        expiry_time=end,
        calendar=market.all_days_calendar(),
    )
    for value in (daily, accumulator, note):
        assert value.replace().calendar == value.calendar
        assert pickle.loads(pickle.dumps(value)).calendar == value.calendar
    assert (
        note.with_coupon(0.2, quote_convention="shift_maturity_coupon").calendar
        == note.calendar
    )


def test_constant_node_curves_match_scalars_and_keep_coverage(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    curve = market.RateTermStructure(
        reference_time=start, times=[end, end + timedelta(days=365)], factors=[1, 1]
    )
    assert curve.exact_flat_rate == 0
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    nodes = context.replace(
        model_parameters=context.model_parameters.replace(
            risk_free_rate=curve, dividend_yield=curve
        )
    )
    short = context.replace(
        model_parameters=context.model_parameters.replace(
            risk_free_rate=market.RateTermStructure(
                reference_time=start,
                times=[start + timedelta(days=180)],
                factors=[1],
            )
        )
    )
    for engine in (
        pricing.AnalyticEuropeanEngine(),
        pricing.CoxRossRubinsteinVanillaEngine(),
        pricing.QuadratureVanillaEngine(),
        pricing.FiniteDifferenceVanillaEngine(),
        pricing.MonteCarloVanillaEngine(path_count=512, seed=42, max_active_workers=1),
    ):
        with subtests.test(engine=type(engine).__name__):
            assert engine.price(option, context) == pytest.approx(
                engine.price(option, nodes), rel=0, abs=1e-9
            )
            with pytest.raises(InvalidInputError):
                engine.price(option, short)


def test_heston_greeks_check_model_before_request_controls(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    for engine in (
        pricing.AnalyticHestonEuropeanEngine(),
        pricing.FiniteDifferenceHestonVanillaEngine(),
    ):
        for request in ([], "all"):
            with (
                subtests.test(engine=type(engine).__name__, request=request),
                pytest.raises(UnsupportedOperationError),
            ):
                numerical_greeks(
                    engine,
                    option,
                    context,
                    request,
                    time_shift=timedelta(days=0),
                )


def test_built_in_pricing_requires_valuation_within_inclusive_contract_life(subtests):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 2)
    life = {"effective_time": start, "expiry_time": end}
    cases = [
        (
            pricing.AnalyticEuropeanEngine(),
            ins.EuropeanOption(option_type="call", strike=100, **life),
        )
    ]
    cases.extend(
        (
            pricing.AnalyticRangeAccrualEngine(),
            ins.RangeAccrualOption(
                observation_clock_time=clock_time(),
                notional=100,
                annual_coupon_rate=coupon,
                lower_bound=90,
                upper_bound=110,
                **life,
            ),
        )
        for coupon in (0, 0.1)
    )
    for engine, option in cases:
        for at, valid in (
            (
                start - timedelta(microseconds=1),
                isinstance(option, ins.EuropeanOption),
            ),
            (start, True),
            (end, True),
            (end + timedelta(microseconds=1), False),
        ):
            context = market.PricingContext(
                model_parameters=market.BlackScholesMertonParameters(
                    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
                ),
                spot_price=100,
                valuation_time=at,
            )
            with subtests.test(instrument=type(option).__name__, at=at):
                if valid:
                    assert engine.price(option, context) >= 0
                    assert engine.price_with_greeks(option, context, "delta").price >= 0
                else:
                    with pytest.raises(InvalidInputError):
                        engine.price(option, context)
                    with pytest.raises(InvalidInputError):
                        engine.price_with_greeks(option, context, "delta")


def test_observation_construction_validates_numeric_terms_before_note_creation(
    subtests,
):
    time = datetime(2026, 1, 1)
    for level in (0, -1, float("nan"), float("inf")):
        with subtests.test(level=level):
            with pytest.raises(InvalidInputError):
                ins.AutocallableObservation.snowball(time, level, coupon_rate=0)
            with pytest.raises(InvalidInputError):
                ins.autocallable_observations(
                    [time], [level], knock_out_coupon_rates=[0]
                )
    for field, keyword, invalids in (
        (
            "coupon_rate",
            "knock_out_coupon_rates",
            (float("nan"), float("inf"), -float("inf")),
        ),
        (
            "coupon_barrier_level",
            "coupon_barrier_levels",
            (-1, float("nan"), float("inf")),
        ),
    ):
        for invalid in invalids:
            with subtests.test(field=field, invalid=invalid):
                with pytest.raises(InvalidInputError):
                    getattr(
                        ins.AutocallableObservation,
                        "snowball" if field == "coupon_rate" else "phoenix",
                    )(time, 110, **{field: invalid})
                with pytest.raises(InvalidInputError):
                    ins.autocallable_observations([time], [110], **{keyword: [invalid]})
    records = ins.autocallable_observations(
        [time], [110], knock_out_coupon_rates=[-0.01]
    )
    assert records[0].coupon_rate == (-0.01)
    assert records[0].coupon_barrier_level is None


def test_asian_history_uses_optional_positive_averages(subtests):
    start, end = datetime(2025, 1, 1), datetime(2025, 1, 11)
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 2),
    )
    for kind, engine in (
        (
            ins.GeometricAveragePriceOption,
            pricing.AnalyticGeometricAveragePriceEngine(),
        ),
        (
            ins.ArithmeticAveragePriceOption,
            pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
        ),
    ):
        with subtests.test(kind=kind.__name__):
            original = kind(
                option_type="call",
                strike=100,
                effective_time=start,
                averaging_start_time=start,
                expiry_time=end,
            )
            assert original.realized_average is None
            with pytest.raises(InvalidInputError):
                engine.price(original, context)
            seasoned = original.replace(
                realized_average=110,
            )
            assert seasoned.realized_average == 110
            assert engine.price(seasoned, context) > 0
            assert seasoned.replace() == seasoned
            assert seasoned.replace(realized_average=None) == original
            assert seasoned.replace(realized_average=None) == original
            assert pickle.loads(pickle.dumps(original)) == original
            assert pickle.loads(pickle.dumps(seasoned)) == seasoned
            for invalid in (0, -1, float("nan"), float("inf")):
                with pytest.raises(InvalidInputError):
                    seasoned.replace(
                        realized_average=invalid,
                    )
            assert seasoned.realized_average == 110


def test_integer_inputs_share_the_index_protocol(subtests):
    class Index:
        def __init__(self, value):
            self.value = value

        def __index__(self):
            return self.value

    schedule = market.fixed_interval_schedule(
        observation_clock_time=clock_time(),
        start_time=datetime(2025, 1, 1),
        end_time=datetime(2025, 1, 10),
        interval_days=Index(2),
    )
    assert schedule[Index(1)] == datetime(2025, 1, 6)
    assert pricing.CoxRossRubinsteinVanillaEngine(step_count=Index(8)).step_count == 8
    assert pricing.MonteCarloVanillaEngine(seed=Index(42)).seed == 42
    assert (
        pricing.AnalyticHestonEuropeanEngine(max_evaluations=Index(32)).max_evaluations
        == 32
    )
    for invalid in (True, 2.5):
        with subtests.test(invalid=invalid), pytest.raises(TypeError):
            pricing.CoxRossRubinsteinVanillaEngine(step_count=invalid)
    with pytest.raises(OverflowError):
        pricing.CoxRossRubinsteinVanillaEngine(step_count=Index(2**31))
    with pytest.raises(OverflowError):
        pricing.MonteCarloVanillaEngine(seed=Index(2**64))
    failure = RuntimeError("index conversion failed")

    class FailingIndex:
        def __index__(self):
            raise failure

    with pytest.raises(RuntimeError) as caught:
        pricing.MonteCarloVanillaEngine(seed=FailingIndex())
    assert caught.value is failure


def test_exact_numbers_reject_implicit_rounding(subtests):
    class CoercingInteger(int):
        def __eq__(self, other):
            return float(self) == other

    fields = {
        "option_type": "call",
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }
    original = ins.EuropeanOption(strike=100, **fields)
    for value in (
        2**53 + 1,
        CoercingInteger(2**53 + 1),
        CoercingInteger(2**63 - 1),
        Fraction(2**53 + 1, 2**53),
        Fraction(1, 10),
        Decimal("0.1"),
        Decimal(2**53 + 1),
    ):
        with subtests.test(value=value):
            with pytest.raises(ValueError):
                ins.EuropeanOption(strike=value, **fields)
            with pytest.raises(ValueError):
                original.replace(strike=value)
            assert original.strike == 100
            assert original.replace(strike=float(value)).strike == float(value)
    for value in (
        2**53 + 2,
        CoercingInteger(2**53 + 2),
        Fraction(1, 2),
        Decimal(100),
        Decimal("0.125"),
        Decimal.from_float(0.1),
    ):
        with subtests.test(value=value):
            assert ins.EuropeanOption(strike=value, **fields).strike == value


def test_float_values_preserve_extremes_and_signed_zero(subtests):
    class Float(float):
        pass

    for value in (-0.0, 0.1, float.fromhex("0x0.0000000000001p-1022"), 1e308):
        for factory in (float, Float):
            with subtests.test(value=value, factory=factory):
                model = market.BlackScholesMertonParameters(
                    risk_free_rate=0.0,
                    dividend_yield=0.0,
                    volatility=factory(value),
                )
                assert model.volatility == value
                assert math.copysign(1.0, model.volatility) == math.copysign(1.0, value)


def test_decimal_conversion_preserves_context_and_error_categories():
    start, expiry = datetime(2025, 1, 1), datetime(2026, 1, 1)
    with localcontext() as decimal_context:
        for signal in (FloatOperation, Inexact, Rounded):
            decimal_context.traps[signal] = True
        decimal_context.clear_flags()
        model = market.BlackScholesMertonParameters(
            risk_free_rate=Decimal("0.125"),
            dividend_yield=Decimal(0),
            volatility=Decimal("0.25"),
        )
        context = market.PricingContext(
            model_parameters=model,
            spot_price=Decimal(100),
            valuation_time=start,
        )
        option = ins.EuropeanOption(
            option_type="call",
            strike=Decimal(100),
            effective_time=start,
            expiry_time=expiry,
        )
        engine = pricing.AnalyticEuropeanEngine()
        quote = engine.price(option, context)
        assert pricing.implied_volatility(
            engine,
            option,
            context,
            Decimal.from_float(quote),
            lower_bound=Decimal("0.125"),
            upper_bound=Decimal("0.5"),
        ) == pytest.approx(0.25, rel=0, abs=1e-8)
        for invalid in (Decimal("1e400"), Decimal("-1e400")):
            with pytest.raises(OverflowError):
                option.replace(strike=invalid)
        for invalid in (Decimal("NaN"), Decimal("Infinity"), Decimal("-Infinity")):
            with pytest.raises(InvalidInputError):
                option.replace(strike=invalid)
        for invalid in (Decimal("0.1"), Decimal("1e-400")):
            with pytest.raises(ValueError):
                option.replace(strike=invalid)
        assert (
            model.replace(risk_free_rate=float(Decimal("1e-400"))).flat_risk_free_rate
            == 0
        )
        with pytest.raises(TypeError):
            option.replace(strike=True)
        assert option.strike == 100
        assert not (any(decimal_context.flags.values()))


def test_settled_accumulators_have_no_remaining_cashflows(subtests):
    from kiyosi import InvalidInputError

    start, end = datetime(2025, 1, 1), datetime(2025, 1, 10)
    option = ins.Accumulator(
        observation_clock_time=clock_time(),
        strike=90,
        knock_out_level=110,
        daily_quantity=1,
        acceleration_factor=2,
        effective_time=start,
        expiry_time=end,
    )
    settled = option.replace(
        knock_out_settled=True,
    )
    assert not (option.knock_out_settled)
    assert settled.replace(strike=95).knock_out_settled
    assert settled.replace(
        accumulated_quantity=3,
    ).knock_out_settled
    assert pickle.loads(pickle.dumps(settled)) == settled
    assert ins.Accumulator(
        observation_clock_time=clock_time(),
        strike=90,
        knock_out_level=110,
        daily_quantity=1,
        acceleration_factor=2,
        effective_time=start,
        expiry_time=end,
        knock_out_settled=True,
    ).knock_out_settled
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        ),
        spot_price=100,
        valuation_time=datetime(2025, 1, 3),
    )
    for factory in (
        pricing.QuadratureAccumulatorEngine,
        pricing.FiniteDifferenceAccumulatorEngine,
        pricing.MonteCarloAccumulatorEngine,
    ):
        engine = factory()
        if isinstance(engine, pricing.MonteCarloAccumulatorEngine):
            engine = engine.with_resolved_seed()
        with subtests.test(engine=factory.__name__):
            assert engine.price(settled, context) == 0
            assert (
                engine.price(
                    settled.replace(
                        accumulated_quantity=3,
                    ),
                    context,
                )
                == 0
            )
            active = settled.replace(
                knock_out_settled=False,
                accumulated_quantity=0,
            )
            assert engine.price(active, context) > 0
            with pytest.raises(InvalidInputError):
                engine.price(
                    settled.replace(
                        knock_out_settled=False,
                    ),
                    context,
                )
            result = engine.price_with_greeks(settled, context, "all")
            for greek in (
                "delta",
                "gamma",
                "theta",
                "vega",
                "rho",
            ):
                assert result.require(greek) == 0
            with pytest.raises(UnsupportedOperationError):
                pricing.implied_volatility(
                    engine,
                    settled,
                    context,
                    0,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
    for invalid in (0, 1, "yes"):
        with subtests.test(invalid=invalid), pytest.raises(TypeError):
            option.replace(
                knock_out_settled=invalid,
            )


def test_automatic_volatility_bumps_preserve_explicit_controls_and_zero_boundary():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    model = market.BlackScholesMertonParameters(
        risk_free_rate=0, dividend_yield=0, volatility=0.00005
    )
    context = market.PricingContext(
        model_parameters=model, spot_price=100, valuation_time=start
    )
    engine = pricing.AnalyticEuropeanEngine()
    unavailable = numerical_greeks(
        engine,
        option,
        context,
        "vega",
        volatility_shift=0.0001,
    )
    assert unavailable.status("vega") == "stencil_unavailable"
    assert unavailable.vega is None
    restored = pickle.loads(pickle.dumps(unavailable))
    assert restored == unavailable
    assert restored.status("vega") == "stencil_unavailable"
    recovered = numerical_greeks(
        engine,
        option,
        context,
        "vega",
        volatility_shift=0.00001,
    )
    exact = engine.price_with_greeks(option, context, "vega")
    assert recovered.status("vega") == "available"
    assert recovered.vega == pytest.approx(exact.vega, rel=0, abs=1e-7)
    for control in ({}, {"volatility_shift": None}, {"volatility_shift": Ellipsis}):
        automatic = numerical_greeks(engine, option, context, "vega", **control)
        assert automatic.status("vega") == "available"
        assert automatic.require("vega") == pytest.approx(
            exact.require("vega"), rel=0, abs=1e-7
        )
    zero = context.replace(model_parameters=model.replace(volatility=0))
    assert numerical_greeks(engine, option, zero, "vega").status("vega") == "undefined"


def test_digital_fallback_shifts_preserve_analytic_spot_greeks(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.00005
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = pricing.AnalyticDigitalEngine()
    for direction, sign in (("call", 1), ("put", -1)):
        for kind, fields, weight in (
            (ins.CashOrNothingOption, {"payout": 10}, -5),
            (ins.AssetOrNothingOption, {}, 50),
        ):
            option = kind(
                option_type=direction,
                strike=100,
                effective_time=start,
                expiry_time=end,
                **fields,
            )
            native = engine.price_with_greeks(
                option, context, ["delta", "gamma", "vega"], volatility_shift=0.0001
            )
            assert native.status("vega") == "stencil_unavailable"
            result = engine.price_with_greeks(
                option,
                context,
                ["delta", "gamma", "vega"],
                volatility_shift=0.00001,
            )
            assert ((result.price, result.delta, result.gamma)) == (
                (native.price, native.delta, native.gamma)
            )
            expected = (
                sign * weight * math.exp(-0.5 * 0.000025**2) / math.sqrt(2 * math.pi)
            )
            assert result.require("vega") == pytest.approx(expected, rel=0, abs=1e-7)
            assert result.status("rho") == "not_requested"
    for at in (start, end):
        for controls in (
            {"spot_shift": 0},
            {"volatility_shift": float("nan")},
            {"rate_shift": -1},
            {"time_shift": timedelta(0)},
        ):
            with (
                subtests.test(at=at, controls=controls),
                pytest.raises(InvalidInputError),
            ):
                engine.price_with_greeks(
                    option, context.replace(valuation_time=at), "delta", **controls
                )
    for controls in (
        {"spot_shift": True},
        {"volatility_shift": True},
        {"rate_shift": True},
        {"time_shift": 1},
    ):
        with subtests.test(controls=controls), pytest.raises(TypeError):
            engine.price_with_greeks(option, context, "delta", **controls)


def test_fd_greek_shifts_recover_vega_and_preserve_native_spot_estimates(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.00005
        ),
        spot_price=100,
        valuation_time=start,
    )
    grid = {"asset_step_count": 200, "time_step_count": 100}
    base = pricing.FiniteDifferenceVanillaEngine(**grid).price_with_greeks(
        option, context, ["delta", "gamma", "vega"]
    )
    assert base.status("vega") == "available"
    shifts = {
        "volatility_shift": 0.00001,
        "rate_shift": 0.0002,
        "time_shift": timedelta(seconds=60),
    }
    engine = pricing.FiniteDifferenceVanillaEngine(**grid)
    tuned = engine.price_with_greeks(
        option, context, ["delta", "gamma", "vega"], **shifts
    )
    for default in (None, Ellipsis):
        assert (
            engine.price_with_greeks(
                option, context, ["delta", "gamma", "vega"], volatility_shift=default
            )
            == base
        )
    assert tuned.price == base.price
    assert tuned.require("delta") == base.require("delta")
    assert tuned.require("gamma") == base.require("gamma")
    exact = pricing.AnalyticEuropeanEngine().price_with_greeks(option, context, "vega")
    assert tuned.require("vega") == pytest.approx(exact.require("vega"), rel=0, abs=0.1)
    regular = context.replace(
        model_parameters=context.model_parameters.replace(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
    )
    measures = ["rho", "dividend_rho", "theta"]
    configured_result = engine.price_with_greeks(option, regular, measures, **shifts)
    scalar = numerical_greeks(engine, option, regular, measures, **shifts)
    for greek in measures:
        assert configured_result.require(greek) == pytest.approx(
            scalar.require(greek), rel=0, abs=1e-10
        )
    for name in pricing.__all__:
        if (
            not (name.startswith("FiniteDifference") and name.endswith("Engine"))
            or "Heston" in name
        ):
            continue
        factory = getattr(pricing, name)
        with subtests.test(engine=name):
            for field, value in shifts.items():
                with pytest.raises(TypeError):
                    factory(**{field: value})
                with pytest.raises(TypeError):
                    factory().replace(**{field: value})


def test_greek_status_distinguishes_requests_support_and_stencils():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    result = pricing.AnalyticEuropeanEngine().price_with_greeks(
        option, context, "delta"
    )
    assert result.status("delta") == "available"
    assert result.status("gamma") == "not_requested"
    assert result.requested == (("delta",))
    terminal = pricing.AnalyticEuropeanEngine().price_with_greeks(
        option,
        context.replace(valuation_time=end),
        "delta",
    )
    assert terminal.status("delta") == "undefined"
    collapsed = numerical_greeks(
        pricing.QuadratureVanillaEngine(),
        option,
        context,
        "delta",
        spot_shift=400,
    )
    assert collapsed.status("delta") == "stencil_unavailable"
    heston = context.replace(
        model_parameters=market.HestonParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            initial_variance=0.04,
            long_run_variance=0.04,
            mean_reversion_speed=1.5,
            vol_of_vol=0.3,
            correlation=-0.7,
        )
    )
    unsupported = pricing.AnalyticHestonEuropeanEngine().price_with_greeks(
        option, heston, "delta"
    )
    from kiyosi import GreekUnavailableError

    for value, measure, status in (
        (result, "gamma", "not_requested"),
        (terminal, "delta", "undefined"),
        (collapsed, "delta", "stencil_unavailable"),
        (unsupported, "delta", "unsupported"),
    ):
        with pytest.raises(GreekUnavailableError) as caught:
            value.require(measure)
        assert isinstance(caught.value, UnsupportedOperationError)
        assert caught.value.greek == measure
        assert caught.value.status == status
        assert measure in str(caught.value)
        assert status in str(caught.value)

    local = context.replace(
        model_parameters=market.LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            surface=market.LocalVolatilitySurface(
                times=[start, end], spots=[80, 120], values=[[0.2, 0.2], [0.2, 0.2]]
            ),
        )
    )
    partial = pricing.FiniteDifferenceVanillaEngine().price_with_greeks(
        option, local, ["delta", "vega"]
    )
    assert partial.status("delta") == "available"
    assert partial.status("vega") == "unsupported"
    assert partial.requested == (("delta", "vega"))
    for original in (result, terminal, collapsed, partial):
        restored = pickle.loads(pickle.dumps(original))
        assert restored.price == original.price
        assert restored.requested == original.requested
        assert restored.supported == original.supported
        for greek in ("delta", "gamma", "vega"):
            assert restored.status(greek) == original.status(greek)
            assert getattr(restored, greek) == getattr(original, greek)
    with pytest.raises(ValueError):
        result.status("unknown")


def test_flat_rate_query_is_independent_of_reference_time():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    for reference in (None, start):
        curve = market.RateTermStructure.flat(0.05, reference_time=reference)
        assert curve.exact_flat_rate == 0.05
        assert_decimal_equal(curve.shifted(0.01).exact_flat_rate, 0.06)
        assert curve.reference_time == reference
        restored = pickle.loads(pickle.dumps(curve))
        assert restored.exact_flat_rate == 0.05
        assert restored.reference_time == reference
    nodes = market.RateTermStructure(reference_time=start, times=[end], factors=[0.95])
    assert_decimal_equal(nodes.exact_flat_rate, -math.log(0.95))


def test_numerical_greeks_has_uniform_controls_across_engines(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    option = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    expected = (
        pricing.AnalyticEuropeanEngine()
        .price_with_greeks(option, context, "delta")
        .require("delta")
    )
    for engine in (
        pricing.AnalyticEuropeanEngine(),
        pricing.QuadratureVanillaEngine(),
        pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=100, time_step_count=100
        ),
        pricing.MonteCarloVanillaEngine(path_count=4096, seed=42, max_active_workers=1),
    ):
        with subtests.test(engine=type(engine).__name__):
            result = pricing.numerical_greeks(
                engine,
                option,
                context,
                "delta",
                spot_shift=0.2,
            )
            assert result.price == engine.price(option, context)
            assert result.require("delta") == pytest.approx(expected, rel=0, abs=0.03)
            assert result.requested == (("delta",))
            with pytest.raises(InvalidInputError):
                pricing.numerical_greeks(
                    engine,
                    option,
                    context,
                    "delta",
                    rate_shift=0,
                )
    with pytest.raises(TypeError):
        pricing.numerical_greeks(
            pricing.AnalyticEuropeanEngine(),
            option,
            context,
            "delta",
            spot_shift=True,
        )
    with pytest.raises(UnsupportedOperationError):
        pricing.numerical_greeks(
            pricing.AnalyticEuropeanEngine(),
            ins.AmericanOption(
                option_type="call",
                strike=100,
                effective_time=start,
                expiry_time=end,
            ),
            context,
            "unknown",
        )


@pytest.mark.parametrize(
    "expected",
    ("knock_in", "knock_out"),
)
def test_double_touch_barrier_type_roundtrips_through_replacement(expected):
    option = ins.DoubleTouchOption(
        barrier_type=expected,
        effective_time=datetime(2025, 1, 1),
        expiry_time=datetime(2026, 1, 1),
        lower_barrier=80,
        upper_barrier=120,
        payout=10,
    )
    assert option.barrier_type == expected
    assert option.replace(barrier_type=option.barrier_type) == option
    other = "knock_out" if expected == "knock_in" else "knock_in"
    assert option.replace(barrier_type=other).barrier_type == other
    assert pickle.loads(pickle.dumps(option)).barrier_type == expected


def test_shift_validation_is_independent_of_estimator_and_valuation_time(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    life = {"effective_time": start, "expiry_time": end}
    bsm = market.BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
    )
    vanilla = ins.EuropeanOption(option_type="call", strike=100, **life)
    digital = ins.CashOrNothingOption(option_type="call", strike=100, payout=10, **life)
    sampling = {"path_count": 32, "seed": 42, "max_active_workers": 1}
    cases = [
        (pricing.AnalyticEuropeanEngine(), vanilla, bsm),
        (pricing.QuadratureVanillaEngine(), vanilla, bsm),
        (pricing.CoxRossRubinsteinDigitalEngine(), digital, bsm),
        (
            pricing.CoxRossRubinsteinBarrierEngine(),
            ins.BarrierOption(
                option_type="call",
                strike=100,
                barrier_level=120,
                barrier_type="up_and_out",
                touch_state="untouched",
                **life,
            ),
            bsm,
        ),
        (pricing.MonteCarloDigitalEngine(**sampling), digital, bsm),
        (
            pricing.MonteCarloBarrierEngine(**sampling),
            ins.BarrierOption(
                option_type="call",
                strike=100,
                barrier_level=120,
                barrier_type="up_and_out",
                touch_state="untouched",
                **life,
            ),
            bsm,
        ),
        (
            pricing.MonteCarloBarrierEngine(**sampling),
            ins.DoubleBarrierOption(
                option_type="call",
                strike=100,
                lower_barrier=80,
                upper_barrier=120,
                barrier_type="knock_out",
                touch_state="untouched",
                **life,
            ),
            bsm,
        ),
        *(
            (
                pricing.MonteCarloAsianEngine(**sampling),
                kind(option_type="call", strike=100, averaging_start_time=end, **life),
                bsm,
            )
            for kind in (
                ins.GeometricAveragePriceOption,
                ins.ArithmeticAveragePriceOption,
            )
        ),
    ]
    for engine, option, model in cases:
        for at in (start, end):
            context = market.PricingContext(
                model_parameters=model, spot_price=110, valuation_time=at
            )
            with subtests.test(engine=type(engine).__name__, at=at):
                assert engine.price_with_greeks(option, context, "delta").price >= 0
                for field in (
                    "spot_shift",
                    "volatility_shift",
                    "rate_shift",
                    "time_shift",
                ):
                    invalids = (
                        (timedelta(), timedelta(days=-1))
                        if field == "time_shift"
                        else (0, -1, float("nan"), float("inf"))
                    )
                    for bad in invalids:
                        with (
                            subtests.test(field=field, invalid=bad),
                            pytest.raises(InvalidInputError),
                        ):
                            numerical_greeks(
                                engine,
                                option,
                                context,
                                "delta",
                                **{field: bad},
                            )


def test_settings_diagnostics_identify_invalid_fields(subtests):
    cases = [
        (pricing.CoxRossRubinsteinVanillaEngine, "step_count", 0),
        (pricing.CoxRossRubinsteinVanillaEngine, "step_count", 1_000_001),
        (pricing.MonteCarloVanillaEngine, "path_count", 0),
        (pricing.MonteCarloVanillaEngine, "path_count", 10_000_001),
        (pricing.MonteCarloVanillaEngine, "max_active_workers", -1),
        (pricing.FiniteDifferenceVanillaEngine, "asset_step_count", 2),
        (pricing.FiniteDifferenceVanillaEngine, "time_step_count", 0),
        (pricing.FiniteDifferenceVanillaEngine, "asset_upper_boundary", 0.0),
        (pricing.FiniteDifferenceVanillaEngine, "asset_upper_boundary", math.nan),
        (pricing.FiniteDifferenceHestonVanillaEngine, "variance_step_count", 2),
        (pricing.FiniteDifferenceHestonVanillaEngine, "asset_step_count", 2),
        (pricing.FiniteDifferenceHestonVanillaEngine, "time_step_count", 0),
        (
            pricing.FiniteDifferenceHestonVanillaEngine,
            "variance_upper_boundary",
            -1.0,
        ),
        (
            pricing.FiniteDifferenceHestonVanillaEngine,
            "variance_upper_boundary",
            math.inf,
        ),
        (pricing.FiniteDifferenceHestonVanillaEngine, "asset_upper_boundary", 0.0),
        (pricing.QuadratureBarrierEngine, "asset_step_count", 3),
        (pricing.QuadratureBarrierEngine, "integration_standard_deviations", 0.0),
        (
            pricing.QuadratureBarrierEngine,
            "integration_standard_deviations",
            math.nan,
        ),
        (pricing.QuadratureVanillaEngine, "time_step_count", 0),
        (pricing.AnalyticHestonEuropeanEngine, "absolute_tolerance", -1.0),
        (pricing.AnalyticHestonEuropeanEngine, "absolute_tolerance", math.nan),
        (pricing.AnalyticHestonEuropeanEngine, "relative_tolerance", -1.0),
        (pricing.AnalyticHestonEuropeanEngine, "relative_tolerance", math.inf),
        (pricing.AnalyticHestonEuropeanEngine, "max_evaluations", 0),
    ]
    for factory, field, invalid in cases:
        with subtests.test(engine=factory.__name__, field=field):
            with pytest.raises(InvalidInputError) as caught:
                factory(**{field: invalid})
            assert field in str(caught.value)


def test_surface_diagnostics_identify_the_invalid_node():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    with pytest.raises(InvalidInputError) as caught:
        market.LocalVolatilitySurface(
            times=[start, end], spots=[80, 100], values=[[0.2, -0.1], [0.2, 0.2]]
        )
    assert "values[0][1]" in str(caught.value)


def test_greek_methods_check_curve_support_without_rejecting_asian_settlement(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    life = {"effective_time": start, "expiry_time": end}
    european = ins.EuropeanOption(option_type="call", strike=100, **life)
    american = ins.AmericanOption(option_type="put", strike=100, **life)
    digital = ins.CashOrNothingOption(option_type="call", strike=100, payout=10, **life)
    geometric = ins.GeometricAveragePriceOption(
        option_type="call", strike=100, averaging_start_time=start, **life
    )
    arithmetic = ins.ArithmeticAveragePriceOption(
        option_type="call", strike=100, averaging_start_time=start, **life
    )
    mc = pricing.MonteCarloAsianEngine(path_count=16, seed=42, max_active_workers=1)
    curve = market.RateTermStructure(
        reference_time=start,
        times=[start + timedelta(days=180), end],
        factors=[0.98, 0.94],
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    for engine, option, method in [
        (pricing.CoxRossRubinsteinVanillaEngine(), european, "mixed"),
        (pricing.CoxRossRubinsteinVanillaEngine(), american, "mixed"),
        (pricing.CoxRossRubinsteinDigitalEngine(), digital, "native"),
        (pricing.BjerksundStenslandVanillaEngine(), american, "numerical"),
        (mc, geometric, "native"),
        (
            pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
            arithmetic,
            "numerical",
        ),
    ]:
        with subtests.test(engine=type(engine).__name__, option=type(option).__name__):
            assert engine.greek_method(option, context) == method
            assert math.isfinite(engine.price(option, context))
            for dividend in (False, True):
                for volatility in (0, 0.2):
                    unsupported = context.replace(
                        model_parameters=market.BlackScholesMertonParameters(
                            risk_free_rate=0.05 if dividend else curve,
                            dividend_yield=curve if dividend else 0.02,
                            volatility=volatility,
                        )
                    )
                    with pytest.raises(UnsupportedOperationError):
                        engine.price(option, unsupported)
                    with pytest.raises(UnsupportedOperationError):
                        engine.greek_method(option, unsupported)
                    with pytest.raises(UnsupportedOperationError):
                        engine.active_greek_controls(option, unsupported)
    curved = context.replace(
        model_parameters=context.model_parameters.replace(risk_free_rate=curve)
    )
    assert mc.greek_method(arithmetic, curved) == "native"
    assert math.isfinite(mc.price(arithmetic, curved))
    analytic = pricing.AnalyticGeometricAveragePriceEngine()
    assert analytic.greek_method(geometric, curved) == "numerical"
    assert math.isfinite(analytic.price(geometric, curved))
    terminal = curved.replace(valuation_time=end)
    for engine, option in (
        (mc, geometric),
        (pricing.TurnbullWakemanArithmeticAveragePriceEngine(), arithmetic),
    ):
        option = option.replace(
            realized_average=110,
        )
        assert engine.greek_method(option, terminal) == "terminal"
        assert engine.price(option, terminal) == 10


def test_greek_methods_report_estimators_and_active_controls(subtests):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    life = {"effective_time": start, "expiry_time": end}
    option = ins.EuropeanOption(option_type="call", strike=100, **life)
    digital = ins.CashOrNothingOption(option_type="call", strike=100, payout=10, **life)
    barrier = ins.BarrierOption(
        option_type="call",
        strike=100,
        barrier_level=120,
        barrier_type="up_and_out",
        **life,
    )
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    all_controls = [
        "spot_shift",
        "volatility_shift",
        "rate_shift",
        "time_shift",
    ]
    for engine, contract, method, controls in [
        (pricing.AnalyticEuropeanEngine(), option, "native", []),
        (pricing.QuadratureVanillaEngine(), option, "numerical", all_controls),
        (pricing.CoxRossRubinsteinVanillaEngine(), option, "mixed", all_controls),
        (pricing.MonteCarloVanillaEngine(), option, "numerical", all_controls),
        (pricing.MonteCarloDigitalEngine(), digital, "native", []),
        (
            pricing.MonteCarloBarrierEngine(),
            barrier,
            "native_with_spot_control",
            ["spot_shift"],
        ),
    ]:
        with subtests.test(engine=type(engine).__name__):
            assert engine.greek_method(contract, context) == method
            assert engine.active_greek_controls(contract, context) == controls
            terminal = context.replace(valuation_time=end)
            exact = isinstance(engine, pricing.AnalyticEuropeanEngine)
            assert engine.greek_method(contract, terminal) == (
                "native" if exact else "terminal"
            )
            assert engine.active_greek_controls(contract, terminal) == (
                [] if exact else ["spot_shift"]
            )
    zero = context.replace(
        model_parameters=context.model_parameters.replace(volatility=0)
    )
    assert pricing.AnalyticEuropeanEngine().greek_method(option, zero) == "numerical"
    local = context.replace(
        model_parameters=market.LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            surface=market.LocalVolatilitySurface(
                times=[start, end], spots=[80, 120], values=[[0.2, 0.2], [0.2, 0.2]]
            ),
        )
    )
    assert pricing.FiniteDifferenceVanillaEngine().active_greek_controls(
        option, local
    ) == ["spot_shift"]
    with pytest.raises(UnsupportedOperationError):
        pricing.AnalyticEuropeanEngine().greek_method(option, local)
    heston = context.replace(
        model_parameters=market.HestonParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            initial_variance=0.04,
            long_run_variance=0.04,
            mean_reversion_speed=1.5,
            vol_of_vol=0.3,
            correlation=-0.7,
        )
    )
    assert (
        pricing.AnalyticHestonEuropeanEngine().greek_method(option, heston)
        == "unsupported"
    )
    assert (
        pricing.AnalyticHestonEuropeanEngine().active_greek_controls(option, heston)
        == []
    )
    with pytest.raises(UnsupportedOperationError):
        pricing.AnalyticEuropeanEngine().greek_method(digital, context)


@pytest.mark.parametrize("value", (0, 0.08))
def test_observation_coupon_getters_roundtrip_without_placeholder_values(
    subtests, value
):
    at = datetime(2026, 1, 1)
    for record, active, absent in (
        (
            ins.AutocallableObservation.snowball(at, 110, coupon_rate=value),
            "coupon_rate",
            "coupon_barrier_level",
        ),
        (
            ins.AutocallableObservation.phoenix(at, 110, coupon_barrier_level=value),
            "coupon_barrier_level",
            "coupon_rate",
        ),
    ):
        with subtests.test(kind=record.kind, value=value):
            assert getattr(record, active) == value
            assert getattr(record, absent) is None
            assert (
                record.replace(
                    coupon_rate=record.coupon_rate,
                    coupon_barrier_level=record.coupon_barrier_level,
                )
                == record
            )
            for updates in (
                {active: None},
                {"coupon_rate": None, "coupon_barrier_level": None},
            ):
                with pytest.raises(TypeError):
                    record.replace(**updates)
            assert record.replace(**{active: ..., absent: None}) == record
            assert getattr(record, active) == value
            assert pickle.loads(pickle.dumps(record)) == record


def test_zero_coupon_observations_preserve_product_identity(subtests):
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    snowball = ins.AutocallableObservation.snowball(end, 110, coupon_rate=0)
    phoenix = ins.AutocallableObservation.phoenix(end, 110, coupon_barrier_level=0)
    assert ins.autocallable_observations([end], [110], coupon_barrier_levels=[0]) == [
        phoenix
    ]
    assert snowball != phoenix
    for record, kind in ((snowball, "snowball"), (phoenix, "phoenix")):
        with subtests.test(kind=kind):
            assert record.kind == kind
            assert record.replace(knock_out_level=120).kind == kind
            restored = pickle.loads(pickle.dumps(record))
            assert restored == record
            assert restored.kind == kind
            assert (f"AutocallableObservation.{kind}(") in repr(record)
    with pytest.raises(InvalidInputError):
        snowball.replace(coupon_barrier_level=0)
    with pytest.raises(InvalidInputError):
        phoenix.replace(coupon_rate=0)
    life = {"effective_time": start, "expiry_time": end}
    with pytest.raises(InvalidInputError):
        ins.BinarySnowballOption(
            calendar=market.weekdays_calendar(),
            maturity_coupon_rate=0.1,
            autocall_observations=[phoenix],
            **life,
        )
    with pytest.raises(InvalidInputError):
        ins.PhoenixOption(
            calendar=market.weekdays_calendar(),
            coupon_per_period=0.1,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            autocall_observations=[snowball],
            knock_in_observation=KnockInObservation.every_trading_day(end.time()),
            **life,
        )


def test_observation_replacement_preserves_validity_and_original_values(subtests):
    time = datetime(2026, 1, 1)
    original = ins.AutocallableObservation.snowball(time, 110, coupon_rate=-0.01)
    phoenix = ins.AutocallableObservation.phoenix(time, 110, coupon_barrier_level=90)
    assert phoenix.coupon_rate is None
    assert pickle.loads(pickle.dumps(phoenix)) == phoenix
    assert "AutocallableObservation.phoenix" in repr(phoenix)
    with pytest.raises(InvalidInputError):
        phoenix.replace(coupon_rate=0.12)
    with pytest.raises(InvalidInputError):
        ins.autocallable_observations(
            [time], [110], knock_out_coupon_rates=[0.12], coupon_barrier_levels=[90]
        )
    changed = original.replace(
        time=time + timedelta(days=1), knock_out_level=120, coupon_rate=0.08
    )
    assert changed.time == time + timedelta(days=1)
    assert changed.knock_out_level == 120
    assert changed.coupon_rate == 0.08
    assert changed.coupon_barrier_level is None
    assert original.replace() == original
    assert original.replace(coupon_rate=...) == original
    assert pickle.loads(pickle.dumps(changed)) == changed
    for fields in (
        {"knock_out_level": 0},
        {"knock_out_level": math.inf},
        {"coupon_rate": math.nan},
        {"coupon_barrier_level": -1},
        {"coupon_barrier_level": math.inf},
    ):
        with subtests.test(fields=fields), pytest.raises(InvalidInputError):
            original.replace(**fields)
        assert (
            (
                original.time,
                original.knock_out_level,
                original.coupon_rate,
                original.coupon_barrier_level,
            )
        ) == ((time, 110, -0.01, None))
    with pytest.raises(TypeError):
        original.replace(coupon_rate=True)
