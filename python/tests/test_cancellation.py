"""Signals stop numerical requests and preserve Python exception semantics."""

import signal
import subprocess
import sys
import textwrap
from contextlib import contextmanager

import pytest


class RequestInterrupted(Exception):
    pass


requires_cpu_timer = pytest.mark.skipif(
    not hasattr(signal, "ITIMER_VIRTUAL"), reason="requires a POSIX CPU timer"
)


@contextmanager
def cpu_interrupt():
    def stop(*_):
        raise RequestInterrupted

    def arm():
        # The timer counts time spent executing, even when CI deschedules the process.
        signal.setitimer(signal.ITIMER_VIRTUAL, 0.002)

    previous_timer = signal.setitimer(signal.ITIMER_VIRTUAL, 0)
    previous_handler = signal.signal(signal.SIGVTALRM, stop)
    try:
        yield arm
    finally:
        signal.setitimer(signal.ITIMER_VIRTUAL, 0)
        signal.signal(signal.SIGVTALRM, previous_handler)
        signal.setitimer(signal.ITIMER_VIRTUAL, *previous_timer)


@requires_cpu_timer
def test_surface_value_export_polls_signals_before_returning_the_tuple():
    from datetime import datetime, timedelta

    from kiyosi.market import LocalVolatilitySurface

    start = datetime(2025, 1, 1)
    size = 2048
    surface = LocalVolatilitySurface(
        times=[start + timedelta(seconds=i) for i in range(size)],
        spots=range(1, size + 1),
        values=[(0.2,) * size] * size,
    )

    exported = None
    with cpu_interrupt() as arm:
        arm()
        with pytest.raises(RequestInterrupted):
            exported = surface.values
    assert exported is None
    assert surface.volatility(start, 1) == 0.2


@requires_cpu_timer
def test_timestamp_export_polls_signals_before_returning_the_tuple():
    from datetime import datetime, time, timedelta

    from kiyosi.market import all_days_calendar, fixed_interval_schedule

    start = datetime(1, 1, 1)
    schedule = fixed_interval_schedule(
        start_time=start,
        end_time=start + timedelta(days=1_000_000),
        interval_days=1,
        observation_clock_time=time(),
        calendar=all_days_calendar(),
    )

    exported = None
    with cpu_interrupt() as arm:
        arm()
        with pytest.raises(RequestInterrupted):
            exported = schedule.times
    assert exported is None
    assert schedule[:2].times == (start + timedelta(days=1), start + timedelta(days=2))


@requires_cpu_timer
def test_greek_iterable_polls_signals_before_consuming_duplicates():
    from datetime import datetime
    from itertools import repeat
    from operator import length_hint

    from kiyosi.instruments import EuropeanOption
    from kiyosi.market import BlackScholesMertonParameters, PricingContext
    from kiyosi.pricing import AnalyticEuropeanEngine

    count = 50_000_000
    names = repeat("delta", count)

    class Names:
        def __iter__(self):
            arm()
            return names

    start = datetime(2025, 1, 1)
    contract = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=datetime(2026, 1, 1),
    )
    market = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    engine = AnalyticEuropeanEngine()
    expected = engine.price_with_greeks(contract, market, "delta")
    with cpu_interrupt() as arm, pytest.raises(RequestInterrupted):
        engine.price_with_greeks(contract, market, Names())
    assert length_hint(names) > 0
    assert engine.price_with_greeks(contract, market, ["delta", "delta"]) == expected


@requires_cpu_timer
def test_autocallable_observation_iterable_polls_signals_before_consuming_records():
    from datetime import datetime
    from itertools import repeat
    from operator import length_hint

    from kiyosi.instruments import (
        AutocallableObservation,
        KnockInObservation,
        SnowballOption,
    )
    from kiyosi.market import all_days_calendar

    start = datetime(2025, 1, 1)
    end = datetime(2026, 1, 1)
    record = AutocallableObservation.snowball(end, 110, coupon_rate=0.08)
    records = repeat(record, 2_000_000)

    class Observations:
        def __iter__(self):
            arm()
            return records

    note = SnowballOption(
        maturity_coupon_rate=0.06,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=60,
        effective_time=start,
        expiry_time=end,
        knock_in_observation=KnockInObservation.at_expiry(),
        autocall_observations=[record],
        calendar=all_days_calendar(),
    )
    with cpu_interrupt() as arm, pytest.raises(RequestInterrupted):
        note.replace(autocall_observations=Observations())
    assert length_hint(records) > 0
    assert note.autocall_observations == (record,)


@pytest.mark.parametrize(
    "case",
    (
        "serial_mc",
        "parallel_mc",
        "seeded_mc",
        "tree",
        "fd",
        "quadrature",
        "heston_fd",
        "fd_solution_solve",
        "fd_solution_batch",
        "custom_handler",
        "geometric_asian",
        "analytic_greeks",
        "numerical_greeks",
        "analytic_solver",
    ),
)
def test_signals_cancel_numerical_requests_without_poisoning_engines(case):
    result = subprocess.run(
        [
            sys.executable,
            "-E",
            "-c",
            textwrap.dedent("""
                        import signal
                        import threading
                        import sys
                        from datetime import datetime, timedelta
                        from math import exp
                        from kiyosi import instruments as i, market as m, pricing as p

                        case = sys.argv[1]
                        start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
                        option = i.EuropeanOption(option_type="call", strike=100,
                            effective_time=start, expiry_time=end)
                        model = m.BlackScholesMertonParameters(risk_free_rate=0.05,
                            dividend_yield=0.02, volatility=0.2)
                        context = m.PricingContext(model_parameters=model, spot_price=100, valuation_time=start)
                        vanilla = option
                        replay = p.MonteCarloVanillaEngine(path_count=128, seed=42, max_active_workers=2)
                        baseline = replay.price(vanilla, context)
                        if case in ("serial_mc", "parallel_mc", "seeded_mc", "custom_handler"):
                            option = i.ArithmeticAveragePriceOption(option_type="call", strike=100,
                                effective_time=start, averaging_start_time=start, expiry_time=end)
                            engine = p.MonteCarloAsianEngine(path_count=1_000_000, step_count=256,
                                seed=42, max_active_workers=2 if case == "parallel_mc" else 1)
                        elif case == "tree":
                            option = i.AmericanOption(option_type="put", strike=100,
                                effective_time=start, expiry_time=end)
                            engine = p.CoxRossRubinsteinVanillaEngine(step_count=16_000)
                        elif case == "fd":
                            engine = p.FiniteDifferenceVanillaEngine(asset_step_count=4000, time_step_count=100_000)
                        elif case == "fd_solution_solve":
                            engine = p.FiniteDifferenceVanillaEngine(asset_step_count=4000, time_step_count=100_000)
                        elif case == "fd_solution_batch":
                            engine = p.FiniteDifferenceVanillaEngine()
                            saved = engine.solve(option, context, spot_range=(80, 120))
                            saved_price = saved.price(100)
                            spots = [100.0] * 2_000_000
                        elif case == "quadrature":
                            engine = p.QuadratureVanillaEngine(asset_step_count=100_000_000)
                        elif case in ("geometric_asian", "analytic_greeks", "numerical_greeks", "analytic_solver"):
                            count = 1_000_000
                            times = [start + timedelta(seconds=j) for j in range(1, count + 1)]
                            factors = [exp(-0.05 * j / (365 * 86400)) for j in range(1, count + 1)]
                            curve = m.RateTermStructure(reference_time=start,
                                times=[*times, end], factors=[*factors, exp(-0.05)])
                            context = context.replace(model_parameters=model.replace(risk_free_rate=curve))
                            option = i.GeometricAveragePriceOption(option_type="call", strike=100,
                                effective_time=start, averaging_start_time=start, expiry_time=end)
                            engine = p.AnalyticGeometricAveragePriceEngine()
                        else:
                            context = context.replace(model_parameters=m.HestonParameters(
                                risk_free_rate=0.05, dividend_yield=0.02, initial_variance=0.04,
                                long_run_variance=0.04, mean_reversion_speed=1.5,
                                vol_of_vol=0.3, correlation=-0.7))
                            engine = p.FiniteDifferenceHestonVanillaEngine(asset_step_count=1000,
                                variance_step_count=200, time_step_count=2_000)
                        class StopCalculation(Exception):
                            pass
                        if case == "custom_handler":
                            def stop(*args):
                                raise StopCalculation("requested stop")
                            signal.signal(signal.SIGINT, stop)
                        expected = StopCalculation if case == "custom_handler" else KeyboardInterrupt
                        ready, start_request = threading.Event(), threading.Event()
                        def send_interrupt():
                            ready.set()
                            start_request.wait()
                            signal.raise_signal(signal.SIGINT)
                        timer = threading.Thread(target=send_interrupt)
                        timer.start()
                        ready.wait()
                        # The sender can acquire the GIL when the native request releases it.
                        interval = sys.getswitchinterval()
                        sys.setswitchinterval(100)
                        try:
                            start_request.set()
                            if case == "fd_solution_solve":
                                engine.solve(option, context, spot_range=(80, 120))
                            elif case == "fd_solution_batch":
                                saved.price_many(spots)
                            elif case == "seeded_mc":
                                engine.price_with_seed(option, context)
                            elif case in ("analytic_greeks", "tree"):
                                engine.price_with_greeks(option, context, "all")
                            elif case == "numerical_greeks":
                                p.numerical_greeks(engine, option, context, "all")
                            elif case == "analytic_solver":
                                p.implied_volatility(engine, option, context, 10,
                                    lower_bound=0, upper_bound=1)
                            else:
                                engine.price(option, context)
                        except expected as error:
                            assert type(error) is expected
                            if case == "custom_handler":
                                assert error.args == ("requested stop",)
                        else:
                            raise AssertionError("pricing returned instead of propagating the signal")
                        finally:
                            signal.signal(signal.SIGINT, signal.SIG_IGN)
                            sys.setswitchinterval(interval)
                            timer.join()

                        assert replay.price(vanilla, m.PricingContext(
                            model_parameters=model, spot_price=100, valuation_time=start,
                        )) == baseline
                        if case == "fd_solution_batch":
                            assert saved.price(100) == saved_price
                            assert saved.price_many([100]) == (saved_price,)
                        else:
                            # A terminal payoff exercises the interrupted engine with bounded work.
                            terminal = context.replace(
                                spot_price=90 if case == "tree" else 110,
                                valuation_time=end,
                            )
                            if case in ("serial_mc", "parallel_mc", "seeded_mc",
                                        "custom_handler", "geometric_asian", "analytic_greeks",
                                        "numerical_greeks", "analytic_solver"):
                                option = option.replace(averaging_start_time=end)
                            assert abs(engine.price(option, terminal) - 10.0) < 1e-12
                    """),
            case,
        ],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize(
    "case",
    (
        "schedule",
        "accumulation_schedule",
        "daily_knock_out_schedule",
        "explicit_knock_out_schedule",
        "daily_knock_in_schedule",
        "barrier_new",
        "barrier_replace",
        "range_new",
        "range_replace",
        "curve_new",
        "curve_factors",
        "curve_shift",
        "heston_saved",
        "accumulator_saved",
        "calendar_new",
        "calendar_layer",
        "surface",
    ),
)
def test_large_market_operations_release_gil_and_propagate_signals(case):
    result = subprocess.run(
        [
            sys.executable,
            "-E",
            "-c",
            textwrap.dedent("""
                import signal
                import sys
                import threading
                from datetime import datetime, time, timedelta
                from math import exp
                from time import process_time
                from kiyosi.instruments import (
                    Accumulator, AmericanOption, BarrierOption, KnockInObservation,
                    RangeAccrualOption, SnowballOption, autocallable_observations,
                )
                from kiyosi.market import (
                    BlackScholesMertonParameters, HestonParameters, LocalVolatilitySurface, PricingContext,
                    RateTermStructure, all_days_calendar, custom_calendar,
                    fixed_interval_schedule, weekdays_calendar,
                )
                from kiyosi.pricing import (
                    FiniteDifferenceAccumulatorEngine, FiniteDifferenceHestonVanillaEngine,
                )

                start_time = datetime(1, 1, 1)
                short_end = datetime(1, 1, 3)
                long_end = datetime(9999, 12, 31)
                calendar = all_days_calendar()
                terms = dict(
                    notional=100, annual_coupon_rate=0.1, lower_bound=90,
                    upper_bound=110, effective_time=start_time,
                    observation_clock_time=time(), calendar=calendar,
                )
                original = RangeAccrualOption(expiry_time=short_end, **terms)
                operations = {
                    "schedule": lambda: fixed_interval_schedule(
                        start_time=start_time, end_time=long_end, interval_days=1,
                        observation_clock_time=time(), calendar=calendar,
                    ),
                    "range_new": lambda: RangeAccrualOption(expiry_time=long_end, **terms),
                    "range_replace": lambda: original.replace(expiry_time=long_end),
                }
                case = sys.argv[1]
                if case in ("accumulation_schedule", "daily_knock_out_schedule", "explicit_knock_out_schedule"):
                    explicit = case == "explicit_knock_out_schedule"
                    count = 1_000_000
                    times = (start_time + timedelta(seconds=i) for i in range(count)) if explicit else None
                    accumulator = Accumulator(
                        strike=100, knock_out_level=120, daily_quantity=1, acceleration_factor=1,
                        effective_time=start_time, expiry_time=long_end, observation_clock_time=time(),
                        calendar=calendar, knock_out_observation_times=times,
                    )
                    if case == "accumulation_schedule":
                        operation = lambda: accumulator.accumulation_schedule
                    else:
                        operation = lambda: accumulator.knock_out_observation_schedule
                    schedule_size = count if explicit else (long_end - start_time).days + 1
                    schedule_last = start_time + timedelta(seconds=count - 1) if explicit else long_end
                elif case == "daily_knock_in_schedule":
                    note = SnowballOption(
                        maturity_coupon_rate=0.06, initial_spot=100, knock_in_level=80,
                        upper_strike=100, lower_strike=60, effective_time=start_time,
                        expiry_time=long_end, calendar=calendar,
                        knock_in_observation=KnockInObservation.every_trading_day(time()),
                        autocall_observations=autocallable_observations(
                            [long_end], [110], knock_out_coupon_rates=[0.08]),
                    )
                    operation = lambda: note.knock_in_observation_schedule
                    schedule_size = (long_end - start_time).days + 1
                    schedule_last = long_end
                elif case in ("barrier_new", "barrier_replace"):
                    count = 750_000
                    end = start_time + timedelta(days=count)
                    times = fixed_interval_schedule(
                        start_time=start_time, end_time=end, interval_days=1,
                        observation_clock_time=time(), calendar=calendar,
                    )
                    barrier_terms = dict(
                        option_type="call", strike=100, barrier_level=110,
                        barrier_type="up_and_out", effective_time=start_time,
                        expiry_time=end, observation_times=times,
                    )
                    if case == "barrier_new":
                        operation = lambda: BarrierOption(**barrier_terms)
                    else:
                        barrier = BarrierOption(**barrier_terms)
                        operation = lambda: barrier.replace(rebate=2)
                elif case == "curve_new":
                    count = 250_000
                    times = fixed_interval_schedule(
                        start_time=start_time, end_time=start_time + timedelta(days=count),
                        interval_days=1, observation_clock_time=time(), calendar=calendar,
                    )
                    factors = [0.99] * count
                    operation = lambda: RateTermStructure(
                        reference_time=start_time, times=times, factors=factors,
                    )
                elif case == "curve_factors":
                    curve = RateTermStructure.bounded_flat(
                        0.01, reference_time=start_time, max_time=long_end,
                        calendar=weekdays_calendar(),
                    )
                    operation = lambda: curve.factors
                elif case == "curve_shift":
                    count = 250_000
                    curve = RateTermStructure(
                        reference_time=start_time,
                        times=(start_time + timedelta(seconds=offset)
                               for offset in range(1, count + 1)),
                        factors=[1.0] * count,
                    )
                    operation = lambda: curve.shifted(0.01)
                elif case == "heston_saved":
                    count = 86_400
                    end = start_time + timedelta(days=1)
                    curve = RateTermStructure(
                        reference_time=start_time,
                        times=(start_time + timedelta(seconds=i) for i in range(1, count + 1)),
                        factors=[1.0] * count,
                    )
                    model = HestonParameters(
                        risk_free_rate=curve, dividend_yield=0, initial_variance=0,
                        long_run_variance=0, mean_reversion_speed=1.5, vol_of_vol=0.3,
                        correlation=-0.7,
                    )
                    context = PricingContext(
                        model_parameters=model, valuation_time=start_time, spot_price=100,
                    )
                    option = AmericanOption(
                        option_type="put", strike=100, effective_time=start_time, expiry_time=end,
                    )
                    saved = FiniteDifferenceHestonVanillaEngine().solve(
                        option, context, spot_range=(80, 120),
                    )
                    operation = lambda: saved.price_many([90.0] * 32)
                elif case == "accumulator_saved":
                    end = start_time + timedelta(days=1)
                    option = Accumulator(
                        strike=100, knock_out_level=200, daily_quantity=1,
                        acceleration_factor=1, effective_time=start_time, expiry_time=end,
                        observation_clock_time=time(), calendar=calendar,
                        knock_out_observation_times=(
                            start_time + timedelta(seconds=i) for i in range(1, 86_400)
                        ),
                    )
                    model = BlackScholesMertonParameters(
                        risk_free_rate=0, dividend_yield=0, volatility=0,
                    )
                    context = PricingContext(
                        model_parameters=model, valuation_time=start_time, spot_price=105,
                    )
                    saved = FiniteDifferenceAccumulatorEngine().solve(
                        option, context, spot_range=(100, 110),
                    )
                    operation = lambda: saved.price_many([105.0] * 32)
                elif case in ("calendar_new", "calendar_layer"):
                    closed = [start_time.date() + timedelta(days=i) for i in range(600_000)]
                    if case == "calendar_layer":
                        base = custom_calendar(base=calendar, closed_dates=closed)
                        operation = lambda: custom_calendar(base=base, open_dates=[closed[0]])
                    else:
                        operation = lambda: custom_calendar(base=calendar, closed_dates=closed)
                elif case == "surface":
                    times = tuple(start_time + timedelta(days=i) for i in range(1024))
                    spots = tuple(map(float, range(1, 1025)))
                    rows = ((0.2,) * len(spots),) * len(times)
                    operation = lambda: LocalVolatilitySurface(times=times, spots=spots, values=rows)
                else:
                    operation = operations[case]

                class StopOperation(Exception):
                    pass

                def stop(*args):
                    raise StopOperation

                signal.signal(signal.SIGINT, stop)
                ready, start = threading.Event(), threading.Event()

                def send_interrupt():
                    ready.set()
                    start.wait()
                    signal.raise_signal(signal.SIGINT)

                timer = threading.Thread(target=send_interrupt)
                timer.start()
                ready.wait()
                # Prevent incidental interpreter switches from masking a native call
                # that holds the GIL throughout.
                interval = sys.getswitchinterval()
                sys.setswitchinterval(100)
                try:
                    start.set()
                    try:
                        deadline = process_time() + 1
                        # A short request can finish before the sender is scheduled.
                        while process_time() < deadline:
                            operation()
                    except StopOperation:
                        pass
                    else:
                        raise AssertionError(f"{case} did not process the signal")
                finally:
                    signal.signal(signal.SIGINT, signal.SIG_IGN)
                    sys.setswitchinterval(interval)
                    timer.join()

                schedule = fixed_interval_schedule(
                    start_time=datetime(2025, 1, 1), end_time=datetime(2025, 1, 3),
                    interval_days=1, observation_clock_time=time(), calendar=all_days_calendar(),
                )
                assert schedule.times == (datetime(2025, 1, 2), datetime(2025, 1, 3))
                assert original.expiry_time == short_end
                replacement = original.replace(expiry_time=datetime(1, 1, 4))
                assert replacement.observation_schedule.times == tuple(
                    datetime(1, 1, day) for day in (2, 3, 4)
                )
                if case in ("barrier_new", "barrier_replace"):
                    created = operation()
                    assert len(created.observation_schedule) == count
                    assert created.calendar == calendar
                    if case == "barrier_replace":
                        assert created.rebate == 2
                        assert barrier.rebate == 0
                elif case in ("accumulation_schedule", "daily_knock_out_schedule",
                              "explicit_knock_out_schedule", "daily_knock_in_schedule"):
                    fresh = operation()
                    assert len(fresh) == schedule_size
                    assert fresh[0] == start_time and fresh[-1] == schedule_last
                elif case == "curve_new":
                    created = operation()
                    assert created.max_time == times[-1]
                    assert abs(created.factor(start_time, created.max_time) - factors[-1]) < 1e-15
                elif case == "curve_factors":
                    factors = curve.factors
                    assert len(factors) > 1_000_000
                    assert 1.0 > factors[0] > factors[-1] > 0.0
                elif case == "curve_shift":
                    shifted = operation()
                    assert shifted.parallel_shift == 0.01
                    expected = exp(-0.01 * count / (365 * 86400))
                    assert abs(shifted.factor(start_time, shifted.max_time) - expected) < 1e-15
                    assert curve.parallel_shift == 0.0
                elif case == "heston_saved":
                    values = operation()
                    assert len(values) == 32
                    assert all(abs(value - 10.0) < 1e-12 for value in values)
                elif case == "accumulator_saved":
                    assert operation() == (10.0,) * 32
                elif case in ("calendar_new", "calendar_layer"):
                    result = operation()
                    assert result.is_trading_day(closed[0]) == (case == "calendar_layer")
                    assert not result.is_trading_day(closed[-1])
                    if case == "calendar_layer":
                        assert not base.is_trading_day(closed[0])
                elif case == "surface":
                    surface = operation()
                    assert surface.volatility(times[100], 50.5) == 0.2
                """),
            case,
        ],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
