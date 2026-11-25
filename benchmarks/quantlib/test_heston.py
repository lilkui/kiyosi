"""Native Heston accuracy, coverage and report contracts (QuantLib 1.44)."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import (
    DEFAULT_QUANTLIB_EXECUTABLE,
    HERE,
    ROOT,
    load_entries,
    prebuilt_rust_executable,
)


class HestonTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rust_executable = prebuilt_rust_executable()
        cls.executable = DEFAULT_QUANTLIB_EXECUTABLE

    def run_heston(self, cases, case_ids, report):
        return subprocess.run(
            [
                sys.executable,
                str(HERE / "run.py"),
                "--no-build",
                "--rust-executable",
                str(self.rust_executable),
                "--quantlib-executable",
                str(self.executable),
                "--cases",
                str(cases),
                *(arg for identifier in case_ids for arg in ("--case", identifier)),
                "--references",
                str(HERE / "references/heston.json"),
                "--target-ms",
                "1",
                "--report",
                str(report),
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
            timeout=600,
        )

    def test_native_heston_coverage_passes_benchmark_gate(self):
        case_ids = (
            "heston-analytic-call",
            "heston-fd-european-call",
            "heston-fd-american-put",
        )
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = self.run_heston(HERE / "cases/heston.json", case_ids, report)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            rows = json.loads(report.read_text())["cases"]
        self.assertCountEqual([row["id"] for row in rows], case_ids)
        self.assertCountEqual(
            [row["family"] for row in rows],
            ("heston_analytic", "heston_fd_european", "heston_fd_american"),
        )
        for row in rows:
            with self.subTest(case=row["id"]):
                self.assertEqual(row["status"], "passed")
                self.assertIsNotNone(row["ratio"])
                self.assertCountEqual(row["backends"], ("kiyosi", "quantlib"))
                for backend in row["backends"].values():
                    self.assertTrue(backend["accuracy"]["passed"])
                    self.assertEqual(len(backend["timing"]["samples"]), 15)

    def test_inaccurate_heston_profile_suppresses_timing_and_ratio(self):
        case_id = "heston-fd-european-call"
        case = load_entries(HERE / "cases/heston.json")[case_id]
        case["profiles"]["kiyosi"].update(
            asset_steps=10, variance_steps=5, time_steps=5
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "coarse.json"
            path.write_text(json.dumps({"schema_version": 1, "cases": [case]}))
            report = Path(directory) / "report.json"
            result = self.run_heston(path, (case_id,), report)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            rows = json.loads(report.read_text())["cases"]
        self.assertEqual([row["id"] for row in rows], [case_id])
        row = rows[0]
        self.assertEqual(row["status"], "accuracy_failed")
        self.assertIsNone(row["ratio"])
        backends = row["backends"]
        self.assertCountEqual(backends, ("kiyosi", "quantlib"))
        self.assertFalse(backends["kiyosi"]["accuracy"]["passed"])
        self.assertTrue(backends["quantlib"]["accuracy"]["passed"])
        for backend in backends.values():
            self.assertNotIn("timing", backend)


if __name__ == "__main__":
    unittest.main()
