"""Compare seeded CPU samplers at declared price-error targets.

Run with the freshly built kiyosi package on PYTHONPATH. The JSON retains all
seeded prices, reference convergence checks and complete-call timing samples.
"""

import argparse
import json
import math
import statistics
import time
from datetime import UTC, date, datetime
from pathlib import Path

SEEDS = tuple(range(16))
PATH_DEPENDENT_CASES = {"accumulator", "snowball", "phoenix", "snowball_daily_ki"}


def summarize(prices, reference, durations):
    errors = [price - reference for price in prices]
    return {
        "prices": prices,
        "bias": statistics.fmean(errors),
        "rmse": math.sqrt(statistics.fmean(error * error for error in errors)),
        "timings_seconds": durations,
        "total_seconds": statistics.median(durations),
    }


def compare_at_target(rows, target, reference_valid):
    selected = {}
    for sampling in ("pseudo_random", "sobol"):
        eligible = [
            row for row in rows if row["sampling"] == sampling and row["rmse"] <= target
        ]
        selected[sampling] = (
            min(eligible, key=lambda row: row["total_seconds"]) if eligible else None
        )
    baseline, sobol = selected["pseudo_random"], selected["sobol"]
    reduction = (
        1 - sobol["total_seconds"] / baseline["total_seconds"]
        if baseline is not None and sobol is not None
        else None
    )
    return {
        "target_rmse": target,
        "selected_path_counts": {
            method: row["path_count"] if row is not None else None
            for method, row in selected.items()
        },
        "runtime_reduction": reduction,
        "reference_valid": reference_valid,
        "passed": reference_valid
        and baseline is not None
        and sobol is not None
        and sobol["total_seconds"] <= 0.8 * baseline["total_seconds"],
    }


def performance_gate(comparisons):
    return (
        all(item.get("reference_valid", True) for item in comparisons.values())
        and comparisons.get("european", {}).get("passed", False)
        and any(
            comparisons.get(name, {}).get("passed", False)
            for name in PATH_DEPENDENT_CASES
        )
    )


def build_scenarios():
    from kiyosi import pricing
    from kiyosi.instruments import (
        Accumulator,
        AmericanOption,
        EuropeanOption,
        PhoenixOption,
        SnowballOption,
        autocallable_observations,
    )
    from kiyosi.market import (
        BlackScholesMertonParameters,
        PricingContext,
        all_days_calendar,
    )

    start, end = date(2025, 1, 1), date(2026, 1, 1)
    annual_context = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
        calendar=all_days_calendar(),
    )
    vanilla_terms = {"strike": 100, "effective_date": start, "expiry_date": end}
    scenarios = [
        {
            "name": "european",
            "option": EuropeanOption(option_type="call", **vanilla_terms),
            "context": annual_context,
            "engine": pricing.MonteCarloVanillaEngine,
            "settings": {},
            "target": 0.05,
            "references": [pricing.AnalyticVanillaEngine()],
        },
        {
            "name": "american",
            "option": AmericanOption(option_type="put", **vanilla_terms),
            "context": annual_context,
            "engine": pricing.MonteCarloVanillaEngine,
            "settings": {"step_count": 50},
            "target": 0.15,
            "references": [
                pricing.FiniteDifferenceVanillaEngine(
                    asset_step_count=nodes, time_step_count=nodes
                )
                for nodes in (800, 1600, 3200)
            ],
        },
    ]
    quarter_end = date(2025, 4, 1)
    market = PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0.01, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
        calendar=all_days_calendar(),
    )
    accumulator_market = market.replace(
        spot_price=100.25,
        valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
        model_parameters=market.model_parameters.replace(risk_free_rate=0.04),
    )
    scenarios.append(
        {
            "name": "accumulator",
            "option": Accumulator(
                strike=100,
                knock_out_level=110,
                daily_quantity=1,
                acceleration_factor=2,
                accumulated_quantity=3,
                effective_date=start,
                expiry_date=quarter_end,
            ),
            "context": accumulator_market,
            "engine": pricing.MonteCarloAccumulatorEngine,
            "settings": {},
            "target": 2.0,
            "references": [
                pricing.QuadratureAccumulatorEngine(asset_step_count=nodes)
                for nodes in (1024, 2048)
            ]
            + [
                pricing.FiniteDifferenceAccumulatorEngine(
                    asset_step_count=6400,
                    time_step_count=4000,
                    asset_upper_boundary=440,
                )
            ],
        }
    )
    dates = [date(2025, 2, 1), date(2025, 3, 1), quarter_end]
    terms = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 60,
        "barrier_state": "none",
        "effective_date": start,
        "expiry_date": quarter_end,
    }
    snowball = SnowballOption(
        **terms,
        maturity_coupon_rate=0.06,
        knock_in_observation_mode="at_expiry",
        observations=autocallable_observations(
            dates, [110, 108, 105], knock_out_coupon_rates=[0.08] * 3
        ),
    )
    phoenix = PhoenixOption(
        **terms,
        coupon_rate=0.08,
        knock_in_observation_mode="at_expiry",
        observations=autocallable_observations(
            dates, [110, 108, 105], coupon_barrier_levels=[95] * 3
        ),
    )
    for name, option in (
        ("snowball", snowball),
        ("phoenix", phoenix),
        (
            "snowball_daily_ki",
            SnowballOption(
                **terms,
                maturity_coupon_rate=0.06,
                knock_in_observation_mode="every_trading_day",
                observations=snowball.observations,
            ),
        ),
    ):
        scenarios.append(
            {
                "name": name,
                "option": option,
                "context": market,
                "engine": pricing.MonteCarloAutocallableEngine,
                "settings": {},
                "target": 1e-4,
                "references": [
                    pricing.QuadratureAutocallableEngine(
                        asset_step_count=nodes, integration_standard_deviations=width
                    )
                    for nodes, width in ((1024, 8), (2048, 8), (2048, 10))
                ]
                + [
                    pricing.FiniteDifferenceAutocallableEngine(
                        asset_step_count=2000, time_step_count=4000
                    )
                ],
            }
        )
    return scenarios


def measure(scenario, sampling, paths, threads, reference):
    def price(seed):
        engine = scenario["engine"](
            path_count=paths,
            seed=seed,
            thread_count=threads,
            sampling=sampling,
            **scenario["settings"],
        )
        return engine.price(scenario["option"], scenario["context"])

    price(SEEDS[0])
    durations = []
    first_prices = None
    for _ in range(3):
        started = time.perf_counter()
        prices = [price(seed) for seed in SEEDS]
        durations.append(time.perf_counter() - started)
        if first_prices is not None and prices != first_prices:
            raise RuntimeError("fixed seeds produced different prices on repeated runs")
        first_prices = prices
    return {
        "sampling": sampling,
        "path_count": paths,
        **summarize(first_prices, reference, durations),
    }


def run(paths, threads):
    import kiyosi

    results = {}
    for scenario in build_scenarios():
        reference_prices = [
            engine.price(scenario["option"], scenario["context"])
            for engine in scenario["references"]
        ]
        reference = reference_prices[-1]
        spread = max(reference_prices) - min(reference_prices)
        reference_valid = spread <= scenario["target"] / 10
        rows = [
            measure(scenario, sampling, count, threads, reference)
            for count in paths
            for sampling in ("pseudo_random", "sobol")
        ]
        comparison = compare_at_target(rows, scenario["target"], reference_valid)
        results[scenario["name"]] = {
            "instrument": repr(scenario["option"]),
            "context": repr(scenario["context"]),
            "reference_prices": reference_prices,
            "reference_engines": [repr(engine) for engine in scenario["references"]],
            "reference_spread": spread,
            "rows": rows,
            "comparison": comparison,
        }
        print(
            f"{scenario['name']}: reference_spread={spread:.6g}, "
            f"comparison={comparison}",
            flush=True,
        )
    return {
        "kiyosi_version": kiyosi.__version__,
        "seeds": SEEDS,
        "path_counts": paths,
        "thread_count": threads,
        "timing_repetitions": 3,
        "timing_scope": "engine construction and complete synchronous Python price call",
        "scenarios": results,
        "performance_gate_passed": performance_gate(
            {name: value["comparison"] for name, value in results.items()}
        ),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--paths", nargs="+", type=int, default=[4096, 32768])
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.threads < 0:
        parser.error("--threads must be non-negative")
    if any(
        count <= 0 or count > 10_000_000 or count & (count - 1) for count in args.paths
    ):
        parser.error("--paths must contain powers of two in 1..10,000,000")
    result = run(sorted(set(args.paths)), args.threads)
    serialized = json.dumps(result, indent=2, allow_nan=False)
    if args.output:
        args.output.write_text(serialized + "\n", encoding="utf-8")
    else:
        print(serialized)
    return 0 if result["performance_gate_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
