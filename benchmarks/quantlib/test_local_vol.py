"""Shared supplied local-volatility behavior at native command boundaries."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import test_fd

ROOT = test_fd.ROOT

SURFACE = {
    "surface_low": 80,
    "surface_high": 120,
    "surface_start_low": 0.30,
    "surface_start_high": 0.15,
    "surface_end_low": 0.35,
    "surface_end_high": 0.20,
}


class LocalVolatilityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        test_fd.FiniteDifferenceTests.setUpClass()
        cls.executable = test_fd.FiniteDifferenceTests.executable

    def test_supplied_surface_interpolates_and_clamps_spot(self):
        for spot, expected in [
            (60, 0.32493150684931504),
            (80, 0.32493150684931504),
            (100, 0.24993150684931506),
            (120, 0.17493150684931508),
            (160, 0.17493150684931508),
        ]:
            parameters = {
                "family": "fd_local_vol",
                "operation": "surface_point",
                "iterations": 1,
                "valuation": "2025-01-01",
                "expiry": "2026-01-01",
                "query_date": "2025-07-02",
                "query_spot": spot,
                **SURFACE,
            }
            result = subprocess.run(
                [str(self.executable), *(f"{k}={v}" for k, v in parameters.items())],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertAlmostEqual(
                json.loads(result.stdout)["price"], expected, places=14
            )

    def test_all_products_meet_independent_refined_reference(self):
        cases = json.loads(
            (ROOT / "benchmarks/quantlib/cases/fd_local_vol.json").read_text()
        )["cases"]
        references = {
            row["id"]: row
            for row in json.loads(
                (ROOT / "benchmarks/quantlib/references/fd_local_vol.json").read_text()
            )["cases"]
        }
        backend = getattr(self, "backend", "kiyosi")
        self.assertEqual(len(cases), 11)
        for case in cases:
            with self.subTest(case=case["id"]):
                parameters = {
                    "family": case["family"],
                    "iterations": 1,
                    **case["inputs"],
                    **case["profiles"][backend],
                }
                result = subprocess.run(
                    [
                        str(self.executable),
                        *(f"{k}={v}" for k, v in parameters.items()),
                    ],
                    text=True,
                    capture_output=True,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                reference = references[case["id"]]
                self.assertLessEqual(
                    reference["uncertainty"], case["error_budget"] * 0.1
                )
                self.assertLessEqual(
                    abs(json.loads(result.stdout)["price"] - reference["value"])
                    + reference["uncertainty"],
                    case["error_budget"],
                )


@unittest.skipUnless(
    os.environ.get("KIYOSI_QUANTLIB_LIVE"), "requires installed QuantLib 1.44"
)
class QuantLibLocalVolatilityTests(LocalVolatilityTests):
    backend = "quantlib"

    @classmethod
    def setUpClass(cls):
        test_fd.QuantLibFiniteDifferenceTests.setUpClass()
        cls.executable = test_fd.QuantLibFiniteDifferenceTests.executable

    def test_inaccurate_profile_suppresses_timing_in_report(self):
        case = json.loads(
            (ROOT / "benchmarks/quantlib/cases/fd_local_vol.json").read_text()
        )["cases"][0]
        for profile in case["profiles"].values():
            profile.update(space=10, time=10)
        with tempfile.TemporaryDirectory() as directory:
            cases = Path(directory) / "cases.json"
            cases.write_text(json.dumps({"schema_version": 1, "cases": [case]}))
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--no-build",
                    "--quantlib-executable",
                    str(self.executable),
                    "--case",
                    case["id"],
                    "--cases",
                    str(cases),
                    "--report",
                    str(report),
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            row = json.loads(report.read_text())["cases"][0]
            self.assertEqual(row["status"], "accuracy_failed")
            self.assertIsNone(row["ratio"])
            self.assertEqual(row["inputs"]["surface_start_low"], 0.30)
            for backend in row["backends"].values():
                self.assertNotIn("timing", backend)

    @classmethod
    def tearDownClass(cls):
        test_fd.QuantLibFiniteDifferenceTests.tearDownClass()


if __name__ == "__main__":
    unittest.main()
