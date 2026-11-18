"""Checks at the benchmark command and report boundaries."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run import RUST_EXECUTABLE, command, rust_build_command

RUNNER = Path(__file__).with_name("run.py")


class CommandTests(unittest.TestCase):
    def test_missing_native_executable_writes_failed_report(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER),
                    "--no-build",
                    "--rust-executable",
                    str(Path(directory) / "absent"),
                    "--report",
                    str(report),
                ],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            data = json.loads(report.read_text())
            self.assertEqual(data["status"], "failed")
            self.assertIn("executable", data["error"])
            self.assertEqual(len(data["cases"]), 59)
            self.assertTrue(
                all(row["status"] == "prerequisite_failed" for row in data["cases"])
            )

    def test_missing_build_prerequisite_keeps_complete_case_inventory(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER),
                    "--quantlib-config",
                    str(Path(directory) / "absent"),
                    "--report",
                    str(report),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            data = json.loads(report.read_text())
            self.assertEqual(len(data["cases"]), 59)
            self.assertTrue(
                all(row["status"] == "prerequisite_failed" for row in data["cases"])
            )
            self.assertTrue(all(row["ratio"] is None for row in data["cases"]))
            self.assertIn("missing executable", data["error"])


class NativeRustTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        command(rust_build_command())
        cls.executable = RUST_EXECUTABLE

    def test_fresh_native_batches_match_independent_bsm_reference(self):
        result = subprocess.run(
            [
                str(self.executable),
                "family=european_bsm",
                "iterations=100",
                "option=call",
                "valuation=2025-01-01",
                "expiry=2026-01-01",
                "spot=100",
                "strike=100",
                "rate=0.05",
                "dividend=0.02",
                "volatility=0.2",
            ],
            text=True,
            capture_output=True,
            check=True,
        )
        data = json.loads(result.stdout)
        self.assertAlmostEqual(data["price"], 9.227005508154036, places=11)
        self.assertEqual(data["iterations"], 100)
        self.assertGreater(data["elapsed_ns"], 0)
        result = subprocess.run(
            [str(self.executable), "family=unknown", "iterations=1"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown", result.stderr)


class ReportTests(unittest.TestCase):
    def run_fixture(
        self,
        reference=10,
        extra=(),
        version="1.44",
        complete=False,
        batch=None,
        patch=None,
        families=None,
        runtime_name=None,
    ):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            native = root / "native"
            families = ["european_bsm"] if families is None else families
            native.write_text(
                f"#!{sys.executable}\n"
                "import json,sys\n"
                "if sys.argv[1:] == ['--info']:\n"
                f" print(json.dumps({{'library':'fixture','version':{version!r},'families':{families!r}}}))\n"
                "else:\n"
                " p=dict(x.split('=',1) for x in sys.argv[1:])\n"
                " n=int(p['iterations'])\n"
                " from pathlib import Path\n"
                " state=Path(__file__).with_suffix('.count')\n"
                " count=int(state.read_text())+1 if state.exists() else 1\n"
                " state.write_text(str(count))\n"
                " latency=max(1,(count-3)//2)*100000000\n"
                f" print(json.dumps({batch!r} if {batch is not None!r} else {{'price':10,'elapsed_ns':n*latency,'iterations':n,'warmup_iterations':3}}))\n"
            )
            native.chmod(0o755)
            case = {
                "id": "fixture",
                "family": "european_bsm",
                "engine": "AnalyticEuropeanEngine",
                "model": "bsm",
                "inputs": {},
                "profiles": {"kiyosi": {}, "quantlib": {}},
                "error_budget": 0.01,
            }
            case.update(patch or {})
            cases = root / "cases.json"
            cases.write_text(json.dumps({"schema_version": 1, "cases": [case]}))
            refs = root / "refs.json"
            refs.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "cases": [
                            {
                                "id": "fixture",
                                "value": reference,
                                "uncertainty": 0.0001,
                                "provenance": {"method": "fixture"},
                            }
                        ],
                    }
                )
            )
            environment = None
            if runtime_name:
                import os

                library = root / runtime_name
                library.touch()
                loader = root / "ldd"
                loader.write_text(
                    f"#!{sys.executable}\nprint('libQuantLib.so.1 => {library} (0x1)')\n"
                )
                loader.chmod(0o755)
                environment = {
                    **os.environ,
                    "PATH": str(root) + os.pathsep + os.environ["PATH"],
                }
            report = root / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER),
                    "--no-build",
                    "--rust-executable",
                    str(native),
                    "--quantlib-executable",
                    str(native),
                    "--cases",
                    str(cases),
                    "--references",
                    str(refs),
                    "--report",
                    str(report),
                    *([] if complete else ["--case", "fixture"]),
                    *extra,
                ],
                text=True,
                capture_output=True,
                check=False,
                env=environment,
            )
            self.assertTrue(report.exists(), result.stderr)
            return result, json.loads(report.read_text())

    def test_default_rejects_incomplete_manifest_with_explicit_required_rows(self):
        result, report = self.run_fixture(complete=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(report["coverage"]["required_cells"], 51)
        self.assertEqual(report["coverage"]["covered_cells"], 1)
        missing = [c for c in report["cases"] if c["status"] == "missing_coverage"]
        self.assertEqual(len(missing), 50)
        self.assertTrue(all(c["ratio"] is None for c in missing))
        self.assertEqual(report["cases"][0]["status"], "passed")

    def test_malformed_native_numbers_never_produce_ratio(self):
        for key, value in (
            ("price", True),
            ("elapsed_ns", True),
            ("iterations", True),
            ("iterations", 1.5),
            ("warmup_iterations", 3.5),
            ("price", None),
        ):
            with self.subTest(key=key, value=value):
                batch = {
                    "price": 10,
                    "elapsed_ns": 100000000,
                    "iterations": 1,
                    "warmup_iterations": 3,
                    key: value,
                }
                result, report = self.run_fixture(batch=batch)
                self.assertNotEqual(result.returncode, 0)
                self.assertIsNone(report["cases"][0]["ratio"])
                self.assertIn("native result", report["cases"][0]["error"])

    def test_bool_reference_is_rejected(self):
        result, report = self.run_fixture(reference=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reference", report["cases"][0]["error"])

    def test_bool_error_budget_is_rejected(self):
        result, report = self.run_fixture(patch={"error_budget": True})
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(report["cases"][0]["ratio"])

    def test_missing_native_family_retains_unsupported_row(self):
        result, report = self.run_fixture(families=[])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(report["cases"][0]["status"], "unsupported")
        self.assertIn("kiyosi/european_bsm", report["cases"][0]["error"])

    def test_report_uses_all_fifteen_native_samples(self):
        result, report = self.run_fixture()
        self.assertEqual(result.returncode, 0, result.stderr)
        case = report["cases"][0]
        self.assertEqual(case["ratio"], 1)
        for backend in ("kiyosi", "quantlib"):
            timing = case["backends"][backend]["timing"]
            self.assertEqual(len(timing["samples"]), 15)
            self.assertEqual(timing["median_ns"], 800000000)
            self.assertEqual(timing["iqr_ns"], 700000000)

    def test_incompatible_native_version_fails(self):
        result, report = self.run_fixture(version="1.43")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("1.44", report["error"])
        self.assertEqual(report["cases"][0]["status"], "prerequisite_failed")
        self.assertIsNone(report["cases"][0]["ratio"])

    def test_identifiable_runtime_library_mismatch_fails_all_requests(self):
        for version in ("1.43", "2.0"):
            with self.subTest(version=version):
                result, report = self.run_fixture(
                    runtime_name=f"libQuantLib.so.{version}.0"
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("runtime filename", report["error"])
                self.assertEqual(report["cases"][0]["status"], "prerequisite_failed")
                self.assertEqual(
                    report["runtime_library"]["resolved"][0]["filename_version"],
                    version,
                )
                self.assertIn("unavailable", report["runtime_library"]["version_api"])

    def test_accuracy_failure_suppresses_ratio_and_fails(self):
        result, report = self.run_fixture(reference=11)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(report["cases"][0]["ratio"])
        self.assertEqual(report["cases"][0]["status"], "accuracy_failed")
        self.assertEqual(
            report["cases"][0]["backends"]["kiyosi"]["accuracy"]["absolute_error"], 1
        )

    def test_unknown_requested_case_fails_explicitly(self):
        result, report = self.run_fixture(extra=("--case", "absent"))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(report["cases"][-1]["id"], "absent")
        self.assertEqual(report["cases"][-1]["status"], "unsupported")
        self.assertEqual(report["cases"][0]["status"], "passed")


if __name__ == "__main__":
    unittest.main()
