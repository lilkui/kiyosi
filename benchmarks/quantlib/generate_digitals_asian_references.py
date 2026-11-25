#!/usr/bin/env python3
"""Evaluate independent high-precision digital and continuous Asian formulas."""

import datetime
import json
from decimal import Decimal, localcontext
from pathlib import Path

from generate_european_references import normal_cdf

HERE = Path(__file__).resolve().parent


def price(case, precision):
    """Evaluate BSM binary or continuous-average moments without either library."""
    inputs = case["inputs"]
    with localcontext() as context:
        context.prec = precision
        s, k, r, q, sigma = (
            Decimal(str(inputs[name]))
            for name in ("spot", "strike", "rate", "dividend", "volatility")
        )
        t = (
            Decimal(
                (
                    datetime.date.fromisoformat(inputs["expiry"])
                    - datetime.date.fromisoformat(inputs["valuation"])
                ).days
            )
            / 365
        )
        sign = Decimal(1 if inputs["option"] == "call" else -1)
        discount = (-r * t).exp()
        variance = sigma * sigma
        if case["family"] == "digital_bsm":
            width = sigma * t.sqrt()
            d2 = ((s / k).ln() + (r - q - variance / 2) * t) / width
            if inputs["payout"] == "cash":
                return +(
                    Decimal(str(inputs["cash"])) * discount * normal_cdf(sign * d2)
                )
            return +(s * (-q * t).exp() * normal_cdf(sign * (d2 + width)))
        if case["family"] == "geometric_asian_bsm":
            log_variance = variance * t / 3
            forward = s * ((r - q - variance / 2) * t / 2 + log_variance / 2).exp()
        else:
            b = r - q
            forward = s * ((b * t).exp() - 1) / (b * t)
            second = (
                2
                * s
                * s
                / ((b + variance) * t * t)
                * (
                    (((2 * b + variance) * t).exp() - 1) / (2 * b + variance)
                    - ((b * t).exp() - 1) / b
                )
            )
            log_variance = (second / (forward * forward)).ln()
        width = log_variance.sqrt()
        d1 = ((forward / k).ln() + log_variance / 2) / width
        return +(
            discount
            * sign
            * (forward * normal_cdf(sign * d1) - k * normal_cdf(sign * (d1 - width)))
        )


def main():
    """Write precision refinement and provenance outside measured benchmark work."""
    cases = json.loads((HERE / "cases/digitals_asian.json").read_text())["cases"]
    references = []
    for case in cases:
        coarse, fine = price(case, 50), price(case, 80)
        references.append(
            {
                "id": case["id"],
                "value": float(fine),
                "uncertainty": 1e-12,
                "provenance": {
                    "method": "Independent Decimal BSM binary/continuous geometric formula or arithmetic two-moment lognormal fit",
                    "source": "benchmarks/quantlib/generate_digitals_asian_references.py",
                    "version": 1,
                    "evidence": {
                        "precision_digits": [50, 80],
                        "decimal_value": str(fine),
                        "precision_difference": str(abs(fine - coarse)),
                        "uncertainty_policy": "1e-12 absolute allowance exceeds precision refinement and double rounding; budget 1e-9",
                    },
                },
            }
        )
    (HERE / "references/digitals_asian.json").write_text(
        json.dumps({"schema_version": 1, "cases": references}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
