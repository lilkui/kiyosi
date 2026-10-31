"""Product matrices and live QuantLib references for Python tests."""

import json
import re

from . import oracle as g

NUMERICAL_ENGINES = {
    "CoxRossRubinsteinVanillaEngine": {"step_count"},
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
