"""Fixture normalization, numerical budgets and reference-engine coverage."""

import copy
import json
import math
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from kiyosi import pricing
from quantlib_reference import oracle


class QuantLibProfileTests(unittest.TestCase):
    def test_fixture_defaults_and_overrides_are_expanded(self):
        config = oracle.load_fixture("scenarios.json")
        first = config["scenarios"][0]
        self.assertEqual(first["inputs"]["strike"], 100)
        self.assertEqual(first["inputs"]["spot"], 80)
        self.assertEqual(first["tolerances"]["price"], 1e-10)
        self.assertEqual(first["numerical_tolerances"]["delta"], 1e-6)

        profiles = oracle.load_fixture("numerical_engines.json")["profiles"]
        profile = next(
            p for p in profiles if p["engine"] == "FiniteDifferenceVanillaEngine"
        )
        self.assertEqual(profile["shifts"]["spot_shift"], 4)
        self.assertEqual(profile["tolerances"]["price"], 0.03)
        self.assertEqual(profile["tolerances"]["gamma"], 0.0003)

        first["inputs"]["strike"] = 200
        first["tolerances"]["price"] = 1
        self.assertEqual(config["defaults"]["inputs"]["strike"], 100)
        self.assertEqual(config["scenarios"][1]["inputs"]["strike"], 100)
        self.assertEqual(config["scenarios"][1]["tolerances"]["price"], 1e-10)
        self.assertEqual(
            oracle.load_fixture("scenarios.json")["scenarios"][0]["inputs"]["strike"],
            100,
        )

    def test_reference_profiles_cover_expected_engines(self):
        numerical = {
            "CoxRossRubinsteinVanillaEngine",
            "FiniteDifferenceVanillaEngine",
            "MonteCarloVanillaEngine",
        }
        for name, expected in (
            ("numerical_engines.json", numerical),
            ("american.json", numerical | {"BjerksundStenslandVanillaEngine"}),
            (
                "digital.json",
                {"AnalyticDigitalEngine", "FiniteDifferenceDigitalEngine"},
            ),
        ):
            profiles = oracle.load_fixture(name)["profiles"]
            with self.subTest(fixture=name):
                self.assertCountEqual([p["engine"] for p in profiles], expected)
                for profile in profiles:
                    engine = getattr(pricing, profile["engine"])(**profile["settings"])
                    for field, value in profile["settings"].items():
                        self.assertEqual(getattr(engine, field), value)

    def test_profile_time_stencils_fit_reference_scenarios(self):
        for profile in oracle.load_fixture("numerical_engines.json")["profiles"]:
            self.assertTrue(0 < profile["shifts"]["time_shift_days"] <= 3)
        for profile in oracle.load_fixture("digital.json")["profiles"]:
            self.assertEqual(profile["shifts"]["time_shift_days"], 1)

    def test_loader_rejects_invalid_budgets(self):
        profile = oracle.load_fixture("numerical_engines.json")["profiles"][0]
        with TemporaryDirectory() as directory:
            path = Path(directory) / "profiles.json"
            for field in ("tolerances", "numerical_tolerances"):
                for value in (-1, math.nan, math.inf, True, None):
                    invalid = copy.deepcopy(profile)
                    if value is None:
                        del invalid[field]["price"]
                    else:
                        invalid[field]["price"] = value
                    path.write_text(
                        json.dumps({"profiles": [invalid]}), encoding="utf-8"
                    )
                    with (
                        self.subTest(field=field, value=value),
                        self.assertRaises(ValueError),
                    ):
                        oracle.load_fixture(path)

    def test_loader_rejects_empty_or_duplicate_scenarios(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "scenarios.json"
            for rows in ([], [{"case_id": "same"}, {"case_id": "same"}]):
                path.write_text(json.dumps({"scenarios": rows}), encoding="utf-8")
                with self.subTest(rows=rows), self.assertRaises(ValueError):
                    oracle.load_fixture(path)
