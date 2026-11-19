"""Record predetermined MC path/time refinements without selecting seeds."""

import copy
import json
import time

from run import (
    HERE,
    RUST_EXECUTABLE,
    accuracy,
    build_evidence_path,
    command,
    load_entries,
    native,
    seed_ensemble,
)


def analytic_checks(cases, references, executables):
    """Check independent references with both native analytic counterparts."""
    rows = []
    for case in cases.values():
        analytic = {
            **case,
            "family": "european_bsm"
            if case["family"] == "mc_european_bsm"
            else "single_barrier_bsm",
            "profiles": {backend: {} for backend in executables},
        }
        for backend, executable in executables.items():
            batch = native(executable, analytic, backend, 1)
            check = accuracy(batch["price"], references[case["id"]], 1e-9)
            if not check["passed"]:
                raise ValueError(
                    f"analytic reference cross-check failed: {case['id']}/{backend}"
                )
            rows.append({"id": case["id"], "backend": backend, **check})
    return rows


def main():
    started = time.monotonic()
    cases = load_entries(HERE / "cases/monte_carlo.json")
    references = load_entries(HERE / "references/monte_carlo.json")
    executables = {
        "kiyosi": RUST_EXECUTABLE,
        "quantlib": HERE / "build/quantlib_bench",
    }
    build_evidence = json.loads(
        build_evidence_path(executables["quantlib"]).read_text()
    )
    libraries = {
        backend: json.loads(command([str(binary), "--info"]))
        for backend, binary in executables.items()
    }
    checks = analytic_checks(cases, references, executables)
    rows = []
    output = HERE / "calibration/monte_carlo.json"
    for case in cases.values():
        for backend, executable in executables.items():
            profiles = [{"path_count": paths} for paths in (65536, 131072, 262144)]
            if backend == "quantlib" and case["family"] == "mc_barrier_bsm":
                profiles.extend(
                    {"path_count": 262144, "step_count": steps} for steps in (2, 4, 8)
                )
            for refinement in profiles:
                experiment = copy.deepcopy(case)
                experiment["profiles"][backend].update(refinement)
                ensemble = seed_ensemble(
                    experiment, references[case["id"]], executable, backend
                )
                timing_batch = native(executable, experiment, backend, 1)
                repeat = native(executable, experiment, backend, 1)
                rows.append(
                    {
                        "id": case["id"],
                        "backend": backend,
                        "profile": experiment["profiles"][backend],
                        "seed_ensemble": ensemble,
                        "timing_seed_accuracy": accuracy(
                            timing_batch["price"],
                            references[case["id"]],
                            case["error_budget"],
                        ),
                        "repeatable": timing_batch["price"] == repeat["price"],
                    }
                )
                document = {
                    "schema_version": 1,
                    "build_evidence": build_evidence,
                    "libraries": libraries,
                    "validation_seed_policy": "1009+97*i, i0..31; fixed before observing replacement ensemble; timing1729 unchanged",
                    "prior_collision_evidence": "monte_carlo_seed_collision.json",
                    "protocol": "methodology.md#monte-carlo; budget and seeds declared before profile selection",
                    "references": references,
                    "analytic_cross_checks": checks,
                    "experiments": rows,
                    "wall_seconds": time.monotonic() - started,
                }
                output.write_text(
                    json.dumps(document, indent=2, allow_nan=False) + "\n"
                )
                print(
                    case["id"],
                    backend,
                    refinement,
                    "RMSE",
                    ensemble["rmse"],
                    "bias",
                    ensemble["signed_bias"],
                    "SD",
                    ensemble["sample_stddev"],
                    "max",
                    ensemble["max_absolute_error"],
                    "pass",
                    ensemble["passed"],
                    flush=True,
                )

    document["selected_profiles"] = {
        case["id"]: case["profiles"]
        for case in load_entries(HERE / "cases/monte_carlo.json").values()
    }
    document["selection_rule"] = (
        "Smallest tested path count satisfying ensemble and unchanged timing-seed gates; one barrier interval"
    )
    output.write_text(json.dumps(document, indent=2, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
