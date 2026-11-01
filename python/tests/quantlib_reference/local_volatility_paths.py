"""Independent QuantLib local-volatility paths and structured payoffs.

Regenerate from the repository root with:
python python/tests/quantlib_reference/local_volatility_paths.py --pairs 262144 --substeps 4
The result contains antithetic-pair standard errors, not FD acceptance budgets.
This module never imports Kiyosi.
"""

import argparse
import json
import math
from datetime import date, timedelta

import QuantLib as ql

START, END = date(2025, 1, 1), date(2025, 4, 1)
OBSERVATIONS = (date(2025, 2, 3), date(2025, 3, 3), END)
KNOCK_OUT_LEVELS = (110.0, 108.0, 105.0)
NAMES = (
    "accumulator",
    "snowball_daily",
    "snowball_terminal",
    "binary_snowball",
    "ternary_daily",
    "ternary_terminal",
    "phoenix_daily",
    "phoenix_terminal",
)


def process(spot=101.0):
    """Build an explicitly supplied surface, independent of Black-vol fallback."""
    start = ql.Date(1, 1, 2025)
    basis = ql.Actual365Fixed()
    matrix = ql.Matrix(3, 2)
    for i, sigma in enumerate((0.32, 0.24, 0.16)):
        matrix[i][0] = matrix[i][1] = sigma
    surface = ql.FixedLocalVolSurface(
        start, [0.0, 90 / 365], [60.0, 100.0, 160.0], matrix, basis
    )
    risk = ql.YieldTermStructureHandle(ql.FlatForward(start, 0.04, basis))
    dividend = ql.YieldTermStructureHandle(ql.FlatForward(start, 0.01, basis))
    black = ql.BlackVolTermStructureHandle(
        ql.BlackConstantVol(start, ql.NullCalendar(), 0.5, basis)
    )
    return ql.GeneralizedBlackScholesProcess(
        ql.QuoteHandle(ql.SimpleQuote(spot)),
        dividend,
        risk,
        black,
        ql.LocalVolTermStructureHandle(surface),
    )


def trading_dates():
    return [
        START + timedelta(days=day)
        for day in range(1, (END - START).days + 1)
        if (START + timedelta(days=day)).weekday() < 5
    ]


def events(grid):
    records = []
    indices = {time: index for index, time in enumerate(grid)}
    for day in trading_dates():
        time = (day - START).days / 365
        observation = OBSERVATIONS.index(day) if day in OBSERVATIONS else -1
        records.append((indices[time], time, math.exp(-0.04 * time), observation))
    return records


def payoffs(path, records, initial_spot=101.0):
    """Value independently specified contracts from monitored path values."""
    quantity = 3.0 + (2.0 if initial_spot < 100 else 1.0)
    accumulated = None
    knocked_in = initial_spot < 95
    note_values = None
    phoenix_coupons = 0.0
    previous_observation = 0.0
    for index, time, discount, observation in records:
        spot = path[index]
        if accumulated is None:
            if spot >= 110:
                accumulated = quantity * (spot - 100) * discount
            else:
                quantity += 2 if spot < 100 else 1
                if observation == 2:
                    accumulated = quantity * (spot - 100) * discount
        if note_values is not None:
            if accumulated is not None:
                break
            continue
        knocked_in |= spot < 95
        if observation < 0:
            continue
        if spot >= 95:
            phoenix_coupons += 0.08 * (time - previous_observation) * discount
        previous_observation = time
        if spot >= KNOCK_OUT_LEVELS[observation]:
            coupon_note = (1 + 0.08 * time) * discount
            phoenix = discount + phoenix_coupons
            note_values = [coupon_note] * 5 + [phoenix] * 2
        elif observation == 2:
            terminal_ki = spot < 95
            loss = min(max(100 - spot, 0), 40) / 100
            snow = [
                (1 - loss if ki else 1 + 0.06 * time) * discount
                for ki in (knocked_in, terminal_ki)
            ]
            ternary = [
                (1 + (0.01 if ki else 0.06) * time) * discount
                for ki in (knocked_in, terminal_ki)
            ]
            phoenix = [
                (1 - loss if ki else 1) * discount + phoenix_coupons
                for ki in (knocked_in, terminal_ki)
            ]
            note_values = snow + [(1 + 0.06 * time) * discount] + ternary + phoenix
    assert accumulated is not None and note_values is not None
    return [accumulated, *note_values]


def estimate(pairs, substeps, seed=1729, spot=101.0):
    """Report mean and sampling SE for independent antithetic pairs."""
    required = [(day - START).days / 365 for day in trading_dates()]
    grid = ql.TimeGrid(required, 90 * substeps)
    records = events(grid)
    sequence = ql.GaussianRandomSequenceGenerator(
        ql.UniformRandomSequenceGenerator(
            len(grid) - 1, ql.UniformRandomGenerator(seed)
        )
    )
    generator = ql.GaussianPathGenerator(process(spot), grid, sequence, False)
    sums = [0.0] * len(NAMES)
    squares = [0.0] * len(NAMES)
    products = [0.0] * len(NAMES)
    control_sum = control_square = 0.0
    for _ in range(pairs):
        path = generator.next().value()
        first = payoffs(path, records, spot)
        put = max(100 - path[len(grid) - 1], 0)
        path = generator.antithetic().value()
        second = payoffs(path, records, spot)
        control = (
            0.5 * (put + max(100 - path[len(grid) - 1], 0)) * math.exp(-0.04 * 90 / 365)
        )
        control_sum += control
        control_square += control * control
        for i, (a, b) in enumerate(zip(first, second, strict=True)):
            value = 0.5 * (a + b)
            sums[i] += value
            squares[i] += value * value
            products[i] += value * control
    start, end = ql.Date(1, 1, 2025), ql.Date(1, 4, 2025)
    ql.Settings.instance().evaluationDate = start
    put_option = ql.VanillaOption(
        ql.PlainVanillaPayoff(ql.Option.Put, 100), ql.EuropeanExercise(end)
    )
    put_prices = []
    for nodes in (1600, 3200):
        put_option.setPricingEngine(
            ql.FdBlackScholesVanillaEngine(
                process(spot), nodes, nodes, 2, ql.FdmSchemeDesc.CrankNicolson(), True
            )
        )
        put_prices.append(put_option.NPV())
    control_variance = control_square - control_sum * control_sum / pairs
    results = {}
    for name, total, square, product in zip(
        NAMES, sums, squares, products, strict=True
    ):
        covariance = product - total * control_sum / pairs
        beta = covariance / control_variance
        variance = max(0.0, square - total * total / pairs - covariance * beta)
        results[name] = {
            "price": total / pairs - beta * (control_sum / pairs - put_prices[-1]),
            "standard_error": math.sqrt(variance / (pairs - 2) / pairs),
            "raw_price": total / pairs,
            "control_beta": beta,
            "control_reference_error": abs(beta * (put_prices[1] - put_prices[0])),
        }
    return {
        "quantlib_version": ql.__version__,
        "pairs": pairs,
        "substeps_per_day": substeps,
        "time_steps": len(grid) - 1,
        "seed": seed,
        "spot": spot,
        "control_put_prices": put_prices,
        "results": results,
    }


def time_bias(pairs, substeps, seed=31415, spot=101.0):
    """Couple coarse/fine QL evolutions by summing pairs of Brownian increments."""
    coarse_steps = 90 * substeps
    step = 90 / 365 / coarse_steps
    grid = ql.TimeGrid([day / (365 * substeps) for day in range(1, coarse_steps + 1)])
    records = events(grid)
    sequence = ql.GaussianRandomSequenceGenerator(
        ql.UniformRandomSequenceGenerator(
            2 * coarse_steps, ql.UniformRandomGenerator(seed)
        )
    )
    model = process(spot)
    sums = [0.0] * len(NAMES)
    squares = [0.0] * len(NAMES)
    for _ in range(pairs):
        normals = sequence.nextSequence().value()
        paired = [0.0] * len(NAMES)
        for sign in (1, -1):
            coarse = [spot]
            fine = [spot]
            for i in range(coarse_steps):
                z1, z2 = sign * normals[2 * i], sign * normals[2 * i + 1]
                time = i * step
                coarse.append(
                    model.evolve(time, coarse[-1], step, (z1 + z2) / math.sqrt(2))
                )
                half = model.evolve(time, fine[-1], step / 2, z1)
                fine.append(model.evolve(time + step / 2, half, step / 2, z2))
            for i, (a, b) in enumerate(
                zip(
                    payoffs(coarse, records, spot),
                    payoffs(fine, records, spot),
                    strict=True,
                )
            ):
                paired[i] += 0.5 * (b - a)
        for i, difference in enumerate(paired):
            sums[i] += difference
            squares[i] += difference * difference
    return {
        "pairs": pairs,
        "coarse_substeps_per_day": substeps,
        "fine_substeps_per_day": 2 * substeps,
        "seed": seed,
        "results": {
            name: {
                "fine_minus_coarse": total / pairs,
                "standard_error": math.sqrt(
                    max(0.0, square - total * total / pairs) / (pairs - 1) / pairs
                ),
            }
            for name, total, square in zip(NAMES, sums, squares, strict=True)
        },
    }


def sensitivities(pairs, substeps, seed=1729, spot=101.0):
    """Measure two fixed-surface bump widths on shared Gaussian sequences."""
    required = [(day - START).days / 365 for day in trading_dates()]
    grid = ql.TimeGrid(required, 90 * substeps)
    records = events(grid)
    spots = [spot + shift for shift in (-2, -1, 0, 1, 2)]
    generators = [
        ql.GaussianPathGenerator(
            process(s),
            grid,
            ql.GaussianRandomSequenceGenerator(
                ql.UniformRandomSequenceGenerator(
                    len(grid) - 1, ql.UniformRandomGenerator(seed)
                )
            ),
            False,
        )
        for s in spots
    ]
    width = 4 * len(NAMES)
    sums, squares = [0.0] * width, [0.0] * width
    for _ in range(pairs):
        rows = []
        for generator, initial in zip(generators, spots, strict=True):
            first = payoffs(generator.next().value(), records, initial)
            last = payoffs(generator.antithetic().value(), records, initial)
            rows.append([(a + b) / 2 for a, b in zip(first, last, strict=True)])
        measured = []
        for h, left, right in [(1, 1, 3), (2, 0, 4)]:
            measured.extend(
                (b - a) / (2 * h) for a, b in zip(rows[left], rows[right], strict=True)
            )
            measured.extend(
                (a + b - 2 * center) / h**2
                for a, b, center in zip(rows[left], rows[right], rows[2], strict=True)
            )
        for i, value in enumerate(measured):
            sums[i] += value
            squares[i] += value * value
    labels = ("delta_1", "gamma_1", "delta_2", "gamma_2")
    return {
        "pairs": pairs,
        "substeps_per_day": substeps,
        "seed": seed,
        "spot": spot,
        "results": {
            label: {
                name: {
                    "value": sums[row * len(NAMES) + col] / pairs,
                    "standard_error": math.sqrt(
                        max(
                            0.0,
                            squares[row * len(NAMES) + col]
                            - sums[row * len(NAMES) + col] ** 2 / pairs,
                        )
                        / (pairs - 1)
                        / pairs
                    ),
                }
                for col, name in enumerate(NAMES)
            }
            for row, label in enumerate(labels)
        },
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pairs", type=int, default=262144)
    parser.add_argument("--substeps", type=int, default=4)
    parser.add_argument("--seed", type=int, default=1729)
    parser.add_argument("--spot", type=float, default=101.0)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--time-bias", action="store_true")
    mode.add_argument("--sensitivities", action="store_true")
    args = parser.parse_args()
    if args.pairs < 3 or args.substeps < 1:
        parser.error("pairs must be at least three and substeps must be positive")
    run = (
        sensitivities
        if args.sensitivities
        else time_bias
        if args.time_bias
        else estimate
    )
    print(json.dumps(run(args.pairs, args.substeps, args.seed, args.spot), indent=2))
