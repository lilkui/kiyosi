"""CPU product/engine dispatch and terminal contracts through the public API."""

from datetime import datetime, timedelta
from datetime import time as clock_time

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
    weekdays_calendar,
)
from support.assertions import assert_decimal_equal

HESTON_ENGINES = (
    "AnalyticHestonEuropeanEngine",
    "FiniteDifferenceHestonVanillaEngine",
)


def terminal_cases():
    dates = {
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }
    vanilla = dict(dates, option_type="call", strike=100)
    barrier = dict(
        vanilla, barrier_type="up_and_out", barrier_level=120, touch_state="untouched"
    )
    cash_digital = (
        "AnalyticDigitalEngine",
        "QuadratureDigitalEngine",
        "CoxRossRubinsteinDigitalEngine",
        "FiniteDifferenceDigitalEngine",
        "MonteCarloDigitalEngine",
    )
    binary = ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine")
    notes = (
        "QuadratureAutocallableEngine",
        "FiniteDifferenceAutocallableEngine",
        "MonteCarloAutocallableEngine",
    )
    cases = [
        (
            ins.EuropeanOption(**vanilla),
            (
                "AnalyticEuropeanEngine",
                "AnalyticHestonEuropeanEngine",
                "FiniteDifferenceHestonVanillaEngine",
                "CoxRossRubinsteinVanillaEngine",
                "QuadratureVanillaEngine",
                "FiniteDifferenceVanillaEngine",
                "MonteCarloVanillaEngine",
            ),
            5,
        ),
        (
            ins.AmericanOption(**vanilla),
            (
                "BjerksundStenslandVanillaEngine",
                "QuadratureVanillaEngine",
                "CoxRossRubinsteinVanillaEngine",
                "FiniteDifferenceVanillaEngine",
                "FiniteDifferenceHestonVanillaEngine",
                "MonteCarloVanillaEngine",
            ),
            5,
        ),
        (ins.CashOrNothingOption(**vanilla, payout=10), cash_digital, 10),
        (ins.AssetOrNothingOption(**vanilla), cash_digital, 105),
        (
            ins.BarrierOption(**barrier),
            (
                "AnalyticBarrierEngine",
                "CoxRossRubinsteinBarrierEngine",
                "QuadratureBarrierEngine",
                "FiniteDifferenceBarrierEngine",
                "MonteCarloBarrierEngine",
            ),
            5,
        ),
        (
            ins.cash_binary_barrier_option(**barrier, payout=10),
            binary,
            10,
        ),
        (ins.asset_binary_barrier_option(**barrier), binary, 105),
        (
            ins.DoubleBarrierOption(
                **vanilla,
                barrier_type="knock_out",
                lower_barrier=80,
                upper_barrier=120,
                touch_state="untouched",
            ),
            (
                "AnalyticDoubleBarrierEngine",
                "FiniteDifferenceDoubleBarrierEngine",
                "MonteCarloDoubleBarrierEngine",
            ),
            5,
        ),
        (
            ins.GeometricAveragePriceOption(
                **vanilla,
                averaging_start_time=dates["effective_time"],
                realized_average=110,
            ),
            ("AnalyticGeometricAveragePriceEngine", "MonteCarloAsianEngine"),
            10,
        ),
        (
            ins.ArithmeticAveragePriceOption(
                **vanilla,
                averaging_start_time=dates["effective_time"],
                realized_average=110,
            ),
            ("TurnbullWakemanArithmeticAveragePriceEngine", "MonteCarloAsianEngine"),
            10,
        ),
        (
            ins.Accumulator(
                knock_out_settled=False,
                observation_clock_time=clock_time(),
                effective_time=dates["effective_time"] - timedelta(days=1),
                expiry_time=dates["expiry_time"],
                strike=100,
                knock_out_level=120,
                daily_quantity=1,
                acceleration_factor=2,
                accumulated_quantity=3,
            ),
            (
                "QuadratureAccumulatorEngine",
                "FiniteDifferenceAccumulatorEngine",
                "MonteCarloAccumulatorEngine",
            ),
            20,
        ),
        (
            ins.RangeAccrualOption(
                observation_clock_time=clock_time(),
                **dates,
                calendar=all_days_calendar(),
                notional=1000,
                annual_coupon_rate=0.12,
                lower_bound=95,
                upper_bound=110,
                realized_hit_count=0,
            ),
            ("AnalyticRangeAccrualEngine",),
            120 / 365,
        ),
    ]
    for factory, kind, amount in (
        (ins.cash_touch_option, "up_and_in", 0),
        (ins.cash_no_touch_option, "up_and_out", 10),
        (ins.asset_touch_option, "up_and_in", 0),
        (ins.asset_no_touch_option, "up_and_out", 105),
    ):
        payout = (
            {"payout": 10}
            if factory in (ins.cash_touch_option, ins.cash_no_touch_option)
            else {}
        )
        cases.append(
            (
                factory(
                    **dates,
                    barrier_level=120,
                    barrier_type=kind,
                    touch_state="untouched",
                    **payout,
                ),
                binary,
                amount,
            )
        )
    for factory, amount in ((ins.cash_double_touch, 0), (ins.cash_double_no_touch, 10)):
        cases.append(
            (
                factory(
                    **dates,
                    lower_barrier=80,
                    upper_barrier=120,
                    payout=10,
                    touch_state="untouched",
                ),
                ("AnalyticDoubleTouchEngine", "FiniteDifferenceDoubleTouchEngine"),
                amount,
            )
        )
    common = dict(
        dates,
        observations=ins.autocallable_observations(
            [dates["expiry_time"]], [120], knock_out_coupon_rates=[0.08]
        ),
    )
    loss = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 60,
        "knock_in_observation_mode": "at_expiry",
        "barrier_state": "no_event",
    }
    cases.extend(
        [
            (
                ins.SnowballOption(
                    calendar=weekdays_calendar(),
                    observation_clock_time=None,
                    **common,
                    **loss,
                    maturity_coupon_rate=0.06,
                ),
                notes,
                1.06,
            ),
            (
                ins.BinarySnowballOption(
                    calendar=weekdays_calendar(),
                    **common,
                    maturity_coupon_rate=0.06,
                ),
                notes,
                1.06,
            ),
            (
                ins.TernarySnowballOption(
                    calendar=weekdays_calendar(),
                    observation_clock_time=None,
                    **common,
                    maturity_coupon_rate=0.06,
                    minimum_coupon_rate=-0.02,
                    knock_in_level=80,
                    knock_in_observation_mode="at_expiry",
                    barrier_state="no_event",
                ),
                notes,
                1.06,
            ),
            (
                ins.PhoenixOption(
                    calendar=weekdays_calendar(),
                    observation_clock_time=None,
                    **dates,
                    **loss,
                    coupon_per_period=0.08,
                    observations=ins.autocallable_observations(
                        [dates["expiry_time"]], [120], coupon_barrier_levels=[90]
                    ),
                ),
                notes,
                1.08,
            ),
        ]
    )
    return cases


def terminal_delta(instrument):
    if isinstance(
        instrument,
        (
            ins.EuropeanOption,
            ins.AmericanOption,
            ins.AssetOrNothingOption,
            ins.BarrierOption,
            ins.DoubleBarrierOption,
        ),
    ):
        return 1
    if isinstance(instrument, ins.BinaryBarrierOption):
        return int(instrument.payoff_type == "asset")
    if isinstance(instrument, ins.TouchOption):
        return int(instrument.payoff_type == "asset" and not instrument.is_one_touch)
    if isinstance(instrument, ins.Accumulator):
        return 4
    return 0


@pytest.fixture(scope="module")
def base_cases():
    return terminal_cases()


@pytest.fixture(scope="module")
def base_engines():
    return {
        name: getattr(pricing, name)()
        for name in pricing.__all__
        if name.endswith("Engine")
    }


@pytest.fixture(scope="module")
def base_context():
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=105,
        valuation_time=datetime(2026, 1, 1),
    )


def test_monte_carlo_terminal_statistics_and_seeds_match_settlement(
    base_cases, base_engines, context_for, subtests
):
    for instrument, names, expected in base_cases:
        for name in names:
            if not name.startswith("MonteCarlo"):
                continue
            context = context_for(name)
            option = instrument
            for sampling in ("pseudo_random", "sobol"):
                engine = base_engines[name].replace(sampling=sampling)
                with subtests.test(
                    instrument=type(option).__name__, engine=name, sampling=sampling
                ):
                    price, seed = engine.price_with_seed(option, context)
                    assert price == pytest.approx(expected, rel=0, abs=1e-12)
                    assert seed is None
                    if isinstance(option, ins.AmericanOption):
                        continue
                    estimate = engine.price_with_statistics(option, context)
                    assert estimate.price == pytest.approx(expected, rel=0, abs=1e-12)
                    assert estimate.seed is None
                    assert estimate.standard_error == (
                        0 if sampling == "pseudo_random" else None
                    )
                    assert estimate.uncertainty_status == (
                        "available"
                        if sampling == "pseudo_random"
                        else "unsupported_sampling"
                    )


def test_empty_greek_requests_return_price_only_for_every_engine(
    base_cases, base_engines, context_for, subtests
):
    for instrument, names, expected in base_cases:
        for name in names:
            context = context_for(name)
            option = instrument
            with subtests.test(instrument=type(instrument).__name__, engine=name):
                result = base_engines[name].price_with_greeks(option, context, [])
                assert result.price == pytest.approx(expected, rel=0, abs=1e-12)
                assert result.requested == (())
                assert result.status("delta") == "not_requested"
                assert result.delta is None
                assert result.seed is None


def test_empty_requests_price_before_expiry_and_validate_controls(
    base_cases, base_engines, context_for, subtests
):
    for instrument, names, _ in base_cases[:4]:
        for name in names:
            context = context_for(name).replace(
                valuation_time=instrument.effective_time
            )
            engine = base_engines[name]
            if name.startswith("MonteCarlo"):
                engine = (
                    engine.replace(path_count=32, seed=42, step_count=4)
                    if name == "MonteCarloVanillaEngine"
                    else engine.replace(path_count=32, seed=42)
                )
            with subtests.test(instrument=type(instrument).__name__, engine=name):
                result = engine.price_with_greeks(instrument, context, [])
                assert result.price == engine.price(instrument, context)
                assert result.requested == (())
                assert result.status("vega") == "not_requested"
                with pytest.raises(InvalidInputError):
                    engine.price_with_greeks(instrument, context, [], spot_shift=0)
                if name not in HESTON_ENGINES:
                    scalar = pricing.numerical_greeks(engine, instrument, context, [])
                    assert scalar.price == result.price
                    assert scalar.requested == (())


def test_every_exported_engine_has_an_accepted_product(base_cases, base_engines):
    assert set(base_engines) == ({name for _, names, _ in base_cases for name in names})


def test_heston_model_is_rejected_by_other_engines_even_at_expiry(
    base_cases, base_engines, context_for, subtests
):
    context = context_for("AnalyticHestonEuropeanEngine")
    for instrument, supported, _ in base_cases:
        for name in supported:
            if name in HESTON_ENGINES:
                continue
            with subtests.test(instrument=type(instrument).__name__, engine=name):
                with pytest.raises(UnsupportedOperationError):
                    base_engines[name].price(instrument, context)
                with pytest.raises(UnsupportedOperationError):
                    base_engines[name].price_with_greeks(instrument, context, "delta")
                with pytest.raises(UnsupportedOperationError):
                    pricing.implied_volatility(
                        base_engines[name],
                        instrument,
                        context,
                        1,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )


@pytest.fixture
def context_for(base_context):
    def create(name):
        if name in HESTON_ENGINES:
            return base_context.replace(
                model_parameters=HestonParameters(
                    risk_free_rate=0.04,
                    dividend_yield=0.01,
                    initial_variance=0.04,
                    long_run_variance=0.04,
                    mean_reversion_speed=1.5,
                    vol_of_vol=0.3,
                    correlation=-0.7,
                )
            )
        return base_context

    return create


def test_scalar_operations_preserve_expired_contract_errors(
    base_cases, base_context, base_engines, context_for, subtests
):
    for instrument, supported, _ in base_cases:
        for name in supported:
            expired = context_for(name).replace(valuation_time=datetime(2026, 1, 2))
            engine = base_engines[name]
            with subtests.test(instrument=type(instrument).__name__, engine=name):
                with pytest.raises(InvalidInputError):
                    engine.price(instrument, expired)
                with pytest.raises(
                    UnsupportedOperationError
                    if name in HESTON_ENGINES
                    else InvalidInputError
                ):
                    pricing.implied_volatility(
                        engine,
                        instrument,
                        expired,
                        1,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )
    assert base_context.valuation_time == datetime(2026, 1, 1)


def test_solver_rejects_unregistered_engine_representations(
    base_cases, base_context, subtests
):
    instrument = base_cases[0][0]
    for engine in (
        None,
        True,
        100,
        "engine",
        object(),
        base_context,
        instrument,
        pricing.AnalyticEuropeanEngine,
    ):
        with subtests.test(engine=engine), pytest.raises(TypeError):
            pricing.implied_volatility(
                engine,
                instrument,
                base_context,
                1,
                lower_bound=0.0,
                upper_bound=4.0,
            )


def test_supported_terminal_prices_greeks_and_unidentifiable_volatility(
    base_cases, base_context, base_engines, context_for, subtests
):
    for instrument, supported, expected in base_cases:
        for name in supported:
            engine = base_engines[name]
            if name.startswith("MonteCarlo"):
                engine = engine.with_resolved_seed()
            context = context_for(name)
            with subtests.test(instrument=type(instrument).__name__, engine=name):
                assert_decimal_equal(engine.price(instrument, context), expected)
                request = ["delta"]
                result = engine.price_with_greeks(instrument, context, request)
                assert_decimal_equal(result.price, expected)
                if name in HESTON_ENGINES:
                    assert result.delta is None
                else:
                    assert result.delta == pytest.approx(
                        terminal_delta(instrument), rel=0, abs=1e-10
                    )
                assert result.requested == tuple(request)
                assert result.status("delta") == (
                    "unsupported" if name in HESTON_ENGINES else "available"
                )
                assert result.status("gamma") == "not_requested"
                with pytest.raises(UnsupportedOperationError) as error:
                    pricing.implied_volatility(
                        engine,
                        instrument,
                        base_context,
                        expected,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )
                assert type(error.value) is UnsupportedOperationError


def test_local_volatility_status_preserves_requests_across_fd_products(
    base_cases, base_context, base_engines, subtests
):
    local = base_context.replace(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.04,
            dividend_yield=0.01,
            surface=LocalVolatilitySurface(
                times=[datetime(2025, 1, 1), datetime(2026, 1, 1)],
                spots=[80, 120],
                values=[[0.2, 0.2], [0.2, 0.2]],
            ),
        )
    )
    for instrument, supported, _ in base_cases:
        for name in supported:
            if not name.startswith("FiniteDifference") or name in HESTON_ENGINES:
                continue
            with subtests.test(instrument=type(instrument).__name__, engine=name):
                result = base_engines[name].price_with_greeks(
                    instrument, local, ["delta", "vega"]
                )
                assert result.requested == (("delta", "vega"))
                assert result.supported == (("delta", "gamma"))
                assert result.status("delta") == "available"
                assert result.delta == pytest.approx(
                    terminal_delta(instrument), rel=0, abs=1e-10
                )
                assert result.status("vega") == "unsupported"
                assert result.status("gamma") == "not_requested"


def test_other_product_engine_pairs_raise_domain_errors(
    base_cases, base_context, base_engines, subtests
):
    for instrument, supported, _ in base_cases:
        for name, engine in base_engines.items():
            if name in supported:
                continue
            with subtests.test(instrument=type(instrument).__name__, engine=name):
                with pytest.raises(UnsupportedOperationError):
                    engine.price(instrument, base_context)
                with pytest.raises(UnsupportedOperationError):
                    engine.price_with_greeks(
                        instrument,
                        base_context,
                        ["delta"],
                    )
                with pytest.raises(UnsupportedOperationError):
                    pricing.implied_volatility(
                        engine,
                        instrument,
                        base_context,
                        1,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )


def test_invalid_instrument_representations_keep_native_errors(
    base_context, base_engines, subtests
):
    for name, engine in base_engines.items():
        for instrument in (
            None,
            True,
            100,
            "option",
            object(),
            base_context,
            engine,
        ):
            with subtests.test(engine=name, instrument=instrument):
                with pytest.raises(TypeError):
                    engine.price(instrument, base_context)
                with pytest.raises(TypeError):
                    engine.price_with_greeks(
                        instrument,
                        base_context,
                        ["delta"],
                    )
                with pytest.raises(TypeError):
                    pricing.implied_volatility(
                        engine,
                        instrument,
                        base_context,
                        1,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )


def test_autocallable_instrument_conversion_precedes_greek_name_validation(
    base_cases, base_context, base_engines, subtests
):
    for name, engine in base_engines.items():
        if not name.endswith("AutocallableEngine"):
            continue
        for instrument, expected_error in (
            (base_cases[0][0], UnsupportedOperationError),
            (object(), TypeError),
        ):
            with subtests.test(engine=name, instrument=type(instrument).__name__):
                with pytest.raises(expected_error) as error:
                    engine.price_with_greeks(
                        instrument,
                        base_context,
                        ["invalid"],
                    )
                assert type(error.value) is expected_error


def test_statistics_reject_unsupported_products_as_domain_errors(
    base_cases, base_context, base_engines, subtests
):
    products = {
        "MonteCarloVanillaEngine": (ins.EuropeanOption,),
        "MonteCarloAccumulatorEngine": (ins.Accumulator,),
        "MonteCarloAutocallableEngine": (
            ins.SnowballOption,
            ins.BinarySnowballOption,
            ins.TernarySnowballOption,
            ins.PhoenixOption,
        ),
    }
    for name, accepted in products.items():
        engine = base_engines[name]
        for instrument, _, _ in base_cases:
            if isinstance(instrument, accepted):
                continue
            with (
                subtests.test(engine=name, instrument=type(instrument).__name__),
                pytest.raises(UnsupportedOperationError),
            ):
                engine.price_with_statistics(instrument, base_context)
        with pytest.raises(TypeError):
            engine.price_with_statistics(object(), base_context)


def test_coupon_solver_rejects_unsupported_products_and_engines(
    base_cases, base_context, base_engines, subtests
):
    for solver, accepted, keywords in (
        (
            pricing.implied_snowball_coupon,
            (
                ins.SnowballOption,
                ins.BinarySnowballOption,
                ins.TernarySnowballOption,
            ),
            {"quote_convention": "preserve_maturity_coupon"},
        ),
        (pricing.implied_phoenix_coupon, (ins.PhoenixOption,), {}),
    ):
        note = next(
            product for product, _, _ in base_cases if isinstance(product, accepted)
        )
        for name, engine in base_engines.items():
            if name.endswith("AutocallableEngine"):
                for product, _, _ in base_cases:
                    if isinstance(product, accepted):
                        continue
                    with (
                        subtests.test(
                            solver=solver.__name__,
                            engine=name,
                            instrument=type(product).__name__,
                        ),
                        pytest.raises(UnsupportedOperationError),
                    ):
                        solver(engine, product, base_context, 1, **keywords)
            else:
                with (
                    subtests.test(solver=solver.__name__, engine=name),
                    pytest.raises(UnsupportedOperationError),
                ):
                    solver(engine, note, base_context, 1, **keywords)
