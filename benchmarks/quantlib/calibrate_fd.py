"""Retain actual native per-dimension FD convergence, outside normal timing."""

import copy
import json
from pathlib import Path

import run

HERE = Path(__file__).resolve().parent


def domain_probes(case):
    """Build uniform-domain probes for barrier and continuation grid spacing.

    Parameters
    ----------
    case : dict
        Native benchmark case with its Kiyosi profile and product inputs.

    Returns
    -------
    dict
        Profile overrides preserving spacing in the named grid, relative to
        the explicit uniform baseline.
    """
    profile = case["profiles"]["kiyosi"]
    upper = profile["upper"] if profile["upper"] != "automatic" else 400
    lower, end, expanded_end = 0, upper, 600
    inputs = case["inputs"]
    if "barrier" in inputs:
        if inputs["barrier_type"].startswith("down"):
            lower = inputs["barrier"]
        else:
            end = expanded_end = inputs["barrier"]
    count = profile["space"] * (expanded_end - lower) / (end - lower)
    assert count.is_integer(), (case["id"], count)
    probes = {
        "explicit_uniform": {"upper": upper},
        "domain_constant_spacing": {"upper": 600, "space": int(count)},
    }
    if case.get("family") == "fd_local_vol" and inputs.get("barrier_type", "").endswith(
        "in"
    ):
        full_count = profile["space"] * 600 / upper
        assert full_count.is_integer(), (case["id"], full_count)
        probes["continuation_constant_spacing"] = {
            "upper": 600,
            "space": int(full_count),
        }
    return probes


def main():
    """Check declared budgets against independent references and refined grids."""
    cases = run.load_entries(HERE / "cases/fd_bsm.json")
    references = run.load_entries(HERE / "references/fd_bsm.json")
    executables = {
        "kiyosi": run.ROOT / "target/release/examples/quantlib_bench",
        "quantlib": HERE / "build/quantlib_bench",
    }
    evidence = {
        "schema_version": 1,
        "budgets_declared_before_profile_selection": 0.003,
        "quantlib_domain": "native automatic mesher, no public independent boundary control; Rust upper-bound enlargement supplies independent domain evidence",
        "kiyosi_domain": "domain_constant_spacing is compared with explicit_uniform; selected automatic meshes have a different node distribution",
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
