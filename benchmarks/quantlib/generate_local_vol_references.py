"""Offline QuantLib 1.44 local-volatility references and independent refinements."""

import argparse
import copy
import json
import os
import shlex
from pathlib import Path

import run

HERE = Path(__file__).resolve().parent


def main():
    """Refine time, space, and automatic-mesher domain choices separately."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--case",
        action="append",
        help="regenerate selected IDs, preserving other evidence",
    )
    args = parser.parse_args()
    cases = run.load_entries(HERE / "cases/fd_local_vol.json")
    if args.case and set(args.case) - cases.keys():
        parser.error("unknown reference case")
    config = shlex.split(os.environ.get("QUANTLIB_CONFIG", "quantlib-config"))
    assert run.command([*config, "--version"]) == "1.44"
    executable = HERE / "build/local_vol_reference"
    executable.parent.mkdir(exist_ok=True)
    compile_command = [
        "c++",
        "-std=c++17",
        "-O3",
        "-DNDEBUG",
        *shlex.split(run.command([*config, "--cflags"])),
        str(HERE / "calibration/local_vol_reference.cpp"),
        "-o",
        str(executable),
        *shlex.split(run.command([*config, "--libs"])),
    ]
    run.command(compile_command)
    destination = HERE / "references/fd_local_vol.json"
    rows = run.load_entries(destination) if args.case and destination.exists() else {}
    for case in cases.values():
        if args.case and case["id"] not in args.case:
            continue
        # Discontinuous asset payouts need finer reference space grids.
        asset_digital = case["id"] == "fd-local-vol-asset-digital-put"
        asset_binary = case["id"] == "fd-local-vol-terminal-up-in-asset"
        space = 25600 if asset_digital or asset_binary else 3200
        time = 800 if asset_digital or asset_binary else 3200
        values = {}
        for name, changes in {
            "time_coarse": {"time": time, "space": space},
            "time_middle": {"time": 2 * time, "space": space},
            "time_fine": {"time": 4 * time, "space": space},
            "space_refined": {"time": 4 * time, "space": 2 * space},
            "mesher_040": {"time": 4 * time, "space": 2 * space, "mesher_vol": 0.4},
            "mesher_060": {"time": 4 * time, "space": 2 * space, "mesher_vol": 0.6},
        }.items():
            probe = copy.deepcopy(case)
            probe["profiles"]["quantlib"].update(changes)
            if asset_binary:
                # Fully implicit reference avoids underdamped terminal-jump modes.
                probe["profiles"]["quantlib"].update(scheme="implicit_euler", damping=0)
            values[name] = {
                "price": run.native(executable, probe, "quantlib", 1)["price"],
                "profile": probe["profiles"]["quantlib"],
            }
            print(case["id"], name, values[name]["price"], flush=True)
        p = {key: value["price"] for key, value in values.items()}
        extrap_coarse = 2 * p["time_middle"] - p["time_coarse"]
        extrap_fine = 2 * p["time_fine"] - p["time_middle"]
        shifts = {
            "time_extrapolation": abs(extrap_fine - extrap_coarse),
            "space": abs(p["space_refined"] - p["time_fine"]),
            "domain": max(
                abs(p[key] - p["space_refined"]) for key in ("mesher_040", "mesher_060")
            ),
        }
        uncertainty = 2 * sum(shifts.values()) + 1e-10
        row = {
            "id": case["id"],
            "value": extrap_fine + p["space_refined"] - p["time_fine"],
            "uncertainty": uncertainty,
            "provenance": {
                "method": "QuantLib 1.44 external supplied local volatility; first-order time extrapolation plus independently refined space correction",
                "source": "generate_local_vol_references.py; calibration/local_vol_reference.cpp",
                "quantlib_version": "1.44",
                "compile_command": compile_command,
                "uncertainty_method": "twice sum of consecutive time-extrapolate, spatial refinement and mesh-domain sensitivity differences; empirical discretization estimate",
                "domain": "public native automatic mesh; vary mesher-only Black volatility .4/.5/.6 independently of local model",
                "evidence": values,
                "observed_shifts": shifts,
            },
        }
        assert uncertainty <= case["error_budget"] * 0.1, (
            case["id"],
            shifts,
            uncertainty,
        )
        rows[case["id"]] = row
        destination.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "cases": [rows[key] for key in cases if key in rows],
                },
                indent=2,
            )
            + "\n"
        )


if __name__ == "__main__":
    main()
