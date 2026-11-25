"""Continuous vanilla barriers: exact QuantLib analytic contract portfolios."""

from functools import lru_cache

from . import oracle as g

ql = g.ql
KINDS = {
    "up_and_in": ql.Barrier.UpIn,
    "up_and_out": ql.Barrier.UpOut,
    "down_and_in": ql.Barrier.DownIn,
    "down_and_out": ql.Barrier.DownOut,
}
STABILITY = dict(
    g.STABILITY,
    delta=1e-5,
    gamma=1e-6,
    speed=1e-6,
    color=0.001095,
    vanna=0.0001,
    zomma=0.00001,
)
ANALYTIC = {
    "price": 1e-8,
    "delta": 1e-6,
    "gamma": 1e-7,
    "speed": 2e-7,
    "theta": 0.0365,
    "charm": 0.00365,
    "color": 0.001095,
    "vega": 0.0001,
    "vanna": 0.00001,
    "zomma": 0.000002,
    "rho": 0.0001,
}
# Fixed-grid interpolation requires spot bumps spanning several cells.
FD = {
    "price": 0.05,
    "delta": 0.005,
    "gamma": 0.002,
    "speed": 0.0005,
    "theta": 0.365,
    "charm": 0.073,
    "color": 0.01825,
    "vega": 0.5,
    "vanna": 0.1,
    "zomma": 0.02,
    "rho": 0.5,
}


@lru_cache(maxsize=32768)
def price(items):
    inputs = dict(items)
    kind = inputs["BarrierType"]
    g.require(
        kind in KINDS and inputs["settlement"] in {"at_hit", "at_expiry"},
        "unknown barrier terms",
    )
    g.require(
        inputs["monitoring"] == "continuous",
        "scheduled monitoring has no matching reference",
    )
    knock_in = kind.endswith("_in")
    g.require(
        not knock_in or inputs["settlement"] == "at_expiry",
        "knock-in cannot rebate at hit",
    )
    valuation = ql.DateParser.parseISO(inputs["valuation"])
    expiry_time = ql.DateParser.parseISO(inputs["expiry_time"])
    ql.Settings.instance().evaluationDate = valuation
    process = g.market_process(inputs)
    rates = process.riskFreeRate()
    payoff = ql.PlainVanillaPayoff(
        ql.Option.Call if inputs["option"] == "call" else ql.Option.Put,
        inputs["strike"],
    )
    exercise = ql.EuropeanExercise(expiry_time)

    def barrier_value(BarrierType, rebate):
        contract = ql.BarrierOption(
            KINDS[BarrierType], inputs["barrier"], rebate, payoff, exercise
        )
        contract.setPricingEngine(ql.AnalyticBarrierEngine(process))
        return contract.NPV()

    def cash():
        bond = ql.ZeroCouponBond(
            0,
            ql.WeekendsOnly(),
            inputs["rebate"],
            expiry_time,
            ql.Unadjusted,
            100,
            valuation,
        )
        bond.setPricingEngine(ql.DiscountingBondEngine(rates))
        return bond.NPV()

    hit = (
        inputs["spot"] >= inputs["barrier"]
        if kind.startswith("up")
        else inputs["spot"] <= inputs["barrier"]
    )
    if hit:
        if knock_in:
            return g.vanilla_option(inputs).NPV()
        return (
            ql.SimpleCashFlow(inputs["rebate"], valuation).amount()
            if inputs["settlement"] == "at_hit"
            else cash()
        )
    if knock_in or inputs["settlement"] == "at_hit":
        return barrier_value(kind, inputs["rebate"])
    opposite = kind.replace("_out", "_in")
    return (
        barrier_value(kind, 0)
        + cash()
        - (barrier_value(opposite, inputs["rebate"]) - barrier_value(opposite, 0))
    )
