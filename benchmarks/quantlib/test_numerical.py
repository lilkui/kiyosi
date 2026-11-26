"""Real native comparisons for the committed quadrature and CRR cases."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import native

ROOT = Path(__file__).resolve().parents[2]


class NumericalNativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "build.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--price-only",
                    "--case",
                    "european-bsm-atm-call",
                    "--report",
                    str(report),
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
                timeout=600,
            )
            if result.returncode:
                raise RuntimeError(result.stderr + result.stdout)
            build = json.loads(report.read_text())["build"]
            cls.executable = Path(build["executables"]["kiyosi"]["path"])
        cls.quantlib = ROOT / "benchmarks/quantlib/build/quantlib_bench"
        cls.cases = json.loads(
            (ROOT / "benchmarks/quantlib/cases/numerical.json").read_text()
        )["cases"]

    def test_all_committed_profiles_pass_real_native_accuracy_gate(self):
        case_ids = (
            "quadrature-bsm-european-call",
            "quadrature-bsm-cash-call",
            "quadrature-bsm-asset-put",
            "crr-bsm-european-call",
            "crr-bsm-american-put",
            "crr-bsm-down-out-call",
        )
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--no-build",
                    "--rust-executable",
                    str(self.executable),
                    "--price-only",
                    "--cases",
                    str(ROOT / "benchmarks/quantlib/cases/numerical.json"),
                    *(arg for identifier in case_ids for arg in ("--case", identifier)),
                    "--report",
                    str(report),
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            cases = json.loads(report.read_text())["cases"]
            self.assertCountEqual([case["id"] for case in cases], case_ids)
            for case in cases:
                for backend in ("kiyosi", "quantlib"):
                    self.assertTrue(
                        case["backends"][backend]["accuracy"]["passed"], case
                    )
                if case["id"] == "quadrature-bsm-european-call":
                    self.assertLessEqual(
                        case["backends"]["kiyosi"]["accuracy"]["absolute_error"],
                        5e-7,
                    )

    def test_unattainable_crr_budget_fails_without_timing_or_ratio(self):
        case = next(
            case for case in self.cases if case["id"] == "crr-bsm-european-call"
        )
        with tempfile.TemporaryDirectory() as directory:
            cases, report = (
                Path(directory) / name for name in ("cases.json", "report.json")
            )
            cases.write_text(
                json.dumps(
                    {"schema_version": 1, "cases": [case | {"error_budget": 1e-8}]}
                )
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--no-build",
                    "--rust-executable",
                    str(self.executable),
                    "--case",
                    case["id"],
                    "--cases",
                    str(cases),
                    "--report",
                    str(report),
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            data = json.loads(report.read_text())
            self.assertEqual(data["status"], "failed")
            case = data["cases"][0]
            self.assertEqual(case["status"], "accuracy_failed")
            self.assertIsNone(case["ratio"])
            for backend in ("kiyosi", "quantlib"):
                self.assertFalse(case["backends"][backend]["accuracy"]["passed"])
                self.assertNotIn("timing", case["backends"][backend])

    def test_both_native_backends_reject_fractional_negative_and_excessive_steps(self):
        case = next(
            case for case in self.cases if case["id"] == "crr-bsm-european-call"
        )
        for backend, binary in (
            ("kiyosi", self.executable),
            ("quantlib", self.quantlib),
        ):
            for count, rust_error in (
                (1.5, "invalid digit"),
                (-1, "step_count must be between"),
                (1000001, "step_count must be between"),
            ):
                diagnostic = (
                    rust_error
                    if backend == "kiyosi"
                    else "step_count must be an integer"
                )
                with self.subTest(backend=backend, count=count):
                    invalid = case | {"profiles": {backend: {"step_count": count}}}
                    with self.assertRaisesRegex(RuntimeError, diagnostic):
                        native(binary, invalid, backend, 1)


if __name__ == "__main__":
    unittest.main()
