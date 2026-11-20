"""Compare FD pricing accuracy and latency across grid policies and builds."""

import argparse
import copy
import datetime
import json
import os
from pathlib import Path
from types import SimpleNamespace

import run

HERE = Path(__file__).resolve().parent


def main():
    """Record paired samples and check case error budgets and a 5% latency tolerance."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-executable", type=Path, required=True)
    parser.add_argument(
        "--updated-executable",
        type=Path,
        default=run.RUST_EXECUTABLE,
    )
    parser.add_argument(
        "--report", type=Path, default=HERE / "calibration/fd_accuracy.json"
    )
    args = parser.parse_args()
    cpu = min(os.sched_getaffinity(0))
    os.sched_setaffinity(0, {cpu})
    baseline, updated = (
        args.baseline_executable.resolve(),
        args.updated_executable.resolve(),
    )
    info = {
        key: json.loads(run.command([str(executable), "--info"]))
        for key, executable in [("kiyosi", baseline), ("quantlib", updated)]
    }
    evidence = {
        "schema_version": 1,
        "utc": datetime.datetime.now(datetime.UTC).isoformat(),
        "source_revision": run.command(["git", "rev-parse", "HEAD"]),
        "executables": {
            "baseline": run.fingerprint(baseline),
            "updated": run.fingerprint(updated),
        },
        "cpu_affinity": [cpu],
        "machine": run.optional_command(["lscpu"]),
        "protocol": "Cargo release, thin LTO; one thread; 3 warmups per process; 15 alternating pairs, adaptive >=20 ms batches; inclusive IQR; both executables are Kiyosi",
        "latency_tolerance": 0.05,
        "cases": [],
    }
    passed = True
    for family in ["fd_bsm", "fd_local_vol"]:
        cases = run.load_entries(HERE / "cases" / f"{family}.json")
        references = run.load_entries(HERE / "references" / f"{family}.json")
        for case in cases.values():
            if case["inputs"]["product"] not in ("vanilla", "barrier") or (
                case["inputs"].get("exercise") == "american"
            ):
                continue
            selected = case["profiles"]["kiyosi"]
            explicit = {**selected, "upper": 400}
            row = {"id": case["id"], "comparisons": {}}
            for name, before, after, executable in [
                ("call_policy", explicit, selected, baseline),
                ("core_explicit", explicit, explicit, updated),
                ("core_selected", selected, selected, updated),
            ]:
                probe = copy.deepcopy(case)
                # Both runner slots use Kiyosi executables for these comparisons.
                probe["profiles"] = {"kiyosi": before, "quantlib": after}
                measurement = run.measure(
                    probe,
                    references[case["id"]],
                    {"kiyosi": baseline, "quantlib": executable},
                    SimpleNamespace(price_only=False, target_ms=20),
                    info,
                )
                if measurement["status"] != "passed":
                    row["comparisons"][name] = {"failure": measurement}
                    passed = False
                    continue
                old, new = (measurement["backends"][key] for key in run.BACKENDS)
                ratio = new["timing"]["median_ns"] / old["timing"]["median_ns"]
                improved = (
                    new["accuracy"]["absolute_error"]
                    < old["accuracy"]["absolute_error"]
                )
                row["comparisons"][name] = {
                    "profiles": {"before": before, "after": after},
                    "reference": references[case["id"]],
                    "error_budget": case["error_budget"],
                    "before": old,
                    "after": new,
                    "latency_ratio": ratio,
                    "latency_passed": ratio <= 1.05,
                    "accuracy_improved": improved,
                    "sample_order": [
                        ["before", "after"] if i % 2 == 0 else ["after", "before"]
                        for i in range(15)
                    ],
                }
                requires_improvement = name == "core_explicit" or (
                    name == "call_policy" and before != after
                )
                passed &= ratio <= 1.05 and (not requires_improvement or improved)
            evidence["cases"].append(row)
            print(case["id"], flush=True)
    passed &= len(evidence["cases"]) == 4
    evidence["passed"] = passed
    args.report.write_text(json.dumps(evidence, indent=2) + "\n")
    if not passed:
        raise SystemExit(
            "FD accuracy/latency target failed; inspect saved paired samples"
        )


if __name__ == "__main__":
    main()
