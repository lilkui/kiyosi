"""Record installed-native accuracy and exposed series refinement (no timing)."""

import json
import subprocess

from run import HERE, prebuilt_rust_executable


def main():
    cases = json.loads((HERE / "cases/barriers.json").read_text())["cases"]
    references = {
        row["id"]: row
        for row in json.loads((HERE / "references/barriers.json").read_text())["cases"]
    }
    executables = {
        "kiyosi": prebuilt_rust_executable(),
        "quantlib": HERE / "build/quantlib_bench",
    }
    versions = {
        backend: json.loads(
            subprocess.check_output([str(executable), "--info"], text=True)
        )
        for backend, executable in executables.items()
    }
    rows = []
    for case in cases:
        evidence = {}
        for backend, executable in executables.items():
            refinements = []
            counts = (
                (3, 5, 10, 20)
                if backend == "quantlib" and case["family"] == "double_barrier_bsm"
                else (None,)
            )
            for count in counts:
                profile = dict(case["profiles"][backend])
                if count is not None:
                    profile["series"] = count
                parameters = dict(
                    family=case["family"], iterations=1, **case["inputs"], **profile
                )
                output = json.loads(
                    subprocess.check_output(
                        [
                            str(executable),
                            *(f"{key}={value}" for key, value in parameters.items()),
                        ],
                        text=True,
                    )
                )
                error = abs(output["price"] - references[case["id"]]["value"])
                assert (
                    error + references[case["id"]]["uncertainty"]
                    <= case["error_budget"]
                )
                refinements.append(
                    {
                        "profile": profile,
                        "price": output["price"],
                        "absolute_error": error,
                    }
                )
            evidence[backend] = refinements
        rows.append(
            {
                "id": case["id"],
                "error_budget": case["error_budget"],
                "backends": evidence,
            }
        )
    (HERE / "calibration/barriers.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "versions": versions,
                "cases": rows,
                "protocol": "Native public price requests after construction; QuantLib forced recalculate+NPV; 3 warmups, iterations=1; full 15-pair timing verified separately through test_barriers.py.",
            },
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
