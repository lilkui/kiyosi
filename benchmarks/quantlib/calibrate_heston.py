"""Collect fixed-profile native Heston calibration outside ordinary timing.

Run generate_heston_references.py first and run.py to build both adapters.
Results include independent Rust integration/grid/domain refinements; QuantLib
reference refinements and automatic mesh bounds live in references/heston.json.
"""

import copy
import json
import math
from datetime import date
from pathlib import Path

import run

HERE = Path(__file__).resolve().parent


def asset_lower(inputs):
    """Return the lower spot in a flat-curve Heston calibration scenario."""
    years = (
        date.fromisoformat(inputs["expiry"]) - date.fromisoformat(inputs["valuation"])
    ).days / 365
    v0 = inputs["initial_variance"]
    theta = inputs["long_run_variance"]
    kappa = inputs["mean_reversion_speed"]
    decay = -math.expm1(-kappa * years) / kappa if kappa else years
    variance = theta * years + (v0 - theta) * decay
    carry = (inputs["rate"] - inputs["dividend"]) * years
    width = max(math.sqrt(variance), 0.35 * inputs["vol_of_vol"] * years)
    margin = max(1e-3, max(0, -carry) + 0.5 * variance + 6 * width)
    return min(
        inputs["strike"] * math.exp(-margin),
        inputs["spot"] * math.exp(-0.25 * margin),
    )


def main():
    """Persist every calibration price, setting and reference error."""
    cases = run.load_entries(HERE / "cases/heston.json")
    references = run.load_entries(HERE / "references/heston.json")
    executables = {
        "kiyosi": run.prebuilt_rust_executable(),
        "quantlib": HERE / "build/quantlib_bench",
    }
    records = []
    for case in cases.values():
        if case["family"] != "heston_analytic":
            lower = asset_lower(case["inputs"])
            case["domains"] = {
                "kiyosi": {
                    "asset": [lower, case["profiles"]["kiyosi"]["asset_upper"]],
                    "variance": [0, case["profiles"]["kiyosi"]["variance_upper"]],
                }
            }
        for backend, executable in executables.items():
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
                profile = case["profiles"]["kiyosi"]
                lower = case["domains"]["kiyosi"]["asset"][0]
                expanded_steps = round(
                    profile["asset_steps"]
                    * math.log(800 / lower)
                    / math.log(profile["asset_upper"] / lower)
                )
                variants.extend(
                    [
                        ("asset_refinement", {"asset_steps": 400}),
                        ("variance_refinement", {"variance_steps": 100}),
                        ("time_refinement", {"time_steps": 400}),
                        (
                            "asset_domain",
                            {"asset_upper": 800, "asset_steps": expanded_steps},
                        ),
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
    run.write_report(
        HERE / "heston_calibration.json",
        {"schema_version": 1, "quantlib_version": "1.44", "records": records},
    )
    run.write_report(
        HERE / "cases/heston.json",
        {"schema_version": 1, "cases": list(cases.values())},
    )


if __name__ == "__main__":
    main()
