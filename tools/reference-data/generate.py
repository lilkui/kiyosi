"""Regenerate only QuantLib-owned rows; never consume Kiyosi prices."""

import json
import math
import os
from datetime import date
from importlib.metadata import version
from pathlib import Path
import re
import tempfile

import QuantLib as ql

PROJECT = Path(__file__).resolve().parent
FIXTURE = PROJECT.parents[1] / "tests/fixtures/pricing_reference.tsv"
HEADER = "case_id instrument engine variant inputs outputs tolerances validation convergence monte_carlo".split()
INPUTS = {"option", "strike", "effective", "expiry", "valuation", "spot", "rate", "dividend", "volatility"}
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
    return ";".join(f"{key}={format(value, '.17g') if isinstance(value, float) else value}"
                    for key, value in sorted(values.items()))


def validate_scenario(scenario):
    require(set(scenario) == {"case_id", "inputs", "tolerances"}, "incomplete scenario")
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
    require(set(scenario["tolerances"]) == {"price"}, "price tolerance required")
    tolerance = scenario["tolerances"]["price"]
    require(type(tolerance) in (int, float) and math.isfinite(tolerance) and tolerance >= 0,
            "invalid price tolerance")


def european_row(scenario):
    inputs = scenario["inputs"]
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
    option = ql.VanillaOption(ql.PlainVanillaPayoff(
        ql.Option.Call if inputs["option"] == "call" else ql.Option.Put, inputs["strike"]),
        ql.EuropeanExercise(expiry))
    option.setPricingEngine(ql.AnalyticEuropeanEngine(process))
    price = option.NPV()
    require(math.isfinite(price) and price >= 0, "invalid QuantLib price")
    provenance = dict(inputs, owner="QuantLib", source_revision=f"QuantLib-{ql.__version__}",
                      source_symbol="QuantLib.AnalyticEuropeanEngine", convention=CONVENTION,
                      reference_kind="analytic", reference_classification="independent-analytic",
                      quantlib_python=version("QuantLib-Python"), quantlib=version("QuantLib"),
                      numerical_settings="double precision,no grid",
                      reference_uncertainty="floating-point roundoff only, not convergence-estimated",
                      tolerance=scenario["tolerances"]["price"])
    return "\t".join([scenario["case_id"], "EuropeanOption", "AnalyticEuropeanEngine", inputs["option"],
                      encode(provenance), encode({"price": price}), encode(scenario["tolerances"]),
                      "-", "-", "-"])


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


def regenerate(fixture=FIXTURE, scenarios_path=PROJECT / "scenarios.json"):
    require(version("QuantLib-Python") == "1.18" and version("QuantLib") == ql.__version__ == "1.41",
            "run with the frozen uv environment")
    scenarios = json.loads(scenarios_path.read_text(encoding="utf-8"))
    require(isinstance(scenarios, list) and scenarios, "no scenarios")
    for scenario in scenarios:
        validate_scenario(scenario)
    require(len({item["case_id"] for item in scenarios}) == len(scenarios), "duplicate scenario identifier")
    original = fixture.read_text(encoding="utf-8")
    validate_manifest(original)
    retained = [line for line in original.splitlines() if not line.startswith("ql-")]
    generated = [european_row(item) for item in sorted(scenarios, key=lambda item: item["case_id"])]
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
    print(f"Generated {len(generated)} QuantLib European prices")


if __name__ == "__main__":
    regenerate()
