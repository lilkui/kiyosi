"""American contract references from refined QuantLib finite differences."""

import math
from datetime import date
from functools import lru_cache

from . import oracle as g

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
EXPIRY_REASON = "whole-day stability stencil touches expiry_date"
EXERCISE_REASON = "whole-day stability stencil precedes exercise window"


def exclusions(inputs):
    valuation = date.fromisoformat(inputs["valuation"])
    if (date.fromisoformat(inputs["expiry_date"]) - valuation).days <= 2:
        return dict.fromkeys(g.TIME_MEASURES, EXPIRY_REASON)
    if (valuation - date.fromisoformat(inputs["effective_date"])).days < 2:
        return dict.fromkeys(g.TIME_MEASURES, EXERCISE_REASON)
    return {}


@lru_cache(maxsize=4096)
def native(items, grid):
    option = g.vanilla_option(dict(items), grid)
    result = {"price": option.NPV(), "delta": option.delta(), "gamma": option.gamma()}
    g.require(
        all(math.isfinite(v) for v in result.values()), "non-finite American result"
    )
    return result


def measure(inputs, name, grid=GRIDS[-1], scale=1):
    if name in {"price", "delta", "gamma"}:
        return native(tuple(sorted(inputs.items())), grid)[name]
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
    if metadata["refinement_price"] > max(1e-9, metadata["coarse_refinement_price"]):
        # Grid alignment can hide convergence; verify another grid without enlarging uncertainty.
        refined = measure(inputs, "price", (6400, 6400))
        g.require(
            abs(refined - outputs["price"]) <= metadata["uncertainty_price"],
            f"unstable American final refinement: inputs={inputs}",
        )
    return outputs, metadata
