"""Retain native local-volatility profile and supplied-surface agreement evidence."""

import copy
import json
from pathlib import Path

import run
from calibrate_fd import domain_probes

HERE = Path(__file__).resolve().parent


def main():
    """Check separately refined native profiles against independent references."""
    cases = run.load_entries(HERE / "cases/fd_local_vol.json")
    references = run.load_entries(HERE / "references/fd_local_vol.json")
    executables = {
        "kiyosi": run.prebuilt_rust_executable(),
        "quantlib": HERE / "build/quantlib_bench",
    }
    evidence = {
        "schema_version": 1,
        "declared_budget": 0.005,
        "domain": "QuantLib automatic log mesher varied through mesher-only Black volatility; Rust domain_constant_spacing compared with the explicit grid (selected or explicit_boundary) at constant mean log spacing",
        "knock_in_domain": "domain_constant_spacing preserves the barrier grid's mean log spacing; continuation_constant_spacing preserves full-domain continuation mean log spacing. One shared space count cannot hold both spacings fixed during expansion; neither is an isolated two-grid domain probe",
        "cases": [],
        "surface_points": [],
    }
    first = next(iter(cases.values()))
    for spot, expected in [
        (60, 0.32493150684931504),
        (80, 0.32493150684931504),
        (100, 0.24993150684931506),
        (120, 0.17493150684931508),
        (160, 0.17493150684931508),
    ]:
        probe = {
            "id": "surface",
            "family": "fd_local_vol",
            "profiles": {"kiyosi": {}, "quantlib": {}},
            "inputs": {
                key: value
                for key, value in first["inputs"].items()
                if key.startswith("surface_") or key in ("valuation", "expiry")
            },
        }
        probe["inputs"].update(
            operation="surface_point", query_date="2025-07-02", query_spot=spot
        )
        row = {"spot": spot, "date": "2025-07-02", "expected": expected}
        for backend, executable in executables.items():
            row[backend] = run.native(executable, probe, backend, 1)["price"]
            assert abs(row[backend] - expected) < 1e-14
        evidence["surface_points"].append(row)
    for case in cases.values():
        row = {"id": case["id"], "error_budget": case["error_budget"], "backends": {}}
        for backend, executable in executables.items():
            profile = case["profiles"][backend]
            variants = {
                "selected": {},
                "space": {"space": profile["space"] * 2},
                "time": {"time": profile["time"] * 2},
                "coarse_rejected": {"space": 10, "time": 10},
            }
            if backend == "kiyosi":
                variants.update(domain_probes(case))
                if case["id"] == "fd-local-vol-asset-digital-put":
                    variants.update(
                        {
                            "reference_time_1600": {"space": 6400, "time": 1600},
                            "reference_time_3200": {"space": 6400, "time": 3200},
                            "reference_time_6400": {"space": 6400, "time": 6400},
                            "reference_space_12800": {"space": 12800, "time": 6400},
                            "reference_domain_600": {
                                **domain_probes(
                                    {
                                        **case,
                                        "profiles": {
                                            "kiyosi": {**profile, "space": 12800}
                                        },
                                    }
                                )["domain_constant_spacing"],
                                "time": 6400,
                            },
                        }
                    )
                if case["id"] == "fd-local-vol-terminal-up-in-asset":
                    variants.update(
                        {
                            "reference_time_800": {"space": 3200, "time": 800},
                            "reference_time_1600": {"space": 3200, "time": 1600},
                            "reference_time_3200": {"space": 3200, "time": 3200},
                            "reference_space_6400": {"space": 6400, "time": 3200},
                            "reference_domain_600": {
                                **domain_probes(
                                    {
                                        **case,
                                        "profiles": {
                                            "kiyosi": {**profile, "space": 6400}
                                        },
                                    }
                                )["continuation_constant_spacing"],
                                "time": 3200,
                            },
                        }
                    )
            else:
                variants["mesher_040"] = {"mesher_vol": 0.4}
                variants["mesher_060"] = {"mesher_vol": 0.6}
                if profile["time"] > 400:
                    variants["initial_time_400"] = {"time": 400}
            measurements = {}
            for name, changes in variants.items():
                probe = copy.deepcopy(case)
                probe["profiles"][backend].update(changes)
                price = run.native(executable, probe, backend, 1)["price"]
                if name.startswith(("reference_space_", "reference_domain_")):
                    assert (
                        abs(price - references[case["id"]]["value"])
                        <= references[case["id"]]["uncertainty"]
                    ), (case["id"], name, price)
                measurements[name] = {
                    "profile": probe["profiles"][backend],
                    **run.accuracy(price, references[case["id"]], case["error_budget"]),
                }
            assert measurements["selected"]["passed"], (
                case["id"],
                backend,
                measurements,
            )
            assert not measurements["coarse_rejected"]["passed"], (case["id"], backend)
            row["backends"][backend] = measurements
        evidence["cases"].append(row)
        (HERE / "calibration/fd_local_vol.json").write_text(
            json.dumps(evidence, indent=2) + "\n"
        )
        print(case["id"], flush=True)


if __name__ == "__main__":
    main()
