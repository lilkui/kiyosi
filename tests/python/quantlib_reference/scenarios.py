"""Product matrices and live QuantLib references for Python tests."""

import json
import re

from . import american, asian, barrier, binary, digital
from . import oracle as g

NUMERICAL_ENGINES = {
    "CoxRossRubinsteinVanillaEngine": {"step_count"},
    "QuadratureVanillaEngine": set(),
    "FiniteDifferenceVanillaEngine": {
        "asset_step_count",
        "time_step_count",
        "scheme",
        "asset_upper_boundary",
    },
    "MonteCarloVanillaEngine": {"seed", "path_count", "step_count"},
}


def validate_scenario(scenario):
    g.require(
        set(scenario) == {"case_id", "inputs", "tolerances", "numerical_tolerances"},
        "incomplete scenario",
    )
    g.require(
        re.fullmatch(r"ql-european-[a-z0-9-]+", scenario["case_id"]),
        "invalid case identifier",
    )
    g.validate_inputs(scenario["inputs"])
    g.validate_budgets(scenario)


def numerical_profiles(path):
    config = json.loads(path.read_text(encoding="utf-8"))
    g.require(set(config) == {"profiles", "profile_defaults"}, "invalid profiles")
    profiles = g.expand_profiles(config)
    g.require(
        isinstance(profiles, list) and len(profiles) == len(NUMERICAL_ENGINES),
        "incomplete engine profiles",
    )
    g.require(
        {p["engine"] for p in profiles} == set(NUMERICAL_ENGINES),
        "unknown or duplicate engine",
    )
    for profile in profiles:
        g.validate_profile(profile, NUMERICAL_ENGINES[profile["engine"]])
        g.require(profile["shifts"]["time_shift_days"] <= 3, "invalid time shift")
    return profiles


def numerical_row(scenario, profile, reference):
    row = g.reference_row(
        scenario["case_id"] + "-" + profile["engine"].lower(),
        scenario["inputs"],
        reference,
        profile["tolerances"],
        profile["numerical_tolerances"],
        instrument="EuropeanOption",
        engine=profile["engine"],
    )
    inputs = row["inputs"]
    inputs.update(profile["settings"])
    inputs.update(profile["shifts"])
    inputs["wrapper"] = (
        scenario["inputs"]["spot"] == 100
        and scenario["inputs"]["expiry_date"] == "2026-01-06"
    )
    return row


def load_scenarios(path):
    config = json.loads(path.read_text(encoding="utf-8"))
    g.require(
        set(config) == {"defaults", "scenarios"}, "invalid scenario configuration"
    )
    defaults = config["defaults"]
    g.require(
        set(defaults) == {"inputs", "tolerances", "numerical_tolerances"},
        "invalid scenario defaults",
    )
    g.require(
        isinstance(config["scenarios"], list) and config["scenarios"], "no scenarios"
    )
    scenarios = []
    for scenario in config["scenarios"]:
        g.require(
            {"case_id", "inputs"} <= set(scenario) <= {"case_id", *defaults},
            "invalid scenario overrides",
        )
        expanded = dict(scenario)
        for field in defaults:
            expanded[field] = dict(defaults[field], **scenario.get(field, {}))
        validate_scenario(expanded)
        scenarios.append(expanded)
    g.require(
        len({item["case_id"] for item in scenarios}) == len(scenarios),
        "duplicate scenario identifier",
    )
    return scenarios


def rows():
    """Compute each contract once and restore QuantLib settings before returning."""
    g.require_version()
    with g.ql.SavedSettings():
        scenarios = load_scenarios(g.DATA_DIR / "scenarios.json")
        profiles = numerical_profiles(g.DATA_DIR / "numerical_engines.json")
        references = []
        for item in scenarios:
            reference = g.reference(item["inputs"])
            references.append(
                g.reference_row(
                    item["case_id"],
                    item["inputs"],
                    reference,
                    item["tolerances"],
                    item["numerical_tolerances"],
                    instrument="EuropeanOption",
                    engine="AnalyticEuropeanEngine",
                )
            )
            if not g.exclusions(item["inputs"]):
                references.extend(
                    numerical_row(item, profile, reference) for profile in profiles
                )
        for product in (american, digital, barrier, binary, asian):
            references.extend(product.rows())
        g.require(
            len({row["case_id"] for row in references}) == len(references),
            "duplicate case identifier",
        )
        return tuple(sorted(references, key=lambda row: row["case_id"]))
