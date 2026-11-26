"""Complete-suite coverage and diagnostics at the command/report boundary."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import calibrate_local_vol
from benchmark_test_support import write_python_adapter
from calibrate_fd import domain_probes
from compare_fd_accuracy import main as compare_fd_accuracy
from run import load_entries

HERE = Path(__file__).resolve().parent


class CoverageTests(unittest.TestCase):
    def test_local_vol_calibration_timeout_preserves_complete_report(self):
        cases = load_entries(HERE / "cases/fd_local_vol.json")
        references = load_entries(HERE / "references/fd_local_vol.json")
        previous = (HERE / "calibration/fd_local_vol.json").read_bytes()
        points = {
            point["spot"]: point["expected"]
            for point in json.loads(previous)["surface_points"]
        }
        first = next(iter(cases))

        def native(_executable, case, backend, _iterations):
            if case["id"] == "surface":
                return {"price": points[case["inputs"]["query_spot"]]}
            if case["id"] != first:
                raise TimeoutError("second case timed out")
            price = references[first]["value"]
            if case["profiles"][backend]["space"] == 10:
                price += 1
            return {"price": price}

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report = root / "calibration/fd_local_vol.json"
            report.parent.mkdir()
            report.write_bytes(previous)
            with (
                patch.object(calibrate_local_vol, "HERE", root),
                patch("run.load_entries", side_effect=[cases, references]),
                patch("run.prebuilt_rust_executable", return_value=root / "kiyosi"),
                patch("run.native", side_effect=native),
                self.assertRaisesRegex(TimeoutError, "second case timed out"),
            ):
                calibrate_local_vol.main()
            self.assertEqual(report.read_bytes(), previous)

    def test_fd_accuracy_times_each_distinct_comparison_once(self):
        measurement = {
            "status": "passed",
            "backends": {
                "kiyosi": {
                    "timing": {"median_ns": 1000},
                    "accuracy": {"absolute_error": 0.002},
                },
                "quantlib": {
                    "timing": {"median_ns": 1000},
                    "accuracy": {"absolute_error": 0.001},
                },
            },
        }
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "reports" / "comparison.json"
            with (
                patch(
                    "compare_fd_accuracy.os.sched_getaffinity",
                    return_value={0},
                    create=True,
                ),
                patch("compare_fd_accuracy.os.sched_setaffinity", create=True),
                patch("run.command", return_value="{}"),
                patch("run.measure", return_value=measurement) as measure,
            ):
                compare_fd_accuracy(
                    [
                        "--baseline-executable",
                        sys.executable,
                        "--updated-executable",
                        sys.executable,
                        "--report",
                        str(report),
                    ]
                )
            document = json.loads(report.read_text())
        self.assertTrue(document["passed"])
        self.assertEqual(measure.call_count, 8)
        self.assertEqual(
            {row["id"]: set(row["comparisons"]) for row in document["cases"]},
            {
                f"{prefix}-{product}": comparisons
                for prefix in ("fd-bsm", "fd-local-vol")
                for product, comparisons in (
                    (
                        "european-call",
                        {"call_policy", "core_explicit", "core_selected"},
                    ),
                    ("down-out-call", {"core_explicit"}),
                )
            },
        )

    def test_fd_domain_probes_only_change_active_grids(self):
        bsm = load_entries(HERE / "cases/fd_bsm.json")
        local = load_entries(HERE / "cases/fd_local_vol.json")
        for cases, prefix in ((bsm, "fd-bsm"), (local, "fd-local-vol")):
            self.assertEqual(
                domain_probes(cases[f"{prefix}-european-call"])["explicit_boundary"],
                {"upper": 400},
            )
            self.assertEqual(
                domain_probes(cases[f"{prefix}-down-out-call"]),
                {"domain_constant_spacing": {"upper": 600, "space": 1002}},
            )
            for product in ("up-no-touch-asset-expiry", "up-one-touch-cash-hit"):
                self.assertEqual(domain_probes(cases[f"{prefix}-{product}"]), {})
        continuation = domain_probes(local["fd-local-vol-terminal-up-in-asset"])
        self.assertEqual(
            continuation["domain_constant_spacing"], {"upper": 600, "space": 800}
        )
        self.assertGreater(continuation["continuation_constant_spacing"]["space"], 800)

    def test_fd_call_policy_uses_baseline_capabilities_for_both_runs(self):
        references = load_entries(HERE / "references")

        def native(_executable, case, backend, iterations):
            price = references[case["id"]]["value"]
            if backend == "kiyosi":
                price -= 0.00001
            return {"price": price, "elapsed_ns": 20_000_000, "iterations": iterations}

        with tempfile.TemporaryDirectory() as directory:
            baseline = Path(directory) / "baseline"
            updated = Path(directory) / "updated"
            report = Path(directory) / "comparison.json"
            baseline.touch()
            updated.touch()
            with (
                patch(
                    "compare_fd_accuracy.os.sched_getaffinity",
                    return_value={0},
                    create=True,
                ),
                patch("compare_fd_accuracy.os.sched_setaffinity", create=True),
                patch(
                    "run.command",
                    side_effect=[
                        json.dumps({"families": ["fd_bsm", "fd_local_vol"]}),
                        json.dumps({"families": ["fd_local_vol"]}),
                        "revision",
                        "cpu",
                    ],
                ),
                patch("run.native", side_effect=native),
                self.assertRaisesRegex(SystemExit, "FD accuracy/latency target failed"),
            ):
                compare_fd_accuracy(
                    [
                        "--baseline-executable",
                        str(baseline),
                        "--updated-executable",
                        str(updated),
                        "--report",
                        str(report),
                    ]
                )
            document = json.loads(report.read_text())
        comparisons = next(
            row["comparisons"]
            for row in document["cases"]
            if row["id"] == "fd-bsm-european-call"
        )
        self.assertTrue(comparisons["call_policy"]["accuracy_improved"])
        self.assertEqual(
            comparisons["core_explicit"]["failure"]["status"], "unsupported"
        )

    def run_suite(
        self, *, remove=(), patch=None, missing_family=None, missing_reference=None
    ):
        cases = [
            case
            for path in sorted((HERE / "cases").glob("*.json"))
            for case in json.loads(path.read_text())["cases"]
        ]
        families = sorted({case["family"] for case in cases})
        cases = [case for case in cases if case["id"] not in remove]
        for case in cases:
            if patch:
                patch(case)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = []
            for backend in ("kiyosi", "quantlib"):
                binary = root / backend
                supported = [
                    family for family in families if (backend, family) != missing_family
                ]
                write_python_adapter(
                    binary,
                    "import json,sys\n"
                    "if sys.argv[1:] == ['--info']:\n"
                    f" print(json.dumps({{'version':'1.44','families':{supported!r}}}))\n"
                    "else:\n"
                    # Deliberately inaccurate: this coverage test never runs ensembles/timing.
                    " print(json.dumps({'price':-10000,'elapsed_ns':1,'iterations':1,'warmup_iterations':3}))\n",
                )
                binaries.append(binary)
            manifest = root / "cases.json"
            manifest.write_text(json.dumps({"schema_version": 1, "cases": cases}))
            references = root / "references.json"
            references.write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "cases": [
                            {
                                "id": case["id"],
                                "value": 0,
                                "uncertainty": 0,
                                "provenance": {"method": "command fixture"},
                            }
                            for case in cases
                            if case["id"] != missing_reference
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
                    "--price-only",
                    "--rust-executable",
                    str(binaries[0]),
                    "--quantlib-executable",
                    str(binaries[1]),
                    "--cases",
                    str(manifest),
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
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertTrue(report.exists(), result.stderr)
            return json.loads(report.read_text())

    def test_required_inventory_matches_all_committed_semantics(self):
        report = self.run_suite()
        self.assertEqual(report["coverage"]["required_cells"], 51)
        self.assertEqual(report["coverage"]["covered_cells"], 51)
        self.assertEqual(len(report["coverage"]["eligible_engines"]), 20)
        self.assertEqual(len(report["cases"]), 59)
        cells = report["coverage"]["cells"]
        self.assertEqual(len(cells), 51)
        for cell in cells:
            with self.subTest(cell=cell):
                self.assertEqual(
                    cell["native_support"], {"kiyosi": True, "quantlib": True}
                )

    def test_deleted_cell_survives_as_explicit_missing_row(self):
        report = self.run_suite(
            remove=("arithmetic-asian-bsm-call", "arithmetic-asian-bsm-put")
        )
        self.assertEqual(report["coverage"]["covered_cells"], 50)
        missing = [
            row for row in report["cases"] if row["status"] == "missing_coverage"
        ]
        self.assertEqual(len(missing), 1)
        self.assertEqual(
            missing[0]["engine"], "TurnbullWakemanArithmeticAveragePriceEngine"
        )
        self.assertIsNone(missing[0]["ratio"])

    def test_labels_cannot_hide_wrong_model_engine_product_exercise_or_settlement(self):
        def patch(case):
            if case["id"] == "digital-bsm-cash-call":
                case["engine"] = "AnalyticEuropeanEngine"
            if case["id"] == "fd-local-vol-american-put":
                case["model"] = "bsm"
            if case["id"] == "fd-bsm-cash-digital-call":
                case["inputs"]["product"] = "vanilla"
            if case["id"] == "crr-bsm-american-put":
                case["inputs"]["exercise"] = "european"
            if case["id"] == "touch-up-in-asset-hit":
                case["inputs"]["settlement"] = "expiry"

        report = self.run_suite(patch=patch)
        self.assertEqual(report["coverage"]["covered_cells"], 46)
        self.assertEqual(
            sum(row["status"] == "missing_coverage" for row in report["cases"]), 5
        )
        self.assertEqual(
            sum(row["status"] == "unsupported" for row in report["cases"]), 3
        )

    def test_each_native_registry_is_checked_without_omitting_later_cases(self):
        for backend in ("kiyosi", "quantlib"):
            with self.subTest(backend=backend):
                report = self.run_suite(missing_family=(backend, "digital_bsm"))
                rows = {row["id"]: row for row in report["cases"]}
                self.assertEqual(rows["digital-bsm-cash-call"]["status"], "unsupported")
                self.assertIn(
                    backend + "/digital_bsm", rows["digital-bsm-cash-call"]["error"]
                )
                self.assertEqual(
                    rows["crr-bsm-down-out-call"]["status"], "accuracy_failed"
                )
                self.assertEqual(len(rows), 59)

    def test_missing_reference_keeps_row_and_later_results(self):
        report = self.run_suite(missing_reference="mc-american-put")
        self.assertEqual(report["cases"][0]["error"], "missing reference")
        self.assertEqual(report["cases"][-1]["status"], "accuracy_failed")
        self.assertEqual(len(report["cases"]), 59)


if __name__ == "__main__":
    unittest.main()
