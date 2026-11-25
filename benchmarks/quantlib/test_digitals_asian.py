"""Contract checks for native digital and Asian pricing."""

import os
import subprocess
import unittest

from run import DEFAULT_QUANTLIB_EXECUTABLE, HERE, build_rust, load_entries, native

CASES = load_entries(HERE / "cases/digitals_asian.json")
REFERENCES = load_entries(HERE / "references/digitals_asian.json")


class DigitalAsianTests(unittest.TestCase):
    backend = "kiyosi"

    @classmethod
    def setUpClass(cls):
        cls.executable, _ = build_rust(timeout=600)

    def test_native_prices_match_independent_references(self):
        for case_id, places in (
            ("digital-bsm-cash-call", 11),
            ("geometric-asian-bsm-call", 10),
            ("arithmetic-asian-bsm-call", 10),
        ):
            with self.subTest(case=case_id):
                result = native(self.executable, CASES[case_id], self.backend, 100)
                self.assertAlmostEqual(
                    result["price"], REFERENCES[case_id]["value"], places=places
                )

    def test_asian_subset_rejects_forward_start_and_singular_carry(self):
        case = CASES["arithmetic-asian-bsm-call"]
        common = {
            "family": case["family"],
            "iterations": 1,
            **case["inputs"],
            **case["profiles"][self.backend],
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
    backend = "quantlib"

    @classmethod
    def setUpClass(cls):
        cls.executable = DEFAULT_QUANTLIB_EXECUTABLE


if __name__ == "__main__":
    unittest.main()
