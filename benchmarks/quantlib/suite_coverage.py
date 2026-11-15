"""Independent product/model coverage required by the counterpart inventory."""

# Keep this inventory independent of cases/: deleting a scenario must expose a gap.
REQUIRED = []


def require(engine, family, product, *, model="bsm", exercise="european", **terms):
    """Declare one required semantic cell, independent of scenario IDs."""
    selector = {"product": product, "exercise": exercise, **terms}
    REQUIRED.append(
        {
            "id": f"{family}/" + "/".join(map(str, selector.values())),
            "engine": engine,
            "family": family,
            "model": model,
            "selector": selector,
        }
    )


require("AnalyticEuropeanEngine", "european_bsm", "vanilla")
for payout in ("cash", "asset"):
    require("AnalyticDigitalEngine", "digital_bsm", "digital", payout=payout)
require("AnalyticGeometricAveragePriceEngine", "geometric_asian_bsm", "geometric_asian")
require(
    "TurnbullWakemanArithmeticAveragePriceEngine",
    "arithmetic_asian_bsm",
    "arithmetic_asian",
)
require("AnalyticBarrierEngine", "single_barrier_bsm", "barrier")
for payout in ("cash", "asset"):
    require(
        "AnalyticBinaryBarrierEngine",
        "binary_barrier_bsm",
        "binary_barrier",
        payout=payout,
    )
    for activation, settlement in (("in", "hit"), ("in", "expiry"), ("out", "expiry")):
        require(
            "AnalyticBinaryBarrierEngine",
            "touch_bsm",
            "touch",
            payout=payout,
            activation=activation,
            settlement=settlement,
        )
require("AnalyticDoubleBarrierEngine", "double_barrier_bsm", "double_barrier")
for activation in ("in", "out"):
    require(
        "AnalyticDoubleTouchEngine",
        "double_touch_bsm",
        "double_touch",
        payout="cash",
        activation=activation,
        settlement="expiry",
    )
require("AnalyticHestonEuropeanEngine", "heston_analytic", "vanilla", model="heston")
require("QuadratureVanillaEngine", "quadrature_vanilla", "vanilla")
for payout in ("cash", "asset"):
    require("QuadratureDigitalEngine", "quadrature_digital", "digital", payout=payout)
for exercise in ("european", "american"):
    require(
        "CoxRossRubinsteinVanillaEngine", "crr_vanilla", "vanilla", exercise=exercise
    )
require("CoxRossRubinsteinBarrierEngine", "crr_barrier", "barrier")
for family, model in (("fd_bsm", "bsm"), ("fd_local_vol", "local_volatility")):
    for exercise in ("european", "american"):
        require(
            "FiniteDifferenceVanillaEngine",
            family,
            "vanilla",
            model=model,
            exercise=exercise,
        )
    for payout in ("cash", "asset"):
        require(
            "FiniteDifferenceDigitalEngine",
            family,
            "digital",
            model=model,
            payout=payout,
        )
        require(
            "FiniteDifferenceBinaryBarrierEngine",
            family,
            "binary_barrier",
            model=model,
            payout=payout,
        )
        for activation, settlement in (("out", "expiry"), ("in", "hit")):
            require(
                "FiniteDifferenceBinaryBarrierEngine",
                family,
                "touch",
                model=model,
                payout=payout,
                activation=activation,
                settlement=settlement,
            )
    require("FiniteDifferenceBarrierEngine", family, "barrier", model=model)
for exercise in ("european", "american"):
    require(
        "FiniteDifferenceHestonVanillaEngine",
        f"heston_fd_{exercise}",
        "vanilla",
        model="heston",
        exercise=exercise,
    )
    require(
        "MonteCarloVanillaEngine", f"mc_{exercise}_bsm", "vanilla", exercise=exercise
    )
require("MonteCarloBarrierEngine", "mc_barrier_bsm", "barrier")

# Implicit semantics follow the native adapters, not case ID spelling.
PRODUCTS = {
    "european_bsm": "vanilla",
    "digital_bsm": "digital",
    "geometric_asian_bsm": "geometric_asian",
    "arithmetic_asian_bsm": "arithmetic_asian",
    "single_barrier_bsm": "barrier",
    "binary_barrier_bsm": "binary_barrier",
    "touch_bsm": "touch",
    "double_barrier_bsm": "double_barrier",
    "double_touch_bsm": "double_touch",
    "quadrature_vanilla": "vanilla",
    "quadrature_digital": "digital",
    "crr_vanilla": "vanilla",
    "crr_barrier": "barrier",
    "heston_analytic": "vanilla",
    "heston_fd_european": "vanilla",
    "heston_fd_american": "vanilla",
    "mc_european_bsm": "vanilla",
    "mc_american_bsm": "vanilla",
    "mc_barrier_bsm": "barrier",
}


def matches(case, cell):
    """Match actual adapter semantics, including implicit European/expiry terms."""
    if any(case.get(key) != cell[key] for key in ("engine", "family", "model")):
        return False
    family = case["family"]
    inputs = case.get("inputs")
    if not isinstance(inputs, dict):
        return False
    semantics = {
        "product": PRODUCTS.get(family),
        "exercise": "american"
        if family in ("heston_fd_american", "mc_american_bsm")
        else "european",
        "settlement": "expiry",
        **inputs,
    }
    if family == "double_touch_bsm":
        semantics["payout"] = "cash"
    if family in ("binary_barrier_bsm", "touch_bsm", "quadrature_digital"):
        semantics["payout"] = inputs.get("payoff")
    if "barrier_type" in inputs:
        if not isinstance(inputs["barrier_type"], str):
            return False
        semantics["activation"] = inputs["barrier_type"].split("_")[-1]
    if not isinstance(semantics["settlement"], str):
        return False
    semantics["settlement"] = {"at_hit": "hit", "at_expiry": "expiry"}.get(
        semantics["settlement"], semantics["settlement"]
    )
    if family == "fd_local_vol":
        points = [
            inputs.get(key)
            for key in (
                "surface_start_low",
                "surface_start_high",
                "surface_end_low",
                "surface_end_high",
            )
        ]
        if (
            inputs.get("operation") != "price"
            or any(type(point) not in (int, float) for point in points)
            or len(set(points)) < 2
        ):
            return False
    if family in ("geometric_asian_bsm", "arithmetic_asian_bsm") and inputs.get(
        "averaging_start"
    ) != inputs.get("valuation"):
        return False
    return all(semantics.get(key) == value for key, value in cell["selector"].items())


def audit(cases):
    """Return each required cell and its matching scenario IDs."""
    return [
        {
            **cell,
            "cases": [case["id"] for case in cases.values() if matches(case, cell)],
        }
        for cell in REQUIRED
    ]
