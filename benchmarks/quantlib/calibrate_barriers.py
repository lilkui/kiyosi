"""Record installed-native accuracy and exposed series refinement (no timing)."""

import json

from run import (
    DEFAULT_QUANTLIB_EXECUTABLE,
    HERE,
    NATIVE_TIMEOUT_SECONDS,
    accuracy,
    command,
    load_entries,
    native,
    prebuilt_rust_executable,
    write_report,
)


def main():
    cases = load_entries(HERE / "cases/barriers.json")
    references = load_entries(HERE / "references/barriers.json")
    executables = {
        "kiyosi": prebuilt_rust_executable(),
        "quantlib": DEFAULT_QUANTLIB_EXECUTABLE,
    }
    versions = {
        backend: json.loads(
            command([str(executable), "--info"], timeout=NATIVE_TIMEOUT_SECONDS)
        )
        for backend, executable in executables.items()
    }
    rows = []
    for case in cases.values():
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
                probe = case | {"profiles": {backend: profile}}
                price = native(executable, probe, backend, 1)["price"]
                check = accuracy(price, references[case["id"]], case["error_budget"])
                if not check["passed"]:
                    raise RuntimeError(
                        f"{case['id']}/{backend}: accuracy failed: {check}"
                    )
                refinements.append(
                    {
                        "profile": profile,
                        "price": price,
                        "absolute_error": check["absolute_error"],
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
    write_report(
        HERE / "calibration/barriers.json",
        {
            "schema_version": 1,
            "versions": versions,
            "cases": rows,
            "protocol": "Native public price requests after construction; QuantLib forced recalculate+NPV; 3 warmups, iterations=1; full 15-pair timing verified separately through test_barriers.py.",
        },
    )


if __name__ == "__main__":
    main()
