"""American contract references from refined QuantLib finite differences."""

import json
import math
import re
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
    g.require(
        set(data) == {"scenarios", "profiles", "profile_defaults"},
        "invalid American configuration",
    )
    data["profiles"] = g.expand_profiles(data)
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
            re.fullmatch(r"ql-american-[a-z0-9-]+", scenario["case_id"])
            and scenario["case_id"] not in seen,
            "invalid American case identifier",
        )
        seen.add(scenario["case_id"])
        g.require(type(scenario["wrapper"]) is bool, "invalid wrapper selection")
        inputs = scenario["inputs"]
        g.validate_inputs(inputs)
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
            g.validate_settings(profile["settings"], ENGINES[engine])
            g.require(
                profile["settings"].get("scheme", "crank_nicolson") == "crank_nicolson",
                "invalid American scheme",
            )
            g.validate_shifts(profile["shifts"])
            g.validate_budgets(profile)
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
            yield g.serialize_row(
                {
                    "case_id": scenario["case_id"] + "-" + engine.lower(),
                    "instrument": "AmericanOption",
                    "engine": engine,
                    "variant": inputs["option"],
                    "inputs": attributes,
                    "outputs": outputs,
                    "tolerances": {
                        name: profile["tolerances"][name] for name in outputs
                    },
                    "validation": "-",
                    "convergence": "-",
                    "monte_carlo": mc,
                }
            )
