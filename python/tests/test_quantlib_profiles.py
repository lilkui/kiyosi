"""Fixture normalization, numerical budgets and reference-engine coverage."""

import copy
import json
import math
from pathlib import Path
from tempfile import TemporaryDirectory

import pytest
from kiyosi import pricing
from quantlib_reference import oracle, structured
from support.history import history_fields


def test_structured_portfolios_match_direct_payoffs():
    structured.check_portfolios()


def test_fixture_defaults_and_overrides_are_expanded():
    config = oracle.load_fixture("scenarios.json")
    first = config["scenarios"][0]
    assert first["inputs"]["strike"] == 100
    assert first["inputs"]["spot"] == 80
    assert first["tolerances"]["price"] == 1e-10
    assert first["numerical_tolerances"]["delta"] == 1e-6

    profiles = oracle.load_fixture("numerical_engines.json")["profiles"]
    profile = next(
        p for p in profiles if p["engine"] == "FiniteDifferenceVanillaEngine"
    )
    assert profile["shifts"]["spot_shift"] == 4
    assert profile["tolerances"]["price"] == 0.03
    assert profile["tolerances"]["gamma"] == 0.0003

    first["inputs"]["strike"] = 200
    first["tolerances"]["price"] = 1
    assert config["defaults"]["inputs"]["strike"] == 100
    assert config["scenarios"][1]["inputs"]["strike"] == 100
    assert config["scenarios"][1]["tolerances"]["price"] == 1e-10
    assert (
        oracle.load_fixture("scenarios.json")["scenarios"][0]["inputs"]["strike"] == 100
    )


def test_reference_profiles_cover_expected_engines(subtests):
    numerical = {
        "QuadratureVanillaEngine",
        "CoxRossRubinsteinVanillaEngine",
        "FiniteDifferenceVanillaEngine",
        "MonteCarloVanillaEngine",
    }
    for name, expected in (
        ("numerical_engines.json", numerical),
        ("american.json", numerical | {"BjerksundStenslandVanillaEngine"}),
        (
            "digital.json",
            {
                "AnalyticDigitalEngine",
                "QuadratureDigitalEngine",
                "FiniteDifferenceDigitalEngine",
            },
        ),
    ):
        profiles = oracle.load_fixture(name)["profiles"]
        with subtests.test(fixture=name):
            assert sorted([p["engine"] for p in profiles]) == sorted(expected)
            for profile in profiles:
                engine = getattr(pricing, profile["engine"])(
                    **history_fields(**profile["settings"])
                )
                for field, value in profile["settings"].items():
                    assert getattr(engine, field) == value


def test_loader_rejects_invalid_budgets(subtests):
    profile = oracle.load_fixture("numerical_engines.json")["profiles"][0]
    profile.pop("shifts")
    with TemporaryDirectory() as directory:
        path = Path(directory) / "profiles.json"
        for field in ("tolerances", "numerical_tolerances"):
            for value in (-1, math.nan, math.inf, True, None):
                invalid = copy.deepcopy(profile)
                if value is None:
                    del invalid[field]["price"]
                else:
                    invalid[field]["price"] = value
                path.write_text(json.dumps({"profiles": [invalid]}), encoding="utf-8")
                with (
                    subtests.test(field=field, value=value),
                    pytest.raises(ValueError),
                ):
                    oracle.load_fixture(path)


def test_loader_rejects_empty_or_duplicate_scenarios(subtests):
    with TemporaryDirectory() as directory:
        path = Path(directory) / "scenarios.json"
        for rows in ([], [{"case_id": "same"}, {"case_id": "same"}]):
            path.write_text(json.dumps({"scenarios": rows}), encoding="utf-8")
            with subtests.test(rows=rows), pytest.raises(ValueError):
                oracle.load_fixture(path)
