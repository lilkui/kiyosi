"""Independent high precision absorbing log-density / first-passage references.

Uses only Python's standard library. No Kiyosi or QuantLib prices enter the
reference. Image sums integrate the killed Brownian transition density against
cash, asset or vanilla terminal payoffs. First passage is the finite-horizon
Laplace transform of the inverse-Gaussian hitting density. See methodology.md#barriers-and-touches.
"""

import json
from decimal import Decimal as D
from decimal import localcontext
from pathlib import Path

HERE = Path(__file__).resolve().parent
PI = D(
    "3.1415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679"
)


def normal(x):
    """Normal CDF by integrated exponential Taylor series; bounded tail cutoff."""
    if abs(x) >= 16:
        return D(int(x > 0))  # Mills bound: omitted probability < 6.4e-58.
    term = x
    total = x
    for n in range(1, 2000):
        term *= -x * x / (2 * n)
        increment = term / (2 * n + 1)
        total += increment
        if abs(increment) < D("1e-90"):
            return D(".5") + total / (2 * PI).sqrt()
    raise ArithmeticError("normal series failed to converge")


def reference(case, images=8):
    p = {
        key: D(str(value)) if isinstance(value, (float, int)) else value
        for key, value in case["inputs"].items()
    }
    s, r, q, sigma = (p[k] for k in ("spot", "rate", "dividend", "volatility"))
    # All committed scenarios have exactly one Actual/365 Fixed year.
    assert p["valuation"] == "2025-01-01" and p["expiry"] == "2026-01-01"
    variance = sigma * sigma
    drift = r - q - variance / 2
    discount = (-r).exp()
    x = s.ln()
    strike = p.get("strike", s).ln()
    family = case["family"]

    def moment(mean, beta, lo, hi):
        shifted = mean + beta * variance
        upper = D(1) if hi is None else normal((hi - shifted) / sigma)
        lower = D(0) if lo is None else normal((lo - shifted) / sigma)
        return (beta * mean + beta * beta * variance / 2).exp() * (upper - lower)

    def full(beta, lo, hi):
        return moment(x + drift, beta, lo, hi)

    if family.startswith("double_"):
        lower, upper = p["lower"].ln(), p["upper"].ln()
        width = upper - lower

        def survive(beta, lo, hi):
            lo = lower if lo is None else max(lower, lo)
            hi = upper if hi is None else min(upper, hi)
            total = D(0)
            for n in range(-images, images + 1):
                for sign, mean in [
                    (1, x - 2 * n * width),
                    (-1, 2 * lower - x - 2 * n * width),
                ]:
                    weight = (drift * (mean - x) / variance).exp()
                    total += sign * weight * moment(mean + drift, beta, lo, hi)
            return total
    else:
        barrier = p["barrier"].ln()
        up = p["direction"] == "up"

        def survive(beta, lo, hi):
            lo = barrier if lo is None and not up else lo
            hi = barrier if hi is None and up else hi
            if not up and lo is not None:
                lo = max(barrier, lo)
            if up and hi is not None:
                hi = min(barrier, hi)
            if lo is not None and hi is not None and lo >= hi:
                return D(0)
            weight = (2 * drift * (barrier - x) / variance).exp()
            return full(beta, lo, hi) - weight * moment(
                2 * barrier - x + drift, beta, lo, hi
            )

        def first_passage():
            distance = abs(barrier - x)
            toward = drift if up else -drift
            lam = (toward * toward + 2 * r * variance).sqrt()
            return ((toward - lam) * distance / variance).exp() * normal(
                (lam - distance) / sigma
            ) + ((toward + lam) * distance / variance).exp() * normal(
                (-lam - distance) / sigma
            )

    def terminal(integral):
        if family in ("touch_bsm", "double_touch_bsm"):
            beta = int(p.get("payoff") == "asset")
            return integral(beta, None, None) * (1 if beta else p["payout"])
        lo, hi = (strike, None) if p["option"] == "call" else (None, strike)
        if family in ("single_barrier_bsm", "double_barrier_bsm"):
            value = integral(1, lo, hi) - p["strike"] * integral(0, lo, hi)
            return value if p["option"] == "call" else -value
        beta = int(p["payoff"] == "asset")
        return integral(beta, lo, hi) * (1 if beta else p["payout"])

    if family == "touch_bsm" and p["settlement"] == "hit":
        return first_passage() * (
            p["barrier"] if p["payoff"] == "asset" else p["payout"]
        )
    survived = terminal(survive)
    value = discount * (
        terminal(full) - survived if p["activation"] == "in" else survived
    )
    if family == "single_barrier_bsm":
        value += p["rebate"] * (
            discount * survive(0, None, None)
            if p["activation"] == "in"
            else first_passage()
        )
    return value


def main():
    cases = json.loads((HERE / "cases/barriers.json").read_text())["cases"]
    rows = []
    for case in cases:
        values = []
        for precision in (90, 110):
            with localcontext() as context:
                context.prec = precision
                values.append(reference(case))
        with localcontext() as context:
            context.prec = 110
            refined = reference(case, images=12)
        rows.append(
            {
                "id": case["id"],
                "value": float(refined),
                "uncertainty": 1e-12,
                "provenance": {
                    "method": "Integrated absorbing Brownian image density; first-passage Laplace transform for at-hit payments",
                    "source": "generate_barrier_references.py; methodology.md#barriers-and-touches",
                    "evidence": {
                        "decimal_precisions": [90, 110],
                        "values": [str(v) for v in values],
                        "precision_delta": float(abs(values[1] - values[0])),
                        "image_terms_each_side": [8, 12],
                        "refined_value": str(refined),
                        "image_refinement_delta": float(abs(refined - values[1])),
                        "normal_tail_cutoff": 16,
                        "normal_tail_bound": 6.4e-58,
                    },
                },
            }
        )
    (HERE / "references/barriers.json").write_text(
        json.dumps({"schema_version": 1, "cases": rows}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
