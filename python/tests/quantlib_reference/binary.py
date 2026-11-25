"""Binary barrier references from exact QuantLib contract portfolios."""

from datetime import date, timedelta
from functools import lru_cache
from types import SimpleNamespace

from . import barrier
from . import oracle as g

ql = g.ql
STABILITY = dict(barrier.STABILITY)
BUDGET = dict(barrier.ANALYTIC)
BOUNDARY_REASON = "spot equals barrier: hit-state boundary"
EXPIRY_REASON = "terminal payoff: no smooth sensitivities"


def exclusions(inputs):
    if inputs["valuation"] == inputs["expiry_time"]:
        return dict.fromkeys(g.MEASURES[1:], EXPIRY_REASON)
    if float(inputs["spot"]) == float(inputs["barrier"]):
        return dict.fromkeys(g.MEASURES[1:], BOUNDARY_REASON)
    return g.exclusions(inputs)


def option(inputs):
    contract = make_contract(inputs)
    return SimpleNamespace(
        NPV=lambda: price(tuple(sorted(inputs.items()))),
        **{
            name: getattr(contract, name)
            for name in (
                "delta",
                "gamma",
                "theta",
                "vega",
                "rho",
            )
            if hasattr(contract, name)
        },
    )


@lru_cache(maxsize=32768)
def price(items):
    return make_contract(dict(items)).NPV()


def make_contract(inputs):
    kind, direction = inputs["BarrierType"], inputs["option"]
    g.require(
        kind in barrier.KINDS and direction in {"call", "put", "none"},
        "unknown binary contract",
    )
    g.require(
        inputs["monitoring"] == "continuous",
        "scheduled binary monitoring has no matching reference",
    )
    g.require(type(inputs["asset_settlement"]) is bool, "unknown binary payoff")
    g.require(
        inputs["settlement"] in {"at_hit", "at_expiry"}, "unknown binary settlement"
    )
    asset = inputs["asset_settlement"]
    knock_in, up = kind.endswith("_in"), kind.startswith("up")
    at_hit = inputs["settlement"] == "at_hit"
    g.require(
        not at_hit or knock_in and direction == "none",
        "hit payment requires a one-touch",
    )
    g.require(
        not asset or not at_hit or inputs["payout"] == inputs["barrier"],
        "asset hit payment must equal barrier",
    )
    valuation, expiry_time = (
        ql.DateParser.parseISO(inputs[key]) for key in ("valuation", "expiry_time")
    )
    ql.Settings.instance().evaluationDate = valuation
    hit = (
        inputs["spot"] >= inputs["barrier"]
        if up
        else inputs["spot"] <= inputs["barrier"]
    )

    def cash(amount):
        flow = ql.SimpleCashFlow(amount, valuation)
        return SimpleNamespace(NPV=flow.amount)

    if valuation == expiry_time:
        g.require(not at_hit, "terminal hit payment requires a hitting history")
        if knock_in != hit:
            return cash(0)
        if direction == "none":
            return cash(inputs["spot"] if asset else inputs["payout"])
        side = ql.Option.Call if direction == "call" else ql.Option.Put
        payoff = (
            ql.AssetOrNothingPayoff(side, inputs["strike"])
            if asset
            else ql.CashOrNothingPayoff(side, inputs["strike"], inputs["payout"])
        )
        return cash(payoff(inputs["spot"]))
    process = g.market_process(inputs)
    if hit and not knock_in:
        return cash(0)
    if hit and at_hit:
        return cash(inputs["spot"] if asset else inputs["payout"])
    if at_hit:
        # A continuous asset touch transfers H units of cash at the hitting time.
        payoff = ql.CashOrNothingPayoff(
            ql.Option.Call if up else ql.Option.Put, inputs["barrier"], inputs["payout"]
        )
        contract = ql.VanillaOption(
            payoff, ql.AmericanExercise(valuation, expiry_time, False)
        )
        contract.setPricingEngine(ql.AnalyticDigitalAmericanEngine(process))
        return contract

    def value(side):
        payoff = (
            ql.AssetOrNothingPayoff(side, inputs["strike"])
            if asset
            else ql.CashOrNothingPayoff(side, inputs["strike"], inputs["payout"])
        )
        if hit:
            contract = ql.VanillaOption(payoff, ql.EuropeanExercise(expiry_time))
            engine = ql.AnalyticEuropeanEngine(process)
        else:
            contract = ql.BarrierOption(
                barrier.KINDS[kind],
                inputs["barrier"],
                0,
                payoff,
                ql.AmericanExercise(valuation, expiry_time, True),
            )
            engine = ql.AnalyticBinaryBarrierEngine(process)
        contract.setPricingEngine(engine)
        return contract

    if direction != "none":
        return value(ql.Option.Call if direction == "call" else ql.Option.Put)
    components = (value(ql.Option.Call), value(ql.Option.Put))
    return SimpleNamespace(
        **{
            name: lambda name=name: sum(
                getattr(component, name)() for component in components
            )
            for name in (
                "NPV",
                "delta",
                "gamma",
                "theta",
                "vega",
                "rho",
            )
        }
    )


def measure(inputs, name, scale=1, price_only=False):
    return g.measure(
        inputs, name, scale, price_only, option_factory=option, spot_bump=0.02
    )


def scenarios():
    for asset in (False, True):
        for kind in barrier.KINDS:
            for direction in ("none", "call", "put"):
                settlements = (
                    ("at_expiry", "at_hit")
                    if direction == "none" and kind.endswith("_in")
                    else ("at_expiry",)
                )
                for settlement in settlements:
                    level = 130 if kind.startswith("up") else 70
                    # Exercise both strike/barrier orderings, at-strike, at-barrier and already-hit states.
                    points = [
                        (80, 100, 90),
                        (100, 100, 365),
                        (120, 100, 730),
                        (100, 150 if kind.startswith("up") else 50, 365),
                        (level, 100, 365),
                        (level + (10 if kind.startswith("up") else -10), 100, 365),
                        (110, 100, 1),
                    ]
                    if settlement == "at_expiry":
                        points += [(100, 100, 0), (100, 95, 0), (level, 100, 0)]
                    for spot, strike, days in points:
                        inputs = {
                            "option": direction,
                            "strike": strike,
                            "spot": spot,
                            "rate": 0.04,
                            "dividend": 0.01,
                            "volatility": 0.3,
                            "effective_time": "2024-12-30",
                            "valuation": "2025-01-06",
                            "expiry_time": (
                                date(2025, 1, 6) + timedelta(days=days)
                            ).isoformat(),
                            "BarrierType": kind,
                            "barrier": 100 if days == 0 and spot == strike else level,
                            "payout": level if asset and settlement == "at_hit" else 10,
                            "asset_settlement": asset,
                            "settlement": settlement,
                            "monitoring": "continuous",
                        }
                        yield (
                            f"ql-binary-{'asset' if asset else 'cash'}-{direction}-{kind.replace('_', '-')}-{settlement.replace('_', '-')}-{spot}-{strike}-{days}d",
                            inputs,
                        )
