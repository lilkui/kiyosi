"""Binary barrier references from exact QuantLib contract portfolios."""

import math
from datetime import date, timedelta
from functools import lru_cache
from types import SimpleNamespace

import barrier
import generate as g

ql = g.ql
SOURCE = "QuantLib.AnalyticBinaryBarrierEngine+QuantLib.AnalyticDigitalAmericanEngine+QuantLib.AnalyticEuropeanEngine"
STABILITY = dict(barrier.STABILITY)
BUDGET = dict(barrier.ANALYTIC)
BOUNDARY_REASON = "spot equals barrier: hit-state boundary"
EXPIRY_REASON = "terminal payoff: no smooth sensitivities"


def exclusions(inputs):
    if inputs["valuation"] == inputs["expiry"]:
        return dict.fromkeys(g.MEASURES[1:], EXPIRY_REASON)
    if float(inputs["spot"]) == float(inputs["barrier"]):
        return dict.fromkeys(g.MEASURES[1:], BOUNDARY_REASON)
    days = (
        date.fromisoformat(inputs["expiry"]) - date.fromisoformat(inputs["valuation"])
    ).days
    return dict.fromkeys(
        g.TIME_MEASURES if days <= 2 else (),
        "whole-day stability stencil touches expiry",
    )


def option(inputs):
    contract = make_contract(inputs)
    return SimpleNamespace(
        NPV=lambda: price(tuple(sorted(inputs.items()))),
        **{
            name: getattr(contract, name)
            for name in ("delta", "gamma", "theta", "vega", "rho")
            if hasattr(contract, name)
        },
    )


@lru_cache(maxsize=32768)
def price(items):
    return make_contract(dict(items)).NPV()


def make_contract(inputs):
    kind, direction = inputs["barrier_kind"], inputs["option"]
    g.require(
        kind in barrier.KINDS and direction in {"call", "put", "none"},
        "unknown binary contract",
    )
    g.require(
        inputs["monitoring"] == "continuous",
        "scheduled binary monitoring has no matching reference",
    )
    g.require(inputs["asset_settlement"] in {"true", "false"}, "unknown binary payoff")
    g.require(
        inputs["settlement"] in {"at_hit", "at_expiry"}, "unknown binary settlement"
    )
    asset = inputs["asset_settlement"] == "true"
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
    valuation, expiry = (
        ql.DateParser.parseISO(inputs[key]) for key in ("valuation", "expiry")
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

    if valuation == expiry:
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
        return cash(inputs["barrier"] if asset else inputs["payout"])
    if at_hit:
        # A continuous asset touch transfers H units of cash at the hitting time.
        payoff = ql.CashOrNothingPayoff(
            ql.Option.Call if up else ql.Option.Put, inputs["barrier"], inputs["payout"]
        )
        contract = ql.VanillaOption(
            payoff, ql.AmericanExercise(valuation, expiry, False)
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
            contract = ql.VanillaOption(payoff, ql.EuropeanExercise(expiry))
            engine = ql.AnalyticEuropeanEngine(process)
        else:
            contract = ql.BarrierOption(
                barrier.KINDS[kind],
                inputs["barrier"],
                0,
                payoff,
                ql.AmericanExercise(valuation, expiry, True),
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
            for name in ("NPV", "delta", "gamma", "theta", "vega", "rho")
        }
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
                            "effective": "2024-12-30",
                            "valuation": "2025-01-06",
                            "expiry": (
                                date(2025, 1, 6) + timedelta(days=days)
                            ).isoformat(),
                            "barrier_kind": kind,
                            "barrier": 100 if days == 0 and spot == strike else level,
                            "payout": level if asset and settlement == "at_hit" else 10,
                            "asset_settlement": str(asset).lower(),
                            "settlement": settlement,
                            "monitoring": "continuous",
                        }
                        yield (
                            f"ql-binary-{'asset' if asset else 'cash'}-{direction}-{kind.replace('_', '-')}-{settlement.replace('_', '-')}-{spot}-{strike}-{days}d",
                            inputs,
                        )


def metadata():
    return {
        "source_revision": f"QuantLib-{ql.__version__}",
        "source_symbol": SOURCE,
        "reference_kind": "analytic",
        "convention": g.CONVENTION,
        "decomposition": "expiry: binary American deferred,unconditional: call+put,hit: cash American digital (asset pays H),already-hit: European or immediate cash",
        "measure_sources": "native American digital or European Greeks where supplied,otherwise central QuantLib price differences",
        "numerical_settings": "central prices: spot 0.02/0.04,volatility and rate 0.0001/0.0002,time 1/2 calendar days",
        "tolerance_rationale": "analytic roundoff and finite-stencil truncation, see GENERATION.md",
    }


def rows():
    for identifier, inputs in scenarios():
        row = g.contract_row(identifier, inputs, BUDGET, STABILITY)
        row["instrument"], row["engine"] = (
            "BinaryBarrierOption",
            "AnalyticBinaryBarrierEngine",
        )
        terms = row["inputs"]
        terms.update(
            metadata(),
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
    for engine in (ql.AnalyticBinaryBarrierEngine, ql.AnalyticDigitalAmericanEngine):
        for payoff in (
            ql.CashOrNothingPayoff(ql.Option.Call, 130, 10),
            ql.AssetOrNothingPayoff(ql.Option.Call, 130),
        ):
            for exercise, error in (
                (ql.EuropeanExercise(expiry), "non-American exercise given"),
                (
                    ql.AmericanExercise(valuation, expiry, False),
                    "payoff must be at expiry"
                    if engine == ql.AnalyticBinaryBarrierEngine
                    else None,
                ),
                (ql.AmericanExercise(valuation, expiry, True), None),
                (
                    ql.AmericanExercise(valuation + 1, expiry, True),
                    "American option with window exercise not handled yet",
                ),
            ):
                contract = (
                    ql.BarrierOption(ql.Barrier.UpIn, 130, 0, payoff, exercise)
                    if engine == ql.AnalyticBinaryBarrierEngine
                    else ql.VanillaOption(payoff, exercise)
                )
                contract.setPricingEngine(engine(process))
                try:
                    result = contract.NPV()
                except RuntimeError as caught:
                    assert error is not None and error in str(caught), str(caught)
                else:
                    assert error is None and math.isfinite(result), (engine, error)
                    if engine == ql.AnalyticBinaryBarrierEngine:
                        for name in ("delta", "gamma", "theta", "vega", "rho"):
                            try:
                                getattr(contract, name)()
                            except RuntimeError as caught:
                                assert "not provided" in str(caught)
                            else:
                                raise AssertionError("unexpected native binary Greek")
    # Check the call+put portfolio against a separate one-touch representation.
    for side, level, kind in (
        (ql.Option.Call, 130, "up_and_in"),
        (ql.Option.Put, 70, "down_and_in"),
    ):
        for asset in (False, True):
            for deferred in (False, True):
                payout = level if asset else 10
                payoff = (
                    ql.AssetOrNothingPayoff(side, level)
                    if asset
                    else ql.CashOrNothingPayoff(side, level, payout)
                )
                contract = ql.VanillaOption(
                    payoff, ql.AmericanExercise(valuation, expiry, deferred)
                )
                contract.setPricingEngine(ql.AnalyticDigitalAmericanEngine(process))
                inputs = {
                    "option": "none",
                    "strike": 100,
                    "spot": 100,
                    "rate": 0.04,
                    "dividend": 0.01,
                    "volatility": 0.3,
                    "valuation": "2025-01-06",
                    "expiry": "2026-01-06",
                    "effective": "2024-12-30",
                    "barrier": level,
                    "barrier_kind": kind,
                    "payout": payout,
                    "asset_settlement": str(asset).lower(),
                    "monitoring": "continuous",
                    "settlement": "at_expiry" if deferred else "at_hit",
                }
                assert abs(g.measure(inputs, "price") - contract.NPV()) < 1e-11
                if not deferred:
                    for name, unit in (("delta", 1), ("gamma", 1), ("rho", 100)):
                        direct = getattr(contract, name)() / unit
                        assert abs(g.measure(inputs, name) - direct) < 1e-12
                        assert (
                            abs(g.measure(inputs, name, price_only=True) - direct)
                            < STABILITY[name]
                        )
                else:
                    # A strictly already-hit knock-in becomes European digital(s).
                    touched = dict(
                        inputs, spot=level + (10 if side == ql.Option.Call else -10)
                    )
                    for name, unit in (
                        ("delta", 1),
                        ("gamma", 1),
                        ("theta", 365),
                        ("vega", 100),
                        ("rho", 100),
                    ):
                        direct = (
                            sum(
                                getattr(
                                    g.vanilla_option(
                                        dict(
                                            touched,
                                            option=direction,
                                            payoff="asset" if asset else "cash",
                                        )
                                    ),
                                    name,
                                )()
                                for direction in ("call", "put")
                            )
                            / unit
                        )
                        assert abs(g.measure(touched, name) - direct) < 1e-12
                        assert (
                            abs(g.measure(touched, name, price_only=True) - direct)
                            < STABILITY[name]
                        )
    print("QuantLib binary barrier and American digital binding checks passed")
