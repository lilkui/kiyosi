"""European digital references using QuantLib's analytic European engine."""

import json
import math
from datetime import date, timedelta

import generate as g

INSTRUMENTS = {
    "cash": "EuropeanCashOrNothingOption",
    "asset": "EuropeanAssetOrNothingOption",
}
ENGINES = {
    "AnalyticDigitalEngine",
    "IntegralDigitalEngine",
    "FiniteDifferenceDigitalEngine",
}
# Whole-day time stencils see greater curvature for digital asset payouts.
STABILITY = dict(
    g.STABILITY,
    theta=0.0003,
    charm=0.0002,
    color=0.00001,
    delta=0.00001,
    gamma=0.000001,
    speed=0.00001,
    vega=0.00001,
    vanna=0.000001,
    zomma=0.000001,
    rho=0.00001,
)


def configuration():
    config = json.loads((g.PROJECT / "digital.json").read_text(encoding="utf-8"))
    g.require(
        set(config) == {"inputs", "payout", "profiles", "profile_defaults"},
        "invalid digital configuration",
    )
    g.require(
        set(config["inputs"]) == g.INPUTS - {"option", "spot", "expiry"},
        "invalid digital market",
    )
    g.require(
        type(config["payout"]) in (int, float)
        and math.isfinite(config["payout"])
        and config["payout"] > 0,
        "invalid cash payout",
    )
    config["profiles"] = g.expand_profiles(config)
    g.require(
        {p["engine"] for p in config["profiles"]} == ENGINES
        and len(config["profiles"]) == 3,
        "invalid digital engines",
    )
    for profile in config["profiles"]:
        g.require(
            set(profile)
            == {
                "engine",
                "settings",
                "boundary_settings",
                "shifts",
                "tolerances",
                "numerical_tolerances",
            },
            "invalid digital profile",
        )
        settings = profile["settings"]
        expected = (
            {"asset_steps", "time_steps", "scheme", "upper_boundary"}
            if profile["engine"] == "FiniteDifferenceDigitalEngine"
            else set()
        )
        g.validate_settings(settings, expected)
        g.require(
            settings.get("scheme", "crank_nicolson") == "crank_nicolson",
            "invalid digital scheme",
        )
        g.require(
            set(profile["boundary_settings"])
            == ({"asset_steps"} if expected else set()),
            "invalid digital boundary settings",
        )
        g.require(
            all(
                type(v) is int and 3 <= v <= 10000
                for v in profile["boundary_settings"].values()
            ),
            "invalid boundary grid",
        )
        g.validate_shifts(profile["shifts"])
        g.validate_budgets(profile)
        g.require(
            profile["shifts"]["time_shift_days"] == 1,
            "invalid digital time shift",
        )
    return config


def scenarios(config):
    for kind in INSTRUMENTS:
        for direction in ("call", "put"):
            for spot, days in [
                (spot, days) for spot in (80, 100, 120) for days in (30, 365, 730)
            ] + [(100, 1)]:
                inputs = dict(
                    config["inputs"],
                    option=direction,
                    spot=spot,
                    expiry=(
                        date.fromisoformat(config["inputs"]["valuation"])
                        + timedelta(days=days)
                    ).isoformat(),
                )
                yield (
                    f"ql-digital-{kind}-{direction}-{spot}-{days}d",
                    dict(
                        inputs,
                        payoff=kind,
                        **({"payout": config["payout"]} if kind == "cash" else {}),
                    ),
                )


def rows():
    config = configuration()
    for identifier, inputs in scenarios(config):
        for profile in config["profiles"]:
            scenario = {
                "case_id": identifier,
                "inputs": inputs,
                "tolerances": profile["tolerances"],
                "numerical_tolerances": profile["numerical_tolerances"],
            }
            g.validate_inputs(
                {key: value for key, value in inputs.items() if key in g.INPUTS}
            )
            row = g.reference_row(scenario, STABILITY)
            row["case_id"] += "-" + profile["engine"].lower()
            row["instrument"], row["engine"] = (
                INSTRUMENTS[inputs["payoff"]],
                profile["engine"],
            )
            metadata = row["inputs"]
            metadata.update(profile["settings"])
            metadata.update(profile["shifts"])
            # Boundary prices/native Greeks remain checked; no wrapper time stencil touches expiry.
            smooth = (
                date.fromisoformat(inputs["expiry"])
                - date.fromisoformat(inputs["valuation"])
            ).days > 2
            if not smooth:
                metadata.update(profile["boundary_settings"])
            metadata["wrapper"] = str(
                smooth
                and (
                    profile["engine"] != "FiniteDifferenceDigitalEngine"
                    or inputs["spot"] == 100
                    and inputs["expiry"] == "2026-01-06"
                )
            ).lower()
            metadata.update(
                payoff_condition="strict ITM, zero at strike",
                settlement="expiry",
                tolerance_rationale="digital quadrature or grid and bump truncation, see GENERATION.md",
            )
            yield g.serialize_row(row)
