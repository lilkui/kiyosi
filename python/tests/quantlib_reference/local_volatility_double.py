"""Independent local-volatility double-barrier path references.

Notes
-----
Run from this directory or by script path. This module never imports Kiyosi.
Each finest path is monitored on four nested grids, separating monitoring
differences from path evolution and sampling error. No Brownian-bridge
correction is assumed exact for local volatility.
"""

import argparse
import itertools
import json
import math

import QuantLib as ql
from local_volatility_paths import END, OBSERVATIONS, START, process

NAMES = (
    "call_out",
    "call_in",
    "put_out",
    "put_in",
    "touch_expiry",
    "no_touch",
    "touch_hit",
)
STRIDES = (1, 2, 4, 8)


def payoff(terminal, hit_time, maturity):
    discount = math.exp(-0.04 * maturity)
    call = max(terminal - 100.0, 0.0) * discount
    put = max(100.0 - terminal, 0.0) * discount
    cash = 10.0 * discount
    if hit_time is None:
        return (call, 0.0, put, 0.0, 0.0, cash, 0.0)
    return (0.0, call, 0.0, put, cash, 0.0, 10.0 * math.exp(-0.04 * hit_time))


def estimate(pairs, substeps, seed=947):
    maturity = (END - START).days / 365
    required = [(day - START).days / 365 for day in OBSERVATIONS]
    grid = ql.TimeGrid(required, 90 * substeps)
    indices = {time: index for index, time in enumerate(grid)}
    observation_indices = [indices[time] for time in required]
    times = list(grid)
    sequence = ql.GaussianRandomSequenceGenerator(
        ql.UniformRandomSequenceGenerator(
            len(grid) - 1, ql.UniformRandomGenerator(seed)
        )
    )
    generator = ql.GaussianPathGenerator(process(), grid, sequence, False)

    def evaluate(path):
        hits = [None] * len(STRIDES)
        for index in range(1, len(grid)):
            if path[index] <= 85.0 or path[index] >= 115.0:
                for i, stride in enumerate(STRIDES):
                    if hits[i] is None and index % stride == 0:
                        hits[i] = times[index]
                if hits[-1] is not None:
                    break
        scheduled = next(
            (
                times[index]
                for index in observation_indices
                if path[index] <= 85.0 or path[index] >= 115.0
            ),
            None,
        )
        terminal = path.back()
        levels = [payoff(terminal, hit, maturity) for hit in [*hits, scheduled]]
        # Paired monitoring differences have much less noise than separate means.
        differences = [
            tuple(b - a for a, b in zip(levels[i], levels[i + 1], strict=True))
            for i in range(len(STRIDES) - 1)
        ]
        return [value for row in [*levels, *differences] for value in row]

    width = (len(STRIDES) * 2) * len(NAMES)
    sums, squares = [0.0] * width, [0.0] * width
    for _ in range(pairs):
        first = evaluate(generator.next().value())
        second = evaluate(generator.antithetic().value())
        for i, (a, b) in enumerate(zip(first, second, strict=True)):
            value = 0.5 * (a + b)
            sums[i] += value
            squares[i] += value * value
    labels = [*(f"continuous_{stride}" for stride in STRIDES), "scheduled"]
    labels += [f"monitoring_{a}_{b}" for a, b in itertools.pairwise(STRIDES)]
    results = {}
    for row, label in enumerate(labels):
        results[label] = {}
        for col, name in enumerate(NAMES):
            i = row * len(NAMES) + col
            results[label][name] = {
                "price": sums[i] / pairs,
                "standard_error": math.sqrt(
                    max(0.0, squares[i] - sums[i] * sums[i] / pairs)
                    / (pairs - 1)
                    / pairs
                ),
            }
    return {
        "quantlib_version": ql.__version__,
        "pairs": pairs,
        "seed": seed,
        "substeps_per_day": substeps,
        "time_steps": len(grid) - 1,
        "results": results,
    }


if __name__ == "__main__":
    assert payoff(101.0, None, 0.0) == (1.0, 0.0, 0.0, 0.0, 0.0, 10.0, 0.0)
    assert payoff(99.0, 0.0, 0.0) == (0.0, 0.0, 0.0, 1.0, 10.0, 0.0, 10.0)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pairs", type=int, default=32768)
    parser.add_argument("--substeps", type=int, default=16)
    parser.add_argument("--seed", type=int, default=947)
    args = parser.parse_args()
    if args.pairs < 2 or args.substeps < 1:
        parser.error("pairs must exceed one and substeps must be positive")
    print(json.dumps(estimate(args.pairs, args.substeps, args.seed), indent=2))
