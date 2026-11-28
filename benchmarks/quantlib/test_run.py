"""Checks at the benchmark command and report boundaries."""

import errno
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import calibrate_fd
import calibrate_heston
import calibrate_numerical
from benchmark_test_support import write_python_adapter
from run import (
    build,
    build_evidence_path,
    build_rust,
    command,
    fingerprint,
    measure,
    prebuilt_rust_executable,
    write_report,
)

RUNNER = Path(__file__).with_name("run.py")


class CommandTests(unittest.TestCase):
    def test_native_launch_errors_are_reported_as_case_failures(self):
        case = {
            "id": "fixture",
            "family": "european_bsm",
            "inputs": {},
            "profiles": {"kiyosi": {}, "quantlib": {}},
            "error_budget": 0.01,
        }
        reference = {"value": 10, "uncertainty": 0}
        executables = {"kiyosi": Path("rust"), "quantlib": Path("cpp")}
        info = {backend: {"families": ["european_bsm"]} for backend in executables}

        for error, message in (
            (subprocess.TimeoutExpired("rust", 1), "timed out"),
            (OSError(errno.E2BIG, "Argument list too long"), "Argument list too long"),
        ):
            with (
                self.subTest(error=type(error).__name__),
                patch("run.subprocess.run", side_effect=error) as launch,
            ):
                row = measure(
                    case, reference, executables, SimpleNamespace(price_only=True), info
                )
                self.assertGreater(launch.call_args.kwargs["timeout"], 0)
                self.assertEqual(row["status"], "failed")
                self.assertIsNone(row["ratio"])
                self.assertIn(message, row["error"])

    def test_rust_executable_override_requires_no_build(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            result = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER),
                    "--rust-executable",
                    str(Path(directory) / "custom"),
                    "--case",
                    "european-bsm-atm-call",
                    "--report",
                    str(report),
                ],
                text=True,
                capture_output=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 1)
            data = json.loads(report.read_text())
            self.assertEqual(data["cases"][0]["status"], "prerequisite_failed")
            self.assertEqual(data["error"], "--rust-executable requires --no-build")

    def test_build_records_evidence_per_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rust = root / "custom" / "release" / "quantlib_bench"
            rust.parent.mkdir(parents=True)
            rust.write_bytes(b"rust artifact")
            cargo_output = json.dumps(
                {
                    "reason": "compiler-artifact",
                    "target": {"name": "quantlib_bench"},
                    "executable": str(rust),
                }
            )
            args = SimpleNamespace(
                quantlib_config="quantlib-config",
                cxx="c++",
                cxxflags="",
            )
            for name in ("quantlib", "custom-quantlib"):
                binary = root / name
                binary.write_bytes(name.encode())
                args.quantlib_executable = binary
                source = {"revision": name, "dirty_state": ""}
                with (
                    patch("run.HERE", root),
                    patch("run.source_state", return_value=source),
                    patch(
                        "run.command",
                        side_effect=["1.44", "", "", cargo_output, "", "c++"],
                    ),
                ):
                    executable, evidence = build(args)
                self.assertEqual(executable, rust)
                self.assertEqual(evidence["executables"]["kiyosi"], fingerprint(rust))
                self.assertEqual(evidence["source"], source)
            for name in ("quantlib", "custom-quantlib"):
                binary = root / name
                recorded = json.loads(build_evidence_path(binary).read_text())
                self.assertEqual(recorded["source"]["revision"], name)
                self.assertEqual(
                    recorded["executables"]["quantlib"], fingerprint(binary)
                )

    def test_prebuilt_adapter_uses_configured_cargo_target_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "custom-target"
            with (
                patch("run.shutil.which", return_value="/tools/cargo"),
                patch(
                    "run.command",
                    return_value=json.dumps({"target_directory": str(target)}),
                ) as cargo,
            ):
                self.assertEqual(
                    prebuilt_rust_executable({}), target / "release/quantlib_bench"
                )
            self.assertEqual(cargo.call_args.args[0][:2], ["cargo", "metadata"])

    def test_missing_recorded_adapter_falls_back_to_configured_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "new-target"
            old = root / "old-target/release/quantlib_bench"
            executable = target / "release/quantlib_bench"
            executable.parent.mkdir(parents=True)
            executable.touch()
            evidence = {"executables": {"kiyosi": {"path": str(old)}}}
            with (
                patch("run.shutil.which", return_value="/tools/cargo"),
                patch(
                    "run.command",
                    return_value=json.dumps({"target_directory": str(target)}),
                ),
            ):
                self.assertEqual(prebuilt_rust_executable(evidence), executable)

    def test_prebuilt_adapter_requires_both_artifact_fingerprints_to_match(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            rust = root / "recorded-rust"
            quantlib = root / "custom-quantlib"
            executables = {"kiyosi": rust, "quantlib": quantlib}
            for path in executables.values():
                path.write_bytes(b"original artifact")
            evidence = {
                "executables": {
                    backend: fingerprint(path) for backend, path in executables.items()
                }
            }
            build_evidence_path(quantlib).write_text(json.dumps(evidence))
            target = root / "configured-target"
            with (
                patch("run.shutil.which", return_value="/tools/cargo"),
                patch(
                    "run.command",
                    return_value=json.dumps({"target_directory": str(target)}),
                ) as cargo,
            ):
                self.assertEqual(
                    prebuilt_rust_executable(quantlib_executable=quantlib), rust
                )
                cargo.assert_not_called()
                for backend, path in executables.items():
                    with self.subTest(backend=backend):
                        path.write_bytes(b"replacement artifact")
                        self.assertEqual(
                            prebuilt_rust_executable(quantlib_executable=quantlib),
                            target / "release/quantlib_bench",
                        )
                        path.write_bytes(b"original artifact")

    def test_missing_native_executable_writes_failed_report(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            missing = Path(directory) / "absent"
            result = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER),
                    "--no-build",
                    "--rust-executable",
                    str(missing),
                    "--report",
                    str(report),
                ],
                text=True,
                capture_output=True,
                check=False,
                timeout=600,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            data = json.loads(report.read_text())
            self.assertEqual(data["status"], "failed")
            self.assertEqual(data["error"], f"missing executable: {missing.resolve()}")
            self.assertEqual(len(data["cases"]), 62)
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
                timeout=600,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            data = json.loads(report.read_text())
            self.assertEqual(len(data["cases"]), 62)
            self.assertTrue(
                all(row["status"] == "prerequisite_failed" for row in data["cases"])
            )
            self.assertTrue(all(row["ratio"] is None for row in data["cases"]))
            self.assertIn("missing executable", data["error"])


class NativeRustTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.executable, _ = build_rust(timeout=600)

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
            timeout=120,
        )
        data = json.loads(result.stdout)
        self.assertAlmostEqual(data["price"], 9.227005508154036, places=11)
        self.assertEqual(data["iterations"], 100)
        self.assertEqual(data["warmup_iterations"], 3)
        self.assertGreater(data["elapsed_ns"], 0)

    def test_unknown_native_family_is_rejected(self):
        result = subprocess.run(
            [str(self.executable), "family=unknown", "iterations=1"],
            text=True,
            capture_output=True,
            check=False,
            timeout=120,
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("unknown", result.stderr)

    def test_native_json_preserves_small_prices(self):
        result = command(
            [
                str(self.executable),
                "family=european_bsm",
                "iterations=1",
                "skip_warmup=true",
                "option=call",
                "valuation=2025-01-01",
                "expiry=2026-01-01",
                "spot=1e-18",
                "strike=1e-18",
                "rate=0",
                "dividend=0",
                "volatility=0.2",
            ],
            timeout=120,
        )
        expected = 7.965567455405801e-20
        data = json.loads(result)
        self.assertAlmostEqual(data["price"] / expected, 1.0, places=14)
        self.assertEqual(data["warmup_iterations"], 0)


class ReportWriteTests(unittest.TestCase):
    def setUp(self):
        directory = self.enterContext(tempfile.TemporaryDirectory())
        self.path = Path(directory) / "report.json"
        self.previous = b'{"status": "previous"}\n'
        self.path.write_bytes(self.previous)
        self.report = {"status": "passed", "cases": [{"id": "test_case"}]}

    def assert_report_unchanged(self):
        self.assertEqual(self.path.read_bytes(), self.previous)
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])

    def test_partial_report_write_preserves_existing_report(self):
        def partial_write(_report, stream, **_kwargs):
            stream.write('{"status":')
            stream.flush()
            raise OSError("partial report write")

        with (
            patch("run.json.dump", side_effect=partial_write),
            self.assertRaisesRegex(OSError, "partial report write"),
        ):
            write_report(self.path, self.report)
        self.assert_report_unchanged()

    def test_failed_report_replace_preserves_existing_report(self):
        def fail_replace(source, destination):
            self.assertEqual(source.parent, self.path.parent)
            self.assertNotEqual(source, self.path)
            self.assertEqual(destination, self.path)
            self.assertEqual(
                json.loads(source.read_text(encoding="utf-8")), self.report
            )
            raise PermissionError("report replace denied")

        with (
            patch.object(Path, "replace", autospec=True, side_effect=fail_replace),
            self.assertRaisesRegex(PermissionError, "report replace denied"),
        ):
            write_report(self.path, self.report)
        self.assert_report_unchanged()

    def test_successful_report_replace_leaves_only_new_report(self):
        write_report(self.path, self.report)

        self.assertEqual(json.loads(self.path.read_text(encoding="utf-8")), self.report)
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])

    def test_failed_calibration_replace_preserves_destination(self):
        replace = Path.replace
        for script, output_name in (
            (calibrate_fd, "calibration/fd_bsm.json"),
            (calibrate_heston, "heston_calibration.json"),
            (calibrate_heston, "cases/heston.json"),
        ):
            with self.subTest(output=output_name), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                output = root / output_name
                output.parent.mkdir(exist_ok=True)
                output.write_bytes(self.previous)

                def fail_one_replace(source, destination, failing_path=output):
                    if destination == failing_path:
                        raise OSError("calibration replace failed")
                    return replace(source, destination)

                with (
                    patch.object(script, "HERE", root),
                    patch("run.load_entries", return_value={}),
                    patch("run.prebuilt_rust_executable", return_value=Path("rust")),
                    patch.object(
                        Path, "replace", autospec=True, side_effect=fail_one_replace
                    ),
                    self.assertRaisesRegex(OSError, "calibration replace failed"),
                ):
                    script.main()

                self.assertEqual(output.read_bytes(), self.previous)
                self.assertEqual(list(output.parent.iterdir()), [output])

    def test_failed_numerical_calibration_preserves_all_outputs(self):
        root = self.path.parent
        case = {
            "id": "fixture",
            "family": "crr_vanilla",
            "inputs": {"exercise": "american"},
            "profiles": {
                backend: {"step_count": 512} for backend in ("kiyosi", "quantlib")
            },
            "error_budget": 0.01,
        }
        outputs = [
            root / "calibration/american.json",
            root / "calibration/numerical.json",
            root / "references/numerical.json",
        ]
        for output in outputs:
            output.parent.mkdir(exist_ok=True)
            output.write_bytes(self.previous)

        with (
            patch.object(calibrate_numerical, "HERE", root),
            patch.object(
                calibrate_numerical, "load_entries", return_value={"fixture": case}
            ),
            patch.object(
                calibrate_numerical, "american_reference", return_value=(10, {})
            ),
            patch.object(calibrate_numerical, "command", return_value=""),
            patch.object(
                calibrate_numerical,
                "prebuilt_rust_executable",
                return_value=Path("rust"),
            ),
            patch.object(calibrate_numerical, "native", return_value={"price": 11}),
            self.assertRaisesRegex(RuntimeError, "fixture/kiyosi.*budget"),
        ):
            calibrate_numerical.main()

        for output in outputs:
            self.assertEqual(output.read_bytes(), self.previous, output)


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
        build_source=None,
        relative_executables=False,
        rust_from_build=False,
        rebuild_rust=False,
    ):
        prefix = "benchmark fixture " if relative_executables else "kiyosi-benchmark-"
        with tempfile.TemporaryDirectory(prefix=prefix) as directory:
            root = Path(directory)
            native = root / "native"
            families = ["european_bsm"] if families is None else families
            adapter = (
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
            write_python_adapter(native, adapter)
            rust = native
            if rust_from_build:
                rust = root / "x86_64-unknown-linux-gnu/release/quantlib_bench"
                rust.parent.mkdir(parents=True)
                write_python_adapter(rust, adapter)
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
                write_python_adapter(
                    loader, f"print('libQuantLib.so.1 => {library} (0x1)')\n"
                )
                environment = {
                    **os.environ,
                    "PATH": str(root) + os.pathsep + os.environ["PATH"],
                }
            report = root / "report.json"
            runner = [str(RUNNER)]
            if build_source is not None:
                build_evidence_path(native).write_text(
                    json.dumps(
                        {
                            "source": build_source,
                            "executables": {
                                "kiyosi": fingerprint(rust),
                                "quantlib": fingerprint(native),
                            },
                        }
                    )
                )
                if rebuild_rust:
                    rust.write_text(rust.read_text() + "\n# Rebuilt adapter\n")
                runner = [
                    "-c",
                    (
                        "import run, sys\n"
                        "run.source_state = lambda: {'revision': 'checkout-revision', 'dirty_state': ''}\n"
                        "raise SystemExit(run.main())"
                    ),
                ]
            result = subprocess.run(
                [
                    sys.executable,
                    *runner,
                    "--no-build",
                    *(
                        []
                        if rust_from_build and not rebuild_rust
                        else [
                            "--rust-executable",
                            f"./{rust.name}" if relative_executables else str(rust),
                        ]
                    ),
                    "--quantlib-executable",
                    native.name if relative_executables else str(native),
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
                cwd=root if relative_executables else RUNNER.parent,
                timeout=600,
            )
            self.assertTrue(report.exists(), result.stderr)
            return result, json.loads(report.read_text())

    def test_relative_executables_use_the_callers_directory(self):
        result, report = self.run_fixture(
            extra=("--price-only",), relative_executables=True
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report["cases"][0]["status"], "passed")

    def test_no_build_retains_the_binary_build_revision(self):
        build_source = {"revision": "build-revision", "dirty_state": " M Cargo.toml"}
        result, report = self.run_fixture(
            extra=("--price-only",), build_source=build_source
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report["source"]["revision"], "checkout-revision")
        self.assertEqual(report["build"]["source"], build_source)

    def test_no_build_uses_the_recorded_target_specific_artifact(self):
        source = {"revision": "build-revision", "dirty_state": ""}
        result, report = self.run_fixture(
            extra=("--price-only",), build_source=source, rust_from_build=True
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report["cases"][0]["status"], "passed")
        self.assertEqual(report["build"]["source"], source)

    def test_no_build_allows_an_explicit_replaced_adapter_without_build_evidence(self):
        result, report = self.run_fixture(
            extra=("--price-only",),
            build_source={"revision": "build-revision", "dirty_state": ""},
            rust_from_build=True,
            rebuild_rust=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report["cases"][0]["status"], "passed")
        self.assertEqual(
            report["build"],
            "unavailable: supplied executables differ from recorded build artifacts",
        )

    def test_default_rejects_incomplete_manifest_with_explicit_required_rows(self):
        result, report = self.run_fixture(complete=True)
        self.assertEqual(result.returncode, 1, result.stderr)
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
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIsNone(report["cases"][0]["ratio"])
                self.assertIn("native result", report["cases"][0]["error"])

    def test_bool_reference_is_rejected(self):
        result, report = self.run_fixture(reference=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("reference", report["cases"][0]["error"])

    def test_bool_error_budget_is_rejected(self):
        result, report = self.run_fixture(patch={"error_budget": True})
        self.assertEqual(result.returncode, 1, result.stderr)
        row = report["cases"][0]
        self.assertEqual(row["status"], "failed")
        self.assertIn("error_budget must be finite and positive", row["error"])
        self.assertIsNone(row["ratio"])

    def test_missing_native_family_retains_unsupported_row(self):
        result, report = self.run_fixture(families=[])
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(report["cases"][0]["status"], "unsupported")
        self.assertIn("kiyosi/european_bsm", report["cases"][0]["error"])

    def test_report_uses_all_fifteen_native_samples(self):
        result, report = self.run_fixture()
        self.assertEqual(result.returncode, 0, result.stderr)
        case = report["cases"][0]
        self.assertEqual(case["ratio"], 1)
        order = ["kiyosi", "quantlib"]
        self.assertEqual(
            case["sample_order"],
            [order if i % 2 == 0 else order[::-1] for i in range(15)],
        )
        self.assertCountEqual(case["backends"], order)
        for backend in order:
            with self.subTest(backend=backend):
                timing = case["backends"][backend]["timing"]
                self.assertEqual(
                    [
                        sample["elapsed_ns"] // sample["iterations"]
                        for sample in timing["samples"]
                    ],
                    [100_000_000 * i for i in range(1, 16)],
                )
                self.assertEqual(timing["median_ns"], 800_000_000)
                self.assertEqual(timing["iqr_ns"], 700_000_000)

    def test_incompatible_native_version_fails(self):
        result, report = self.run_fixture(version="1.43")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("1.44", report["error"])
        self.assertEqual(report["cases"][0]["status"], "prerequisite_failed")
        self.assertIsNone(report["cases"][0]["ratio"])

    def test_identifiable_runtime_library_mismatch_fails_all_requests(self):
        for version in ("1.43", "2.0"):
            with self.subTest(version=version):
                result, report = self.run_fixture(
                    runtime_name=f"libQuantLib.so.{version}.0"
                )
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("runtime filename", report["error"])
                self.assertEqual(report["cases"][0]["status"], "prerequisite_failed")
                self.assertEqual(
                    report["runtime_library"]["resolved"][0]["filename_version"],
                    version,
                )
                self.assertIn("unavailable", report["runtime_library"]["version_api"])

    def test_accuracy_failure_suppresses_ratio_and_fails(self):
        result, report = self.run_fixture(reference=11)
        self.assertEqual(result.returncode, 1, result.stderr)
        row = report["cases"][0]
        self.assertIsNone(row["ratio"])
        self.assertEqual(row["status"], "accuracy_failed")
        self.assertCountEqual(row["backends"], ("kiyosi", "quantlib"))
        for backend in row["backends"].values():
            self.assertEqual(backend["accuracy"]["absolute_error"], 1)
            self.assertNotIn("timing", backend)

    def test_overflowing_price_error_still_writes_accuracy_report(self):
        result, report = self.run_fixture(
            reference=-1e308,
            extra=("--price-only",),
            patch={"error_budget": 1e308},
            batch={
                "price": 1e308,
                "elapsed_ns": 1,
                "iterations": 1,
                "warmup_iterations": 3,
            },
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        row = report["cases"][0]
        self.assertEqual(row["status"], "accuracy_failed")
        self.assertIsNone(row["ratio"])
        self.assertCountEqual(row["backends"], ("kiyosi", "quantlib"))
        for backend in row["backends"].values():
            accuracy = backend["accuracy"]
            self.assertFalse(accuracy["passed"])
            self.assertIsNone(accuracy["absolute_error"])
            self.assertIsNone(accuracy["error_including_reference_uncertainty"])
            self.assertIn("overflowed", accuracy["error"])

    def test_unknown_requested_case_fails_explicitly(self):
        result, report = self.run_fixture(extra=("--case", "absent"))
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(report["cases"][-1]["id"], "absent")
        self.assertEqual(report["cases"][-1]["status"], "unsupported")
        self.assertEqual(report["cases"][0]["status"], "passed")


if __name__ == "__main__":
    unittest.main()
