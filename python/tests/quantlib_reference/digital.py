"""European digital references using QuantLib's analytic European engine."""

import json
import math
from datetime import date, timedelta

from . import oracle as g

INSTRUMENTS = {
    "cash": "CashOrNothingOption",
    "asset": "AssetOrNothingOption",
}
ENGINES = {
    "AnalyticDigitalEngine",
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
    config = json.loads((g.DATA_DIR / "digital.json").read_text(encoding="utf-8"))
    g.require(
        set(config) == {"inputs", "payout", "profiles", "profile_defaults"},
        "invalid digital configuration",
    )
    g.require(
        set(config["inputs"]) == g.INPUTS - {"option", "spot", "expiry_date"},
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
        and len(config["profiles"]) == len(ENGINES),
        "invalid digital engines",
    )
    for profile in config["profiles"]:
        g.validate_profile(profile, extra_fields={"boundary_settings"})
        g.require(
            set(profile["boundary_settings"])
            == (
                {"asset_step_count"}
                if profile["engine"] == "FiniteDifferenceDigitalEngine"
                else set()
            ),
            "invalid digital boundary settings",
        )
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
                    expiry_date=(
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
