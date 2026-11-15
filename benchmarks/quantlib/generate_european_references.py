#!/usr/bin/env python3
"""Regenerate independent Decimal BSM references outside benchmark timing."""

import datetime
import json
from decimal import Decimal, localcontext
from pathlib import Path

HERE = Path(__file__).resolve().parent
PI = Decimal(
    "3.1415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679"
)


def normal_cdf(x):
    """Integrate the Gaussian density using its convergent power series."""
    term = x
    total = term
    n = 0
    while True:
        n += 1
        term *= -x * x / (2 * n)
        addition = term / (2 * n + 1)
        updated = total + addition
        if updated == total:
            return Decimal("0.5") + total / (2 * PI).sqrt()
        total = updated


def price(inputs, precision):
    """Evaluate the closed form independently of either pricing library."""
    with localcontext() as context:
        context.prec = precision
        s, k, r, q, sigma = (
            Decimal(str(inputs[name]))
            for name in ("spot", "strike", "rate", "dividend", "volatility")
        )
        days = (
            datetime.date.fromisoformat(inputs["expiry"])
            - datetime.date.fromisoformat(inputs["valuation"])
        ).days
        t = Decimal(days) / 365
        width = sigma * t.sqrt()
        d1 = ((s / k).ln() + (r - q + sigma * sigma / 2) * t) / width
        d2 = d1 - width
        direction = Decimal(1 if inputs["option"] == "call" else -1)
        return +(
            direction
            * (
                s * (-q * t).exp() * normal_cdf(direction * d1)
                - k * (-r * t).exp() * normal_cdf(direction * d2)
            )
        )


def main():
    """Write versioned reference evidence with a precision refinement check."""
    cases = json.loads((HERE / "cases/european.json").read_text())["cases"]
    references = []
    for case in cases:
        coarse = price(case["inputs"], 50)
        fine = price(case["inputs"], 80)
        references.append(
            {
                "id": case["id"],
                "value": float(fine),
                "uncertainty": 1e-13,
                "provenance": {
                    "method": "BSM closed form with independent Python Decimal Gaussian power-series integration",
                    "source": "benchmarks/quantlib/generate_european_references.py",
                    "version": 1,
                    "evidence": {
                        "precision_digits": [50, 80],
                        "native_calibration": "benchmarks/quantlib/calibration/european.json",
                        "decimal_value": str(fine),
                        "precision_difference": str(abs(fine - coarse)),
                        "uncertainty_policy": "1e-13 conservative absolute allowance for double conversion; budget 1e-9",
                    },
                },
            }
        )
    (HERE / "references/european.json").write_text(
        json.dumps({"schema_version": 1, "cases": references}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
