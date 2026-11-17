"""Generate independent analytic MC references with precision refinement."""

import json
from decimal import localcontext

from generate_barrier_references import reference
from generate_european_references import price
from run import HERE, load_entries


def main():
    rows = []
    for case in load_entries(HERE / "cases/monte_carlo.json").values():
        values = []
        for digits in (90, 110):
            with localcontext() as context:
                context.prec = digits
                if case["family"] == "mc_european_bsm":
                    values.append(price(case["inputs"], digits))
                else:
                    values.append(reference({**case, "family": "single_barrier_bsm"}))
        rows.append(
            {
                "id": case["id"],
                "value": float(values[-1]),
                "uncertainty": 1e-12,
                "provenance": {
                    "method": "Independent Decimal BSM closed form / absorbing Brownian image-density integral",
                    "source": "generate_mc_references.py; generate_european_references.py; generate_barrier_references.py; methodology.md#barriers-and-touches",
                    "evidence": {
                        "decimal_precisions": [90, 110],
                        "decimal_values": list(map(str, values)),
                        "precision_delta": str(abs(values[1] - values[0])),
                    },
                },
            }
        )
    (HERE / "references/monte_carlo.json").write_text(
        json.dumps({"schema_version": 1, "cases": rows}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
