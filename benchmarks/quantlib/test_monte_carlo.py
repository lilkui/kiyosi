"""Monte Carlo accuracy and precision through the public CLI/report seam."""

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


class MonteCarloReportTests(unittest.TestCase):
    def fixture(self, noise=0.08, bias=0, *, include_policy=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / "native"
            executable.write_text(
                "#!/usr/bin/env python3\n"
                "import json,sys\n"
                "if sys.argv[1:] == ['--info']:\n"
                " print(json.dumps({'version':'1.44','families':['mc_european_bsm']}))\n"
                "else:\n"
                " p=dict(x.split('=',1) for x in sys.argv[1:])\n"
                " seed=int(p['seed'])\n"
                f" price=10 if seed==1729 else 10+{bias}+({noise} if seed%2 else -{noise})\n"
                " print(json.dumps({'price':price,'elapsed_ns':100000000,'iterations':int(p['iterations']),'warmup_iterations':3}))\n"
            )
            executable.chmod(0o755)
            case = {
                "id": "mc-fixture",
                "family": "mc_european_bsm",
                "engine": "MonteCarloVanillaEngine",
                "model": "bsm",
                "inputs": {},
                "profiles": {b: {"seed": 1729} for b in ("kiyosi", "quantlib")},
                "error_budget": 0.1,
                "mc_validation": {
                    "seeds": list(range(1, 33)),
                    "timing_seed": 1729,
                    "max_rmse": 0.09,
                    "max_abs_bias": 0.025,
                    "max_sample_stddev": 0.05,
                },
            }
            if not include_policy:
                del case["mc_validation"]
            cases = root / "cases.json"
            cases.write_text(json.dumps({"schema_version": 1, "cases": [case]}))
            references = root / "references.json"
            references.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "cases": [
                            {"id": "mc-fixture", "value": 10, "uncertainty": 1e-12}
                        ],
                    }
                )
            )
            report = root / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(HERE / "run.py"),
                    "--no-build",
                    "--rust-executable",
                    str(executable),
                    "--quantlib-executable",
                    str(executable),
                    "--case",
                    "mc-fixture",
                    "--cases",
                    str(cases),
                    "--references",
                    str(references),
                    "--report",
                    str(report),
                ],
                capture_output=True,
                text=True,
                check=False,
                timeout=600,
            )
            self.assertTrue(report.is_file(), result.stdout + result.stderr)
            rows = json.loads(report.read_text())["cases"]
            self.assertEqual([row["id"] for row in rows], [case["id"]])
            row = rows[0]
            self.assertCountEqual(row["backends"], ("kiyosi", "quantlib"))
            return result, row

    def test_missing_seed_policy_prevents_timing(self):
        result, data = self.fixture(include_policy=False)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertEqual(data["status"], "failed")
        self.assertIn("require mc_validation", data["error"])
        self.assertIsNone(data["ratio"])
        for backend in data["backends"].values():
            self.assertTrue(backend["accuracy"]["passed"])
            self.assertNotIn("timing", backend)

    def test_wide_sampling_dispersion_suppresses_ratio(self):
        result, data = self.fixture()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(data["status"], "precision_failed")
        self.assertIsNone(data["ratio"])
        for backend in data["backends"].values():
            self.assertTrue(backend["accuracy"]["passed"])
            self.assertAlmostEqual(backend["seed_ensemble"]["rmse"], 0.08)
            self.assertGreater(backend["seed_ensemble"]["sample_stddev"], 0.08)
            self.assertNotIn("timing", backend)

    def test_ensemble_bias_fails_despite_accurate_timing_seed(self):
        result, data = self.fixture(noise=0, bias=0.06)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(data["status"], "accuracy_failed")
        self.assertIsNone(data["ratio"])
        for backend in data["backends"].values():
            ensemble = backend["seed_ensemble"]
            self.assertTrue(ensemble["precision_passed"])
            self.assertAlmostEqual(ensemble["signed_bias"], 0.06)
            self.assertNotIn("timing", backend)

    def test_large_finite_seed_errors_keep_a_finite_rmse_in_the_report(self):
        result, data = self.fixture(noise=1e155)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(data["status"], "precision_failed")
        self.assertIsNone(data["ratio"])
        for backend in data["backends"].values():
            self.assertAlmostEqual(backend["seed_ensemble"]["rmse"] / 1e155, 1)

    def test_passing_ensemble_retains_every_seed_and_fifteen_timing_pairs(self):
        result, data = self.fixture(noise=0.02)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertEqual(data["status"], "passed")
        self.assertEqual(data["ratio"], 1)
        for backend in data["backends"].values():
            ensemble = backend["seed_ensemble"]
            self.assertEqual(
                [row["seed"] for row in ensemble["samples"]], list(range(1, 33))
            )
            self.assertAlmostEqual(ensemble["signed_bias"], 0)
            self.assertAlmostEqual(ensemble["rmse"], 0.02)
            self.assertEqual(len(backend["timing"]["samples"]), 15)


class MonteCarloNativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from run import RUST_EXECUTABLE, load_entries

        cls.cases = load_entries(HERE / "cases/monte_carlo.json")
        cls.executables = {
            "kiyosi": RUST_EXECUTABLE,
            "quantlib": HERE / "build/quantlib_bench",
        }
        if not all(path.exists() for path in cls.executables.values()):
            raise RuntimeError(
                "build native companions with run.py before running native MC tests"
            )

    def test_fixed_seed_replay_and_policy_enforcement(self):
        from run import native

        for case in self.cases.values():
            for backend, binary in self.executables.items():
                with self.subTest(case=case["id"], backend=backend):
                    first = native(binary, case, backend, 1)
                    self.assertEqual(
                        first["price"], native(binary, case, backend, 1)["price"]
                    )
                    if case["family"] == "mc_barrier_bsm":
                        invalid = copy.deepcopy(case)
                        invalid["profiles"][backend]["seed"] = 5
                        with self.assertRaisesRegex(RuntimeError, "couples QuantLib"):
                            native(binary, invalid, backend, 1)
                    policy = "MC benchmark requires one worker"
                    for key, value, rust_error, quantlib_error in (
                        ("workers", 2, policy, policy),
                        ("seed", 0, policy, "seed$"),
                        ("seed", 4294967301, "number too large", "seed$"),
                        (
                            "path_count",
                            10000001,
                            "path_count must be between",
                            "path_count$",
                        ),
                        ("antithetic", "true", policy, policy),
                        ("control_variate", "true", policy, policy),
                        ("sampling", "sobol", policy, policy),
                        ("path_count", 1.5, "invalid digit", "path_count$"),
                    ):
                        invalid = copy.deepcopy(case)
                        invalid["profiles"][backend][key] = value
                        diagnostic = (
                            rust_error if backend == "kiyosi" else quantlib_error
                        )
                        with (
                            self.subTest(key=key, value=value),
                            self.assertRaisesRegex(RuntimeError, diagnostic),
                        ):
                            native(binary, invalid, backend, 1)

    def test_committed_profiles_pass_live_ensemble_gate(self):
        case_ids = (
            "mc-european-call",
            "mc-european-put",
            "mc-barrier-down-out-call",
            "mc-barrier-up-in-put",
        )
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(HERE / "run.py"),
                    "--no-build",
                    "--price-only",
                    "--cases",
                    str(HERE / "cases/monte_carlo.json"),
                    *(
                        argument
                        for identifier in case_ids
                        for argument in ("--case", identifier)
                    ),
                    "--report",
                    str(report),
                ],
                capture_output=True,
                text=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            rows = json.loads(report.read_text())["cases"]
            self.assertCountEqual([row["id"] for row in rows], case_ids)
            for row in rows:
                with self.subTest(case=row["id"]):
                    self.assertCountEqual(row["backends"], ("kiyosi", "quantlib"))
                    for name, backend in row["backends"].items():
                        with self.subTest(backend=name):
                            ensemble = backend["seed_ensemble"]
                            self.assertEqual(len(ensemble["samples"]), 32)
                            self.assertTrue(ensemble["passed"], ensemble)


if __name__ == "__main__":
    unittest.main()
