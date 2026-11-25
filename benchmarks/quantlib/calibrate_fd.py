"""Retain actual native per-dimension FD convergence, outside normal timing."""

import copy
import json
import math
from datetime import date
from pathlib import Path

import run

HERE = Path(__file__).resolve().parent


def domain_probes(case):
    """Expand the upper bound at approximately constant mean log spacing."""
    profile = case["profiles"]["kiyosi"]
    upper = profile["upper"] if profile["upper"] != "automatic" else 400
    probes = {}
    if profile["upper"] == "automatic":
        probes["explicit_boundary"] = {"upper": upper}
    inputs = case["inputs"]
    local = case["family"] == "fd_local_vol"
    direction = inputs.get("barrier_type", "")
    up = direction.startswith("up")
    continuation = local and direction.endswith("_in") and inputs["product"] != "touch"
    if up and not continuation:
        return probes
    years = (
        date.fromisoformat(inputs["expiry"]) - date.fromisoformat(inputs["valuation"])
    ).days / 365
    sigma = (
        max(
            value
            for key, value in inputs.items()
            if key.startswith(("surface_start_", "surface_end_"))
        )
        if local
        else inputs["volatility"]
    )
    spot = inputs["spot"]
    barrier = inputs.get("barrier")
    strike = inputs.get("strike", barrier)
    drift = (inputs["rate"] - inputs["dividend"] - sigma**2 / 2) * years
    margin = 1.5 * (4 * sigma * math.sqrt(years) + abs(drift))
    if barrier is None:
        # One-dimensional vanilla and digital engines use the same explicit domain policy.
        margin = math.log(4) if not local and margin < 2.5 else max(margin, math.log(4))
        level = strike
    else:
        margin = max(margin, math.log(2))
        level = min(strike, barrier)
    full_lower = min(level * math.exp(-margin), spot * math.exp(-0.25 * margin))
    expanded_upper = 600

    def expanded_steps(lower, old_end, new_end):
        return round(
            profile["space"] * math.log(new_end / lower) / math.log(old_end / lower)
        )

    lower = barrier if direction.startswith("down") else full_lower
    probes["domain_constant_spacing"] = {
        "upper": expanded_upper,
        "space": profile["space"]
        if up
        else expanded_steps(lower, upper, expanded_upper),
    }
    if continuation:
        probes["continuation_constant_spacing"] = {
            "upper": expanded_upper,
            "space": expanded_steps(full_lower, upper, expanded_upper),
        }
    return probes


def main():
    """Check declared budgets against independent references and refined grids."""
    cases = run.load_entries(HERE / "cases/fd_bsm.json")
    references = run.load_entries(HERE / "references/fd_bsm.json")
    executables = {
        "kiyosi": run.prebuilt_rust_executable(),
        "quantlib": HERE / "build/quantlib_bench",
    }
    evidence = {
        "schema_version": 1,
        "budgets_declared_before_profile_selection": 0.003,
        "quantlib_domain": "native automatic mesher, no public independent boundary control; Rust upper-bound enlargement supplies independent domain evidence",
        "kiyosi_domain": "domain_constant_spacing expands the explicit grid at constant mean log spacing, up to rounding. The explicit baseline is selected or explicit_boundary. Concentrated meshes may vary locally",
        "cases": [],
    }
    for case in cases.values():
        row = {"id": case["id"], "error_budget": case["error_budget"], "backends": {}}
        for backend, executable in executables.items():
            profile = case["profiles"][backend]
            variants = {
                "selected": {},
                "space": {"space": 2 * profile["space"]},
                "time": {"time": 2 * profile["time"]},
            }
            if backend == "kiyosi":
                variants.update(domain_probes(case))
            else:
                variants["damping"] = {"damping": 4}
            if backend == "quantlib" and profile["time"] > 400:
                variants["initial_time_400"] = {"time": 400}
            measurements = {}
            for name, changes in variants.items():
                probe = copy.deepcopy(case)
                probe["profiles"][backend].update(changes)
                price = run.native(executable, probe, backend, 1)["price"]
                measurements[name] = {
                    "profile": probe["profiles"][backend],
                    **run.accuracy(price, references[case["id"]], case["error_budget"]),
                }
            assert measurements["selected"]["passed"], (
                case["id"],
                backend,
                measurements,
            )
            row["backends"][backend] = measurements
        evidence["cases"].append(row)
        print(case["id"], flush=True)
    (HERE / "calibration/fd_bsm.json").write_text(json.dumps(evidence, indent=2) + "\n")


if __name__ == "__main__":
    main()
