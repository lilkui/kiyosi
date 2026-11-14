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
        "custom_handler",
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
                            engine = p.MonteCarloAsianEngine(path_count=10_000_000, step_count=256,
                                seed=42, max_active_workers=2 if case == "parallel_mc" else 1)
                        elif case == "tree":
                            option = i.AmericanOption(option_type="put", strike=100,
                                effective_time=start, expiry_time=end)
                            engine = p.CoxRossRubinsteinVanillaEngine(step_count=100_000)
                        elif case == "fd":
                            engine = p.FiniteDifferenceVanillaEngine(asset_step_count=4000, time_step_count=100_000)
                        elif case == "quadrature":
                            engine = p.QuadratureVanillaEngine(asset_step_count=100_000_000)
                        elif case in ("analytic_greeks", "numerical_greeks", "analytic_solver"):
                            count = 1_000_000
                            times = [start + timedelta(seconds=j) for j in range(1, count + 1)]
                            factors = [exp(-0.05 * j / (365 * 86400)) for j in range(1, count + 1)]
                            curve = m.RateTermStructure(reference_time=start,
                                times=[*times, end], factors=[*factors, exp(-0.05)])
                            context = context.replace(model_parameters=model.replace(
                                risk_free_rate=curve, volatility=0 if case != "analytic_solver" else 0.2))
                            engine = p.AnalyticEuropeanEngine()
                        else:
                            context = context.replace(model_parameters=m.HestonParameters(
                                risk_free_rate=0.05, dividend_yield=0.02, initial_variance=0.04,
                                long_run_variance=0.04, mean_reversion_speed=1.5,
                                vol_of_vol=0.3, correlation=-0.7))
                            engine = p.FiniteDifferenceHestonVanillaEngine(asset_step_count=1000,
                                variance_step_count=200, time_step_count=100_000)
                        class StopCalculation(Exception):
                            pass
                        if case == "custom_handler":
                            def stop(*args):
                                raise StopCalculation("requested stop")
                            signal.signal(signal.SIGINT, stop)
                        expected = StopCalculation if case == "custom_handler" else KeyboardInterrupt
                        delay = 0.001 if case in ("analytic_greeks", "numerical_greeks", "analytic_solver") else 0.05
                        timer = threading.Timer(delay, lambda: signal.raise_signal(signal.SIGINT))
                        timer.start()
                        try:
                            if case == "seeded_mc":
                                engine.price_with_seed(option, context)
                            elif case == "analytic_greeks":
                                engine.price_with_greeks(option, context, "all")
                            elif case == "numerical_greeks":
                                p.numerical_greeks(engine, option, context, "all")
                            elif case == "analytic_solver":
                                p.implied_volatility(engine, option, context, 10,
                                    lower_bound=0.01, upper_bound=1)
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
