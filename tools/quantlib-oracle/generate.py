"""Generate all reference rows from QuantLib; never consume Kiyosi prices."""

import json
import math
import os
import re
import tempfile
from datetime import date, timedelta
from importlib.metadata import version
from pathlib import Path

import QuantLib as ql

PROJECT = Path(__file__).resolve().parent
FIXTURE = PROJECT.parents[1] / "tests/fixtures/pricing_reference.tsv"
HEADER = [
    "case_id",
    "instrument",
    "engine",
    "variant",
    "inputs",
    "outputs",
    "tolerances",
    "validation",
    "convergence",
    "monte_carlo",
]
INPUTS = {
    "option",
    "strike",
    "effective",
    "expiry",
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
NUMERICAL_ENGINES = {
    "BinomialEuropeanEngine": {"steps"},
    "CrrEngine": {"steps"},
    "IntegralEuropeanEngine": set(),
    "FiniteDifferenceEuropeanEngine": {
        "asset_steps",
        "time_steps",
        "scheme",
        "upper_boundary",
    },
    "MonteCarloEuropeanEngine": {"seed", "paths", "steps"},
}


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


def validate_scenario(scenario):
    require(
        set(scenario) == {"case_id", "inputs", "tolerances", "numerical_tolerances"},
        "incomplete scenario",
    )
    require(
        re.fullmatch(r"ql-european-[a-z0-9-]+", scenario["case_id"]),
        "invalid case identifier",
    )
    validate_inputs(scenario["inputs"])
    validate_budgets(scenario)


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
        for key in ("effective", "valuation", "expiry")
    }
    require(
        all(dates[key].isoformat() == inputs[key] for key in dates),
        "dates must be YYYY-MM-DD",
    )
    require(
        dates["effective"] <= dates["valuation"] < dates["expiry"],
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
    expiry = ql.DateParser.parseISO(inputs["expiry"])
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
        ql.EuropeanExercise(expiry)
        if american_grid is None
        else ql.AmericanExercise(ql.DateParser.parseISO(inputs["effective"]), expiry),
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


def measure(inputs, name, scale=1, price_only=False):
    if "averaging" in inputs:
        import asian

        option = asian.option(inputs)
    elif "barrier_kind" in inputs:
        import barrier
        import binary

        is_binary = "asset_settlement" in inputs
        option = (binary if is_binary else barrier).option(inputs)
        price_only = price_only or not is_binary
    else:
        option = vanilla_option(inputs)
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
                date.fromisoformat(inputs["expiry"])
                - date.fromisoformat(inputs["valuation"])
            ).days
            > 2
        ):
            h = 0.01
        if field == "spot" and "barrier_kind" in inputs:
            h = 0.02 if "asset_settlement" in inputs else 0.03
        if field == "spot" and "averaging" in inputs:
            h = 0.02
        h *= scale
        if field == "valuation":
            if "averaging" in inputs:
                require(
                    date.fromisoformat(inputs["valuation"]) - timedelta(days=h)
                    > date.fromisoformat(inputs["average_start"]),
                    "time stencil touches averaging start",
                )
            require(
                date.fromisoformat(inputs["valuation"]) + timedelta(days=h)
                < date.fromisoformat(inputs["expiry"]),
                "time stencil touches expiry",
            )
            require(
                date.fromisoformat(inputs["valuation"]) - timedelta(days=h)
                >= date.fromisoformat(inputs["effective"]),
                "time stencil precedes effective date",
            )
        up = measure(shifted(inputs, field, h), base, scale, price_only)
        down = measure(shifted(inputs, field, -h), base, scale, price_only)
        value = (
            (up - 2 * option.NPV() + down) / h**2
            if name == "gamma"
            else (up - down) / (2 * h * unit)
        )
    require(math.isfinite(value), f"non-finite {name}")
    return value


def contract_row(identifier, inputs, budget, stability):
    return reference_row(
        {
            "case_id": identifier,
            "inputs": inputs,
            "tolerances": budget,
            "numerical_tolerances": budget,
        },
        stability,
    )


def reference_row(scenario, stability=STABILITY):
    inputs = scenario["inputs"]
    outputs, metadata = {}, {}
    days = (
        date.fromisoformat(inputs["expiry"]) - date.fromisoformat(inputs["valuation"])
    ).days
    import asian
    import binary

    unavailable = (
        asian.exclusions(inputs)
        if "averaging" in inputs
        else binary.exclusions(inputs)
        if "asset_settlement" in inputs
        else dict.fromkeys(
            TIME_MEASURES if days <= 2 else (),
            "whole-day stability stencil touches expiry",
        )
    )
    for name in MEASURES:
        metadata[f"unit_{name}"] = UNITS[name]
        if name in unavailable:
            metadata[f"unavailable_{name}"] = unavailable[name]
            continue
        value = measure(inputs, name)
        # Force price fallback for first-order measures and gamma to validate both paths.
        fallback = name in {"delta", "gamma", "theta", "vega", "rho"}
        estimates = [measure(inputs, name, scale, fallback) for scale in (1, 2)]
        require(
            all(math.isfinite(v) for v in [value, *estimates]), f"non-finite {name}"
        )
        uncertainty = max(abs(value - estimate) for estimate in estimates)
        require(
            uncertainty <= stability[name],
            f"{scenario['case_id']}: unstable {name}: {uncertainty}",
        )
        outputs[name] = value
        metadata[f"uncertainty_{name}"] = uncertainty
        metadata[f"stability_limit_{name}"] = stability[name]
        metadata[f"numerical_tolerance_{name}"] = scenario["numerical_tolerances"][name]
    require(outputs["price"] >= 0, "invalid QuantLib price")
    provenance = dict(
        inputs,
        owner="QuantLib",
        source_revision=f"QuantLib-{ql.__version__}",
        source_symbol="QuantLib.AnalyticEuropeanEngine",
        convention=CONVENTION,
        reference_kind="analytic",
        reference_classification="independent-analytic",
        quantlib=version("QuantLib"),
        numerical_settings="central differences: spot 0.01/0.02 (0.001/0.002 within 2 days of expiry),volatility and rate 0.0001/0.0002,time 1/2 calendar days",
        measure_sources="price/delta/gamma/theta/vega/rho native with price fallback,others central delta/gamma differences with price fallback",
        reference_uncertainty="per-measure maximum bump discrepancy, not a rigorous bound",
        tolerance=scenario["tolerances"]["price"],
    )
    provenance.update(metadata)
    return {
        "case_id": scenario["case_id"],
        "instrument": "EuropeanOption",
        "engine": "AnalyticEuropeanEngine",
        "variant": inputs["option"],
        "inputs": provenance,
        "outputs": outputs,
        "tolerances": {name: scenario["tolerances"][name] for name in outputs},
        "validation": "-",
        "convergence": "-",
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


def numerical_profiles(path):
    config = json.loads(path.read_text(encoding="utf-8"))
    require(set(config) == {"profiles", "profile_defaults"}, "invalid profiles")
    profiles = expand_profiles(config)
    require(
        isinstance(profiles, list) and len(profiles) == len(NUMERICAL_ENGINES),
        "incomplete engine profiles",
    )
    require(
        {p["engine"] for p in profiles} == set(NUMERICAL_ENGINES),
        "unknown or duplicate engine",
    )
    for profile in profiles:
        require(
            set(profile)
            == {"engine", "settings", "shifts", "tolerances", "numerical_tolerances"},
            "invalid profile",
        )
        validate_settings(profile["settings"], NUMERICAL_ENGINES[profile["engine"]])
        validate_shifts(profile["shifts"])
        require(profile["shifts"]["time_shift_days"] <= 3, "invalid time shift")
        validate_budgets(profile)
    return profiles


def numerical_row(scenario, profile):
    row = reference_row(
        dict(
            scenario,
            tolerances=profile["tolerances"],
            numerical_tolerances=profile["numerical_tolerances"],
        )
    )
    row["case_id"] += "-" + profile["engine"].lower()
    row["engine"] = profile["engine"]
    inputs = row["inputs"]
    inputs.update(profile["settings"])
    inputs.update(profile["shifts"])
    inputs["wrapper"] = str(
        scenario["inputs"]["spot"] == 100
        and scenario["inputs"]["expiry"] == "2026-01-06"
    ).lower()
    inputs["tolerance_rationale"] = (
        "engine-specific discretization or sampling budget, see GENERATION.md"
    )
    if row["engine"] == "MonteCarloEuropeanEngine":
        row["monte_carlo"] = "|".join(
            format(inputs[key], ".17g")
            if isinstance(inputs[key], float)
            else str(inputs[key])
            for key in ("seed", "paths", "steps", "tolerance")
        )
    return serialize_row(row)


def validate_manifest(text):
    seen = set()
    lines = [line for line in text.splitlines() if line and not line.startswith("#")]
    require(lines and lines[0].split("\t") == HEADER, "invalid TSV header")
    for line in lines[1:]:
        fields = line.split("\t")
        require(len(fields) == 10 and all(fields), "invalid TSV row")
        require(fields[0] not in seen, "duplicate case identifier")
        seen.add(fields[0])
        inputs, outputs, tolerances = map(attributes, fields[4:7])
        require(set(outputs) == set(tolerances), "outputs and tolerances must match")
        require(
            all(math.isfinite(float(value)) for value in outputs.values()),
            "non-finite output",
        )
        require(
            all(
                math.isfinite(float(value)) and float(value) >= 0
                for value in tolerances.values()
            ),
            "invalid output tolerance",
        )
        for key in (
            "source_revision",
            "source_symbol",
            "convention",
            "reference_kind",
            "tolerance",
        ):
            require(key in inputs, f"missing provenance {key}")
        require(
            math.isfinite(float(inputs["tolerance"]))
            and float(inputs["tolerance"]) >= 0,
            "invalid provenance tolerance",
        )
        require(
            inputs.get("owner") == "QuantLib" and fields[0].startswith("ql-"),
            "all reference rows must be generated by QuantLib",
        )

        import american
        import asian
        import barrier
        import binary
        import digital

        is_asian = fields[1] in asian.INSTRUMENTS.values()
        is_binary = fields[1] == "BinaryBarrierOption"
        is_barrier = fields[1] == "BarrierOption"
        is_american = fields[1] == "AmericanOption"
        is_digital = fields[1] in digital.INSTRUMENTS.values()
        require(
            is_asian
            or is_binary
            or is_barrier
            or is_american
            or is_digital
            or fields[1] == "EuropeanOption",
            "unknown generated instrument",
        )
        limits = (
            asian.STABILITY
            if is_asian
            else binary.STABILITY
            if is_binary
            else barrier.STABILITY
            if is_barrier
            else american.STABILITY
            if is_american
            else digital.STABILITY
            if is_digital
            else STABILITY
        )
        days = (
            date.fromisoformat(inputs["expiry"])
            - date.fromisoformat(inputs["valuation"])
        ).days
        unavailable = (
            asian.exclusions(inputs)
            if is_asian
            else binary.exclusions(inputs)
            if is_binary
            else american.exclusions(inputs)
            if is_american
            else dict.fromkeys(
                TIME_MEASURES if days <= 2 else set(),
                "whole-day stability stencil touches expiry",
            )
        )
        require(
            set(outputs) == set(MEASURES) - unavailable.keys(),
            "incomplete measures",
        )
        require(
            {
                key.removeprefix("unavailable_")
                for key in inputs
                if key.startswith("unavailable_")
            }
            == unavailable.keys(),
            "invalid unavailable declarations",
        )
        for name in MEASURES:
            require(inputs.get(f"unit_{name}") == UNITS[name], f"invalid {name} unit")
            if name in unavailable:
                require(
                    inputs[f"unavailable_{name}"] == unavailable[name],
                    "invalid unavailable reason",
                )
                continue
            uncertainty = float(inputs[f"uncertainty_{name}"])
            require(
                math.isfinite(uncertainty) and 0 <= uncertainty <= limits[name],
                "unstable output",
            )
            require(
                float(inputs[f"stability_limit_{name}"]) == limits[name],
                "invalid stability limit",
            )
            budget = float(inputs[f"numerical_tolerance_{name}"])
            require(
                math.isfinite(budget) and budget >= 0, "invalid numerical tolerance"
            )


def load_scenarios(path):
    config = json.loads(path.read_text(encoding="utf-8"))
    require(set(config) == {"defaults", "scenarios"}, "invalid scenario configuration")
    defaults = config["defaults"]
    require(
        set(defaults) == {"inputs", "tolerances", "numerical_tolerances"},
        "invalid scenario defaults",
    )
    require(
        isinstance(config["scenarios"], list) and config["scenarios"], "no scenarios"
    )
    scenarios = []
    for scenario in config["scenarios"]:
        require(
            {"case_id", "inputs"} <= set(scenario) <= {"case_id", *defaults},
            "invalid scenario overrides",
        )
        expanded = dict(scenario)
        for field in defaults:
            expanded[field] = dict(defaults[field], **scenario.get(field, {}))
        validate_scenario(expanded)
        scenarios.append(expanded)
    require(
        len({item["case_id"] for item in scenarios}) == len(scenarios),
        "duplicate scenario identifier",
    )
    return scenarios


def regenerate(
    fixture=FIXTURE,
    scenarios_path=PROJECT / "scenarios.json",
    profiles_path=PROJECT / "numerical_engines.json",
):
    import american
    import asian
    import barrier
    import binary
    import digital

    require(
        version("QuantLib") == ql.__version__ == "1.43",
        "run with the frozen uv environment",
    )
    scenarios = load_scenarios(scenarios_path)
    profiles = numerical_profiles(profiles_path)
    generated = [
        serialize_row(reference_row(item))
        for item in sorted(scenarios, key=lambda item: item["case_id"])
    ]
    generated += [
        numerical_row(item, profile)
        for item in scenarios
        for profile in profiles
        if (
            date.fromisoformat(item["inputs"]["expiry"])
            - date.fromisoformat(item["inputs"]["valuation"])
        ).days
        > 2
    ]
    generated += list(american.rows())
    generated += list(digital.rows())
    generated += list(barrier.rows())
    generated += list(binary.rows())
    generated += list(asian.rows())
    generated.sort(key=lambda row: row.split("\t")[0])
    content = (
        "\n".join(
            [
                "# Language-neutral reference manifest. Generated by tools/quantlib-oracle/generate.py using QuantLib.",
                "# inputs/outputs/tolerances use key=value; convergence uses parameter|r1,r2|reference|tolerance;",
                '# Monte Carlo uses seed|paths|steps|tolerance. A dash means "not applicable".',
                "\t".join(HEADER),
                *generated,
            ]
        )
        + "\n"
    )
    validate_manifest(content)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=fixture.parent, delete=False) as output:
            temporary = Path(output.name)
            output.write(content.encode("utf-8"))
        os.replace(temporary, fixture)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(f"Generated {len(generated)} QuantLib price and Greek references")


if __name__ == "__main__":
    regenerate()
