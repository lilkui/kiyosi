"""Record LSM pricing-path, exercise-grid and calibration-path refinements."""

import copy
import json
import statistics
import time

from run import (
    HERE,
    RUST_EXECUTABLE,
    accuracy,
    command,
    load_entries,
    native,
    seed_ensemble,
)


def main():
    started = time.monotonic()
    case = load_entries(HERE / "cases/american_monte_carlo.json")["mc-american-put"]
    reference = load_entries(HERE / "references/american_monte_carlo.json")[case["id"]]
    executables = {
        "kiyosi": RUST_EXECUTABLE,
        "quantlib": HERE / "build/quantlib_bench",
    }
    document = {
        "schema_version": 1,
        "protocol": "methodology.md#american-least-squares-monte-carlo; limits and seeds declared before outputs",
        "build_evidence": json.loads((HERE / "build/build.json").read_text()),
        "libraries": {
            backend: json.loads(command([str(binary), "--info"]))
            for backend, binary in executables.items()
        },
        "reference": {key: reference[key] for key in ("value", "uncertainty")},
        "error_budget": case["error_budget"],
        "mc_validation": case["mc_validation"],
        "experiments": [],
        "development_timing": "Potentially contended by concurrent calibration; final suite measures quiet timing",
    }
    output = HERE / "calibration/american_monte_carlo.json"
    for backend, executable in executables.items():
        profiles = [
            ("baseline", {"path_count": 65536, "step_count": 50}),
            ("pricing_paths", {"path_count": 131072, "step_count": 50}),
            ("coarser_exercise", {"path_count": 131072, "step_count": 25}),
            ("finer_exercise", {"path_count": 131072, "step_count": 100}),
        ]
        if backend == "quantlib":
            profiles.append(
                (
                    "calibration_paths",
                    {
                        "path_count": 131072,
                        "step_count": 50,
                        "calibration_path_count": 65536,
                    },
                )
            )
        baseline = None
        for dimension, refinement in profiles:
            experiment = copy.deepcopy(case)
            experiment["profiles"][backend].update(refinement)
            ensemble = seed_ensemble(experiment, reference, executable, backend)
            timing = native(executable, experiment, backend, 1)
            row = {
                "backend": backend,
                "dimension": dimension,
                "profile": experiment["profiles"][backend],
                "seed_ensemble": ensemble,
                "timing_seed_accuracy": accuracy(
                    timing["price"], reference, case["error_budget"]
                ),
            }
            if dimension == "pricing_paths":
                baseline = ensemble
            if baseline is not None and dimension != "pricing_paths":
                deltas = [
                    fine["price"] - coarse["price"]
                    for fine, coarse in zip(
                        ensemble["samples"], baseline["samples"], strict=True
                    )
                ]
                row["difference_from_pricing_paths"] = {
                    "mean": statistics.mean(deltas),
                    "mean_standard_error": statistics.stdev(deltas)
                    / len(deltas) ** 0.5,
                    "maximum_absolute": max(map(abs, deltas)),
                }
            document["experiments"].append(row)
            document["wall_seconds"] = time.monotonic() - started
            output.write_text(json.dumps(document, indent=2, allow_nan=False) + "\n")
            print(
                backend,
                dimension,
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

    document["selected_profiles"] = case["profiles"]
    document["selection_rule"] = (
        "Smallest tested pricing path count passing all ensemble and unchanged timing-seed gates; "
        "50 exercise intervals and 32768 QuantLib calibration paths independently refined"
    )
    output.write_text(json.dumps(document, indent=2, allow_nan=False) + "\n")
    reference["provenance"]["lsm_refinement"] = {
        "source": "calibration/american_monte_carlo.json",
        "experiments": [
            {
                **{key: value for key, value in row.items() if key != "seed_ensemble"},
                "metrics": {
                    key: value
                    for key, value in row["seed_ensemble"].items()
                    if key != "samples"
                },
            }
            for row in document["experiments"]
        ],
    }
    (HERE / "references/american_monte_carlo.json").write_text(
        json.dumps(
            {"schema_version": 1, "cases": [reference]}, indent=2, allow_nan=False
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
