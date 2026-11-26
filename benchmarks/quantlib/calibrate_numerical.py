#!/usr/bin/env python3
"""Regenerate offline quadrature/CRR reference and refinement evidence."""

import copy
import datetime
import json
import os
import shlex
from decimal import Decimal, localcontext

from generate_digitals_asian_references import price as digital_price
from generate_european_references import normal_cdf
from generate_european_references import price as vanilla_price
from run import (
    HERE,
    ROOT,
    build_rust,
    command,
    load_entries,
    native,
    prebuilt_rust_executable,
    write_report,
)


def analytic(inputs, precision):
    """Evaluate binary and down-out reflection formulas independently in Decimal."""
    if "payoff" in inputs:
        case = {
            "family": "digital_bsm",
            "inputs": inputs | {"payout": inputs["payoff"]},
        }
        return digital_price(case, precision)
    if "barrier" not in inputs:
        return vanilla_price(inputs, precision)
    with localcontext() as context:
        context.prec = precision
        s, k, r, q, sigma = (
            Decimal(str(inputs[name]))
            for name in ("spot", "strike", "rate", "dividend", "volatility")
        )
        days = (
            datetime.date.fromisoformat(inputs["expiry"])
            - datetime.date.fromisoformat(inputs["valuation"])
        ).days
        t = Decimal(days) / 365
        width = sigma * t.sqrt()
        # H < K down-and-out call, zero rebate: vanilla minus reflected knock-in.
        assert inputs["barrier_type"] == "down_out" and inputs["option"] == "call"
        h = Decimal(str(inputs["barrier"]))
        assert h < k and h < s
        mu = (r - q - sigma * sigma / 2) / (sigma * sigma)
        y = (h * h / (s * k)).ln() / width + (mu + 1) * width
        reflected = s * (-q * t).exp() * (h / s) ** (2 * (mu + 1)) * normal_cdf(
            y
        ) - k * (-r * t).exp() * (h / s) ** (2 * mu) * normal_cdf(y - width)
        return vanilla_price(inputs, precision) - reflected


def american_reference():
    """Cross-check independent native FD solvers over time, space and domain."""
    config = shlex.split(os.environ.get("QUANTLIB_CONFIG", "quantlib-config"))
    if command([*config, "--version"]) != "1.44":
        raise ValueError("QuantLib 1.44 required for reference generation")
    output = HERE / "build/american_reference"
    compiler = [
        *shlex.split(os.environ.get("CXX", "c++")),
        "-std=c++17",
        "-O3",
        "-DNDEBUG",
        *shlex.split(command([*config, "--cflags"])),
        str((HERE / "calibration/american_reference.cpp").relative_to(ROOT)),
        "-o",
        str(output.relative_to(ROOT)),
        *shlex.split(command([*config, "--libs"])),
    ]
    output.parent.mkdir(exist_ok=True)
    command(compiler)
    reference_binary, _ = build_rust("quantlib_bench_references")
    ql = [json.loads(line) for line in command([str(output)]).splitlines()]
    rust = [json.loads(line) for line in command([str(reference_binary)]).splitlines()]
    ql_extrapolated = 2 * ql[-1]["price"] - ql[-2]["price"]
    rust_extrapolated = 2 * rust[4]["price"] - rust[3]["price"]
    evidence = {
        "scenario": {
            "spot": 100,
            "strike": 100,
            "rate": 0.05,
            "dividend": 0.02,
            "volatility": 0.2,
            "maturity_years": 1,
            "option": "put",
            "exercise": "american",
        },
        "quantlib_version": "1.44",
        "quantlib_build_command": compiler,
        "quantlib_source": "benchmarks/quantlib/calibration/american_reference.cpp",
        "kiyosi_source": "benchmarks/quantlib/rust/src/bin/quantlib_bench_references.rs",
        "quantlib_controls": "CrankNicolson; dampingSteps=2; automatic mesher domain (not publicly adjustable)",
        "kiyosi_controls": "Rannacher; concentrated log-spot grid; domain refinements preserve mean log spacing up to interval rounding",
        "quantlib_fd": ql,
        "kiyosi_fd": rust,
        "quantlib_first_order_time_extrapolation": ql_extrapolated,
        "kiyosi_first_order_time_extrapolation": rust_extrapolated,
        "independent_extrapolation_difference": abs(
            ql_extrapolated - rust_extrapolated
        ),
        "quantlib_last_space_delta": abs(ql[2]["price"] - ql[1]["price"]),
        "kiyosi_last_space_delta": abs(rust[2]["price"] - rust[1]["price"]),
        "kiyosi_domain_span": max(
            abs(row["price"] - rust[4]["price"]) for row in rust[5:]
        ),
        "uncertainty_policy": "5e-5 conservative numerical uncertainty; covers independent extrapolation difference, last temporal changes, spatial changes and domain span; not a rigorous error bound",
    }
    assert (
        max(
            evidence[key]
            for key in (
                "independent_extrapolation_difference",
                "quantlib_last_space_delta",
                "kiyosi_last_space_delta",
                "kiyosi_domain_span",
            )
        )
        < 1e-5
    )
    return ql_extrapolated, evidence


def main():
    """Save references and actual native results; never called by normal runs."""
    cases = load_entries(HERE / "cases/numerical.json")
    american_value, american_evidence = american_reference()
    references = []
    calibration = {
        "quantlib_version": "1.44",
        "source_revision": command(["git", "rev-parse", "HEAD"]),
        "source_dirty_state": command(["git", "status", "--porcelain"]),
        "utc": datetime.datetime.now(datetime.UTC).isoformat(),
        "cases": [],
    }
    binaries = {
        "kiyosi": prebuilt_rust_executable(),
        "quantlib": HERE / "build/quantlib_bench",
    }
    for case in cases.values():
        if case["inputs"].get("exercise") == "american":
            value, uncertainty = american_value, 5e-5
            provenance = {
                "method": "QuantLib 1.44 FD first-order time extrapolation, independently checked against Rust FD with separate space/time/domain refinements",
                "source": "benchmarks/quantlib/calibration/american.json",
                "evidence": american_evidence,
            }
        else:
            coarse, fine = (analytic(case["inputs"], digits) for digits in (50, 80))
            value, uncertainty = float(fine), 1e-13
            provenance = {
                "method": "Independent Decimal BSM closed form; Gaussian power-series CDF; binary formula or down-out call reflection formula as applicable",
                "source": "benchmarks/quantlib/calibrate_numerical.py",
                "evidence": {
                    "precision_digits": [50, 80],
                    "decimal_value": str(fine),
                    "precision_difference": str(abs(fine - coarse)),
                },
            }
            assert abs(fine - coarse) < Decimal("1e-45")
        reference = {
            "id": case["id"],
            "value": value,
            "uncertainty": uncertainty,
            "provenance": provenance,
        }
        reference["provenance"]["native_calibration"] = (
            "benchmarks/quantlib/calibration/numerical.json"
        )
        references.append(reference)
        row = {
            "id": case["id"],
            "budget": case["error_budget"],
            "reference": value,
            "reference_uncertainty": uncertainty,
            "backends": {},
        }
        for backend, binary in binaries.items():
            selected = native(binary, case, backend, 1)
            error = abs(selected["price"] - value)
            budget = case["error_budget"]
            passed = error + uncertainty <= budget
            if not passed:
                raise RuntimeError(
                    f"{case['id']}/{backend}: error {error} plus reference uncertainty "
                    f"{uncertainty} exceeds budget {budget}"
                )
            rows = []
            if case["family"].startswith("quadrature"):
                profiles = (
                    []
                    if backend == "quantlib"
                    else [
                        case["profiles"][backend] | {key: setting}
                        for key, settings in (
                            ("asset_step_count", (256, 512, 1024, 2048)),
                            ("integration_standard_deviations", (8, 10, 12)),
                        )
                        for setting in settings
                    ]
                )
            else:
                counts = (255, 256, 511, 512, 1023, 1024, 2047, 2048, 4095, 4096)
                if case["family"] == "crr_barrier":
                    counts += (8191, 8192)
                profiles = [{"step_count": count} for count in counts]
            for profile in profiles:
                if profile == case["profiles"][backend]:
                    continue
                refined = copy.deepcopy(case)
                refined["profiles"][backend] = profile
                result = native(binary, refined, backend, 1)
                rows.append(
                    {
                        "profile": profile,
                        "price": result["price"],
                        "absolute_error": abs(result["price"] - value),
                    }
                )
            row["backends"][backend] = {
                "selected_profile": case["profiles"][backend],
                "selected_price": selected["price"],
                "absolute_error": error,
                "passed": passed,
                "refinements": rows,
            }
            if not profiles:
                row["backends"][backend]["fixed_integration"] = (
                    "5000 trapezoid intervals; native IntegralEngine exposes no controls; actual error checked independently"
                )
        calibration["cases"].append(row)
        print(
            case["id"],
            {key: entry["absolute_error"] for key, entry in row["backends"].items()},
        )
    write_report(HERE / "calibration/american.json", american_evidence)
    write_report(
        HERE / "references/numerical.json", {"schema_version": 1, "cases": references}
    )
    write_report(HERE / "calibration/numerical.json", calibration)


if __name__ == "__main__":
    main()
