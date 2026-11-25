"""Regenerate native QuantLib 1.44 Heston evidence outside timed benchmarks.

Run after run.py has built both native adapters. This compiles the independent
reference probe and performs separate integration, grid and domain refinements.
The largest FD reference takes several minutes. Partial raw evidence is flushed
into build/heston-refinements.jsonl after every completed calculation.
"""

import itertools
import json
import shlex
from pathlib import Path

import run

HERE = Path(__file__).resolve().parent


def main():
    """Write references only after every independent refinement succeeds."""
    if run.command(["quantlib-config", "--version"]) != "1.44":
        raise RuntimeError("requires QuantLib 1.44")
    # The independent native probe is deliberately fixed to this ordinary case.
    expected = {
        "option": "call",
        "valuation": "2025-01-01",
        "expiry": "2026-01-01",
        "spot": 100,
        "strike": 100,
        "rate": 0.05,
        "dividend": 0.02,
        "initial_variance": 0.04,
        "long_run_variance": 0.04,
        "mean_reversion_speed": 1.5,
        "vol_of_vol": 0.3,
        "correlation": -0.7,
    }
    for case in run.load_entries(HERE / "cases/heston.json").values():
        if case["inputs"] != {
            **expected,
            "option": "put" if case["family"] == "heston_fd_american" else "call",
        }:
            raise RuntimeError(
                "scenario differs from fixed independent reference probe"
            )
    executable = HERE / "build/heston_reference"
    run.command(
        [
            "c++",
            "-std=c++17",
            "-O3",
            "-DNDEBUG",
            str(HERE / "heston_reference.cpp"),
            "-o",
            str(executable),
            *shlex.split(run.command(["quantlib-config", "--cflags", "--libs"])),
        ]
    )
    output = HERE / "build/heston-refinements.jsonl"
    output.write_text("")
    evidence = []

    def probe(parameters):
        result = json.loads(
            run.command([str(executable), *(f"{k}={v}" for k, v in parameters.items())])
        )
        record = {"parameters": parameters, **result}
        evidence.append(record)
        with output.open("a") as stream:
            stream.write(json.dumps(record) + "\n")
        print(json.dumps(record), flush=True)
        return result["price"]

    analytic = []
    for quadrature in ("lobatto", "exp_sinh"):
        for tolerance in (1e-10, 1e-12):
            analytic.append(
                probe(
                    {
                        "method": "analytic",
                        "quadrature": quadrature,
                        "tolerance": tolerance,
                    }
                )
            )
    if max(analytic) - min(analytic) > 1e-11:
        raise RuntimeError("independent Fourier integrations disagree")
    integration_evidence = list(evidence)
    european = []
    for t, x, v in ((400, 400, 200), (800, 400, 200), (400, 800, 200), (400, 400, 400)):
        european.append(
            probe(
                {
                    "method": "fd",
                    "exercise": "european",
                    "time_steps": t,
                    "asset_steps": x,
                    "variance_steps": v,
                }
            )
        )
    if max(abs(value - analytic[-1]) for value in european) > 0.005:
        raise RuntimeError("European FD refinements miss analytic reference")
    european_evidence = evidence[len(integration_evidence) :]
    american_start = len(evidence)
    raw = []
    for n in (400, 800, 1600):
        raw.append(
            probe(
                {
                    "method": "fd",
                    "exercise": "american",
                    "time_steps": n,
                    "asset_steps": n,
                    "variance_steps": n // 2,
                }
            )
        )
    # Refine each dimension independently, including automatic mesher bounds that depend on v.
    for t, x, v in ((1600, 800, 400), (800, 1600, 400), (800, 800, 800)):
        probe(
            {
                "method": "fd",
                "exercise": "american",
                "time_steps": t,
                "asset_steps": x,
                "variance_steps": v,
            }
        )
    american_evidence = evidence[american_start:]
    extrapolates = [2 * b - a for a, b in itertools.pairwise(raw)]
    # Combine joint-grid extrapolation with independent dimension refinements.
    corrections = [record["price"] - raw[1] for record in american_evidence[3:]]
    independent_extrapolate = raw[1] + 2 * sum(corrections)
    uncertainty = max(
        abs(extrapolates[-1] - extrapolates[-2]),
        abs(extrapolates[-1] - independent_extrapolate),
    )
    if uncertainty > 0.0005:
        raise RuntimeError(
            f"American reference uncertainty {uncertainty} exceeds budget"
        )
    provenance = {
        "quantlib_version": "1.44",
        "source": "native installed QuantLib; heston_reference.cpp",
        "controls": {
            "analytic_max_evaluations": 100000,
            "fd_scheme": "hundsdorfer",
            "damping_steps": 0,
            "mesher": "standard FdHestonVanillaEngine automatic domains",
        },
        "inputs": run.load_entries(HERE / "cases/heston.json")["heston-analytic-call"][
            "inputs"
        ],
    }
    references = []
    for identifier in ("heston-analytic-call", "heston-fd-european-call"):
        references.append(
            {
                "id": identifier,
                "value": analytic[-1],
                "uncertainty": 1e-11,
                "provenance": {
                    **provenance,
                    "method": "Gatheral adaptive Lobatto and independent exp-sinh Fourier integration",
                    "evidence": {
                        "integration": integration_evidence,
                        "fd_european": european_evidence,
                    },
                },
            }
        )
    references.append(
        {
            "id": "heston-fd-american-put",
            "value": extrapolates[-1],
            "uncertainty": uncertainty,
            "provenance": {
                **provenance,
                "inputs": {**provenance["inputs"], "option": "put"},
                "method": "Hundsdorfer American projection; first-order joint and independent-dimension extrapolation",
                "evidence": {
                    "raw": american_evidence,
                    "extrapolated_prices": extrapolates,
                    "independent_extrapolate": independent_extrapolate,
                    "dimension_corrections": corrections,
                },
            },
        }
    )
    (HERE / "references/heston.json").write_text(
        json.dumps({"schema_version": 1, "cases": references}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
