"""Contract checks at the native pricing and benchmark report seams."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import RUST_EXECUTABLE, command, rust_build_command
from test_fd import build_quantlib_test_executable

ROOT = Path(__file__).resolve().parents[2]


class DigitalAsianTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        command(rust_build_command(), timeout=600)
        cls.executable = RUST_EXECUTABLE

    def test_cash_digital_native_price_matches_independent_formula(self):
        result = subprocess.run(
            [
                str(self.executable),
                "family=digital_bsm",
                "iterations=100",
                "option=call",
                "valuation=2025-01-01",
                "expiry=2026-01-01",
                "spot=100",
                "strike=100",
                "rate=0.04",
                "dividend=0.01",
                "volatility=0.2",
                "payout=cash",
                "cash=10",
            ],
            text=True,
            capture_output=True,
            check=False,
            timeout=120,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        data = json.loads(result.stdout)
        self.assertAlmostEqual(data["price"], 4.995517136549785, places=11)
        self.assertEqual(data["iterations"], 100)
        self.assertGreater(data["elapsed_ns"], 0)

    def test_continuous_asian_native_prices_match_independent_references(self):
        for family, expected in (
            ("geometric_asian_bsm", 5.035867546515384),
            ("arithmetic_asian_bsm", 5.246501860362777),
        ):
            with self.subTest(family=family):
                result = subprocess.run(
                    [
                        str(self.executable),
                        f"family={family}",
                        "iterations=100",
                        "option=call",
                        "valuation=2025-01-01",
                        "expiry=2026-01-01",
                        "averaging_start=2025-01-01",
                        "spot=100",
                        "strike=100",
                        "rate=0.04",
                        "dividend=0.01",
                        "volatility=0.2",
                    ],
                    text=True,
                    capture_output=True,
                    check=False,
                    timeout=120,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertAlmostEqual(
                    json.loads(result.stdout)["price"], expected, places=10
                )

    def test_asian_subset_rejects_forward_start_and_singular_carry(self):
        common = {
            "family": "arithmetic_asian_bsm",
            "iterations": 1,
            "option": "call",
            "valuation": "2025-01-01",
            "expiry": "2026-01-01",
            "averaging_start": "2025-01-01",
            "spot": 100,
            "strike": 100,
            "rate": 0.04,
            "dividend": 0.01,
            "volatility": 0.2,
        }
        for changes, diagnostic in (
            ({"averaging_start": "2025-02-01"}, "averaging_start"),
            ({"dividend": 0.04}, "ordinary carry"),
            ({"dividend": 0.08}, "Levy denominators"),
            ({"dividend": 0.06}, "Levy denominators"),
            ({"volatility": 0}, "positive"),
        ):
            with self.subTest(changes=changes):
                result = subprocess.run(
                    [
                        str(self.executable),
                        *(
                            f"{key}={value}"
                            for key, value in (common | changes).items()
                        ),
                    ],
                    text=True,
                    capture_output=True,
                    check=False,
                    timeout=120,
                )
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn(diagnostic, result.stderr)


@unittest.skipUnless(
    os.environ.get("KIYOSI_QUANTLIB_LIVE"), "requires installed QuantLib 1.44"
)
class QuantLibDigitalAsianTests(DigitalAsianTests):
    @classmethod
    def setUpClass(cls):
        cls.executable = build_quantlib_test_executable(cls)

    def test_live_accuracy_failure_suppresses_timing_and_ratio(self):
        with tempfile.TemporaryDirectory() as directory:
            references = json.loads(
                (
                    ROOT / "benchmarks/quantlib/references/digitals_asian.json"
                ).read_text()
            )
            references["cases"][0]["value"] += 1
            path = Path(directory) / "references.json"
            path.write_text(json.dumps(references))
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "benchmarks/quantlib/run.py"),
                    "--no-build",
                    "--quantlib-executable",
                    str(self.executable),
                    "--case",
                    references["cases"][0]["id"],
                    "--references",
                    str(path),
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
