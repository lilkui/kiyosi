"""American LSM contract behavior through the native CLI and report."""

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import HERE, load_entries, native, prebuilt_rust_executable


class AmericanMonteCarloTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.case = load_entries(HERE / "cases/american_monte_carlo.json")[
            "mc-american-put"
        ]
        cls.executables = {
            "kiyosi": prebuilt_rust_executable(),
            "quantlib": HERE / "build/quantlib_bench",
        }

    def test_genuine_early_exercise_and_fresh_seed_replay(self):
        for backend, executable in self.executables.items():
            with self.subTest(backend=backend):
                first = native(executable, self.case, backend, 1)
                self.assertGreater(first["price"], 6.330080627549918 + 0.15)
                self.assertAlmostEqual(first["price"], 6.660684545893456, delta=0.15)
                self.assertEqual(
                    first["price"], native(executable, self.case, backend, 1)["price"]
                )

    def test_insufficient_paths_fail_live_gate_without_timing_or_ratio(self):
        case = copy.deepcopy(self.case)
        for profile in case["profiles"].values():
            profile["path_count"] = 16
        case["profiles"]["quantlib"]["calibration_path_count"] = 16
        # A loose scalar check lets the live dispersion gate reject this noisy profile.
        case["error_budget"] = 100
        case["mc_validation"].update(max_rmse=100, max_abs_bias=100)
        with tempfile.TemporaryDirectory() as directory:
            cases = Path(directory) / "cases.json"
            report = Path(directory) / "report.json"
            cases.write_text(json.dumps({"schema_version": 1, "cases": [case]}))
            result = subprocess.run(
                [
                    sys.executable,
                    str(HERE / "run.py"),
                    "--no-build",
                    "--case",
                    case["id"],
                    "--cases",
                    str(cases),
                    "--report",
                    str(report),
                ],
                capture_output=True,
                text=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            rows = json.loads(report.read_text())["cases"]
            self.assertEqual([row["id"] for row in rows], [case["id"]])
            data = rows[0]
            self.assertIsNone(data["ratio"])
            self.assertEqual(data["status"], "precision_failed")
            self.assertCountEqual(data["backends"], ("kiyosi", "quantlib"))
            for name, backend in data["backends"].items():
                ensemble = backend["seed_ensemble"]
                self.assertFalse(ensemble["passed"])
                self.assertFalse(ensemble["precision_passed"])
                self.assertNotIn("timing", backend)
                self.assertEqual(len(ensemble["samples"]), 32)
                for sample in ensemble["samples"]:
                    if name == "quantlib":
                        self.assertEqual(
                            sample["profile"]["calibration_seed"],
                            sample["seed"] + 10000,
                        )
                    else:
                        self.assertNotIn("calibration_seed", sample["profile"])

    def test_native_work_policy_rejects_invalid_profiles(self):
        rust_policy = "American MC requires a nonzero seed"
        quantlib_policy = "American MC requires one worker"
        for backend, executable in self.executables.items():
            for key, value, rust_error, quantlib_error in (
                ("workers", 0, "workers must be positive", quantlib_policy),
                ("seed", 0, rust_policy, "seed$"),
                ("seed", 4294967296, "number too large", "seed$"),
                ("step_count", 1, rust_policy, quantlib_policy),
                ("step_count", 2.5, "invalid digit", "step_count$"),
                ("path_count", 10000001, rust_policy, "path_count$"),
                ("basis", "laguerre", rust_policy, quantlib_policy),
                ("antithetic", "true", rust_policy, quantlib_policy),
            ):
                invalid = copy.deepcopy(self.case)
                invalid["profiles"][backend][key] = value
                diagnostic = rust_error if backend == "kiyosi" else quantlib_error
                with (
                    self.subTest(backend=backend, key=key, value=value),
                    self.assertRaisesRegex(RuntimeError, diagnostic),
                ):
                    native(executable, invalid, backend, 1)
        invalid = copy.deepcopy(self.case)
        invalid["profiles"]["quantlib"]["workers"] = 2
        with self.assertRaisesRegex(RuntimeError, quantlib_policy):
            native(self.executables["quantlib"], invalid, "quantlib", 1)
        invalid["profiles"]["quantlib"]["workers"] = 1
        invalid["profiles"]["quantlib"]["calibration_seed"] = 1729
        with self.assertRaisesRegex(RuntimeError, "independent nonzero seeds"):
            native(self.executables["quantlib"], invalid, "quantlib", 1)


if __name__ == "__main__":
    unittest.main()
