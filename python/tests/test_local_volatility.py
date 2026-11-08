"""Local-volatility contracts and independent numerical references."""

import inspect
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import QuantLib as ql
from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    RateTermStructure,
)
from support.history import dated as dated_history
from support.terminal import assert_terminal_greeks
from test_support_matrix import HESTON_ENGINES, terminal_cases, terminal_delta

START, END = datetime(2025, 1, 1), datetime(2026, 1, 1)


def surface(values=((0.2, 0.2), (0.2, 0.2))):
    return LocalVolatilitySurface(times=[START, END], spots=[80, 120], values=values)


def context(surf=None, spot=100):
    return PricingContext(
        model_parameters=LocalVolatilityParameters(
            risk_free_rate=0.05, dividend_yield=0.02, surface=surf or surface()
        ),
        spot_price=spot,
        valuation_time=START,
    )


def option(direction="call", end=END):
    return EuropeanOption(
        option_type=direction, strike=100, effective_time=START, expiry_time=end
    )


def engine(n=800, t=800, upper=400):
    return pricing.FiniteDifferenceVanillaEngine(
        asset_step_count=n, time_step_count=t, asset_upper_boundary=upper
    )


def quantlib_skew(n, spot, direction):
    """Independent log-space mesher and PDE; never fit an implied-vol surface."""
    start, end = ql.Date(1, 1, 2025), ql.Date(1, 1, 2026)
    ql.Settings.instance().evaluationDate = start
    # QuantLib uses price rows and time columns, the transpose of our API.
    matrix = ql.Matrix(2, 2)
    matrix[0][0], matrix[1][0] = 0.3, 0.15
    matrix[0][1], matrix[1][1] = 0.35, 0.2
    local = ql.FixedLocalVolSurface(
        start, [0.0, 1.0], [80.0, 120.0], matrix, ql.Actual365Fixed()
    )
    risk = ql.YieldTermStructureHandle(ql.FlatForward(start, 0.05, ql.Actual365Fixed()))
    dividend = ql.YieldTermStructureHandle(
        ql.FlatForward(start, 0.02, ql.Actual365Fixed())
    )
    # This Black vol sizes the independent mesher, not the PDE's diffusion.
    black = ql.BlackVolTermStructureHandle(
        ql.BlackConstantVol(start, ql.NullCalendar(), 0.5, ql.Actual365Fixed())
    )
    process = ql.GeneralizedBlackScholesProcess(
        ql.QuoteHandle(ql.SimpleQuote(spot)),
        dividend,
        risk,
        black,
        ql.LocalVolTermStructureHandle(local),
    )
    payoff = ql.PlainVanillaPayoff(
        ql.Option.Call if direction == "call" else ql.Option.Put, 100
    )
    contract = ql.VanillaOption(payoff, ql.EuropeanExercise(end))
    contract.setPricingEngine(
        ql.FdBlackScholesVanillaEngine(
            process, n, n, 2, ql.FdmSchemeDesc.CrankNicolson(), True
        )
    )
    return contract.NPV(), contract.delta(), contract.gamma(), local


class LocalVolatilityTests(unittest.TestCase):
    def test_interpolation_preserves_extreme_positive_constants(self):
        for value in [
            math.ulp(0.0),
            float.fromhex("0x1p-1022"),
            float.fromhex("0x1.fffffffffffffp1023"),
        ]:
            s = surface(((value, value), (value, value)))
            for spot in [80, 90, 100, 110, 140]:
                self.assertEqual(s.volatility(START + timedelta(days=182), spot), value)

    def test_sigma_interpolation_time_boundaries_and_flat_price_tails(self):
        s = surface(((0.2, 0.4), (0.4, 0.8)))
        mid = START + timedelta(days=182)
        self.assertAlmostEqual(s.volatility(mid, 100), 0.3 + 0.3 * 182 / 365)
        self.assertEqual(s.volatility(START, 1), 0.2)
        self.assertEqual(s.volatility(END, 140), 0.8)
        self.assertEqual(s.volatility(END, 80), 0.4)
        self.assertEqual(s.maximum_volatility, 0.8)
        for time in [START - timedelta(days=1), END + timedelta(days=1)]:
            with self.assertRaises(InvalidInputError):
                s.volatility(time, 100)
        for spot in [0, -1, math.nan, math.inf]:
            with self.assertRaises(InvalidInputError):
                s.volatility(START, spot)

    def test_shape_node_value_and_native_conversion_errors(self):
        base = {"times": [START, END], "spots": [80, 120], "values": [[0.2, 0.2]] * 2}
        for change in [
            {"times": []},
            {"times": [START]},
            {"times": [END, START]},
            {"times": [START, START]},
            {"spots": []},
            {"spots": [80]},
            {"spots": [120, 80]},
            {"spots": [80, 80]},
            {"values": []},
            {"values": [[0.2]] * 2},
            {"values": [[0.2, 0.2]]},
            *[{"spots": [v, 120]} for v in [0, -1, math.nan, math.inf]],
            *[{"values": [[0.2, v]] * 2} for v in [0, -1, math.nan, math.inf]],
        ]:
            with self.subTest(change=change), self.assertRaises(InvalidInputError):
                LocalVolatilitySurface(**(base | change))
        for change in [
            {"spots": [True, 120]},
            {"values": [[0.2, False]] * 2},
            {"values": [0.2, 0.3]},
            {"times": [datetime(2025, 1, 1).date(), END]},
        ]:
            with self.subTest(change=change), self.assertRaises(TypeError):
                LocalVolatilitySurface(**(base | change))
        with self.assertRaises(TypeError):
            surface().volatility(START, True)
        with self.assertRaises(TypeError):
            LocalVolatilityParameters(
                risk_free_rate=True, dividend_yield=0, surface=surface()
            )
        with self.assertRaises(TypeError):
            PricingContext(
                model_parameters=object(), spot_price=100, valuation_time=START
            )

    def test_owned_inputs_properties_replacement_and_fixed_coordinates(self):
        rows = [[0.2, 0.4], [0.4, 0.8]]
        s = LocalVolatilitySurface(
            times=iter([START, END]), spots=iter([80, 120]), values=iter(rows)
        )
        rows[0][0] = 99
        copied = [list(row) for row in s.values]
        copied[0][0] = 98
        copied_spots = list(s.spots)
        copied_spots[0] = 1
        self.assertEqual(s.values, ((0.2, 0.4), (0.4, 0.8)))
        self.assertEqual(s.spots, (80, 120))
        self.assertEqual(s.times[0], datetime(2025, 1, 1))
        c = context(s)
        m = c.model_parameters
        self.assertIsInstance(m, LocalVolatilityParameters)
        moved = c.replace(spot_price=140, valuation_time=START + timedelta(days=182))
        self.assertEqual(moved.model_parameters.surface, s)
        self.assertEqual(c.spot_price, 100)
        replacement = m.replace(risk_free_rate=0.07, surface=surface())
        self.assertEqual(replacement.flat_risk_free_rate, 0.07)
        self.assertEqual(m.flat_risk_free_rate, 0.05)
        self.assertEqual(m.surface, s)
        for change in [{"risk_free_rate": math.nan}, {"dividend_yield": math.inf}]:
            with self.assertRaises(InvalidInputError):
                m.replace(**change)
            self.assertEqual(m.surface, s)
        with self.assertRaises(InvalidInputError):
            c.replace(valuation_time=END + timedelta(days=1))
        with self.assertRaises(AttributeError):
            s.spots = [1, 2]
        with self.assertRaises(TypeError):
            m.replace(surface=None)
        self.assertEqual(m.replace(surface=Ellipsis), m)

    def test_automatic_domains_ignore_surface_outside_remaining_horizon(self):
        e = pricing.FiniteDifferenceVanillaEngine()
        baseline = e.price_with_greeks(
            dated_history(option(), context()), context(), ["delta", "gamma"]
        )
        self.assertAlmostEqual(baseline.price, 9.227005508154036, delta=0.002)
        for past, future in [(1.5, 0.2), (0.2, 1.5), (1.5, 1.5)]:
            with self.subTest(past=past, future=future):
                extended = LocalVolatilitySurface(
                    times=[datetime(2024, 1, 1), START, END, datetime(2030, 1, 1)],
                    spots=[80, 120],
                    values=[[past] * 2, [0.2] * 2, [0.2] * 2, [future] * 2],
                )
                actual = e.price_with_greeks(
                    dated_history(option(), context(extended)),
                    context(extended),
                    ["delta", "gamma"],
                )
                for measure in ["price", "delta", "gamma"]:
                    self.assertAlmostEqual(
                        getattr(actual, measure),
                        getattr(baseline, measure),
                        delta=1e-12,
                    )

    def test_automatic_domains_preserve_interpolated_endpoint_volatility(self):
        extended = LocalVolatilitySurface(
            times=[START - timedelta(days=365), END + timedelta(days=365)],
            spots=[80, 120],
            values=[[0.2] * 2, [0.8] * 2],
        )
        clipped = surface(((0.4, 0.4), (0.6, 0.6)))
        e = pricing.FiniteDifferenceVanillaEngine()
        self.assertAlmostEqual(
            e.price(dated_history(option(), context(extended)), context(extended)),
            e.price(dated_history(option(), context(clipped)), context(clipped)),
            delta=1e-10,
        )

    def test_constant_surface_prices_and_greeks_match_bsm(self):
        bsm = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=START,
        )
        for direction in ["call", "put"]:
            actual = engine().price_with_greeks(
                dated_history(option(direction), context()),
                context(),
                ["delta", "gamma"],
            )
            expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
                dated_history(option(direction), bsm), bsm, ["delta", "gamma"]
            )
            for value, ref, tolerance in [
                (actual.price, expected.price, 0.002),
                (actual.delta, expected.delta, 0.0002),
                (actual.gamma, expected.gamma, 0.00003),
            ]:
                self.assertAlmostEqual(value, ref, delta=tolerance)

    def test_time_variation_matches_independent_integrated_variance(self):
        mid = START + timedelta(days=182)
        sigmas = [0.15, 0.35, 0.25]
        variance = (
            sum(
                days * (a * a + a * b + b * b) / 3
                for a, b, days in [(0.15, 0.35, 182), (0.35, 0.25, 183)]
            )
            / 365
        )
        s = LocalVolatilitySurface(
            times=[START, mid, END], spots=[80, 120], values=[[v, v] for v in sigmas]
        )
        bsm = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=math.sqrt(variance)
            ),
            spot_price=100,
            valuation_time=START,
        )
        for direction in ["call", "put"]:
            self.assertAlmostEqual(
                engine().price(
                    dated_history(option(direction), context(s)), context(s)
                ),
                pricing.AnalyticEuropeanEngine().price(
                    dated_history(option(direction), bsm), bsm
                ),
                delta=0.002,
            )
        remaining_sigma = math.sqrt((0.35**2 + 0.35 * 0.25 + 0.25**2) / 3)
        rolled = context(s).replace(valuation_time=mid)
        reference = bsm.replace(
            valuation_time=mid,
            model_parameters=bsm.model_parameters.replace(volatility=remaining_sigma),
        )
        self.assertAlmostEqual(
            engine().price(dated_history(option(), rolled), rolled),
            pricing.AnalyticEuropeanEngine().price(
                dated_history(option(), reference), reference
            ),
            delta=0.002,
        )

    def test_skew_refines_time_space_domain_and_fixed_surface_greeks(self):
        c = context(surface(((0.3, 0.15), (0.35, 0.2))))
        fine = engine(1600, 1600).price_with_greeks(
            dated_history(option(), c), c, ["delta", "gamma"]
        )
        for e, budget in [
            (engine(800, 800), 0.002),
            (engine(1600, 3200), 0.0001),
            (engine(2400, 1600, 600), 0.0001),
            (engine(1600, 1600, None), 0.002),
        ]:
            self.assertAlmostEqual(
                e.price(dated_history(option(), c), c), fine.price, delta=budget
            )
        up = engine(1600, 1600).price(
            dated_history(option(), c.replace(spot_price=100.1)),
            c.replace(spot_price=100.1),
        )
        down = engine(1600, 1600).price(
            dated_history(option(), c.replace(spot_price=99.9)),
            c.replace(spot_price=99.9),
        )
        self.assertAlmostEqual(fine.delta, (up - down) / 0.2, delta=0.00001)
        self.assertAlmostEqual(
            fine.gamma, (up + down - 2 * fine.price) / 0.01, delta=0.00001
        )

    def test_short_maturity_matches_bsm(self):
        o = option(end=START + timedelta(days=1))
        bsm = context().replace(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            )
        )
        actual = engine(12800, 1600).price_with_greeks(
            dated_history(o, context()), context(), ["delta", "gamma"]
        )
        expected = pricing.AnalyticEuropeanEngine().price_with_greeks(
            dated_history(o, bsm), bsm, ["delta", "gamma"]
        )
        for value, ref, budget in [
            (actual.price, expected.price, 0.001),
            (actual.delta, expected.delta, 0.0001),
            (actual.gamma, expected.gamma, 0.0003),
        ]:
            self.assertAlmostEqual(value, ref, delta=budget)

    def test_existing_schemes_and_explicit_stability(self):
        for scheme in [
            "explicit_euler",
            "implicit_euler",
            "crank_nicolson",
            "rannacher",
        ]:
            e = pricing.FiniteDifferenceVanillaEngine(
                asset_step_count=200,
                time_step_count=4000,
                scheme=scheme,
                asset_upper_boundary=400,
            )
            self.assertAlmostEqual(
                e.price(dated_history(option(), context()), context()),
                9.227005508154036,
                delta=0.015,
            )
        with self.assertRaises(InvalidInputError):
            pricing.FiniteDifferenceVanillaEngine(
                scheme="explicit_euler", time_step_count=1
            ).price(dated_history(option(), context()), context())

    def test_skew_prices_and_greeks_match_converged_quantlib_reference(self):
        previous_date = ql.Settings.instance().evaluationDate
        try:
            for direction in ["call", "put"]:
                for spot in [70.0, 100.0, 140.0]:
                    with self.subTest(direction=direction, spot=spot):
                        coarse = quantlib_skew(1600, spot, direction)
                        fine = quantlib_skew(3200, spot, direction)
                        for a, b, budget in zip(
                            coarse[:3],
                            fine[:3],
                            [0.0002, 0.00001, 0.000001],
                            strict=True,
                        ):
                            self.assertAlmostEqual(a, b, delta=budget)
                        s = surface(((0.3, 0.15), (0.35, 0.2)))
                        # Check oracle interpolation and both price tails away from time knots.
                        for price in [70, 80, 100, 120, 140]:
                            self.assertAlmostEqual(
                                s.volatility(START + timedelta(days=182), price),
                                fine[3].localVol(182 / 365, price, True),
                            )
                        result = engine(1600, 1600).price_with_greeks(
                            dated_history(option(direction), context(s, spot)),
                            context(s, spot),
                            ["delta", "gamma"],
                        )
                        for value, ref, budget in zip(
                            [result.price, result.delta, result.gamma],
                            fine[:3],
                            [0.002, 0.0002, 0.00003],
                            strict=True,
                        ):
                            self.assertAlmostEqual(value, ref, delta=budget)
        finally:
            ql.Settings.instance().evaluationDate = previous_date

    def test_product_engine_matrix_including_terminal_shortcuts(self):
        for contract, names, terminal_price in terminal_cases():
            for name in names:
                for time in [START, END]:
                    with self.subTest(
                        product=type(contract).__name__, engine=name, time=time
                    ):
                        e = getattr(pricing, name)()
                        c = context().replace(valuation_time=time, spot_price=105)
                        if (
                            not name.startswith("FiniteDifference")
                            or name in HESTON_ENGINES
                        ):
                            with self.assertRaises(UnsupportedOperationError):
                                e.price(dated_history(contract, c), c)
                            continue
                        bsm = c.replace(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
                            )
                        )
                        result = e.price_with_greeks(
                            dated_history(contract, c), c, ["delta", "gamma"]
                        )
                        expected = e.price_with_greeks(
                            dated_history(contract, bsm), bsm, ["delta", "gamma"]
                        )
                        self.assertAlmostEqual(
                            result.price, expected.price, delta=0.002
                        )
                        for greek in ["delta", "gamma"]:
                            a, b = getattr(result, greek), getattr(expected, greek)
                            if b is None:
                                self.assertIsNone(a)
                            else:
                                self.assertAlmostEqual(a, b, delta=0.0002)
                        if time == END:
                            self.assertAlmostEqual(
                                result.price, terminal_price, delta=1e-10
                            )
                            self.assertAlmostEqual(
                                result.require("delta"),
                                terminal_delta(contract),
                                delta=1e-10,
                            )
                            self.assertEqual(result.require("gamma"), 0)
                        for request in [
                            "all",
                            ["delta", "vega"],
                            "theta",
                        ]:
                            result = e.price_with_greeks(
                                dated_history(contract, c), c, request
                            )
                            self.assertIsNone(result.vega)

    def test_unsupported_greeks_solvers_expiry_and_coverage(self):
        c = context()
        for time in [START, END]:
            rolled = c.replace(valuation_time=time)
            baseline = engine().price_with_greeks(
                dated_history(option(), rolled), rolled, ["delta", "gamma"]
            )
            for greeks in ["all", ["delta", "vega"], "theta"]:
                result = engine().price_with_greeks(
                    dated_history(option(), rolled), rolled, greeks
                )
                self.assertEqual(result.price, baseline.price)
                self.assertIsNone(result.vega)
                self.assertIsNone(result.theta)
                self.assertIsNone(result.speed)
                if greeks != "theta":
                    self.assertEqual(result.delta, baseline.delta)
                else:
                    self.assertIsNone(result.delta)
            with self.assertRaises(UnsupportedOperationError):
                pricing.implied_volatility(
                    engine(),
                    dated_history(option(), rolled),
                    rolled,
                    10,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )
        terminal = engine().price_with_greeks(
            dated_history(option(), c.replace(valuation_time=END, spot_price=110)),
            c.replace(valuation_time=END, spot_price=110),
            ["delta", "gamma"],
        )
        self.assertEqual(terminal.price, 10)
        assert_terminal_greeks(self, terminal, 1)
        with self.assertRaises(InvalidInputError):
            engine().price(dated_history(option(end=END + timedelta(days=1)), c), c)
        dated = RateTermStructure(
            reference_time=START, times=[START + timedelta(days=182)], factors=[0.98]
        )
        short = c.replace(
            model_parameters=c.model_parameters.replace(risk_free_rate=dated)
        )
        with self.assertRaises(InvalidInputError):
            engine().price(dated_history(option(), short), short)

    def test_nonflat_rates_and_shared_concurrent_pricing(self):
        risk = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.05)]
        )
        dividend = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.02)]
        )
        c = context().replace(
            model_parameters=LocalVolatilityParameters(
                risk_free_rate=risk, dividend_yield=dividend, surface=surface()
            )
        )
        self.assertEqual(c.model_parameters.risk_free_curve, risk)
        self.assertEqual(c.model_parameters.dividend_curve, dividend)
        e = engine(200, 200)
        expected = e.price(dated_history(option(), c), c)
        self.assertAlmostEqual(expected, 9.227005508154036, delta=0.02)
        with ThreadPoolExecutor(max_workers=4) as pool:
            self.assertEqual(
                list(
                    pool.map(lambda _: e.price(dated_history(option(), c), c), range(8))
                ),
                [expected] * 8,
            )

    def test_signatures_and_example_are_discoverable(self):
        for cls in [LocalVolatilitySurface, LocalVolatilityParameters]:
            for parameter in inspect.signature(cls).parameters.values():
                self.assertEqual(parameter.kind, inspect.Parameter.KEYWORD_ONLY)
        self.assertEqual(
            list(inspect.signature(LocalVolatilitySurface).parameters),
            ["times", "spots", "values"],
        )


if __name__ == "__main__":
    unittest.main()
