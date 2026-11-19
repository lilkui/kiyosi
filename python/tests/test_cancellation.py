"""Signals stop numerical requests and preserve Python exception semantics."""

import subprocess
import sys
import textwrap

import pytest


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
            "-c",
            textwrap.dedent("""
                        import signal
                        import threading
                        import sys
                        import time
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
                        delay = 0.001 if case in ("geometric_asian", "analytic_greeks", "numerical_greeks", "analytic_solver") else 0.05
                        ready = threading.Event()
                        def send_interrupt():
                            ready.wait()
                            time.sleep(delay)
                            signal.raise_signal(signal.SIGINT)
                        timer = threading.Thread(target=send_interrupt)
                        timer.start()
                        ready.set()
                        try:
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
                        except expected:
                            pass
                        else:
                            raise AssertionError("pricing returned instead of propagating the signal")
                        finally:
                            timer.join()
                        context = context.replace(model_parameters=model)
                        assert replay.price(vanilla, context) == baseline
                        if case == "fd_solution_batch":
                            assert saved.price(100) == saved_price

                        print("cancelled and replayed")
                    """),
            case,
        ],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert result.stdout.strip() == "cancelled and replayed"


@pytest.mark.parametrize(
    "case",
    (
        "schedule",
        "range_new",
        "range_replace",
        "curve_factors",
        "curve_shift",
        "surface",
    ),
)
def test_large_market_operations_release_gil_and_propagate_signals(case):
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            textwrap.dedent("""
                import signal
                import sys
                import threading
                from datetime import datetime, time, timedelta
                from kiyosi.instruments import RangeAccrualOption
                from kiyosi.market import (
                    LocalVolatilitySurface, RateTermStructure, all_days_calendar,
                    fixed_interval_schedule, weekdays_calendar,
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
                if case == "curve_factors":
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
                        operation()
                    except StopOperation:
                        pass
                    else:
                        raise AssertionError(f"{case} returned before processing the signal")
                finally:
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
                if case == "curve_factors":
                    factors = curve.factors
                    assert len(factors) > 1_000_000
                    assert 1.0 > factors[0] > factors[-1] > 0.0
                elif case == "curve_shift":
                    shifted = operation()
                    assert shifted.parallel_shift == 0.01
                    assert shifted.factor(start_time, shifted.max_time) < 1.0
                    assert curve.parallel_shift == 0.0
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
