"""Continuous vanilla barriers: exact QuantLib analytic contract portfolios."""
from datetime import date, timedelta
from functools import lru_cache
import math
from types import SimpleNamespace

import generate as g

ql = g.ql
KINDS = dict(up_and_in=ql.Barrier.UpIn, up_and_out=ql.Barrier.UpOut,
             down_and_in=ql.Barrier.DownIn, down_and_out=ql.Barrier.DownOut)
ENGINES = ("AnalyticBarrierEngine", "FiniteDifferenceBarrierEngine")
STABILITY = dict(g.STABILITY, delta=1e-5, gamma=1e-6, speed=1e-6, color=3e-6, vanna=1e-6, zomma=1e-7)
ANALYTIC = dict(price=1e-8, delta=1e-6, gamma=1e-7, speed=2e-7, theta=1e-4,
                charm=1e-5, color=3e-6, vega=1e-6, vanna=1e-7, zomma=2e-8, rho=1e-6)
# Fixed-grid interpolation requires spot bumps spanning several cells.
FD = dict(price=0.05, delta=0.005, gamma=0.002, speed=0.0005, theta=0.001,
          charm=0.0002, color=0.00005, vega=0.005, vanna=0.001, zomma=0.0002, rho=0.005)
SOURCE = "QuantLib.AnalyticBarrierEngine+QuantLib.DiscountingBondEngine+QuantLib.AnalyticEuropeanEngine"


def option(inputs):
    # g.measure needs only NPV: barrier Greeks come from independent price stencils.
    return SimpleNamespace(NPV=lambda: price(tuple(sorted(inputs.items()))))


@lru_cache(maxsize=32768)
def price(items):
    inputs = dict(items)
    kind = inputs["barrier_kind"]
    g.require(kind in KINDS and inputs["settlement"] in {"at_hit", "at_expiry"}, "unknown barrier terms")
    g.require(inputs["monitoring"] == "continuous", "scheduled monitoring has no matching reference")
    knock_in = kind.endswith("_in")
    g.require(not knock_in or inputs["settlement"] == "at_expiry", "knock-in cannot rebate at hit")
    valuation = ql.DateParser.parseISO(inputs["valuation"])
    expiry = ql.DateParser.parseISO(inputs["expiry"])
    ql.Settings.instance().evaluationDate = valuation
    curve = lambda rate: ql.YieldTermStructureHandle(ql.FlatForward(valuation, rate, ql.Actual365Fixed()))
    rates = curve(inputs["rate"])
    process = ql.BlackScholesMertonProcess(ql.QuoteHandle(ql.SimpleQuote(inputs["spot"])),
        curve(inputs["dividend"]), rates, ql.BlackVolTermStructureHandle(
            ql.BlackConstantVol(valuation, ql.NullCalendar(), inputs["volatility"], ql.Actual365Fixed())))
    payoff = ql.PlainVanillaPayoff(ql.Option.Call if inputs["option"] == "call" else ql.Option.Put, inputs["strike"])
    exercise = ql.EuropeanExercise(expiry)
    def barrier_value(barrier_kind, rebate):
        contract = ql.BarrierOption(KINDS[barrier_kind], inputs["barrier"], rebate, payoff, exercise)
        contract.setPricingEngine(ql.AnalyticBarrierEngine(process))
        return contract.NPV()
    def cash():
        bond = ql.ZeroCouponBond(0, ql.NullCalendar(), inputs["rebate"], expiry, ql.Unadjusted, 100, valuation)
        bond.setPricingEngine(ql.DiscountingBondEngine(rates))
        return bond.NPV()
    hit = inputs["spot"] >= inputs["barrier"] if kind.startswith("up") else inputs["spot"] <= inputs["barrier"]
    if hit:
        if knock_in:
            return g.european_option(inputs).NPV()
        return ql.SimpleCashFlow(inputs["rebate"], valuation).amount() if inputs["settlement"] == "at_hit" else cash()
    if knock_in or inputs["settlement"] == "at_hit":
        return barrier_value(kind, inputs["rebate"])
    opposite = kind.replace("_out", "_in")
    return barrier_value(kind, 0) + cash() - (barrier_value(opposite, inputs["rebate"]) - barrier_value(opposite, 0))


def scenarios():
    for direction in ("call", "put"):
        for kind in KINDS:
            for settlement in (("at_expiry",) if kind.endswith("_in") else ("at_hit", "at_expiry")):
                # Compact paired matrix, plus a strictly already-hit state and expiry boundary.
                for spot, days in ((80, 30), (100, 365), (120, 730), (150 if kind.startswith("up") else 50, 365), (110, 1)):
                    inputs = dict(option=direction, strike=100, spot=spot, rate=0.04, dividend=0.01,
                        volatility=0.3, effective="2024-12-30", valuation="2025-01-06",
                        expiry=(date(2025, 1, 6) + timedelta(days=days)).isoformat(),
                        barrier_kind=kind, barrier=140 if kind.startswith("up") else 60,
                        rebate=10, settlement=settlement, monitoring="continuous")
                    yield f"ql-barrier-{direction}-{kind.replace('_', '-')}-{settlement.replace('_', '-')}-{spot}-{days}d", inputs


def metadata(inputs):
    return dict(source_revision=f"QuantLib-{ql.__version__}", source_symbol=SOURCE,
        reference_kind="analytic", convention=g.CONVENTION,
        decomposition="KI or KO-hit direct,KO-expiry=KO-zero+bond-(KI-rebate-KI-zero),already-hit=vanilla or cash",
        measure_sources="central differences of QuantLib portfolio prices, no native Greeks",
        tolerance_rationale="analytic roundoff or fixed-grid discretization and bump truncation, see GENERATION.md")


def rows():
    for identifier, inputs in scenarios():
        for engine in ENGINES:
            budget = ANALYTIC if engine == ENGINES[0] else FD
            fields = g.european_row(dict(case_id=identifier, inputs=inputs,
                tolerances=budget, numerical_tolerances=budget), STABILITY).split("\t")
            fields[0] += "-" + engine.lower()
            fields[1:3] = ["BarrierOption", engine]
            terms = g.attributes(fields[4])
            terms.update(metadata(inputs))
            terms["numerical_settings"] = "central prices: spot 0.03/0.06,volatility and rate 0.0001/0.0002,time 1/2 calendar days"
            terms.update(spot_shift=0.01 if engine == ENGINES[0] else 2,
                         volatility_shift=0.0001 if engine == ENGINES[0] else 0.002,
                         rate_shift=0.0001 if engine == ENGINES[0] else 0.001, time_shift_days=1)
            terms["wrapper"] = str(inputs["expiry"] != "2025-01-07" and
                (engine == ENGINES[0] or inputs["spot"] == 100)).lower()
            if engine == ENGINES[1]:
                terms.update(asset_steps=1600, time_steps=1600, upper_boundary=400, scheme="crank_nicolson")
            fields[4] = g.encode(terms)
            yield "\t".join(fields)


def migrate(line):
    fields = line.split("\t")
    if len(fields) != 10 or fields[0].startswith("ql-") or fields[1] != "BarrierOption" or fields[5] == "-":
        return line
    terms = g.attributes(fields[4])
    if terms.get("monitoring") == "scheduled":
        return line
    g.require(fields[2] in ENGINES, "unknown barrier engine")
    inputs = dict(option=terms["option"], strike=float(terms["strike"]), spot=100, rate=0.04,
        dividend=0.01, volatility=0.3, effective="2024-12-30", valuation="2025-01-06",
        expiry=terms["expiry"], barrier=float(terms["barrier"]), rebate=float(terms["rebate"]),
        barrier_kind=terms.get("barrier_kind", "down_and_in"), settlement=terms["settlement"], monitoring="continuous")
    value = g.measure(inputs, "price")
    terms.update(inputs)
    terms.update(metadata(inputs), reference_provider="QuantLib", reference_uncertainty=0)
    if fields[2] == ENGINES[1]:
        terms.update(asset_steps=1000, time_steps=1000, upper_boundary=0, scheme="crank_nicolson")
        if fields[0] == "barrier-fd":
            terms.update(asset_steps=1600, time_steps=1600, upper_boundary=400)
    fields[4], fields[5] = g.encode(terms), g.encode(dict(price=value))
    if fields[8] != "-":
        parts = fields[8].split("|")
        if fields[0] == "barrier-fd":
            parts[1] = "400,800,1600"
        parts[2] = format(value, ".17g")
        fields[8] = "|".join(parts)
    return "\t".join(fields)




def check_bindings():
    """Probe the pinned binding, including actual engine restrictions."""
    valuation, expiry = ql.Date(6, 1, 2025), ql.Date(6, 1, 2026)
    ql.Settings.instance().evaluationDate = valuation
    curve = lambda value: ql.YieldTermStructureHandle(ql.FlatForward(valuation, value, ql.Actual365Fixed()))
    process = ql.BlackScholesMertonProcess(ql.QuoteHandle(ql.SimpleQuote(100)), curve(0.01), curve(0.04),
        ql.BlackVolTermStructureHandle(ql.BlackConstantVol(valuation, ql.NullCalendar(), 0.3, ql.Actual365Fixed())))
    vanilla = ql.PlainVanillaPayoff(ql.Option.Call, 100)
    for payoff, exercise, error in (
        (ql.CashOrNothingPayoff(ql.Option.Call, 100, 10), ql.EuropeanExercise(expiry), "non-plain payoff given"),
        (vanilla, ql.AmericanExercise(valuation, expiry), "only european style option are supported")):
        contract = ql.BarrierOption(ql.Barrier.UpOut, 140, 10, payoff, exercise)
        contract.setPricingEngine(ql.AnalyticBarrierEngine(process))
        try:
            contract.NPV()
        except RuntimeError as caught:
            assert error in str(caught), str(caught)
        else:
            raise AssertionError("unexpected analytic barrier binding support")
    contract = ql.BarrierOption(ql.Barrier.UpOut, 140, 10, vanilla, ql.EuropeanExercise(expiry))
    contract.setPricingEngine(ql.AnalyticBarrierEngine(process))
    assert math.isfinite(contract.NPV())
    for name in ("delta", "gamma", "theta", "vega", "rho"):
        try:
            getattr(contract, name)()
        except RuntimeError as caught:
            assert "not provided" in str(caught)
        else:
            raise AssertionError("unexpected native barrier Greek")
    # Independently verify the rebate portfolio using a cash one-touch with deferred payment.
    for direction, level in ((ql.Option.Call, 140), (ql.Option.Put, 60)):
        for at_expiry in (False, True):
            touch = ql.VanillaOption(ql.CashOrNothingPayoff(direction, level, 10),
                ql.AmericanExercise(valuation, expiry, at_expiry))
            touch.setPricingEngine(ql.AnalyticDigitalAmericanEngine(process))
            inputs = dict(option="call", strike=100, spot=100, rate=0.04, dividend=0.01,
                volatility=0.3, effective="2024-12-30", valuation="2025-01-06", expiry="2026-01-06",
                barrier_kind="up_and_out" if direction == ql.Option.Call else "down_and_out",
                barrier=level, rebate=10, settlement="at_expiry" if at_expiry else "at_hit", monitoring="continuous")
            rebate = g.measure(inputs, "price") - g.measure(dict(inputs, rebate=0), "price")
            assert abs(rebate - touch.NPV()) < 1e-11
    print("QuantLib continuous barrier binding and rebate decomposition checks passed")
