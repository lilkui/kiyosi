"""Independent local-volatility finite-difference references and discretization checks."""

import itertools
import unittest
from datetime import date
from functools import cache

import QuantLib as ql
from kiyosi import pricing
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    BarrierOption,
    CashOrNothingOption,
    asset_binary_barrier_option,
    cash_binary_barrier_option,
)
from kiyosi.market import (
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
)

START, END = date(2025, 1, 1), date(2026, 1, 1)


def market(spot=100, dividend=0.02):
    return PricingContext(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05,
            dividend_yield=dividend,
            surface=LocalVolatilitySurface(
                times=[START, END],
                spots=[80, 120],
                values=[[0.3, 0.15], [0.35, 0.2]],
            ),
        ),
        spot_price=spot,
        valuation_time=START,
    )


def instrument(kind, direction):
    terms = {
        "option_type": direction,
        "strike": 100,
        "effective_date": START,
        "expiry_date": END,
    }
    if kind == "american":
        return AmericanOption(**terms)
    if kind == "cash":
        return CashOrNothingOption(**terms, payout=10)
    if kind == "asset":
        return AssetOrNothingOption(**terms)
    factory = (
        cash_binary_barrier_option
        if kind == "binary"
        else asset_binary_barrier_option
        if kind == "asset_barrier"
        else BarrierOption
    )
    return factory(
        **terms,
        barrier_level=120 if direction == "call" else 80,
        barrier_type="up_and_out" if direction == "call" else "down_and_out",
        **({"payout": 10} if kind == "binary" else {}),
    )


def engine(kind, space=1600, time=1600, upper=400):
    factory = {
        "american": pricing.FiniteDifferenceVanillaEngine,
        "cash": pricing.FiniteDifferenceDigitalEngine,
        "asset": pricing.FiniteDifferenceDigitalEngine,
        "barrier": pricing.FiniteDifferenceBarrierEngine,
        "binary": pricing.FiniteDifferenceBinaryBarrierEngine,
        "asset_barrier": pricing.FiniteDifferenceBinaryBarrierEngine,
    }[kind]
    return factory(
        asset_step_count=space, time_step_count=time, asset_upper_boundary=upper
    )


def values(result):
    return result.price, result.delta, result.gamma


@cache
def reference(kind, direction, time, space, spot=100, dividend=0.02, mesher_vol=0.5):
    """Explicit local-volatility process; Black volatility configures only the independent mesh."""
    start, end = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
    ql.Settings.instance().evaluationDate = start
    dc = ql.Actual365Fixed()
    matrix = ql.Matrix(2, 2)
    matrix[0][0], matrix[1][0] = 0.3, 0.15
    matrix[0][1], matrix[1][1] = 0.35, 0.2
    local = ql.FixedLocalVolSurface(start, [0.0, 1.0], [80.0, 120.0], matrix, dc)
    process = ql.GeneralizedBlackScholesProcess(
        ql.QuoteHandle(ql.SimpleQuote(spot)),
        ql.YieldTermStructureHandle(ql.FlatForward(start, dividend, dc)),
        ql.YieldTermStructureHandle(ql.FlatForward(start, 0.05, dc)),
        ql.BlackVolTermStructureHandle(
            ql.BlackConstantVol(start, ql.NullCalendar(), mesher_vol, dc)
        ),
        ql.LocalVolTermStructureHandle(local),
    )
    option_type = ql.Option.Call if direction == "call" else ql.Option.Put
    payoff = (
        ql.CashOrNothingPayoff(option_type, 100, 10)
        if kind in ("cash", "binary")
        else ql.AssetOrNothingPayoff(option_type, 100)
        if kind in ("asset", "asset_barrier")
        else ql.PlainVanillaPayoff(option_type, 100)
    )
    if kind in ("barrier", "binary", "asset_barrier"):
        contract = ql.BarrierOption(
            ql.Barrier.UpOut if direction == "call" else ql.Barrier.DownOut,
            120 if direction == "call" else 80,
            0,
            payoff,
            ql.EuropeanExercise(end),
        )
        factory = ql.FdBlackScholesBarrierEngine
    else:
        contract = ql.VanillaOption(
            payoff,
            ql.AmericanExercise(start, end)
            if kind == "american"
            else ql.EuropeanExercise(end),
        )
        factory = ql.FdBlackScholesVanillaEngine
    contract.setPricingEngine(
        factory(process, time, space, 2, ql.FdmSchemeDesc.CrankNicolson(), True)
    )
    return contract.NPV(), contract.delta(), contract.gamma()


class LocalVolatilityFDReferenceTests(unittest.TestCase):
    def setUp(self):
        self.previous_date = ql.Settings.instance().evaluationDate

    def tearDown(self):
        ql.Settings.instance().evaluationDate = self.previous_date

    def assert_measures_close(self, actual, expected, budgets):
        for name, a, b, budget in zip(
            ("price", "delta", "gamma"), actual, expected, budgets, strict=True
        ):
            self.assertAlmostEqual(a, b, delta=budget, msg=name)

    def test_american_and_digital_match_refined_independent_local_volatility(self):
        for kind in ["american", "cash", "asset"]:
            for direction in ["call", "put"]:
                with self.subTest(kind=kind, direction=direction):
                    coarse = reference(kind, direction, 1600, 1600)
                    fine = reference(kind, direction, 3200, 3200)
                    # Measured reference changes: price <= 0.00070, delta <= 4.7e-6,
                    # gamma <= 1.1e-6; these budgets include American time error.
                    self.assert_measures_close(coarse, fine, [0.001, 1e-5, 2e-6])
                    actual = values(
                        engine(kind).price_with_greeks(
                            instrument(kind, direction), market(), ["delta", "gamma"]
                        )
                    )
                    self.assert_measures_close(actual, fine, [0.002, 0.0002, 0.00003])

    def test_single_barriers_match_time_extrapolated_quantlib_reference(self):
        for direction in ["call", "put"]:
            with self.subTest(direction=direction):
                refs = [
                    reference("barrier", direction, steps, 3200)
                    for steps in [3200, 6400, 12800]
                ]
                estimates = [
                    tuple(2 * b - a for a, b in zip(x, y, strict=True))
                    for x, y in itertools.pairwise(refs)
                ]
                # QuantLib's absorbing-boundary time error is first order here.
                self.assert_measures_close(*estimates, [1e-5, 3e-7, 2e-8])
                finer_space = reference("barrier", direction, 12800, 6400)
                self.assert_measures_close(refs[-1], finer_space, [0.0001, 1e-5, 1e-6])
                actual = values(
                    engine("barrier").price_with_greeks(
                        instrument("barrier", direction), market(), ["delta", "gamma"]
                    )
                )
                self.assert_measures_close(
                    actual, estimates[-1], [0.0005, 0.00003, 0.000003]
                )

    def test_skew_time_space_domain_refinement_and_fixed_surface_derivatives(self):
        for kind in ["american", "cash", "asset", "barrier"]:
            contract = instrument(kind, "put")
            central = engine(kind).price_with_greeks(
                contract, market(), ["delta", "gamma"]
            )
            for other in [
                engine(kind, time=800),
                engine(kind, space=800),
                engine(kind, space=2400, upper=600),
            ]:
                with self.subTest(kind=kind, grid=other):
                    self.assertAlmostEqual(
                        other.price(contract, market()), central.price, delta=0.005
                    )
            bumped = [
                engine(kind).price(contract, market(spot)) for spot in [100.1, 99.9]
            ]
            self.assertAlmostEqual(
                central.delta, (bumped[0] - bumped[1]) / 0.2, delta=0.0001
            )
            self.assertAlmostEqual(
                central.gamma,
                (bumped[0] + bumped[1] - 2 * central.price) / 0.01,
                delta=0.00003,
            )

    def test_binary_barriers_match_independent_time_refined_reference(self):
        for kind in ["binary", "asset_barrier"]:
            for direction in ["call", "put"]:
                with self.subTest(kind=kind, direction=direction):
                    refs = [
                        reference(kind, direction, steps, 3200)
                        for steps in [3200, 6400, 12800]
                    ]
                    estimates = [
                        tuple(2 * b - a for a, b in zip(x, y, strict=True))
                        for x, y in itertools.pairwise(refs)
                    ]
                    self.assert_measures_close(*estimates, [0.0002, 1e-5, 1e-6])
                    actual = values(
                        engine(kind).price_with_greeks(
                            instrument(kind, direction), market(), ["delta", "gamma"]
                        )
                    )
                    self.assert_measures_close(
                        actual, estimates[-1], [0.003, 0.0003, 0.00003]
                    )


if __name__ == "__main__":
    unittest.main()
