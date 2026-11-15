"""Regenerate independent BSM FD references outside measured work."""

import copy
import json
from decimal import localcontext
from pathlib import Path

from generate_barrier_references import reference as barrier_price
from generate_digitals_asian_references import price as digital_price
from generate_european_references import price as vanilla_price

HERE = Path(__file__).resolve().parent


def analytic(case, precision):
    """Reuse independent closed forms and absorbing-density integrals."""
    case = copy.deepcopy(case)
    inputs = case["inputs"]
    product = inputs.pop("product")
    if product == "vanilla":
        return vanilla_price(inputs, precision)
    if product == "digital":
        case["family"] = "digital_bsm"
        return digital_price(case, precision)
    case["family"] = {
        "barrier": "single_barrier_bsm",
        "binary_barrier": "binary_barrier_bsm",
        "touch": "touch_bsm",
    }[product]
    inputs["direction"], inputs["activation"] = inputs.pop("barrier_type").split("_")
    if product == "barrier":
        inputs["rebate"] = 0
    else:
        inputs["payoff"] = inputs.pop("payout")
        if inputs["payoff"] == "cash":
            inputs["payout"] = inputs.pop("cash")
    if product == "touch":
        inputs["settlement"] = inputs["settlement"].removeprefix("at_")
    with localcontext() as context:
        context.prec = precision
        return barrier_price(case)


def main():
    """Retain precision refinement and independent American FD evidence."""
    rows = []
    for case in json.loads((HERE / "cases/fd_bsm.json").read_text())["cases"]:
        if case["inputs"].get("exercise") == "american":
            evidence = json.loads((HERE / "calibration/american.json").read_text())
            rows.append(
                {
                    "id": case["id"],
                    "value": evidence["quantlib_first_order_time_extrapolation"],
                    "uncertainty": 5e-5,
                    "provenance": {
                        "method": "Independent Rust/QuantLib FD space, time and domain refinement with first-order time extrapolation",
                        "source": "calibration/american.json; calibration/american_reference.cpp; crates/kiyosi/examples/quantlib_bench_references.rs",
                        "evidence": evidence,
                    },
                }
            )
        else:
            coarse, fine = analytic(case, 90), analytic(case, 110)
            rows.append(
                {
                    "id": case["id"],
                    "value": float(fine),
                    "uncertainty": 1e-12,
                    "provenance": {
                        "method": "Independent Decimal BSM closed form or absorbing log-density / first-passage integral",
                        "source": "generate_fd_references.py; generate_european_references.py; generate_digitals_asian_references.py; generate_barrier_references.py; barriers.md",
                        "evidence": {
                            "precisions": [90, 110],
                            "values": [str(coarse), str(fine)],
                            "precision_delta": str(abs(fine - coarse)),
                            "normal_tail_cutoff": 16,
                            "normal_tail_bound": 6.4e-58,
                        },
                    },
                }
            )
    (HERE / "references/fd_bsm.json").write_text(
        json.dumps({"schema_version": 1, "cases": rows}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
