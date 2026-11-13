"""Immutable value copying and transport through spawned Python processes."""

import copy
import inspect
import multiprocessing
import pickle
from concurrent.futures import ProcessPoolExecutor
from datetime import datetime
from datetime import time as clock_time

import pytest
from kiyosi import InvalidInputError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from kiyosi.instruments import KnockInObservation

START, END = datetime(2025, 1, 2), datetime(2025, 1, 10)


def price_in_worker(engine, option, context):
    return engine.price_with_greeks(option, context, "delta")


def values():
    life = {"effective_time": START, "expiry_time": END}
    vanilla = {"option_type": "call", "strike": 100, **life}
    curve = m.RateTermStructure(
        reference_time=START, times=[END], factors=[0.99]
    ).shifted(0.01)
    surface = m.LocalVolatilitySurface(
        times=[START, END], spots=[80, 120], values=[[0.2, 0.3], [0.3, 0.4]]
    )
    model = m.BlackScholesMertonParameters(
        risk_free_rate=curve, dividend_yield=0.02, volatility=0.2
    )
    calendar = m.custom_calendar(
        base=m.custom_calendar(base=m.sse_calendar(), closed_dates=[START.date()]),
        open_dates=[datetime(2025, 1, 4).date()],
        trading_days_per_year=260,
    )
    schedule = m.monthly_schedule(
        observation_clock_time=clock_time(),
        start_time=START,
        end_time=END,
        first_observation_month=1,
    )
    common = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 0,
        "autocall_observations": [
            i.AutocallableObservation.snowball(END, 110, coupon_rate=0)
        ],
        **life,
    }
    yield from (
        curve,
        m.RateTermStructure.flat(0.02),
        m.RateTermStructure.flat(-0.01, reference_time=START),
        surface,
        model,
        m.LocalVolatilityParameters(
            risk_free_rate=curve, dividend_yield=0, surface=surface
        ),
        m.HestonParameters(
            risk_free_rate=curve,
            dividend_yield=0,
            initial_variance=0.04,
            long_run_variance=0.04,
            mean_reversion_speed=1,
            vol_of_vol=0.3,
            correlation=-0.5,
        ),
        calendar,
        m.weekdays_calendar(),
        m.all_days_calendar(),
        schedule,
        m.fixed_interval_schedule(
            observation_clock_time=clock_time(),
            start_time=START,
            end_time=END,
            interval_days=2,
        ),
        m.PricingContext(
            model_parameters=model,
            spot_price=100,
            valuation_time=START,
        ),
        i.AutocallableObservation.snowball(END, 110, coupon_rate=0),
        i.EuropeanOption(**vanilla),
        i.AmericanOption(**vanilla),
        i.CashOrNothingOption(payout=10, **vanilla),
        i.AssetOrNothingOption(**vanilla),
        i.GeometricAveragePriceOption(averaging_start_time=START, **vanilla),
        i.ArithmeticAveragePriceOption(averaging_start_time=START, **vanilla),
        i.BarrierOption(
            barrier_level=120,
            barrier_type="up_and_out",
            observation_times=[END],
            touch_state="untouched",
            **vanilla,
        ),
        i.DoubleBarrierOption(
            lower_barrier=80, upper_barrier=120, barrier_type="knock_out", **vanilla
        ),
        i.RangeAccrualOption(
            observation_clock_time=clock_time(),
            notional=100,
            annual_coupon_rate=0.1,
            lower_bound=80,
            upper_bound=120,
            realized_hit_count=0,
            **life,
        ),
        i.Accumulator(
            observation_clock_time=clock_time(),
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            **life,
        ),
        i.Accumulator(
            knock_out_settled=False,
            observation_clock_time=clock_time(),
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
            knock_out_observation_times=[],
            **life,
        ),
        i.SnowballOption(
            calendar=m.weekdays_calendar(),
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            maturity_coupon_rate=0.1,
            **common,
        ),
        i.PhoenixOption(
            calendar=m.weekdays_calendar(),
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            coupon_per_period=0.1,
            **dict(
                common,
                autocall_observations=[
                    i.AutocallableObservation.phoenix(END, 110, coupon_barrier_level=0)
                ],
            ),
        ),
        i.PhoenixOption(
            calendar=m.weekdays_calendar(),
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            coupon_per_period=0.01,
            **dict(
                common,
                autocall_observations=[
                    i.AutocallableObservation.phoenix(END, 110, coupon_barrier_level=0)
                ],
            ),
        ),
        i.BinarySnowballOption(
            calendar=m.weekdays_calendar(),
            maturity_coupon_rate=0.1,
            autocall_observations=common["autocall_observations"],
            **life,
        ),
        i.TernarySnowballOption(
            calendar=m.weekdays_calendar(),
            knock_in_observation=KnockInObservation.every_trading_day(clock_time()),
            maturity_coupon_rate=0.1,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            autocall_observations=common["autocall_observations"],
            **life,
        ),
    )
    for factory, cash in (
        (i.cash_binary_barrier_option, True),
        (i.asset_binary_barrier_option, False),
    ):
        yield factory(
            barrier_level=120,
            barrier_type="up_and_out",
            **vanilla,
            **({"payout": 10} if cash else {}),
        )
    for cash in (True, False):
        for kind in ("up_and_in", "down_and_out"):
            if kind.endswith("_in"):
                factory = i.cash_touch_option if cash else i.asset_touch_option
            else:
                factory = i.cash_no_touch_option if cash else i.asset_no_touch_option
            yield factory(
                barrier_level=120,
                barrier_type=kind,
                **life,
                **({"payout": 10} if cash else {}),
            )
    for factory in (i.cash_double_touch, i.cash_double_no_touch):
        yield factory(lower_barrier=80, upper_barrier=120, payout=10, **life)


def test_owned_collection_properties_reject_silent_edits():
    surface = m.LocalVolatilitySurface(
        times=[START, END], spots=[80, 120], values=[[0.2, 0.3], [0.3, 0.4]]
    )
    with pytest.raises(TypeError):
        surface.values[0][0] = 0.5
    observation = i.AutocallableObservation.snowball(END, 110, coupon_rate=0.1)
    note = i.BinarySnowballOption(
        calendar=m.weekdays_calendar(),
        maturity_coupon_rate=0.1,
        effective_time=START,
        expiry_time=END,
        autocall_observations=[observation],
    )
    updated = observation.replace(coupon_rate=0.2)
    with pytest.raises(TypeError):
        note.autocall_observations[0] = updated
    editable = list(note.autocall_observations)
    editable[0] = updated
    replacement = note.replace(autocall_observations=editable)
    assert replacement.autocall_observations == ((updated,))
    assert note.autocall_observations == ((observation,))
    assert surface.values == (((0.2, 0.3), (0.3, 0.4)))


def test_results_compare_by_value_including_metadata_and_uncertainty():
    option = i.EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=110,
        valuation_time=END,
    )
    engine = p.AnalyticEuropeanEngine()
    result = engine.price_with_greeks(option, context, "delta")
    restored = pickle.loads(pickle.dumps(result))
    assert result is not restored
    assert result == restored
    assert result != engine.price_with_greeks(
        option.replace(strike=99), context, "delta"
    )
    assert result != engine.price_with_greeks(
        option.replace(option_type="put", strike=120),
        context,
        "delta",
    )
    theta = engine.price_with_greeks(option, context, "theta")
    assert theta != engine.price_with_greeks(option, context, "color")
    digital = i.CashOrNothingOption(
        option_type="call",
        strike=100,
        payout=10,
        effective_time=START,
        expiry_time=END,
    )
    unsupported = p.CoxRossRubinsteinDigitalEngine().price_with_greeks(
        digital, context, ["delta", "theta"]
    )
    assert theta.price == unsupported.price
    assert theta.theta is None
    assert unsupported.theta is None
    assert theta != unsupported
    estimate = p.MonteCarloVanillaEngine(path_count=32, seed=42).price_with_statistics(
        option, context
    )
    other = p.MonteCarloVanillaEngine(
        path_count=32, seed=42, sampling="sobol"
    ).price_with_statistics(option, context)
    assert estimate == pickle.loads(pickle.dumps(estimate))
    assert estimate.price == other.price
    assert estimate != other
    assert estimate != result
    assert result != object()


def test_values_round_trip_and_immutable_copies_keep_identity(subtests):
    for value in values():
        with subtests.test(value=type(value).__name__):
            assert copy.copy(value) is value
            assert copy.deepcopy(value) is value
            restored = pickle.loads(pickle.dumps(value))
            assert restored == value
            assert restored is not value
    for name in p.__all__:
        if name.endswith("Engine"):
            engine = getattr(p, name)()
            restored = pickle.loads(pickle.dumps(engine))
            assert type(restored) is type(engine)
            for field in inspect.signature(type(engine)).parameters:
                assert getattr(restored, field) == getattr(engine, field)


def test_repr_exposes_terms_settings_and_results(subtests):
    for value in values():
        with subtests.test(value=type(value).__name__):
            text = repr(value)
            assert type(value).__name__ in text
            assert "object at 0x" not in text
            assert repr(pickle.loads(pickle.dumps(value))) == text
    vanilla = i.EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    assert "strike=100.0" in repr(vanilla)
    assert "option_type='call'" in repr(vanilla)
    for name in p.__all__:
        if name.endswith("Engine"):
            engine = getattr(p, name)()
            with subtests.test(engine=name):
                text = repr(engine)
                assert name in text
                assert "object at 0x" not in text
                assert repr(pickle.loads(pickle.dumps(engine))) == text
    assert "asset_step_count=123" in repr(
        p.FiniteDifferenceVanillaEngine(asset_step_count=123)
    )
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=START,
    )
    assert "spot_price=100.0" in repr(context)
    result = p.AnalyticEuropeanEngine().price_with_greeks(vanilla, context, "delta")
    assert "price=" in repr(result)
    assert "'delta':" in repr(result)
    assert repr(pickle.loads(pickle.dumps(result))) == repr(result)
    estimate = p.MonteCarloVanillaEngine(
        path_count=16, seed=42, max_active_workers=1
    ).price_with_statistics(vanilla, context)
    assert "standard_error=" in repr(estimate)
    assert repr(pickle.loads(pickle.dumps(estimate))) == repr(estimate)


def test_repr_and_pickle_reuse_constructor_metadata(monkeypatch):
    from kiyosi import _serialization

    objects = (
        p.AnalyticEuropeanEngine(),
        i.EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        ),
        i.BinarySnowballOption(
            maturity_coupon_rate=0.1,
            autocall_observations=[
                i.AutocallableObservation.snowball(END, 110, coupon_rate=0.1)
            ],
            effective_time=START,
            expiry_time=END,
            calendar=m.weekdays_calendar(),
        ),
    )
    representations = [repr(value) for value in objects]

    def unexpected_signature(_factory):
        pytest.fail("constructor metadata was already resolved")

    monkeypatch.setattr(_serialization, "signature", unexpected_signature)
    for value, expected in zip(objects, representations, strict=True):
        assert repr(value) == expected
        restored = pickle.loads(pickle.dumps(value))
        assert restored == value
        assert repr(restored) == expected


def test_range_accrual_repr_identifies_observation_clock():
    morning = i.RangeAccrualOption(
        notional=1000,
        annual_coupon_rate=0.12,
        lower_bound=90,
        upper_bound=110,
        effective_time=START,
        expiry_time=END,
        observation_clock_time=clock_time(9, 0, 0, 123456),
    )
    afternoon = morning.replace(observation_clock_time=clock_time(15, 0, 0, 654321))
    for option in (morning, afternoon):
        assert (f"observation_clock_time={option.observation_clock_time!r}") in repr(
            option
        )
    assert repr(morning) != repr(afternoon)


def test_repr_bounds_large_surfaces_and_handles_extreme_curve_shifts():
    from datetime import timedelta

    surface = m.LocalVolatilitySurface(
        times=[START + timedelta(days=n) for n in range(20)],
        spots=list(range(1, 21)),
        values=[[0.2] * 20 for _ in range(20)],
    )
    assert len(repr(surface)) < 2000
    assert "..." in repr(surface)
    curve = m.RateTermStructure(
        reference_time=START, times=[END], factors=[0.99]
    ).shifted(-1e6)
    assert ".shifted(-1000000.0)" in repr(curve)
    assert repr(pickle.loads(pickle.dumps(curve))) == repr(curve)


def test_repr_memory_is_bounded_for_surfaces_contexts_and_contract_calendars(subtests):
    import tracemalloc
    from datetime import timedelta

    surface = m.LocalVolatilitySurface(
        times=[START + timedelta(days=n) for n in range(500)],
        spots=list(range(1, 501)),
        values=[[0.2] * 500 for _ in range(500)],
    )
    model = m.LocalVolatilityParameters(
        risk_free_rate=0.05, dividend_yield=0.02, surface=surface
    )
    context = m.PricingContext(
        model_parameters=model, spot_price=100, valuation_time=START
    )
    calendar = m.custom_calendar(
        base=m.weekdays_calendar(),
        closed_dates=[(START - timedelta(days=n + 1)).date() for n in range(5000)],
    )
    contracts = [
        value.replace(calendar=calendar)
        for value in values()
        if isinstance(
            value,
            (
                i.Accumulator,
                i.RangeAccrualOption,
                i.SnowballOption,
                i.BinarySnowballOption,
                i.TernarySnowballOption,
                i.PhoenixOption,
            ),
        )
    ]
    for value in (surface, model, context, *contracts):
        with subtests.test(value=type(value).__name__):
            tracemalloc.start()
            try:
                text = repr(value)
                _, peak = tracemalloc.get_traced_memory()
            finally:
                tracemalloc.stop()
            assert "..." in text
            assert peak < 256 * 1024


def test_repr_bounds_large_contract_schedules(subtests):
    from datetime import timedelta

    times = [START + timedelta(days=n) for n in range(1000)]
    observations = [
        i.AutocallableObservation.snowball(time, 110, coupon_rate=0) for time in times
    ]
    for original in values():
        if isinstance(original, i.Accumulator):
            option = original.replace(
                expiry_time=times[-1],
                knock_out_observation_times=times,
                accumulated_quantity=original.accumulated_quantity,
                knock_out_settled=original.knock_out_settled,
                calendar=m.all_days_calendar(),
            )
        elif isinstance(
            original,
            (
                i.SnowballOption,
                i.BinarySnowballOption,
                i.TernarySnowballOption,
                i.PhoenixOption,
            ),
        ):
            option = original.replace(
                expiry_time=times[-1],
                autocall_observations=[
                    i.AutocallableObservation.phoenix(time, 110, coupon_barrier_level=0)
                    for time in times
                ]
                if isinstance(original, i.PhoenixOption)
                else observations,
                barrier_state=original.barrier_state,
                calendar=m.all_days_calendar(),
            )
        else:
            continue
        with subtests.test(contract=type(option).__name__):
            text = repr(option)
            assert type(option).__name__ in text
            assert "..." in text
            assert len(text) < 2000
            assert pickle.loads(pickle.dumps(option)) == option


def test_calendar_repr_identifies_base_and_overrides():
    from datetime import timedelta

    base = m.weekdays_calendar()
    calendar = m.custom_calendar(
        base=base,
        closed_dates=[START.date()],
        open_dates=[datetime(2025, 1, 4).date()],
    )
    assert "kind='weekdays'" in repr(base)
    assert "kind='custom'" in repr(calendar)
    assert "base=TradingCalendar" in repr(calendar)
    assert "closed_dates=[datetime.date(2025, 1, 2)]" in repr(calendar)
    assert "open_dates=[datetime.date(2025, 1, 4)]" in repr(calendar)
    assert repr(calendar) != repr(base)
    assert not (calendar.is_trading_day(START.date()))
    assert repr(pickle.loads(pickle.dumps(calendar))) == repr(calendar)
    large = m.custom_calendar(
        base=base,
        closed_dates=[START.date() + timedelta(days=n) for n in range(100)],
    )
    assert len(repr(large)) < 1000
    assert "..." in repr(large)


def test_nondefault_engine_settings_survive_serialization(subtests):
    cases = (
        (
            p.MonteCarloVanillaEngine,
            {
                "path_count": 128,
                "step_count": 4,
                "seed": 77,
                "max_active_workers": 1,
                "sampling": "sobol",
                "antithetic": True,
            },
        ),
        (
            p.FiniteDifferenceVanillaEngine,
            {
                "asset_step_count": 120,
                "time_step_count": 30,
                "scheme": "implicit_euler",
                "asset_upper_boundary": 500,
            },
        ),
    )
    for factory, fields in cases:
        engine = factory(**fields)
        restored = pickle.loads(pickle.dumps(engine))
        for name, expected in fields.items():
            with subtests.test(engine=factory.__name__, field=name):
                assert getattr(restored, name) == expected


def test_spawn_transports_contracts_contexts_engines_and_results():
    option = i.EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=END,
    )
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=START,
    )
    engine = p.AnalyticEuropeanEngine()
    expected = price_in_worker(engine, option, context)
    with ProcessPoolExecutor(
        max_workers=1, mp_context=multiprocessing.get_context("spawn")
    ) as pool:
        actual = pool.submit(price_in_worker, engine, option, context).result(
            timeout=30
        )
    assert actual.price == expected.price
    assert actual.delta == expected.delta
    assert actual.gamma is None
    estimate = p.MonteCarloVanillaEngine(path_count=64, seed=42).price_with_statistics(
        option, context
    )
    restored = pickle.loads(pickle.dumps(estimate))
    assert (
        (restored.price, restored.standard_error, restored.confidence_interval)
    ) == ((estimate.price, estimate.standard_error, estimate.confidence_interval))


def test_reconstruction_retains_domain_validation():
    from kiyosi._serialization import _restore

    with pytest.raises(InvalidInputError):
        _restore(
            i.EuropeanOption,
            {
                "option_type": "call",
                "strike": -1,
                "effective_time": START,
                "expiry_time": END,
            },
        )


def test_estimate_reconstruction_validates_sampling_seeds(subtests):
    from kiyosi import _native

    fields = {"price": 10, "standard_error": 0, "uncertainty_status": "available"}
    for seed in (None, 0, 2**64 - 1):
        estimate = _native._monte_carlo_estimate(**fields, seed=seed)
        assert estimate.seed == seed
        assert pickle.loads(pickle.dumps(estimate)) == estimate
    for seed in (True, 1.5, "42"):
        with subtests.test(seed=seed), pytest.raises(TypeError):
            _native._monte_carlo_estimate(**fields, seed=seed)
    for seed in (-1, 2**64):
        with subtests.test(seed=seed), pytest.raises(OverflowError):
            _native._monte_carlo_estimate(**fields, seed=seed)
