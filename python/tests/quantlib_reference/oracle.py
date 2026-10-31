"""Shared independent QuantLib calculations; no product-module dependencies."""

import json
import math
from datetime import date, timedelta
from importlib.metadata import version
from pathlib import Path

import QuantLib as ql

DATA_DIR = Path(__file__).resolve().parent
MEASURES = [
    "price",
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "vanna",
    "zomma",
    "rho",
]
TIME_MEASURES = {"theta", "charm", "color"}
UNITS = {
    "price": "price",
    "delta": "price/spot",
    "gamma": "price/spot^2",
    "speed": "price/spot^3",
    "theta": "price/day",
    "charm": "delta/day",
    "color": "gamma/day",
    "vega": "price/volatility-pp",
    "vanna": "delta/volatility-pp",
    "zomma": "gamma/volatility-pp",
    "rho": "price/rate-pp",
}
# Independent stability ceilings, not Kiyosi error allowances.
STABILITY = {
    "price": 1e-10,
    "delta": 1e-6,
    "gamma": 1e-7,
    "speed": 1e-7,
    "theta": 1e-4,
    "charm": 1e-5,
    "color": 2e-6,
    "vega": 1e-6,
    "vanna": 1e-7,
    "zomma": 1e-8,
    "rho": 1e-6,
}
CONVENTION = "Actual/365 Fixed, continuously compounded BSM"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def require_version():
    require(
        version("QuantLib") == ql.__version__ == "1.43",
        "QuantLib 1.43 is required for pricing reference tests",
    )


def validate_budgets(profile):
    for field in ("tolerances", "numerical_tolerances"):
        require(set(profile[field]) == set(MEASURES), f"all measure {field} required")
        require(
            all(
                type(v) in (int, float) and math.isfinite(v) and v >= 0
                for v in profile[field].values()
            ),
            f"invalid {field}",
        )


def market_process(inputs):
    valuation = ql.DateParser.parseISO(inputs["valuation"])
    ql.Settings.instance().evaluationDate = valuation
    day_count = ql.Actual365Fixed()
    curve = lambda rate: ql.YieldTermStructureHandle(
        ql.FlatForward(valuation, rate, day_count, ql.Continuous, ql.Annual)
    )
    return ql.BlackScholesMertonProcess(
        ql.QuoteHandle(ql.SimpleQuote(inputs["spot"])),
        curve(inputs["dividend"]),
        curve(inputs["rate"]),
        ql.BlackVolTermStructureHandle(
            ql.BlackConstantVol(
                valuation, ql.WeekendsOnly(), inputs["volatility"], day_count
            )
        ),
    )


def vanilla_option(inputs, american_grid=None):
    process = market_process(inputs)
    expiry_date = ql.DateParser.parseISO(inputs["expiry_date"])
    direction = ql.Option.Call if inputs["option"] == "call" else ql.Option.Put
    kind = inputs.get("payoff", "vanilla")
    require(kind in {"vanilla", "cash", "asset"}, "unknown payoff")
    payoff = (
        ql.CashOrNothingPayoff(direction, inputs["strike"], inputs["payout"])
        if kind == "cash"
        else ql.AssetOrNothingPayoff(direction, inputs["strike"])
        if kind == "asset"
        else ql.PlainVanillaPayoff(direction, inputs["strike"])
    )
    option = ql.VanillaOption(
        payoff,
        ql.EuropeanExercise(expiry_date)
        if american_grid is None
        else ql.AmericanExercise(
            ql.DateParser.parseISO(inputs["effective_date"]), expiry_date
        ),
    )
    option.setPricingEngine(
        ql.AnalyticEuropeanEngine(process)
        if american_grid is None
        else ql.FdBlackScholesVanillaEngine(
            process, *american_grid, 2, ql.FdmSchemeDesc.Douglas()
        )
    )
    return option


def shifted(inputs, field, bump):
    value = dict(inputs)
    value[field] = (
        (date.fromisoformat(inputs[field]) + timedelta(days=bump)).isoformat()
        if field == "valuation"
        else inputs[field] + bump
    )
    return value


def measure(
    inputs,
    name,
    scale=1,
    price_only=False,
    *,
    option_factory=vanilla_option,
    spot_bump=None,
    earliest_valuation=None,
):
    option = option_factory(inputs)
    if name == "price":
        value = option.NPV()
    else:
        native = {"delta": 1, "gamma": 1, "theta": 365, "vega": 100, "rho": 100}
        if name in native and not price_only and hasattr(option, name):
            try:
                value = getattr(option, name)() / native[name]
            except RuntimeError as error:
                if "not provided" not in str(error):
                    raise
            else:
                require(math.isfinite(value), f"non-finite {name}")
                return value
        field, base, h, unit = {
            "delta": ("spot", "price", 0.001, 1),
            "gamma": ("spot", "price", 0.001, 1),
            "speed": ("spot", "gamma", 0.001, 1),
            "theta": ("valuation", "price", 1, 1),
            "charm": ("valuation", "delta", 1, 1),
            "color": ("valuation", "gamma", 1, 1),
            "vega": ("volatility", "price", 0.0001, 100),
            "vanna": ("volatility", "delta", 0.0001, 100),
            "zomma": ("volatility", "gamma", 0.0001, 100),
            "rho": ("rate", "price", 0.0001, 100),
        }[name]
        if (
            field == "spot"
            and (
                date.fromisoformat(inputs["expiry_date"])
                - date.fromisoformat(inputs["valuation"])
            ).days
            > 2
        ):
            h = 0.01
        if field == "spot" and spot_bump is not None:
            h = spot_bump
        h *= scale
        if field == "valuation":
            if earliest_valuation is not None:
                require(
                    date.fromisoformat(inputs["valuation"]) - timedelta(days=h)
                    >= earliest_valuation,
                    "time stencil precedes supported valuation period",
                )
            require(
                date.fromisoformat(inputs["valuation"]) + timedelta(days=h)
                < date.fromisoformat(inputs["expiry_date"]),
                "time stencil touches expiry_date",
            )
            require(
                date.fromisoformat(inputs["valuation"]) - timedelta(days=h)
                >= date.fromisoformat(inputs["effective_date"]),
                "time stencil precedes effective_date date",
            )
        options = {
            "option_factory": option_factory,
            "spot_bump": spot_bump,
            "earliest_valuation": earliest_valuation,
        }
        up = measure(shifted(inputs, field, h), base, scale, price_only, **options)
        down = measure(shifted(inputs, field, -h), base, scale, price_only, **options)
        value = (
            (up - 2 * option.NPV() + down) / h**2
            if name == "gamma"
            else (up - down) / (2 * h * unit)
        )
    require(math.isfinite(value), f"non-finite {name}")
    return value


def exclusions(inputs):
    days = (
        date.fromisoformat(inputs["expiry_date"])
        - date.fromisoformat(inputs["valuation"])
    ).days
    return dict.fromkeys(
        TIME_MEASURES if days <= 2 else (),
        "whole-day stability stencil touches expiry_date",
    )


def reference(inputs, stability=STABILITY, *, unavailable=None, measure_fn=None):
    """Compute one contract reference, independent of tested engines and budgets.

    Parameters
    ----------
    inputs : dict
        Independent scenario inputs.
    stability : dict of str to float, optional
        Maximum allowed independent-reference uncertainty by measure.
    unavailable : dict or None, optional
        Excluded measures and reasons; None derives exclusions from inputs.
    measure_fn : callable or None, optional
        Independent measure calculator; None uses the default oracle.

    Returns
    -------
    tuple of dict
        Finite outputs and independent uncertainty metadata.
    """
    measure_fn = measure if measure_fn is None else measure_fn
    unavailable = exclusions(inputs) if unavailable is None else unavailable
    outputs, metadata = {}, {}
    for name in MEASURES:
        metadata[f"unit_{name}"] = UNITS[name]
        if name in unavailable:
            metadata[f"unavailable_{name}"] = unavailable[name]
            continue
        value = measure_fn(inputs, name)
        # Force price fallback for first-order measures and gamma to validate both paths.
        fallback = name in {"delta", "gamma", "theta", "vega", "rho"}
        estimates = [measure_fn(inputs, name, scale, fallback) for scale in (1, 2)]
        require(
            all(math.isfinite(v) for v in [value, *estimates]), f"non-finite {name}"
        )
        uncertainty = max(abs(value - estimate) for estimate in estimates)
        require(
            uncertainty <= stability[name],
            f"unstable {name}: {uncertainty}, inputs={inputs}",
        )
        outputs[name] = value
        metadata[f"uncertainty_{name}"] = uncertainty
        metadata[f"stability_limit_{name}"] = stability[name]
    require(outputs["price"] >= 0, "invalid QuantLib price")
    return outputs, metadata


def reference_row(
    identifier,
    inputs,
    reference,
    tolerances,
    numerical_tolerances,
    *,
    instrument,
    engine,
):
    """Attach product identity and comparison budgets without mutating inputs.

    Parameters
    ----------
    identifier : str
        Unique scenario identifier.
    inputs : dict
        Independent scenario inputs.
    reference : tuple of dict
        Independent outputs and metadata.
    tolerances : dict of str to float
        Acceptance budgets for native results.
    numerical_tolerances : dict of str to float
        Acceptance budgets for numerical Greeks.
    instrument : str
        Product identity used by the scenario.
    engine : str
        Engine identity used by the scenario.

    Returns
    -------
    dict
        Scenario identity, copied inputs, outputs and comparison budgets.
    """
    outputs, metadata = reference
    attributes = dict(
        inputs,
        convention=CONVENTION,
        calendar="weekends_only",
    )
    attributes.update(metadata)
    attributes.update(
        {f"numerical_tolerance_{name}": numerical_tolerances[name] for name in outputs}
    )
    return {
        "case_id": identifier,
        "instrument": instrument,
        "engine": engine,
        "inputs": attributes,
        "outputs": dict(outputs),
        "tolerances": {name: tolerances[name] for name in outputs},
    }


def load_fixture(name):
    """Load fresh fixture data with defaults expanded and budgets checked."""
    config = json.loads((DATA_DIR / name).read_text(encoding="utf-8"))
    for collection, defaults_key in (
        ("scenarios", "defaults"),
        ("profiles", "profile_defaults"),
    ):
        for record in config.get(collection, []):
            for field, values in config.get(defaults_key, {}).items():
                record[field] = dict(values, **record.get(field, {}))
            if collection == "profiles" or defaults_key in config:
                validate_budgets(record)
    if "scenarios" in config:
        rows = config["scenarios"]
        require(bool(rows), "no scenarios")
        require(
            len({row["case_id"] for row in rows}) == len(rows),
            "duplicate scenario identifier",
        )
    return config
