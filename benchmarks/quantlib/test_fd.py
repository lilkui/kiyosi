"""Native finite-difference contract and report checks."""

import json
import os
import shlex
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import ClassVar

from calibrate_fd import domain_probes
from run import RUST_EXECUTABLE, command, rust_build_command

ROOT = Path(__file__).resolve().parents[2]


def build_quantlib_test_executable(test_class):
    directory = test_class.enterClassContext(tempfile.TemporaryDirectory())
    executable = Path(directory) / "quantlib_bench"
    config = os.environ.get("QUANTLIB_CONFIG", "quantlib-config")

    def flags(flag):
        return shlex.split(
            subprocess.check_output([config, flag], text=True, timeout=30)
        )

    subprocess.run(
        [
            "c++",
            "-std=c++17",
            "-O3",
            "-DNDEBUG",
            *flags("--cflags"),
            str(ROOT / "benchmarks/quantlib/quantlib.cpp"),
            "-o",
            str(executable),
            *flags("--libs"),
        ],
        check=True,
        capture_output=True,
        timeout=600,
    )

    return executable


class FiniteDifferenceTests(unittest.TestCase):
    backend = "kiyosi"
    profile: ClassVar[dict] = {"scheme": "rannacher", "upper": 400}

    @classmethod
    def setUpClass(cls):
        command(rust_build_command(), timeout=600)
        cls.executable = RUST_EXECUTABLE

    def invoke(self, **changes):
        arguments = {
            "family": "fd_bsm",
            "iterations": 1,
            "product": "vanilla",
            "exercise": "european",
            "option": "call",
            "valuation": "2025-01-01",
            "expiry": "2026-01-01",
            "spot": 100,
            "strike": 100,
            "rate": 0.05,
            "dividend": 0.02,
            "volatility": 0.2,
            "space": 800,
            "time": 400,
            **self.profile,
        }
        arguments.update(changes)
        return subprocess.run(
            [
                str(self.executable),
                *(f"{key}={value}" for key, value in arguments.items()),
            ],
            text=True,
            capture_output=True,
            check=False,
            timeout=120,
        )

    def test_european_and_american_exercise(self):
        european = self.invoke(option="put")
        self.assertEqual(european.returncode, 0, european.stderr)
        american = self.invoke(option="put", exercise="american")
        self.assertEqual(american.returncode, 0, american.stderr)
        self.assertAlmostEqual(
            json.loads(european.stdout)["price"], 6.330080627549918, delta=0.003
        )
        self.assertAlmostEqual(
            json.loads(american.stdout)["price"], 6.660684545893456, delta=0.003
        )

    def test_automatic_domain(self):
        key = "upper" if self.backend == "kiyosi" else "domain"
        result = self.invoke(**{key: "automatic"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertAlmostEqual(
            json.loads(result.stdout)["price"], 9.227005508154036, delta=0.0001
        )

    def test_invalid_domain_is_rejected(self):
        key = "upper" if self.backend == "kiyosi" else "domain"
        for value, rust_error in (
            ("auto", "invalid float literal"),
            ("True", "invalid float literal"),
            ("-1", "asset_upper_boundary must be finite and positive"),
            ("NaN", "asset_upper_boundary must be finite and positive"),
            ("inf", "asset_upper_boundary must be finite and positive"),
        ):
            with self.subTest(value=value):
                result = self.invoke(**{key: value})
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn(
                    rust_error
                    if self.backend == "kiyosi"
                    else "FD domain must be automatic",
                    result.stderr,
                )

    def test_all_product_profiles_against_independent_references(self):
        cases = json.loads(
            (ROOT / "benchmarks/quantlib/cases/fd_bsm.json").read_text()
        )["cases"]
        references = {
            row["id"]: row
            for row in json.loads(
                (ROOT / "benchmarks/quantlib/references/fd_bsm.json").read_text()
            )["cases"]
        }
        for case in cases:
            with self.subTest(case=case["id"]):
                params = {
                    "family": case["family"],
                    "iterations": 1,
                    **case["inputs"],
                    **case["profiles"][self.backend],
                }
                result = subprocess.run(
                    [
                        str(self.executable),
                        *(f"{key}={value}" for key, value in params.items()),
                    ],
                    text=True,
                    capture_output=True,
                    check=False,
                    timeout=120,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                reference = references[case["id"]]
                self.assertLessEqual(
                    abs(json.loads(result.stdout)["price"] - reference["value"])
                    + reference["uncertainty"],
                    case["error_budget"],
                )
                if case["inputs"]["product"] == "touch":
                    params["settlement"] = (
                        "at_expiry" if params["settlement"] == "at_hit" else "at_hit"
                    )
                    rejected = subprocess.run(
                        [
                            str(self.executable),
                            *(f"{key}={value}" for key, value in params.items()),
                        ],
                        text=True,
                        capture_output=True,
                        check=False,
                        timeout=120,
                    )
                    self.assertEqual(rejected.returncode, 1, rejected.stderr)
                    self.assertIn("FD subset", rejected.stderr)
                if "barrier" in params:
                    params["spot"] = params["barrier"]
                    rejected = subprocess.run(
                        [
                            str(self.executable),
                            *(f"{key}={value}" for key, value in params.items()),
                        ],
                        text=True,
                        capture_output=True,
                        check=False,
                        timeout=120,
                    )
                    self.assertEqual(rejected.returncode, 1, rejected.stderr)
                    self.assertIn("untouched", rejected.stderr)


class DomainProbeTests(unittest.TestCase):
    def test_expansion_preserves_actual_uniform_spacing(self):
        for inputs, upper, count in [
            ({}, "automatic", 1200),
            ({"barrier": 80, "barrier_type": "down_out"}, 400, 1300),
            ({"barrier": 120, "barrier_type": "up_in"}, 400, 800),
        ]:
            with self.subTest(inputs=inputs):
                probes = domain_probes(
                    {
                        "id": "probe",
                        "inputs": inputs,
                        "profiles": {"kiyosi": {"space": 800, "upper": upper}},
                    }
                )
                self.assertEqual(probes["explicit_uniform"], {"upper": 400})
                self.assertEqual(
                    probes["domain_constant_spacing"], {"upper": 600, "space": count}
                )

    def test_local_knock_in_distinguishes_barrier_and_continuation_spacing(self):
        for direction, barrier, count in [("up_in", 120, 800), ("down_in", 80, 1300)]:
            with self.subTest(direction=direction):
                probes = domain_probes(
                    {
                        "id": "probe",
                        "family": "fd_local_vol",
                        "inputs": {"barrier": barrier, "barrier_type": direction},
                        "profiles": {"kiyosi": {"space": 800, "upper": 400}},
                    }
                )
                self.assertEqual(probes["domain_constant_spacing"]["space"], count)
                self.assertEqual(
                    probes["continuation_constant_spacing"],
                    {"upper": 600, "space": 1200},
                )


@unittest.skipUnless(
    os.environ.get("KIYOSI_QUANTLIB_LIVE"), "requires installed QuantLib 1.44"
)
class QuantLibFiniteDifferenceTests(FiniteDifferenceTests):
    backend = "quantlib"
    profile: ClassVar[dict] = {
        "scheme": "crank_nicolson",
        "damping": 2,
        "domain": "automatic",
    }

    @classmethod
    def setUpClass(cls):
        cls.executable = build_quantlib_test_executable(cls)

    def test_failed_reference_suppresses_timing_and_ratio(self):
        with tempfile.TemporaryDirectory() as directory:
            document = json.loads(
                (ROOT / "benchmarks/quantlib/references/fd_bsm.json").read_text()
            )
            document["cases"][0]["value"] += 1
            path = Path(directory) / "references.json"
            path.write_text(json.dumps(document))
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--no-build",
                    "--case",
                    document["cases"][0]["id"],
                    "--references",
                    str(path),
                    "--quantlib-executable",
                    str(self.executable),
                    "--report",
                    str(report),
                ],
                text=True,
                capture_output=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            case = json.loads(report.read_text())["cases"][0]
            self.assertEqual(case["status"], "accuracy_failed")
            self.assertIsNone(case["ratio"])
            for backend in case["backends"].values():
                self.assertFalse(backend["accuracy"]["passed"])
                self.assertNotIn("timing", backend)


if __name__ == "__main__":
    unittest.main()
