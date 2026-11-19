#!/usr/bin/env python3
"""Build and compare fresh native Kiyosi and QuantLib price requests."""

import argparse
import datetime
import hashlib
import json
import math
import os
import platform
import re
import shlex
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from suite_coverage import REQUIRED, audit, matches

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
RUST_EXECUTABLE = ROOT / "target/release/quantlib_bench"
BACKENDS = ("kiyosi", "quantlib")
NATIVE_TIMEOUT_SECONDS = 120


def write_report(path, report):
    """Replace the report only after the new JSON has been written successfully."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=path.parent, delete=False
        ) as output:
            temporary = Path(output.name)
            json.dump(report, output, indent=2, allow_nan=False)
            output.write("\n")
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def command(argv, *, cwd=ROOT, timeout=None):
    """Run a command and return stdout with useful failure diagnostics."""
    try:
        result = subprocess.run(
            argv, cwd=cwd, text=True, capture_output=True, check=True, timeout=timeout
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(
            f"{argv[0]} timed out after {error.timeout:g} seconds"
        ) from error
    except FileNotFoundError as error:
        raise RuntimeError(f"missing executable: {argv[0]}") from error
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            f"{' '.join(map(str, argv))}: {error.stderr.strip() or error.stdout.strip() or f'exit {error.returncode}'}"
        ) from error
    return result.stdout.strip()


def optional_command(argv):
    """Identify unavailable metadata explicitly."""
    try:
        return command(argv)
    except RuntimeError as error:
        return f"unavailable: {error}"


def source_state():
    return {
        "revision": optional_command(["git", "rev-parse", "HEAD"]),
        "dirty_state": optional_command(["git", "status", "--porcelain"]),
    }


def reject_constant(value):
    """Reject nonfinite JSON numbers at the manifest boundary."""
    raise ValueError(f"nonfinite JSON number: {value}")


def finite_number(value):
    """Accept JSON numbers without bool-to-number coercion."""
    return type(value) is int or (type(value) is float and math.isfinite(value))


def finite_float(value):
    """Reject JSON exponent overflow as well as NaN/Infinity tokens."""
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"nonfinite JSON number: {value}")
    return result


def fingerprint(path):
    """Tie recorded build commands to the supplied executable artifact."""
    return {
        "path": str(path.resolve()),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


def build_evidence_path(executable):
    return executable.with_name(f"{executable.name}.build.json")


def load_entries(path):
    """Load unique scenario/reference IDs from one file or sorted fragments."""
    entries = {}
    files = sorted(path.glob("*.json")) if path.is_dir() else [path]
    for file in files:
        document = json.loads(
            file.read_text(), parse_constant=reject_constant, parse_float=finite_float
        )
        if (
            type(document["schema_version"]) is not int
            or document["schema_version"] != 1
        ):
            raise ValueError(f"unsupported schema: {file}")
        for entry in document["cases"]:
            identifier = entry["id"]
            if not isinstance(identifier, str) or not identifier:
                raise ValueError(f"case ID must be a nonempty string: {file}")
            if identifier in entries:
                raise ValueError(f"duplicate case: {identifier}")
            entries[identifier] = entry
    if not entries:
        raise ValueError(f"no cases found: {path}")
    return entries


def rust_build_command(binary="quantlib_bench"):
    """Build a release binary from the Rust benchmark package."""
    return [
        "cargo",
        "build",
        "--release",
        "--locked",
        "-p",
        "kiyosi-quantlib-bench",
        "--bin",
        binary,
    ]


def build(args):
    """Build optimized companions without changing the installed QuantLib."""
    source = source_state()
    config = shlex.split(args.quantlib_config)
    version = command([*config, "--version"])
    if version != "1.44":
        raise ValueError(f"QuantLib 1.44 required; quantlib-config reports {version}")
    flags = [*shlex.split(command([*config, "--cflags"])), *shlex.split(args.cxxflags)]
    libs = shlex.split(command([*config, "--libs"]))
    rust = [*rust_build_command(), "--message-format=json-render-diagnostics"]
    cpp = [
        *shlex.split(args.cxx),
        "-std=c++17",
        *flags,
        "-O3",
        "-DNDEBUG",
        str(HERE / "quantlib.cpp"),
        "-o",
        str(args.quantlib_executable),
        *libs,
    ]
    args.quantlib_executable.parent.mkdir(parents=True, exist_ok=True)
    artifacts = (json.loads(line) for line in command(rust).splitlines())
    executable = next(
        (
            artifact["executable"]
            for artifact in artifacts
            if artifact["reason"] == "compiler-artifact"
            and artifact["target"]["name"] == "quantlib_bench"
            and artifact["executable"]
        ),
        None,
    )
    if executable is None:
        raise RuntimeError("Cargo did not report the quantlib_bench executable")
    rust_executable = Path(executable)
    command(cpp)
    evidence = {
        "source": source,
        "rust_command": rust,
        "cpp_command": cpp,
        "quantlib_config_version": version,
        "rust_profile": "Cargo release; thin LTO",
        "installed_quantlib_build_flags": "unavailable: verify optimized installation manually",
        "executables": {
            "kiyosi": fingerprint(rust_executable),
            "quantlib": fingerprint(args.quantlib_executable),
        },
        "rustflags": os.environ.get("RUSTFLAGS", "unset"),
        "cxx_version": command([*shlex.split(args.cxx), "--version"]),
    }
    write_report(build_evidence_path(args.quantlib_executable), evidence)
    return rust_executable, evidence


def native(executable, case, backend, iterations):
    """Invoke the public native batch protocol outside its timed region."""
    inputs = case["inputs"]
    profile = case["profiles"][backend]
    if set(inputs) & set(profile) or {"family", "iterations"} & (
        set(inputs) | set(profile)
    ):
        raise ValueError(f"overlapping native parameters: {case['id']}")
    parameters = {
        "family": case["family"],
        "iterations": iterations,
        **inputs,
        **profile,
    }
    if any(
        type(value) not in (int, float, str)
        or (type(value) in (int, float) and not finite_number(value))
        for value in parameters.values()
    ):
        raise ValueError(
            f"native parameters must be scalar numbers or strings: {case['id']}"
        )
    result = json.loads(
        command(
            [str(executable), *(f"{key}={value}" for key, value in parameters.items())],
            timeout=NATIVE_TIMEOUT_SECONDS,
        ),
        parse_constant=reject_constant,
        parse_float=finite_float,
    )
    if (
        not isinstance(result, dict)
        or not finite_number(result.get("price"))
        or type(result.get("iterations")) is not int
        or result["iterations"] != iterations
        or type(result.get("elapsed_ns")) is not int
        or result["elapsed_ns"] <= 0
        or type(result.get("warmup_iterations")) is not int
        or result["warmup_iterations"] != 3
    ):
        raise ValueError(f"invalid native result: {backend}/{case['id']}: {result}")
    return result


def accuracy(price, reference, budget):
    """Apply the same conservative absolute error budget to both libraries."""
    error = abs(price - reference["value"])
    total = error + reference["uncertainty"]
    finite_total = finite_number(total)
    result = {
        "price": price,
        "absolute_error": error if finite_number(error) else None,
        "error_including_reference_uncertainty": total if finite_total else None,
        "passed": finite_total and total <= budget,
    }
    if not finite_total:
        result["error"] = "price error including reference uncertainty overflowed"
    return result


def seed_ensemble(case, reference, executable, backend):
    """Measure empirical error and sampling dispersion outside timing."""
    policy = case["mc_validation"]
    seeds = policy["seeds"]
    timing_seed = policy["timing_seed"]
    if (
        len(seeds) < 16
        or len(set(seeds)) != len(seeds)
        or any(
            type(seed) is not int or not 0 < seed <= 4294967295
            for seed in [*seeds, timing_seed]
        )
        or timing_seed in seeds
    ):
        raise ValueError(
            "MC requires >=16 unique positive 32-bit validation seeds and a separate timing seed"
        )
    profile = case["profiles"][backend]
    if profile["seed"] != timing_seed:
        raise ValueError("native seed must equal predetermined timing_seed")
    limits = ("max_rmse", "max_abs_bias", "max_sample_stddev")
    if any(
        not finite_number(policy[key]) or not 0 < policy[key] <= case["error_budget"]
        for key in limits
    ):
        raise ValueError("MC precision limits must be positive and <= case budget")
    offset = policy.get("calibration_seed_offset")
    if offset is not None and (
        type(offset) is not int
        or offset <= 0
        or (
            "calibration_seed" in profile
            and profile["calibration_seed"] != timing_seed + offset
        )
    ):
        raise ValueError("calibration seeds must use the predetermined positive offset")
    rows = []
    for seed in seeds:
        validation_profile = {**profile, "seed": seed}
        if offset is not None and "calibration_seed" in profile:
            validation_profile["calibration_seed"] = seed + offset
        validation_case = {**case, "profiles": {backend: validation_profile}}
        batch = native(executable, validation_case, backend, 1)
        signed_error = batch["price"] - reference["value"]
        if not finite_number(signed_error):
            raise ValueError(f"price error overflowed for {backend} seed {seed}")
        rows.append(
            {
                "seed": seed,
                "profile": validation_profile,
                "price": batch["price"],
                "signed_error": signed_error,
            }
        )
    errors = [row["signed_error"] for row in rows]
    maximum = max(map(abs, errors))
    rmse = (
        maximum * math.sqrt(statistics.mean((error / maximum) ** 2 for error in errors))
        if maximum
        else 0.0
    )
    bias = statistics.mean(errors)
    dispersion = statistics.stdev(errors)
    if not all(finite_number(metric) for metric in (rmse, bias, dispersion)):
        raise ValueError(f"Monte Carlo error metrics overflowed for {backend}")
    uncertainty = reference["uncertainty"]
    accuracy_passed = (
        rmse + uncertainty <= policy["max_rmse"]
        and abs(bias) + uncertainty <= policy["max_abs_bias"]
        and maximum + uncertainty <= case["error_budget"]
    )
    precision_passed = dispersion <= policy["max_sample_stddev"]
    return {
        "samples": rows,
        "rmse": rmse,
        "signed_bias": bias,
        "sample_stddev": dispersion,
        "mean_standard_error": dispersion / math.sqrt(len(seeds)),
        "max_absolute_error": maximum,
        "accuracy_passed": accuracy_passed,
        "precision_passed": precision_passed,
        "passed": accuracy_passed and precision_passed,
    }


def measure(case, reference, executables, args, info):
    """Check accuracy, independently adapt batches, then collect 15 pairs."""
    result = {
        **case,
        "reference": reference,
        "backends": {},
        "ratio": None,
        "status": "failed",
    }
    try:
        budget = case["error_budget"]
        if not finite_number(budget) or budget <= 0:
            raise ValueError("error_budget must be finite and positive")
        if (
            not finite_number(reference["value"])
            or not finite_number(reference["uncertainty"])
            or not 0 <= reference["uncertainty"] <= budget * 0.1
        ):
            raise ValueError("reference uncertainty must be <= 10% of case budget")
        missing = [
            f"{backend}/{case['family']}"
            for backend in BACKENDS
            if case["family"] not in info[backend]["families"]
        ]
        if missing:
            result["status"] = "unsupported"
            raise ValueError("missing native family " + ", ".join(missing))
        for backend in BACKENDS:
            batch = native(executables[backend], case, backend, 1)
            result["backends"][backend] = {
                "accuracy": accuracy(batch["price"], reference, budget),
                "warmup": batch,
            }
        if not all(
            value["accuracy"]["passed"] for value in result["backends"].values()
        ):
            result["status"] = "accuracy_failed"
            return result
        if case["family"].startswith("mc_") or "mc_validation" in case:
            if "mc_validation" not in case:
                raise ValueError("Monte Carlo cases require mc_validation")
            for backend in BACKENDS:
                result["backends"][backend]["seed_ensemble"] = seed_ensemble(
                    case, reference, executables[backend], backend
                )
            ensembles = [
                value["seed_ensemble"] for value in result["backends"].values()
            ]
            if not all(value["precision_passed"] for value in ensembles):
                result["status"] = "precision_failed"
                return result
            if not all(value["accuracy_passed"] for value in ensembles):
                result["status"] = "accuracy_failed"
                return result
        if args.price_only:
            result["status"] = "passed"
            return result
        counts = {}
        for backend in BACKENDS:
            iterations = 1
            adaptation = []
            for _ in range(12):
                batch = native(executables[backend], case, backend, iterations)
                adaptation.append(batch)
                if batch["elapsed_ns"] >= args.target_ms * 1e6:
                    break
                iterations = min(
                    1_000_000_000,
                    max(
                        iterations + 1,
                        math.ceil(
                            iterations
                            * args.target_ms
                            * 1e6
                            / batch["elapsed_ns"]
                            * 1.1
                        ),
                    ),
                )
            else:
                raise ValueError(f"cannot adapt batch: {backend}")
            counts[backend] = iterations
            result["backends"][backend]["timing"] = {
                "adaptation": adaptation,
                "samples": [],
            }
        result["sample_order"] = []
        for index in range(15):
            order = BACKENDS if index % 2 == 0 else BACKENDS[::-1]
            result["sample_order"].append(list(order))
            for backend in order:
                batch = native(executables[backend], case, backend, counts[backend])
                result["backends"][backend]["timing"]["samples"].append(batch)
                check = accuracy(batch["price"], reference, budget)
                if not check["passed"]:
                    result["backends"][backend]["accuracy"] = check
        for backend in BACKENDS:
            timing = result["backends"][backend]["timing"]
            latencies = [
                batch["elapsed_ns"] / batch["iterations"] for batch in timing["samples"]
            ]
            quartiles = statistics.quantiles(latencies, n=4, method="inclusive")
            timing.update(
                median_ns=statistics.median(latencies),
                q1_ns=quartiles[0],
                q3_ns=quartiles[2],
                iqr_ns=quartiles[2] - quartiles[0],
            )
        if all(value["accuracy"]["passed"] for value in result["backends"].values()):
            result["ratio"] = (
                result["backends"]["quantlib"]["timing"]["median_ns"]
                / result["backends"]["kiyosi"]["timing"]["median_ns"]
            )
            result["status"] = "passed"
        else:
            result["status"] = "accuracy_failed"
    except (
        RuntimeError,
        ValueError,
        OSError,
        KeyError,
        TypeError,
        OverflowError,
    ) as error:
        result["error"] = str(error)
    return result


def table(cases):
    """Print per-case latency and spread without an aggregate score."""
    print(
        "case / engine / model | accuracy (K / Q abs error) | K median [IQR] us | Q median [IQR] us | Q/K"
    )
    for case in cases:
        values = []
        errors = []
        for backend in BACKENDS:
            data = case["backends"].get(backend, {})
            accuracy_result = data.get("accuracy")
            error = accuracy_result["absolute_error"] if accuracy_result else None
            errors.append(f"{error:.3g}" if error is not None else "unavailable")
            timing = data.get("timing", {})
            values.append(
                f"{timing['median_ns'] / 1000:.3f} [{timing['iqr_ns'] / 1000:.3f}]"
                if "median_ns" in timing
                else "unavailable"
            )
        ratio = f"{case['ratio']:.3f}" if case["ratio"] is not None else "suppressed"
        print(
            f"{case['id']} / {case.get('engine', 'unknown')} / {case.get('model', 'unknown')} | {case['status']} ({' / '.join(errors)}) | {' | '.join(values)} | {ratio}"
        )
        if "error" in case:
            print(f"  {case['error']}")


def main(argv=None):
    """Write diagnostic JSON and return failure for any requested failed case."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--no-build", action="store_true", help="use existing native executables"
    )
    parser.add_argument(
        "--rust-executable",
        type=Path,
        help="Rust executable to use with --no-build",
    )
    parser.add_argument(
        "--quantlib-executable", type=Path, default=HERE / "build/quantlib_bench"
    )
    parser.add_argument(
        "--quantlib-config",
        default=os.environ.get("QUANTLIB_CONFIG", "quantlib-config"),
    )
    parser.add_argument("--cxx", default=os.environ.get("CXX", "c++"))
    parser.add_argument("--cxxflags", default=os.environ.get("CXXFLAGS", ""))
    parser.add_argument("--cases", type=Path, default=HERE / "cases")
    parser.add_argument("--references", type=Path, default=HERE / "references")
    parser.add_argument(
        "--case", action="append", help="select an ID; repeat for multiple cases"
    )
    parser.add_argument(
        "--price-only",
        action="store_true",
        help="check prices without collecting timing pairs",
    )
    parser.add_argument(
        "--target-ms", type=float, default=20, help="minimum adaptive batch duration"
    )
    parser.add_argument("--report", type=Path, default=HERE / "report.json")
    args = parser.parse_args(argv)
    start = time.monotonic()
    report = {
        "schema_version": 1,
        "status": "failed",
        "cases": [],
        "utc": datetime.datetime.now(datetime.UTC).isoformat(),
        "protocol": {
            "paired_samples": 15,
            "target_ms": args.target_ms if finite_number(args.target_ms) else None,
            "workers_per_backend": 1,
            "quartiles": "inclusive linear interpolation",
            "price_only": args.price_only,
        },
        "machine": {
            "os": platform.platform(),
            "architecture": platform.machine(),
            "cpu": optional_command(["lscpu"]),
        },
        "source": source_state(),
        "toolchains": {
            "rustc": optional_command(["rustc", "--version"]),
            "cargo": optional_command(["cargo", "--version"]),
            "python": sys.version,
        },
    }
    try:
        if not sys.platform.startswith("linux"):
            raise ValueError("native benchmarks currently support Linux")
        if not math.isfinite(args.target_ms) or args.target_ms <= 0:
            raise ValueError("target-ms must be finite and positive")
        cases = load_entries(args.cases)
        selected = list(dict.fromkeys(args.case if args.case is not None else cases))
        cells = audit(cases)
        report["coverage"] = {
            "mode": "focused" if args.case is not None else "complete",
            "required_cells": len(cells),
            "covered_cells": sum(bool(cell["cases"]) for cell in cells),
            "eligible_engines": sorted({cell["engine"] for cell in cells}),
            "cells": cells,
        }
        for identifier in selected:
            case = cases.get(
                identifier, {"id": identifier, "engine": "unknown", "model": "unknown"}
            )
            error = None
            status = "pending"
            if identifier not in cases:
                error, status = "unknown requested case", "unsupported"
            elif not any(matches(case, cell) for cell in REQUIRED):
                error, status = (
                    "unsupported engine/model/product semantics",
                    "unsupported",
                )
            report["cases"].append(
                {**case, "status": status, "backends": {}, "ratio": None}
            )
            if error:
                report["cases"][-1]["error"] = error
        if args.case is None:
            for cell in cells:
                if not cell["cases"]:
                    report["cases"].append(
                        {
                            **cell,
                            "id": "required:" + cell["id"],
                            "status": "missing_coverage",
                            "error": "missing required semantic cell",
                            "backends": {},
                            "ratio": None,
                        }
                    )
        references = load_entries(args.references)
        if args.no_build:
            rust_executable = args.rust_executable or RUST_EXECUTABLE
            build_file = build_evidence_path(args.quantlib_executable)
            report["build"] = (
                json.loads(
                    build_file.read_text(),
                    parse_constant=reject_constant,
                    parse_float=finite_float,
                )
                if build_file.exists()
                else "unavailable: executables supplied with --no-build"
            )
        else:
            if args.rust_executable is not None:
                raise ValueError("--rust-executable requires --no-build")
            rust_executable, report["build"] = build(args)
        executables = {
            "kiyosi": rust_executable,
            "quantlib": args.quantlib_executable,
        }
        info = {
            backend: json.loads(
                command([str(executable), "--info"], timeout=NATIVE_TIMEOUT_SECONDS),
                parse_constant=reject_constant,
                parse_float=finite_float,
            )
            for backend, executable in executables.items()
        }
        if args.no_build and isinstance(report["build"], dict):
            artifacts = {
                backend: fingerprint(path) for backend, path in executables.items()
            }
            if artifacts != report["build"].get("executables"):
                report["build"] = (
                    "unavailable: supplied executables differ from recorded build artifacts"
                )
        if isinstance(report["build"], dict):
            report["build"].setdefault(
                "source", "unavailable: build source was not recorded"
            )
        report["libraries"] = info
        for backend, metadata in info.items():
            if (
                not isinstance(metadata, dict)
                or not isinstance(metadata.get("version"), str)
                or not isinstance(metadata.get("families"), list)
                or any(not isinstance(family, str) for family in metadata["families"])
            ):
                raise ValueError(f"invalid native registry: {backend}")
        for cell in report["coverage"]["cells"]:
            cell["native_support"] = {
                backend: cell["family"] in info[backend]["families"]
                for backend in BACKENDS
            }
        report["runtime_linkage"] = optional_command(
            ["ldd", str(args.quantlib_executable)]
        )
        resolved = []
        for line in report["runtime_linkage"].splitlines():
            if "libQuantLib" not in line:
                continue
            match = re.search(r"=> (\S+)", line)
            if match and Path(match[1]).exists():
                path = Path(match[1]).resolve()
                version = re.search(
                    r"(?:QuantLib[-_]|\.so\.)(\d+\.\d+)(?:\D|$)", path.name
                )
                resolved.append(
                    {
                        "path": str(path),
                        "filename_version": version[1] if version else None,
                    }
                )
        report["runtime_library"] = {
            "resolved": resolved,
            "version_api": "unavailable: QuantLib exposes no runtime library version API",
        }
        if any(item["filename_version"] not in (None, "1.44") for item in resolved):
            raise ValueError(f"QuantLib 1.44 runtime filename required: {resolved}")
        if info["quantlib"]["version"] != "1.44":
            raise ValueError(f"QuantLib 1.44 headers required: {info['quantlib']}")
        for index, row in enumerate(report["cases"]):
            if row["status"] != "pending":
                continue
            identifier = row["id"]
            if identifier not in references:
                row.update(status="failed", error="missing reference")
            else:
                report["cases"][index] = measure(
                    cases[identifier], references[identifier], executables, args, info
                )
        if report["cases"] and all(
            case["status"] == "passed" for case in report["cases"]
        ):
            report["status"] = "passed"
    except (
        RuntimeError,
        ValueError,
        OSError,
        KeyError,
        TypeError,
        OverflowError,
    ) as error:
        report["error"] = str(error)
        print(f"benchmark failed: {error}", file=sys.stderr)
        for row in report["cases"]:
            if row["status"] == "pending":
                row.update(status="prerequisite_failed", error=str(error))
    table(report["cases"])
    report["wall_seconds"] = time.monotonic() - start
    write_report(args.report, report)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
