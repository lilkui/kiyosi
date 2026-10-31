"""Reference profiles leave engine-setting validation to the public API."""

import json
import unittest
from pathlib import Path
from unittest.mock import patch

from kiyosi import pricing
from quantlib_reference import digital, oracle, scenarios


class QuantLibProfileTests(unittest.TestCase):
    def test_numerical_profiles_accept_public_engine_settings(self):
        path = oracle.DATA_DIR / "numerical_engines.json"
        for name, field, value in (
            ("FiniteDifferenceVanillaEngine", "asset_upper_boundary", None),
            ("FiniteDifferenceVanillaEngine", "asset_upper_boundary", 400.5),
            ("MonteCarloVanillaEngine", "seed", 0),
        ):
            config = json.loads(path.read_text())
            profile = next(p for p in config["profiles"] if p["engine"] == name)
            profile["settings"][field] = value
            with (
                self.subTest(engine=name, field=field, value=value),
                patch.object(Path, "read_text", return_value=json.dumps(config)),
            ):
                loaded = scenarios.numerical_profiles(path)
            settings = next(p["settings"] for p in loaded if p["engine"] == name)
            engine = getattr(pricing, name)(**settings)
            self.assertEqual(getattr(engine, field), value)

    def test_digital_profiles_accept_automatic_and_fractional_boundaries(self):
        path = oracle.DATA_DIR / "digital.json"
        for boundary in (None, 400.5):
            config = json.loads(path.read_text())
            profile = next(
                p
                for p in config["profiles"]
                if p["engine"] == "FiniteDifferenceDigitalEngine"
            )
            profile["settings"]["asset_upper_boundary"] = boundary
            with (
                self.subTest(boundary=boundary),
                patch.object(Path, "read_text", return_value=json.dumps(config)),
            ):
                loaded = digital.configuration()
            settings = next(
                p["settings"]
                for p in loaded["profiles"]
                if p["engine"] == "FiniteDifferenceDigitalEngine"
            )
            engine = pricing.FiniteDifferenceDigitalEngine(**settings)
            self.assertEqual(engine.asset_upper_boundary, boundary)

    def test_numerical_profiles_preserve_identity_and_budget_checks(self):
        path = oracle.DATA_DIR / "numerical_engines.json"
        for invalid in ("engine", "duplicate", "budget"):
            config = json.loads(path.read_text())
            if invalid == "engine":
                config["profiles"][0]["engine"] = "unknown"
            elif invalid == "duplicate":
                config["profiles"].append(config["profiles"][0])
            else:
                config["profile_defaults"]["tolerances"]["price"] = -1
            with (
                self.subTest(invalid=invalid),
                patch.object(Path, "read_text", return_value=json.dumps(config)),
                self.assertRaises(ValueError),
            ):
                scenarios.numerical_profiles(path)
