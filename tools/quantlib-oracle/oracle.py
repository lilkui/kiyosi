"""Shared QuantLib calculations and fixture encoding; no product-module dependencies."""

import math
from datetime import date, timedelta
from importlib.metadata import version
from pathlib import Path

import QuantLib as ql

PROJECT = Path(__file__).resolve().parent
HEADER = [
    "case_id",
    "instrument",
    "engine",
    "variant",
    "inputs",
    "outputs",
    "tolerances",
    "monte_carlo",
]
INPUTS = {
    "option",
    "strike",
    "effective_date",
    "expiry_date",
    "valuation",
    "spot",
    "rate",
    "dividend",
    "volatility",
}
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


def attributes(text):
    result = {}
    if text == "-":
        return result
    for item in text.split(";"):
        key, value = item.split("=", 1)
        require(key and value and key not in result, "invalid or duplicate attribute")
        result[key] = value
    return result


def encode(values):
    return ";".join(
        f"{key}={format(value, '.17g') if isinstance(value, float) else value}"
        for key, value in sorted(values.items())
    )


def validate_inputs(inputs):
    require(set(inputs) == INPUTS, "incomplete or unknown inputs")
    require(inputs["option"] in ("call", "put"), "unknown option")
    for key in ("strike", "spot", "rate", "dividend", "volatility"):
        require(
            type(inputs[key]) in (int, float) and math.isfinite(inputs[key]),
            f"invalid {key}",
        )
    require(
        all(inputs[key] > 0 for key in ("strike", "spot", "volatility")),
        "non-positive input",
    )
    dates = {
        key: date.fromisoformat(inputs[key])
        for key in ("effective_date", "valuation", "expiry_date")
    }
    require(
        all(dates[key].isoformat() == inputs[key] for key in dates),
        "dates must be YYYY-MM-DD",
    )
    require(
        dates["effective_date"] <= dates["valuation"] < dates["expiry_date"],
        "invalid date ordering",
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
                valuation, ql.NullCalendar(), inputs["volatility"], day_count
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
        else ql.AmericanExercise(ql.DateParser.parseISO(inputs["effective_date"]), expiry_date),
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
        date.fromisoformat(inputs["expiry_date"]) - date.fromisoformat(inputs["valuation"])
    ).days
    return dict.fromkeys(
        TIME_MEASURES if days <= 2 else (), "whole-day stability stencil touches expiry_date"
    )


def reference(inputs, stability=STABILITY, *, unavailable=None, measure_fn=None):
    """Compute one contract reference, independent of tested engines and budgets."""
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


def reference_row(identifier, inputs, reference, tolerances, numerical_tolerances):
    """Attach test budgets to an already computed reference without mutating it."""
    outputs, metadata = reference
    provenance = dict(
        inputs,
        owner="QuantLib",
        source_revision=f"QuantLib-{ql.__version__}",
        source_symbol="QuantLib.AnalyticEuropeanEngine",
        convention=CONVENTION,
        reference_kind="analytic",
        reference_classification="independent-analytic",
        quantlib=version("QuantLib"),
        numerical_settings="central differences: spot 0.01/0.02 (0.001/0.002 within 2 days of expiry_date),volatility and rate 0.0001/0.0002,time 1/2 calendar days",
        measure_sources="price/delta/gamma/theta/vega/rho native with price fallback,others central delta/gamma differences with price fallback",
        reference_uncertainty="per-measure maximum bump discrepancy, not a rigorous bound",
        tolerance=tolerances["price"],
    )
    provenance.update(metadata)
    provenance.update(
        {f"numerical_tolerance_{name}": numerical_tolerances[name] for name in outputs}
    )
    return {
        "case_id": identifier,
        "instrument": "EuropeanOption",
        "engine": "AnalyticEuropeanEngine",
        "variant": inputs["option"],
        "inputs": provenance,
        "outputs": dict(outputs),
        "tolerances": {name: tolerances[name] for name in outputs},
        "monte_carlo": "-",
    }


def serialize_row(row):
    return "\t".join(
        encode(row[key]) if isinstance(row[key], dict) else row[key] for key in HEADER
    )


def validate_settings(settings, expected):
    require(set(settings) == expected, "invalid engine settings")
    for name, value in settings.items():
        require(
            value in {"explicit_euler", "implicit_euler", "crank_nicolson"}
            if name == "scheme"
            else type(value) is int and value > 0,
            "invalid numerical setting",
        )


def validate_shifts(shifts):
    require(
        set(shifts)
        == {"spot_shift", "volatility_shift", "rate_shift", "time_shift_days"},
        "invalid shifts",
    )
    require(
        all(
            type(value) in (int, float) and math.isfinite(value) and value > 0
            for value in shifts.values()
        )
        and type(shifts["time_shift_days"]) is int,
        "invalid shift",
    )


def expand_profiles(config):
    defaults = config["profile_defaults"]
    require(
        set(defaults) == {"shifts", "tolerances", "numerical_tolerances"},
        "invalid profile defaults",
    )
    return [
        dict(
            profile,
            **{
                field: dict(values, **profile.get(field, {}))
                for field, values in defaults.items()
            },
        )
        for profile in config["profiles"]
    ]
