"""Real native contract and benchmark-report checks (requires QuantLib 1.44)."""

import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import RUST_EXECUTABLE

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


class NativeBarrierTests(unittest.TestCase):
    def test_every_barrier_family_passes_shared_reference_and_fifteen_pairs(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(HERE / "run.py"),
                    "--cases",
                    str(HERE / "cases/barriers.json"),
                    *(
                        arg
                        for identifier in (
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
                        for arg in ("--case", identifier)
                    ),
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
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            data = json.loads(report.read_text())
            cases = {row["id"]: row for row in data["cases"]}
            self.assertEqual(len(cases), 14)
            # These identities check activation, unconditional survival payoff,
            # asset transfer at continuous first passage, and payment discounting.
            for backend in ("kiyosi", "quantlib"):

                def price(identifier, backend=backend):
                    return cases[identifier]["backends"][backend]["accuracy"]["price"]

                for payoff, total in (
                    ("cash", 10 * math.exp(-0.04)),
                    ("asset", 100 * math.exp(-0.01)),
                ):
                    self.assertAlmostEqual(
                        price(f"touch-up-in-{payoff}-expiry")
                        + price(f"touch-up-out-{payoff}-expiry"),
                        total,
                        places=10,
                    )
                self.assertAlmostEqual(
                    price("double-touch-in-cash-expiry")
                    + price("double-touch-out-cash-expiry"),
                    10 * math.exp(-0.04),
                    places=10,
                )
                self.assertAlmostEqual(
                    price("touch-up-in-asset-hit"),
                    12 * price("touch-up-in-cash-hit"),
                    places=10,
                )
                self.assertGreater(
                    price("touch-up-in-cash-hit"), price("touch-up-in-cash-expiry")
                )
            executables = {
                "kiyosi": RUST_EXECUTABLE,
                "quantlib": HERE / "build/quantlib_bench",
            }
            for backend, executable in executables.items():
                case = cases["touch-up-in-cash-hit"]
                parameters = {
                    "family": case["family"],
                    "iterations": 1,
                    **case["inputs"],
                    **case["profiles"][backend],
                    "spot": 120,
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
            case = cases["double-barrier-out-call"]
            for series in (2.5, -1, 1001):
                parameters = {
                    "family": case["family"],
                    "iterations": 1,
                    **case["inputs"],
                    "series": series,
                }
                invalid = subprocess.run(
                    [
                        str(executables["quantlib"]),
                        *(f"{k}={v}" for k, v in parameters.items()),
                    ],
                    text=True,
                    capture_output=True,
                    check=False,
                    timeout=120,
                )
                self.assertEqual(invalid.returncode, 1, invalid.stderr)
                self.assertIn("series", invalid.stderr)
            for row in cases.values():
                self.assertEqual(row["status"], "passed")
                self.assertIsNotNone(row["ratio"])
                for backend in row["backends"].values():
                    self.assertEqual(len(backend["timing"]["samples"]), 15)


if __name__ == "__main__":
    unittest.main()
