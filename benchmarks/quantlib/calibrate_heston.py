"""Collect fixed-profile native Heston calibration outside ordinary timing.

Run generate_heston_references.py first and run.py to build both adapters.
Results include independent Rust integration/grid/domain refinements; QuantLib
reference refinements and automatic mesh bounds live in references/heston.json.
"""

import copy
import json
from pathlib import Path

import run

HERE = Path(__file__).resolve().parent


def main():
    """Persist every calibration price, setting and reference error."""
    cases = run.load_entries(HERE / "cases/heston.json")
    references = run.load_entries(HERE / "references/heston.json")
    records = []
    for case in cases.values():
        if case["family"] != "heston_analytic":
            case["domains"] = {
                "kiyosi": {
                    "asset": [0, case["profiles"]["kiyosi"]["asset_upper"]],
                    "variance": [0, case["profiles"]["kiyosi"]["variance_upper"]],
                }
            }
        for backend, executable in (
            ("kiyosi", run.RUST_EXECUTABLE),
            ("quantlib", HERE / "build/quantlib_bench"),
        ):
            variants = [("committed", {})]
            if case["family"] == "heston_analytic":
                variants.append(
                    (
                        "tight_integration",
                        {
                            "relative_tolerance": 1e-11,
                            **(
                                {"absolute_tolerance": 1e-11}
                                if backend == "kiyosi"
                                else {}
                            ),
                        },
                    )
                )
            elif backend == "kiyosi":
                variants.extend(
                    [
                        ("asset_refinement", {"asset_steps": 400}),
                        ("variance_refinement", {"variance_steps": 100}),
                        ("time_refinement", {"time_steps": 400}),
                        ("asset_domain", {"asset_upper": 800, "asset_steps": 400}),
                        (
                            "variance_domain",
                            {"variance_upper": 1.2, "variance_steps": 100},
                        ),
                    ]
                )
            else:
                variants.extend(
                    [
                        ("asset_refinement", {"asset_steps": 400}),
                        ("variance_refinement", {"variance_steps": 200}),
                        ("time_refinement", {"time_steps": 400}),
                    ]
                )
            for label, changes in variants:
                variant = copy.deepcopy(case)
                variant["profiles"][backend].update(changes)
                result = run.native(executable, variant, backend, 1)
                check = run.accuracy(
                    result["price"], references[case["id"]], case["error_budget"]
                )
                record = {
                    "id": case["id"],
                    "backend": backend,
                    "variant": label,
                    "profile": variant["profiles"][backend],
                    "accuracy": check,
                }
                if (
                    backend == "quantlib"
                    and label == "committed"
                    and case["family"] != "heston_analytic"
                ):
                    profile = variant["profiles"][backend]
                    parameters = {
                        "method": "fd",
                        "exercise": "american"
                        if case["family"] == "heston_fd_american"
                        else "european",
                        **{
                            key: profile[key]
                            for key in ("time_steps", "asset_steps", "variance_steps")
                        },
                    }
                    probe = json.loads(
                        run.command(
                            [
                                str(HERE / "build/heston_reference"),
                                *(
                                    f"{key}={value}"
                                    for key, value in parameters.items()
                                ),
                            ]
                        )
                    )
                    if abs(probe["price"] - result["price"]) > 1e-11:
                        raise RuntimeError(
                            "native reference probe and timed adapter disagree"
                        )
                    record["automatic_mesh"] = probe
                    case["domains"]["quantlib"] = {
                        "asset": probe["asset_domain"],
                        "variance": probe["variance_domain"],
                        "source": "native getSolverDesc(1.0), automatic mesher at committed profile",
                    }
                records.append(record)
                print(json.dumps(record), flush=True)
                if not check["passed"]:
                    raise RuntimeError(f"calibration failed: {record}")
    (HERE / "cases/heston.json").write_text(
        json.dumps({"schema_version": 1, "cases": list(cases.values())}, indent=2)
        + "\n"
    )
    output = HERE / "heston_calibration.json"
    output.write_text(
        json.dumps(
            {"schema_version": 1, "quantlib_version": "1.44", "records": records},
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
