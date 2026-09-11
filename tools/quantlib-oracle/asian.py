"""Continuous Asian references; Levy prices and Greeks are approximate."""

import math
from datetime import date, timedelta
from types import SimpleNamespace

import generate as g

ql = g.ql
INSTRUMENTS = {
    "geometric": "GeometricAverageOption",
    "arithmetic": "ArithmeticAverageOption",
}
ENGINES = {
    "geometric": "GeometricAverageAsianEngine",
    "arithmetic": "ArithmeticAverageAsianEngine",
}
SOURCES = {
    "geometric": "QuantLib.AnalyticContinuousGeometricAveragePriceAsianEngine",
    "arithmetic": "QuantLib.ContinuousArithmeticAsianLevyEngine",
}
STABILITY = dict(g.STABILITY, gamma=1e-6, speed=1e-6, vanna=1e-6, zomma=1e-6)
BUDGET = {
    "price": 1e-9,
    "delta": 1e-6,
    "gamma": 1e-6,
    "speed": 1e-6,
    "theta": 1e-4,
    "charm": 1e-5,
    "color": 2e-6,
    "vega": 1e-6,
    "vanna": 1e-6,
    "zomma": 1e-6,
    "rho": 1e-6,
}
START_REASON = "averaging-start boundary: price only, no smooth time stencil"
EXPIRY_REASON = "terminal average payoff: no smooth sensitivities"


def exclusions(inputs):
    if inputs["valuation"] == inputs["expiry"]:
        return dict.fromkeys(g.MEASURES[1:], EXPIRY_REASON)
    if (
        date.fromisoformat(inputs["valuation"])
        - date.fromisoformat(inputs["average_start"])
    ).days <= 2:
        return dict.fromkeys(g.MEASURES[1:], START_REASON)
    days = (
        date.fromisoformat(inputs["expiry"]) - date.fromisoformat(inputs["valuation"])
    ).days
    return dict.fromkeys(
        g.TIME_MEASURES if days <= 2 else (),
        "whole-day stability stencil touches expiry",
    )


def option(inputs):
    kind = inputs["averaging"]
    g.require(
        kind in INSTRUMENTS and inputs["option"] in {"call", "put"},
        "unknown Asian contract",
    )
    g.require(inputs["monitoring"] == "continuous", "unknown Asian monitoring")
    valuation, start, expiry = (
        ql.DateParser.parseISO(inputs[key])
        for key in ("valuation", "average_start", "expiry")
    )
    ql.Settings.instance().evaluationDate = valuation
    payoff = ql.PlainVanillaPayoff(
        ql.Option.Call if inputs["option"] == "call" else ql.Option.Put,
        inputs["strike"],
    )
    if valuation == expiry:
        # Instrument NPV is zero on expiry; evaluate its contractual payoff instead.
        return SimpleNamespace(NPV=lambda: payoff(inputs["realized_average"]))
    process = g.market_process(inputs)
    if kind == "geometric":
        g.require(
            start == valuation and inputs["realized_average"] == 0,
            "unmatched geometric averaging period",
        )
        contract = ql.ContinuousAveragingAsianOption(
            ql.Average.Geometric, payoff, ql.EuropeanExercise(expiry)
        )
        engine = ql.AnalyticContinuousGeometricAveragePriceAsianEngine(process)
    else:
        contract = ql.ContinuousAveragingAsianOption(
            ql.Average.Arithmetic, start, payoff, ql.EuropeanExercise(expiry)
        )
        engine = ql.ContinuousArithmeticAsianLevyEngine(
            process, ql.QuoteHandle(ql.SimpleQuote(inputs["realized_average"]))
        )
    contract.setPricingEngine(engine)
    return contract


def scenarios():
    for kind in INSTRUMENTS:
        for direction in ("call", "put"):
            for spot, days in ((80, 90), (100, 365), (120, 730)):
                for elapsed in (0,) if kind == "geometric" else (0, 90):
                    inputs = {
                        "option": direction,
                        "strike": 100,
                        "spot": spot,
                        "rate": 0.04,
                        "dividend": 0.01,
                        "volatility": 0.3,
                        "effective": "2024-09-01",
                        "valuation": "2025-01-06",
                        "expiry": (date(2025, 1, 6) + timedelta(days=days)).isoformat(),
                        "average_start": (
                            date(2025, 1, 6) - timedelta(days=elapsed)
                        ).isoformat(),
                        "realized_average": 101 if elapsed else 0,
                        "averaging": kind,
                        "monitoring": "continuous",
                        "calendar": "null",
                    }
                    yield (
                        f"ql-asian-{kind}-{direction}-{spot}-{days}d-{elapsed}elapsed",
                        inputs,
                    )
            if kind == "arithmetic":
                for average in (90, 100, 110):
                    inputs = {
                        "option": direction,
                        "strike": 100,
                        "spot": 100,
                        "rate": 0.04,
                        "dividend": 0.01,
                        "volatility": 0.3,
                        "effective": "2024-09-01",
                        "valuation": "2026-01-06",
                        "expiry": "2026-01-06",
                        "average_start": "2025-01-06",
                        "realized_average": average,
                        "averaging": kind,
                        "monitoring": "continuous",
                        "calendar": "null",
                    }
                    yield f"ql-asian-{kind}-{direction}-expiry-{average}", inputs


def metadata(inputs):
    terminal = inputs["valuation"] == inputs["expiry"]
    approximate = inputs["averaging"] == "arithmetic" and not terminal
    return {
        "source_revision": f"QuantLib-{ql.__version__}",
        "source_symbol": "QuantLib.PlainVanillaPayoff"
        if terminal
        else SOURCES[inputs["averaging"]],
        "reference_kind": "approximate" if approximate else "analytic",
        "convention": g.CONVENTION,
        "reference_classification": "independent-Levy-lognormal-moment-approximation"
        if approximate
        else "independent-analytic",
        "measure_sources": "Levy price differences: sensitivities of approximation"
        if approximate
        else "QuantLib contractual price",
        "numerical_settings": "spot 0.02/0.04,volatility and rate 0.0001/0.0002,time 1/2 calendar days,contract dates and running average fixed",
        "tolerance_rationale": "same moment approximation: roundoff and stencil budgets only, no bound on true arithmetic value error",
        "averaging_semantics": "continuous time average over average_start to expiry,realized_average is elapsed-period average",
        "settlement": "expiry",
        "date_roll": "none",
    }


def rows():
    for identifier, inputs in scenarios():
        row = g.contract_row(identifier, inputs, BUDGET, STABILITY)
        row["instrument"], row["engine"] = (
            INSTRUMENTS[inputs["averaging"]],
            ENGINES[inputs["averaging"]],
        )
        terms = row["inputs"]
        terms.update(
            metadata(inputs),
            spot_shift=0.02,
            volatility_shift=0.0001,
            rate_shift=0.0001,
            time_shift_days=1,
            wrapper=str(not exclusions(inputs)).lower(),
        )
        yield g.serialize_row(row)


def check_bindings():
    valuation, expiry = ql.Date(6, 1, 2025), ql.Date(6, 1, 2026)
    ql.Settings.instance().evaluationDate = valuation
    curve = lambda rate: ql.YieldTermStructureHandle(
        ql.FlatForward(valuation, rate, ql.Actual365Fixed())
    )
    process = ql.BlackScholesMertonProcess(
        ql.QuoteHandle(ql.SimpleQuote(100)),
        curve(0.01),
        curve(0.04),
        ql.BlackVolTermStructureHandle(
            ql.BlackConstantVol(valuation, ql.NullCalendar(), 0.3, ql.Actual365Fixed())
        ),
    )
    payoff = ql.PlainVanillaPayoff(ql.Option.Call, 100)
    exercise = ql.EuropeanExercise(expiry)
    for kind in INSTRUMENTS:
        engine = (
            ql.AnalyticContinuousGeometricAveragePriceAsianEngine(process)
            if kind == "geometric"
            else ql.ContinuousArithmeticAsianLevyEngine(
                process, ql.QuoteHandle(ql.SimpleQuote(101))
            )
        )
        for offset in (-90, 0, 90):
            contract = ql.ContinuousAveragingAsianOption(
                ql.Average.Geometric if kind == "geometric" else ql.Average.Arithmetic,
                valuation + offset,
                payoff,
                exercise,
            )
            contract.setPricingEngine(engine)
            try:
                value = contract.NPV()
            except RuntimeError as error:
                assert (
                    "seasoned continuous geometric Asian options not yet supported"
                    in str(error)
                    if kind == "geometric"
                    else offset > 0
                    and "start date must be earlier than or equal to reference date"
                    in str(error)
                ), str(error)
            else:
                assert kind == "arithmetic" and offset <= 0 and math.isfinite(value)
                for name in ("delta", "gamma", "theta", "vega", "rho"):
                    try:
                        getattr(contract, name)()
                    except RuntimeError as error:
                        assert "not provided" in str(error)
                    else:
                        raise AssertionError("unexpected native Levy Greek")
    for identifier, inputs in scenarios():
        assert math.isfinite(option(inputs).NPV()), identifier
    geometric = next(inputs for _, inputs in scenarios())
    for name in ("delta", "gamma", "vega", "rho"):
        assert (
            abs(
                g.measure(geometric, name) - g.measure(geometric, name, price_only=True)
            )
            <= STABILITY[name]
        )
    # A native geometric theta rolls the averaging period: it is not a fixed-start theta.
    assert math.isfinite(option(geometric).theta())
