"""Native Heston accuracy, coverage and report contracts (QuantLib 1.44)."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


class HestonTests(unittest.TestCase):
    def test_native_heston_coverage_and_failed_profile_suppresses_ratio(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            arguments = [
                sys.executable,
                str(HERE / "run.py"),
                "--cases",
                str(HERE / "cases/heston.json"),
                *(
                    arg
                    for identifier in (
                        "heston-analytic-call",
                        "heston-fd-european-call",
                        "heston-fd-american-put",
                    )
                    for arg in ("--case", identifier)
                ),
                "--references",
                str(HERE / "references/heston.json"),
                "--target-ms",
                "1",
                "--report",
                str(report),
            ]
            result = subprocess.run(
                arguments,
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            data = json.loads(report.read_text())
            self.assertEqual(
                {row["family"] for row in data["cases"]},
                {
                    "heston_analytic",
                    "heston_fd_european",
                    "heston_fd_american",
                },
            )
            for row in data["cases"]:
                self.assertEqual(row["status"], "passed")
                self.assertIsNotNone(row["ratio"])
                for backend in row["backends"].values():
                    self.assertTrue(backend["accuracy"]["passed"])
                    self.assertEqual(len(backend["timing"]["samples"]), 15)
            cases = json.loads((HERE / "cases/heston.json").read_text())
            cases["cases"] = [
                next(c for c in cases["cases"] if c["family"] == "heston_fd_european")
            ]
            cases["cases"][0]["profiles"]["kiyosi"].update(
                asset_steps=10, variance_steps=5, time_steps=5
            )
            path = Path(directory) / "coarse.json"
            path.write_text(json.dumps(cases))
            arguments[arguments.index("--cases") + 1] = str(path)
            result = subprocess.run(
                [
                    *arguments[: arguments.index("--case")],
                    "--case",
                    "heston-fd-european-call",
                    *arguments[arguments.index("--references") :],
                    "--no-build",
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            row = json.loads(report.read_text())["cases"][0]
            self.assertEqual(row["status"], "accuracy_failed")
            self.assertIsNone(row["ratio"])
            self.assertTrue(all("timing" not in b for b in row["backends"].values()))


if __name__ == "__main__":
    unittest.main()
