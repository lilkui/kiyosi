"""Real native comparisons for the committed quadrature and CRR cases."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import RUST_EXECUTABLE, native

ROOT = Path(__file__).resolve().parents[2]


class NumericalNativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--price-only",
                    "--case",
                    "european-bsm-atm-call",
                    "--report",
                    str(Path(directory) / "build.json"),
                ],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
                timeout=600,
            )
            if result.returncode:
                raise RuntimeError(result.stderr + result.stdout)
        cls.executable = RUST_EXECUTABLE
        cls.quantlib = ROOT / "benchmarks/quantlib/build/quantlib_bench"
        cls.cases = json.loads(
            (ROOT / "benchmarks/quantlib/cases/numerical.json").read_text()
        )["cases"]

    def test_quadrature_vanilla_matches_independent_analytic_price(self):
        result = subprocess.run(
            [
                str(self.executable),
                "family=quadrature_vanilla",
                "iterations=1",
                "option=call",
                "valuation=2025-01-01",
                "expiry=2026-01-01",
                "spot=100",
                "strike=100",
                "rate=0.05",
                "dividend=0.02",
                "volatility=0.2",
                "asset_step_count=512",
                "time_step_count=1",
                "integration_standard_deviations=8",
            ],
            text=True,
            capture_output=True,
            check=False,
            timeout=120,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertAlmostEqual(
            json.loads(result.stdout)["price"], 9.227005508154036, places=6
        )

    def test_crr_european_matches_independent_analytic_price(self):
        result = subprocess.run(
            [
                str(self.executable),
                "family=crr_vanilla",
                "iterations=1",
                "option=call",
                "valuation=2025-01-01",
                "expiry=2026-01-01",
                "spot=100",
                "strike=100",
                "rate=0.05",
                "dividend=0.02",
                "volatility=0.2",
                "exercise=european",
                "step_count=1024",
            ],
            text=True,
            capture_output=True,
            check=False,
            timeout=120,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertLess(
            abs(json.loads(result.stdout)["price"] - 9.227005508154036), 0.005
        )

    def test_all_committed_profiles_pass_real_native_accuracy_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--no-build",
                    "--price-only",
                    "--cases",
                    str(ROOT / "benchmarks/quantlib/cases/numerical.json"),
                    *(
                        arg
                        for identifier in (
                            "quadrature-bsm-european-call",
                            "quadrature-bsm-cash-call",
                            "quadrature-bsm-asset-put",
                            "crr-bsm-european-call",
                            "crr-bsm-american-put",
                            "crr-bsm-down-out-call",
                        )
                        for arg in ("--case", identifier)
                    ),
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
            self.assertEqual(len(cases), 6)
            for case in cases:
                for backend in ("kiyosi", "quantlib"):
                    self.assertTrue(
                        case["backends"][backend]["accuracy"]["passed"], case
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
