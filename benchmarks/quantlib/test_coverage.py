"""Complete-suite coverage and diagnostics at the command/report boundary."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


class CoverageTests(unittest.TestCase):
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
                binary.write_text(
                    f"#!{sys.executable}\nimport json,sys\n"
                    "if sys.argv[1:] == ['--info']:\n"
                    f" print(json.dumps({{'version':'1.44','families':{supported!r}}}))\n"
                    "else:\n"
                    # Deliberately inaccurate: this coverage test never runs ensembles/timing.
                    " print(json.dumps({'price':-10000,'elapsed_ns':1,'iterations':1,'warmup_iterations':3}))\n"
                )
                binary.chmod(0o755)
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
        self.assertTrue(
            all(
                all(cell["native_support"].values())
                for cell in report["coverage"]["cells"]
            )
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
