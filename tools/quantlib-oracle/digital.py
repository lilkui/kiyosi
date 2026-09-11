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
LEGACY = {
    "cash-digital-analytic",
    "asset-digital-analytic",
    "digital-integral",
    "digital-fd",
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
        set(config) == {"inputs", "spots", "maturities", "payout", "profiles"},
        "invalid digital configuration",
    )
    g.require(
        set(config["inputs"]) == g.INPUTS - {"option", "spot", "expiry"},
        "invalid digital market",
    )
    g.require(
        config["spots"] == [80, 100, 120] and config["maturities"] == [30, 365, 730],
        "invalid digital matrix",
    )
    g.require(
        type(config["payout"]) in (int, float)
        and math.isfinite(config["payout"])
        and config["payout"] > 0,
        "invalid cash payout",
    )
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
        g.require(set(settings) == expected, "invalid digital settings")
        for key, value in settings.items():
            g.require(
                value == "crank_nicolson"
                if key == "scheme"
                else type(value) is int and value > 0,
                "invalid digital setting",
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
        g.require(
            set(profile["shifts"])
            == {"spot_shift", "volatility_shift", "rate_shift", "time_shift_days"},
            "invalid digital shifts",
        )
        g.require(
            all(
                type(v) in (int, float) and math.isfinite(v) and v > 0
                for v in profile["shifts"].values()
            ),
            "invalid digital shift",
        )
        g.require(
            type(profile["shifts"]["time_shift_days"]) is int
            and profile["shifts"]["time_shift_days"] == 1,
            "invalid digital time shift",
        )
    return config


def scenarios(config):
    for kind in INSTRUMENTS:
        for direction in ("call", "put"):
            for spot, days in [
                (s, t) for s in config["spots"] for t in config["maturities"]
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
            # Reuse the common market/date/budget validation without admitting digital keys into vanilla inputs.
            g.validate_scenario(
                dict(
                    scenario,
                    case_id=identifier.replace("ql-digital-", "ql-european-"),
                    inputs={k: v for k, v in inputs.items() if k in g.INPUTS},
                )
            )
            fields = g.european_row(scenario, STABILITY).split("\t")
            fields[0] += "-" + profile["engine"].lower()
            fields[1], fields[2] = INSTRUMENTS[inputs["payoff"]], profile["engine"]
            metadata = g.attributes(fields[4])
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
            fields[4] = g.encode(metadata)
            yield "\t".join(fields)


def migrate(line):
    fields = line.split("\t")
    if fields[0] not in LEGACY:
        return line
    inputs = g.attributes(fields[4])
    kind = "asset" if fields[1] == INSTRUMENTS["asset"] else "cash"
    g.require(
        fields[1] == INSTRUMENTS[kind] and fields[2] in ENGINES,
        "invalid digital migration",
    )
    market = {
        "option": inputs["option"],
        "strike": float(inputs["strike"]),
        "spot": 100,
        "rate": 0.04,
        "dividend": 0.01,
        "volatility": 0.3,
        "effective": "2024-12-30",
        "valuation": "2025-01-06",
        "expiry": inputs["expiry"],
        "payoff": kind,
    }
    if kind == "cash":
        market["payout"] = float(inputs["payout"])
    value = g.measure(market, "price")
    inputs.update(market)
    inputs.update(
        source_revision=f"QuantLib-{g.ql.__version__}",
        source_symbol="QuantLib.AnalyticEuropeanEngine",
        reference_kind="analytic",
        reference_provider="QuantLib",
        reference_uncertainty=0,
        payoff_condition="strict ITM, zero at strike",
        settlement="expiry",
        tolerance_rationale="retained comparison budget, see GENERATION.md",
    )
    if fields[0] == "digital-fd":
        inputs.update(
            asset_steps=200, time_steps=200, scheme="crank_nicolson", upper_boundary=0
        )
    fields[4], fields[5] = g.encode(inputs), g.encode({"price": value})
    if fields[8] != "-":
        parts = fields[8].split("|")
        parts[2] = format(value, ".17g")
        fields[8] = "|".join(parts)
    return "\t".join(fields)
