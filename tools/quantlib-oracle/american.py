"""American contract references from refined QuantLib finite differences."""

import json
import math
from datetime import date
from functools import lru_cache
from importlib.metadata import version

import generate as g
import QuantLib as ql

GRIDS = ((800, 800), (1600, 1600), (3200, 3200))
STABILITY = {
    "price": 0.001,
    "delta": 0.0002,
    "gamma": 0.00005,
    "speed": 0.00002,
    "theta": 0.00003,
    "charm": 0.00002,
    "color": 0.000002,
    "vega": 0.001,
    "vanna": 0.00003,
    "zomma": 0.00001,
    "rho": 0.001,
}
ENGINES = {
    "BinomialAmericanEngine": {"steps"},
    "CrrEngine": {"steps"},
    "FiniteDifferenceAmericanEngine": {
        "asset_steps",
        "time_steps",
        "scheme",
        "upper_boundary",
    },
    "BjerksundStenslandAmericanEngine": set(),
    "MonteCarloAmericanEngine": {"seed", "paths", "steps"},
}
LEGACY = {
    "american-binomial": {"steps": 200},
    "american-fd": {
        "asset_steps": 200,
        "time_steps": 4000,
        "scheme": "explicit_euler",
        "upper_boundary": 0,
    },
    "american-bs": {},
    "american-mc": {"seed": 42, "paths": 20000, "steps": 50},
}
EXPIRY_REASON = "whole-day stability stencil touches expiry"
EXERCISE_REASON = "whole-day stability stencil precedes exercise window"


def exclusions(inputs):
    valuation = date.fromisoformat(inputs["valuation"])
    if (date.fromisoformat(inputs["expiry"]) - valuation).days <= 2:
        return dict.fromkeys(g.TIME_MEASURES, EXPIRY_REASON)
    if (valuation - date.fromisoformat(inputs["effective"])).days < 2:
        return dict.fromkeys(g.TIME_MEASURES, EXERCISE_REASON)
    return {}


@lru_cache(maxsize=4096)
def native(encoded, grid):
    inputs = json.loads(encoded)
    option = g.vanilla_option(inputs, grid)
    result = dict(price=option.NPV(), delta=option.delta(), gamma=option.gamma())
    g.require(
        all(math.isfinite(v) for v in result.values()), "non-finite American result"
    )
    return result


def measure(inputs, name, grid=GRIDS[-1], scale=1):
    if name in {"price", "delta", "gamma"}:
        return native(json.dumps(inputs, sort_keys=True), grid)[name]
    field, base, bump, unit = {
        "speed": ("spot", "gamma", 0.5, 1),
        "theta": ("valuation", "price", 1, 1),
        "charm": ("valuation", "delta", 1, 1),
        "color": ("valuation", "gamma", 1, 1),
        "vega": ("volatility", "price", 0.002, 100),
        "vanna": ("volatility", "delta", 0.002, 100),
        "zomma": ("volatility", "gamma", 0.002, 100),
        "rho": ("rate", "price", 0.001, 100),
    }[name]
    h = bump * scale
    g.require(name not in exclusions(inputs), f"unavailable American {name}")
    return (
        measure(g.shifted(inputs, field, h), base, grid)
        - measure(g.shifted(inputs, field, -h), base, grid)
    ) / (2 * h * unit)


def reference(inputs):
    outputs, metadata = {}, {}
    unavailable = exclusions(inputs)
    for name in g.MEASURES:
        metadata[f"unit_{name}"] = g.UNITS[name]
        if name in unavailable:
            metadata[f"unavailable_{name}"] = unavailable[name]
            continue
        values = [measure(inputs, name, grid) for grid in GRIDS]
        value = values[-1]
        refinement = abs(values[-1] - values[-2])
        if name in {"delta", "gamma"}:
            estimates = []
            for h in (0.5, 1.0):
                up = measure(g.shifted(inputs, "spot", h), "price")
                down = measure(g.shifted(inputs, "spot", -h), "price")
                estimates.append(
                    (up - down) / (2 * h)
                    if name == "delta"
                    else (up - 2 * measure(inputs, "price") + down) / h**2
                )
        else:
            estimates = [measure(inputs, name, scale=2)]
        bump_error = max(abs(value - estimate) for estimate in estimates)
        uncertainty = max(refinement, bump_error)
        g.require(
            all(math.isfinite(v) for v in [*values, *estimates]),
            f"non-finite American {name}",
        )
        g.require(
            uncertainty <= STABILITY[name],
            f"unstable American {name}: {uncertainty}, inputs={inputs}",
        )
        outputs[name] = value
        metadata.update(
            {
                f"uncertainty_{name}": uncertainty,
                f"refinement_{name}": refinement,
                f"coarse_refinement_{name}": abs(values[1] - values[0]),
                f"bump_error_{name}": bump_error,
                f"stability_limit_{name}": STABILITY[name],
            }
        )
    return outputs, metadata


def provenance(inputs):
    return dict(
        inputs,
        source_revision=f"QuantLib-{ql.__version__}",
        source_symbol="QuantLib.FdBlackScholesVanillaEngine",
        convention=g.CONVENTION,
        reference_kind="discretized",
        reference_classification="convergence-verified-contract",
        quantlib_python=version("QuantLib-Python"),
        quantlib=version("QuantLib"),
        exercise="AmericanExercise(effective,expiry,payoffAtExpiry=false)",
        reference_grids="800x800,1600x1600,3200x3200 time-by-space",
        reference_scheme="Douglas with 2 damping steps, localVol=false, continuous dividend yield",
        numerical_settings="spot 0.5/1,volatility 0.002/0.004,rate 0.001/0.002,time 1/2 calendar days",
        measure_sources="price/delta/gamma native,others central differences of price/delta/gamma",
        reference_uncertainty="maximum final-grid and bump discrepancy, not a rigorous bound",
    )


def rows():
    data = json.loads((g.PROJECT / "american.json").read_text())
    g.require(set(data) == {"scenarios", "profiles"}, "invalid American configuration")
    g.require(
        {p["engine"] for p in data["profiles"]} == set(ENGINES)
        and len(data["profiles"]) == len(ENGINES),
        "incomplete American engines",
    )
    seen = set()
    for scenario in data["scenarios"]:
        g.require(
            set(scenario) == {"case_id", "inputs", "wrapper"},
            "invalid American scenario",
        )
        g.require(
            scenario["case_id"].startswith("ql-american-")
            and scenario["case_id"] not in seen,
            "invalid American case identifier",
        )
        seen.add(scenario["case_id"])
        g.require(type(scenario["wrapper"]) is bool, "invalid wrapper selection")
        inputs = scenario["inputs"]
        # Reuse the common contract/market validation with European identifier syntax.
        g.validate_scenario(
            {
                "case_id": scenario["case_id"].replace("ql-american-", "ql-european-"),
                "inputs": inputs,
                "tolerances": STABILITY,
                "numerical_tolerances": STABILITY,
            }
        )
        outputs, metadata = reference(inputs)
        for profile in data["profiles"]:
            engine = profile["engine"]
            g.require(
                set(profile)
                == {
                    "engine",
                    "settings",
                    "shifts",
                    "tolerances",
                    "numerical_tolerances",
                },
                "invalid American profile",
            )
            g.require(
                set(profile["settings"]) == ENGINES[engine], "invalid American settings"
            )
            for key, value in profile["settings"].items():
                g.require(
                    value == "crank_nicolson"
                    if key == "scheme"
                    else type(value) is int and value > 0,
                    "invalid American setting",
                )
            g.require(
                set(profile["shifts"])
                == {"spot_shift", "volatility_shift", "rate_shift", "time_shift_days"},
                "invalid American shifts",
            )
            g.require(
                all(
                    type(v) in (int, float) and math.isfinite(v) and v > 0
                    for v in profile["shifts"].values()
                )
                and type(profile["shifts"]["time_shift_days"]) is int,
                "invalid American shift",
            )
            for field in ("tolerances", "numerical_tolerances"):
                g.require(
                    set(profile[field]) == set(g.MEASURES)
                    and all(
                        type(v) in (int, float) and math.isfinite(v) and v >= 0
                        for v in profile[field].values()
                    ),
                    "invalid American budgets",
                )
            attributes = provenance(inputs)
            attributes.update(metadata)
            attributes.update(profile["settings"])
            attributes.update(profile["shifts"])
            attributes.update(
                owner="QuantLib",
                wrapper=str(scenario["wrapper"]).lower(),
                tolerance=profile["tolerances"]["price"],
                tolerance_rationale="separate discretization, BS2002 approximation or LSM sampling budgets: GENERATION.md",
            )
            attributes.update(
                {
                    f"numerical_tolerance_{name}": profile["numerical_tolerances"][name]
                    for name in outputs
                }
            )
            mc = (
                "|".join(
                    str(attributes[key])
                    for key in ("seed", "paths", "steps", "tolerance")
                )
                if engine == "MonteCarloAmericanEngine"
                else "-"
            )
            yield "\t".join(
                [
                    scenario["case_id"] + "-" + engine.lower(),
                    "AmericanOption",
                    engine,
                    inputs["option"],
                    g.encode(attributes),
                    g.encode(outputs),
                    g.encode({name: profile["tolerances"][name] for name in outputs}),
                    "-",
                    "-",
                    mc,
                ]
            )


def migrate(line):
    fields = line.split("\t")
    if fields[0] not in LEGACY:
        return line
    old = g.attributes(fields[4])
    inputs = {
        "option": old["option"],
        "strike": 100,
        "spot": 100,
        "rate": 0.04,
        "dividend": 0.01,
        "volatility": 0.3,
        "effective": "2024-12-30",
        "valuation": "2025-01-06",
        "expiry": "2026-01-06",
    }
    for key in g.INPUTS & old.keys():
        inputs[key] = (
            float(old[key])
            if key in {"strike", "spot", "rate", "dividend", "volatility"}
            else old[key]
        )
    prices = [measure(inputs, "price", grid) for grid in GRIDS]
    uncertainty = abs(prices[-1] - prices[-2])
    g.require(uncertainty <= STABILITY["price"], "unconverged migrated American price")
    old.update(provenance(inputs))
    old.update(LEGACY[fields[0]])
    old.update(
        reference_provider="QuantLib",
        reference_uncertainty=uncertainty,
        tolerance_rationale="retained absolute comparison budget: GENERATION.md",
    )
    fields[4] = g.encode(old)
    fields[5] = g.encode({"price": prices[-1]})
    if fields[8] != "-":
        convergence = fields[8].split("|")
        convergence[2] = format(prices[-1], ".17g")
        fields[8] = "|".join(convergence)
    return "\t".join(fields)
