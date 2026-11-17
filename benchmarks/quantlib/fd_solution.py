"""Measure preparation, cached queries and repeated scalar FD pricing.

Run with the project's editable Python build, for example::

    python benchmarks/quantlib/fd_solution.py --counts 1 10 100 --repeats 3

Independent accuracy checks gate all reported timings. RSS is the process high-water mark, including
solver workspaces and allocator retention, not the size of the result alone.
"""

import argparse
import json
import statistics
import sys
from datetime import datetime, time
from time import perf_counter_ns

from kiyosi import instruments as ins
from kiyosi import market as m
from kiyosi import pricing as p


def cases():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    snapshot = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=start,
    )
    vanilla = ins.EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    yield "vanilla", p.FiniteDifferenceVanillaEngine(), vanilla, snapshot
    yield (
        "barrier",
        p.FiniteDifferenceBarrierEngine(),
        ins.BarrierOption(
            option_type="call",
            strike=100,
            effective_time=start,
            expiry_time=end,
            barrier_type="down_and_out",
            barrier_level=80,
        ),
        snapshot,
    )
    yield (
        "accumulator",
        p.FiniteDifferenceAccumulatorEngine(
            asset_step_count=6400, time_step_count=1600, asset_upper_boundary=400
        ),
        ins.Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            effective_time=start,
            expiry_time=datetime(2025, 2, 3),
            observation_clock_time=time(),
            accumulated_quantity=0,
            knock_out_settled=False,
        ),
        snapshot,
    )
    yield (
        "heston",
        p.FiniteDifferenceHestonVanillaEngine(),
        vanilla,
        snapshot.replace(
            model_parameters=m.HestonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                initial_variance=0.04,
                long_run_variance=0.04,
                mean_reversion_speed=1.5,
                vol_of_vol=0.3,
                correlation=-0.7,
            )
        ),
    )


def median_ns(operation, repeats):
    start = perf_counter_ns()
    operation()
    pilot = max(1, perf_counter_ns() - start)
    loops = max(1, 20_000_000 // pilot)
    samples = []
    for _ in range(repeats):
        start = perf_counter_ns()
        for _ in range(loops):
            operation()
        samples.append((perf_counter_ns() - start) / loops)
    return statistics.median(samples)


def peak_rss_bytes():
    try:
        import resource
    except ImportError:
        return None
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return rss if sys.platform == "darwin" else rss * 1024


def measure(name, engine, instrument, snapshot, counts, repeats):
    def prepare():
        return engine.solve(instrument, snapshot, spot_range=(80, 120))

    solution = prepare()
    reference = {
        "vanilla": p.AnalyticEuropeanEngine(),
        "barrier": p.AnalyticBarrierEngine(),
        "accumulator": p.QuadratureAccumulatorEngine(asset_step_count=4096),
        "heston": p.AnalyticHestonEuropeanEngine(),
    }[name]
    rows = []
    prepare_ns = median_ns(prepare, repeats)
    for count in counts:
        spots = (
            [100.0] if count == 1 else [80 + 40 * i / (count - 1) for i in range(count)]
        )
        contexts = [snapshot.replace(spot_price=spot) for spot in spots]
        expected = tuple(engine.price(instrument, value) for value in contexts)
        prices = solution.price_many(spots)
        error = max(
            abs(price - reference)
            for price, reference in zip(prices, expected, strict=True)
        )
        independent = tuple(reference.price(instrument, value) for value in contexts)
        reference_error = max(
            max(abs(saved - ref), abs(fresh - ref))
            for saved, fresh, ref in zip(prices, expected, independent, strict=True)
        )
        assert reference_error <= 0.005, (
            name,
            count,
            "independent accuracy budget exceeded",
            reference_error,
        )
        assert error <= 0.005, (
            name,
            count,
            "saved/scalar accuracy budget exceeded",
            error,
        )
        assert prices == tuple(solution.price(spot) for spot in spots)
        scalar_ns = median_ns(
            lambda contexts=contexts: tuple(
                engine.price(instrument, value) for value in contexts
            ),
            repeats,
        )
        query_ns = median_ns(
            lambda spots=spots: tuple(solution.price(spot) for spot in spots), repeats
        )
        batch_ns = median_ns(lambda spots=spots: solution.price_many(spots), repeats)
        rows.append(
            {
                "spots": count,
                "max_price_difference": error,
                "max_reference_error": reference_error,
                "repeated_scalar_ns": scalar_ns,
                "cached_scalar_ns": query_ns,
                "cached_batch_ns": batch_ns,
                "batch_query_speedup": scalar_ns / batch_ns,
                "prepare_and_batch_speedup": scalar_ns / (prepare_ns + batch_ns),
            }
        )
    return {
        "case": name,
        "prepare_ns": prepare_ns,
        "peak_process_rss_bytes": peak_rss_bytes(),
        "measurements": rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--counts", type=int, nargs="+", default=[1, 10, 100])
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if args.repeats < 1 or any(count < 1 for count in args.counts):
        parser.error("counts and repeats must be positive")
    for name, engine, instrument, snapshot in cases():
        print(
            json.dumps(
                measure(name, engine, instrument, snapshot, args.counts, args.repeats)
            ),
            flush=True,
        )


if __name__ == "__main__":
    main()
