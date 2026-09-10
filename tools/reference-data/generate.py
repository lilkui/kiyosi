"""Regenerate only QuantLib-owned rows; never consume Kiyosi prices."""

import json
import math
import os
from datetime import date, timedelta
from importlib.metadata import version
from pathlib import Path
import re
import tempfile

import QuantLib as ql

PROJECT = Path(__file__).resolve().parent
FIXTURE = PROJECT.parents[1] / "tests/fixtures/pricing_reference.tsv"
HEADER = "case_id instrument engine variant inputs outputs tolerances validation convergence monte_carlo".split()
INPUTS = {"option", "strike", "effective", "expiry", "valuation", "spot", "rate", "dividend", "volatility"}
MEASURES = "price delta gamma speed theta charm color vega vanna zomma rho".split()
TIME_MEASURES = {"theta", "charm", "color"}
UNITS = dict(price="price", delta="price/spot", gamma="price/spot^2", speed="price/spot^3",
             theta="price/day", charm="delta/day", color="gamma/day", vega="price/volatility-pp",
             vanna="delta/volatility-pp", zomma="gamma/volatility-pp", rho="price/rate-pp")
# Independent stability ceilings, not Kiyosi error allowances.
STABILITY = dict(price=1e-10, delta=1e-6, gamma=1e-7, speed=1e-7,
                 theta=1e-4, charm=1e-5, color=2e-6, vega=1e-6,
                 vanna=1e-7, zomma=1e-8, rho=1e-6)
CONVENTION = "Actual/365 Fixed, continuously compounded BSM"
NUMERICAL_ENGINES = {
    "BinomialEuropeanEngine": {"steps"}, "CrrEngine": {"steps"},
    "IntegralEuropeanEngine": set(),
    "FiniteDifferenceEuropeanEngine": {"asset_steps", "time_steps", "scheme", "upper_boundary"},
    "MonteCarloEuropeanEngine": {"seed", "paths", "steps"},
}
LEGACY_NUMERICAL = {
    "european-binomial": dict(steps=200), "crr-vanilla": dict(steps=200),
    "european-integral": {},
    "european-fd": dict(asset_steps=200, time_steps=4000, scheme="explicit_euler", upper_boundary=0),
    "european-mc": dict(seed=42, paths=20000, steps=252),
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
    return ";".join(f"{key}={format(value, '.17g') if isinstance(value, float) else value}"
                    for key, value in sorted(values.items()))


def validate_scenario(scenario):
    require(set(scenario) == {"case_id", "inputs", "tolerances", "numerical_tolerances"}, "incomplete scenario")
    require(re.fullmatch(r"ql-european-[a-z0-9-]+", scenario["case_id"]), "invalid case identifier")
    inputs = scenario["inputs"]
    require(set(inputs) == INPUTS, "incomplete or unknown inputs")
    require(inputs["option"] in ("call", "put"), "unknown option")
    for key in ("strike", "spot", "rate", "dividend", "volatility"):
        require(type(inputs[key]) in (int, float) and math.isfinite(inputs[key]), f"invalid {key}")
    require(all(inputs[key] > 0 for key in ("strike", "spot", "volatility")), "non-positive input")
    dates = {key: date.fromisoformat(inputs[key]) for key in ("effective", "valuation", "expiry")}
    require(all(dates[key].isoformat() == inputs[key] for key in dates), "dates must be YYYY-MM-DD")
    require(dates["effective"] <= dates["valuation"] < dates["expiry"], "invalid date ordering")
    for field in ("tolerances", "numerical_tolerances"):
        require(set(scenario[field]) == set(MEASURES), f"all measure {field} required")
        require(all(type(v) in (int, float) and math.isfinite(v) and v >= 0
                    for v in scenario[field].values()), f"invalid {field}")


def vanilla_option(inputs, american_grid=None):
    valuation = ql.DateParser.parseISO(inputs["valuation"])
    expiry = ql.DateParser.parseISO(inputs["expiry"])
    ql.Settings.instance().evaluationDate = valuation
    day_count = ql.Actual365Fixed()
    curve = lambda rate: ql.YieldTermStructureHandle(
        ql.FlatForward(valuation, rate, day_count, ql.Continuous, ql.Annual))
    process = ql.BlackScholesMertonProcess(
        ql.QuoteHandle(ql.SimpleQuote(inputs["spot"])), curve(inputs["dividend"]), curve(inputs["rate"]),
        ql.BlackVolTermStructureHandle(ql.BlackConstantVol(
            valuation, ql.NullCalendar(), inputs["volatility"], day_count)))
    direction = ql.Option.Call if inputs["option"] == "call" else ql.Option.Put
    kind = inputs.get("payoff", "vanilla")
    require(kind in {"vanilla", "cash", "asset"}, "unknown payoff")
    payoff = (ql.CashOrNothingPayoff(direction, inputs["strike"], inputs["payout"]) if kind == "cash" else
              ql.AssetOrNothingPayoff(direction, inputs["strike"]) if kind == "asset" else
              ql.PlainVanillaPayoff(direction, inputs["strike"]))
    option = ql.VanillaOption(payoff,
        ql.EuropeanExercise(expiry) if american_grid is None else
        ql.AmericanExercise(ql.DateParser.parseISO(inputs["effective"]), expiry))
    option.setPricingEngine(ql.AnalyticEuropeanEngine(process) if american_grid is None else
                           ql.FdBlackScholesVanillaEngine(process, *american_grid, 2,
                                                         ql.FdmSchemeDesc.Douglas()))
    return option


def european_option(inputs):
    return vanilla_option(inputs)


def shifted(inputs, field, bump):
    value = dict(inputs)
    value[field] = ((date.fromisoformat(inputs[field]) + timedelta(days=bump)).isoformat()
                    if field == "valuation" else inputs[field] + bump)
    return value


def measure(inputs, name, scale=1, price_only=False):
    if "barrier_kind" in inputs:
        import barrier
        option = barrier.option(inputs)
        price_only = True
    else:
        option = european_option(inputs)
    if name == "price":
        value = option.NPV()
    else:
        native = {"delta": 1, "gamma": 1, "theta": 365, "vega": 100, "rho": 100}
        if name in native and not price_only:
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
        if field == "spot" and (date.fromisoformat(inputs["expiry"]) -
                                date.fromisoformat(inputs["valuation"])).days > 2:
            h = 0.01
        if field == "spot" and "barrier_kind" in inputs:
            h = 0.03
        h *= scale
        if field == "valuation":
            require(date.fromisoformat(inputs["valuation"]) + timedelta(days=h) <
                    date.fromisoformat(inputs["expiry"]), "time stencil touches expiry")
            require(date.fromisoformat(inputs["valuation"]) - timedelta(days=h) >=
                    date.fromisoformat(inputs["effective"]), "time stencil precedes effective date")
        up = measure(shifted(inputs, field, h), base, scale, price_only)
        down = measure(shifted(inputs, field, -h), base, scale, price_only)
        value = ((up - 2 * option.NPV() + down) / h**2 if name == "gamma"
                 else (up - down) / (2 * h * unit))
    require(math.isfinite(value), f"non-finite {name}")
    return value


def european_row(scenario, stability=STABILITY):
    inputs = scenario["inputs"]
    outputs, metadata = {}, {}
    days = (date.fromisoformat(inputs["expiry"]) - date.fromisoformat(inputs["valuation"])).days
    for name in MEASURES:
        metadata[f"unit_{name}"] = UNITS[name]
        if name in TIME_MEASURES and days <= 2:
            metadata[f"unavailable_{name}"] = "whole-day stability stencil touches expiry"
            continue
        value = measure(inputs, name)
        # Force price fallback for first-order measures and gamma to validate both paths.
        fallback = name in {"delta", "gamma", "theta", "vega", "rho"}
        estimates = [measure(inputs, name, scale, fallback) for scale in (1, 2)]
        require(all(math.isfinite(v) for v in [value, *estimates]), f"non-finite {name}")
        uncertainty = max(abs(value - estimate) for estimate in estimates)
        require(uncertainty <= stability[name], f"{scenario['case_id']}: unstable {name}: {uncertainty}")
        outputs[name] = value
        metadata[f"uncertainty_{name}"] = uncertainty
        metadata[f"stability_limit_{name}"] = stability[name]
        metadata[f"numerical_tolerance_{name}"] = scenario["numerical_tolerances"][name]
    require(outputs["price"] >= 0, "invalid QuantLib price")
    provenance = dict(inputs, owner="QuantLib", source_revision=f"QuantLib-{ql.__version__}",
                      source_symbol="QuantLib.AnalyticEuropeanEngine", convention=CONVENTION,
                      reference_kind="analytic", reference_classification="independent-analytic",
                      quantlib_python=version("QuantLib-Python"), quantlib=version("QuantLib"),
                      numerical_settings="central differences: spot 0.01/0.02 (0.001/0.002 within 2 days of expiry),volatility and rate 0.0001/0.0002,time 1/2 calendar days",
                      measure_sources="price/delta/gamma/theta/vega/rho native with price fallback,others central delta/gamma differences with price fallback",
                      reference_uncertainty="per-measure maximum bump discrepancy, not a rigorous bound",
                      tolerance=scenario["tolerances"]["price"])
    provenance.update(metadata)
    return "\t".join([scenario["case_id"], "EuropeanOption", "AnalyticEuropeanEngine", inputs["option"],
                      encode(provenance), encode(outputs), encode({name: scenario["tolerances"][name] for name in outputs}),
                      "-", "-", "-"])


def numerical_profiles(path):
    profiles = json.loads(path.read_text(encoding="utf-8"))
    require(isinstance(profiles, list) and len(profiles) == len(NUMERICAL_ENGINES), "incomplete engine profiles")
    require({p["engine"] for p in profiles} == set(NUMERICAL_ENGINES), "unknown or duplicate engine")
    for profile in profiles:
        require(set(profile) == {"engine", "settings", "shifts", "tolerances", "numerical_tolerances"}, "invalid profile")
        settings = profile["settings"]
        require(set(settings) == NUMERICAL_ENGINES[profile["engine"]], "invalid engine settings")
        for name, value in settings.items():
            if name == "scheme":
                require(value in {"explicit_euler", "implicit_euler", "crank_nicolson"}, "unknown scheme")
            else:
                require(type(value) is int and value > 0, "invalid numerical setting")
        shifts = profile["shifts"]
        require(set(shifts) == {"spot_shift", "volatility_shift", "rate_shift", "time_shift_days"}, "invalid shifts")
        require(all(type(v) in (int, float) and math.isfinite(v) and v > 0 for v in shifts.values()), "invalid shift")
        require(type(shifts["time_shift_days"]) is int and shifts["time_shift_days"] <= 3, "invalid time shift")
        for key in ("tolerances", "numerical_tolerances"):
            require(set(profile[key]) == set(MEASURES), "incomplete numerical budgets")
            require(all(type(v) in (int, float) and math.isfinite(v) and v >= 0 for v in profile[key].values()), "invalid budget")
    return profiles


def numerical_row(scenario, profile):
    fields = european_row(dict(scenario, tolerances=profile["tolerances"],
                               numerical_tolerances=profile["numerical_tolerances"])).split("\t")
    fields[0] += "-" + profile["engine"].lower()
    fields[2] = profile["engine"]
    inputs = attributes(fields[4])
    inputs.update(profile["settings"])
    inputs.update(profile["shifts"])
    inputs["wrapper"] = str(scenario["inputs"]["spot"] == 100 and
                            scenario["inputs"]["expiry"] == "2026-01-06").lower()
    inputs["tolerance_rationale"] = "engine-specific discretization or sampling budget, see GENERATION.md"
    fields[4] = encode(inputs)
    if fields[2] == "MonteCarloEuropeanEngine":
        fields[9] = "|".join(str(inputs[key]) for key in ("seed", "paths", "steps", "tolerance"))
    return "\t".join(fields)


def migrate_numerical(line):
    fields = line.split("\t")
    if fields[0] not in LEGACY_NUMERICAL:
        return line
    inputs = attributes(fields[4])
    require(fields[1] == "EuropeanOption" and fields[2] in NUMERICAL_ENGINES, "invalid migration target")
    market = dict(option=inputs["option"], strike=100, spot=100, rate=0.04, dividend=0.01,
                  volatility=0.3, effective="2024-12-30", valuation="2025-01-06", expiry="2026-01-06")
    for key in INPUTS & inputs.keys():
        market[key] = float(inputs[key]) if key in {"strike", "spot", "rate", "dividend", "volatility"} else inputs[key]
    value = measure(market, "price")
    inputs.update(market)
    inputs.update(LEGACY_NUMERICAL[fields[0]])
    inputs.update(source_revision=f"QuantLib-{ql.__version__}", source_symbol="QuantLib.AnalyticEuropeanEngine",
                  reference_kind="analytic", reference_provider="QuantLib", reference_uncertainty=0,
                  tolerance_rationale="retained numerical comparison budget, see GENERATION.md")
    fields[4] = encode(inputs)
    fields[5] = encode(dict(price=value))
    if fields[8] != "-":
        convergence = fields[8].split("|")
        convergence[2] = format(value, ".17g")
        fields[8] = "|".join(convergence)
    return "\t".join(fields)


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
        require(all(math.isfinite(float(value)) for value in outputs.values()), "non-finite output")
        require(all(math.isfinite(float(value)) and float(value) >= 0 for value in tolerances.values()),
                "invalid output tolerance")
        for key in ("source_revision", "source_symbol", "convention", "reference_kind", "tolerance"):
            require(key in inputs, f"missing provenance {key}")
        require(math.isfinite(float(inputs["tolerance"])) and float(inputs["tolerance"]) >= 0,
                "invalid provenance tolerance")
        owned = inputs.get("owner") == "QuantLib"
        require(owned == fields[0].startswith("ql-"), "inconsistent QuantLib ownership")

        if owned:
            import american
            import digital
            import barrier
            is_barrier = fields[1] == "BarrierOption"
            is_american = fields[1] == "AmericanOption"
            is_digital = fields[1] in digital.INSTRUMENTS.values()
            require(is_barrier or is_american or is_digital or fields[1] == "EuropeanOption", "unknown generated instrument")
            limits = barrier.STABILITY if is_barrier else american.STABILITY if is_american else digital.STABILITY if is_digital else STABILITY
            days = (date.fromisoformat(inputs["expiry"]) - date.fromisoformat(inputs["valuation"])).days
            unavailable = american.exclusions(inputs) if is_american else dict.fromkeys(
                TIME_MEASURES if days <= 2 else set(), "whole-day stability stencil touches expiry")
            require(set(outputs) == set(MEASURES) - unavailable.keys(), "incomplete measures")
            require({key.removeprefix("unavailable_") for key in inputs if key.startswith("unavailable_")}
                    == unavailable.keys(), "invalid unavailable declarations")
            for name in MEASURES:
                require(inputs.get(f"unit_{name}") == UNITS[name], f"invalid {name} unit")
                if name in unavailable:
                    require(inputs[f"unavailable_{name}"] == unavailable[name],
                            "invalid unavailable reason")
                    continue
                uncertainty = float(inputs[f"uncertainty_{name}"])
                require(math.isfinite(uncertainty) and 0 <= uncertainty <= limits[name], "unstable output")
                require(float(inputs[f"stability_limit_{name}"]) == limits[name], "invalid stability limit")
                budget = float(inputs[f"numerical_tolerance_{name}"])
                require(math.isfinite(budget) and budget >= 0, "invalid numerical tolerance")


def regenerate(fixture=FIXTURE, scenarios_path=PROJECT / "scenarios.json",
               profiles_path=PROJECT / "numerical_engines.json"):
    import american
    import digital
    import barrier
    require(version("QuantLib-Python") == "1.18" and version("QuantLib") == ql.__version__ == "1.41",
            "run with the frozen uv environment")
    scenarios = json.loads(scenarios_path.read_text(encoding="utf-8"))
    require(isinstance(scenarios, list) and scenarios, "no scenarios")
    for scenario in scenarios:
        validate_scenario(scenario)
    require(len({item["case_id"] for item in scenarios}) == len(scenarios), "duplicate scenario identifier")
    original = fixture.read_text(encoding="utf-8")
    validate_manifest(original)
    profiles = numerical_profiles(profiles_path)
    retained = [barrier.migrate(digital.migrate(american.migrate(migrate_numerical(line)))) for line in original.splitlines() if not line.startswith("ql-")]
    generated = [european_row(item) for item in sorted(scenarios, key=lambda item: item["case_id"])]
    generated += [numerical_row(item, profile) for item in scenarios for profile in profiles
                  if (date.fromisoformat(item["inputs"]["expiry"]) - date.fromisoformat(item["inputs"]["valuation"])).days > 2]
    generated += list(american.rows())
    generated += list(digital.rows())
    generated += list(barrier.rows())
    generated.sort(key=lambda row: row.split("\t")[0])
    content = "\n".join(retained + generated) + "\n"
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
