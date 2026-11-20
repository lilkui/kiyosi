"""Real native contract and benchmark-report checks (requires QuantLib 1.44)."""

import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import HERE, ROOT, RUST_EXECUTABLE

CASE_IDS = (
    "barrier-down-out-call-rebate",
    "barrier-up-in-put-rebate",
    "binary-down-out-cash-call",
    "binary-up-in-asset-put",
    "touch-up-in-cash-hit",
    "touch-up-in-cash-expiry",
    "touch-up-out-cash-expiry",
    "touch-up-in-asset-hit",
    "touch-up-in-asset-expiry",
    "touch-up-out-asset-expiry",
    "double-barrier-out-call",
    "double-barrier-in-put",
    "double-touch-in-cash-expiry",
    "double-touch-out-cash-expiry",
)


class NativeBarrierTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        directory = Path(cls.enterClassContext(tempfile.TemporaryDirectory()))
        report = directory / "report.json"
        quantlib = directory / "quantlib_bench"
        result = subprocess.run(
            [
                sys.executable,
                str(HERE / "run.py"),
                "--quantlib-executable",
                str(quantlib),
                "--cases",
                str(HERE / "cases/barriers.json"),
                *(arg for identifier in CASE_IDS for arg in ("--case", identifier)),
                "--references",
                str(HERE / "references/barriers.json"),
                "--target-ms",
                "1",
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
            raise RuntimeError(result.stdout + result.stderr)
        cls.rows = json.loads(report.read_text())["cases"]
        cls.cases = {row["id"]: row for row in cls.rows}
        cls.executables = {"kiyosi": RUST_EXECUTABLE, "quantlib": quantlib}

    def test_every_barrier_family_passes_shared_reference_and_fifteen_pairs(self):
        self.assertCountEqual([row["id"] for row in self.rows], CASE_IDS)
        for row in self.rows:
            with self.subTest(case=row["id"]):
                self.assertEqual(row["status"], "passed")
                self.assertIsNotNone(row["ratio"])
                self.assertCountEqual(row["backends"], self.executables)
                for name, backend in row["backends"].items():
                    with self.subTest(backend=name):
                        self.assertTrue(backend["accuracy"]["passed"])
                        self.assertEqual(len(backend["timing"]["samples"]), 15)

    def test_native_touch_payoffs_obey_parity_and_settlement_identities(self):
        for backend in self.executables:
            with self.subTest(backend=backend):

                def price(identifier, backend=backend):
                    return self.cases[identifier]["backends"][backend]["accuracy"][
                        "price"
                    ]

                for payoff, discounted_total in (
                    ("cash", 10 * math.exp(-0.04)),
                    ("asset", 100 * math.exp(-0.01)),
                ):
                    self.assertAlmostEqual(
                        price(f"touch-up-in-{payoff}-expiry")
                        + price(f"touch-up-out-{payoff}-expiry"),
                        discounted_total,
                        places=10,
                    )
                self.assertAlmostEqual(
                    price("double-touch-in-cash-expiry")
                    + price("double-touch-out-cash-expiry"),
                    10 * math.exp(-0.04),
                    places=10,
                )
                # At first passage the asset transfer is worth the barrier level, 120.
                self.assertAlmostEqual(
                    price("touch-up-in-asset-hit"),
                    12 * price("touch-up-in-cash-hit"),
                    places=10,
                )
                self.assertGreater(
                    price("touch-up-in-cash-hit"), price("touch-up-in-cash-expiry")
                )

    def test_native_adapters_reject_spot_on_untouched_barrier(self):
        case = self.cases["touch-up-in-cash-hit"]
        for backend, executable in self.executables.items():
            with self.subTest(backend=backend):
                parameters = {
                    "family": case["family"],
                    "iterations": 1,
                    **case["inputs"],
                    **case["profiles"][backend],
                    "spot": case["inputs"]["barrier"],
                }
                invalid = subprocess.run(
                    [str(executable), *(f"{k}={v}" for k, v in parameters.items())],
                    text=True,
                    capture_output=True,
                    check=False,
                    timeout=120,
                )
                self.assertEqual(invalid.returncode, 1, invalid.stderr)
                self.assertIn("untouched", invalid.stderr)

    def test_quantlib_rejects_invalid_double_barrier_series_counts(self):
        case = self.cases["double-barrier-out-call"]
        for series in (2.5, -1, 1001):
            with self.subTest(series=series):
                parameters = {
                    "family": case["family"],
                    "iterations": 1,
                    **case["inputs"],
                    "series": series,
                }
                invalid = subprocess.run(
                    [
                        str(self.executables["quantlib"]),
                        *(f"{k}={v}" for k, v in parameters.items()),
                    ],
                    text=True,
                    capture_output=True,
                    check=False,
                    timeout=120,
                )
                self.assertEqual(invalid.returncode, 1, invalid.stderr)
                self.assertIn("series", invalid.stderr)


if __name__ == "__main__":
    unittest.main()
