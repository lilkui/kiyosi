"""Volga and parallel dividend-yield sensitivities through the public API."""

import math
import unittest
from datetime import date

import QuantLib as ql
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    FiniteDifferenceVanillaEngine,
    MonteCarloVanillaEngine,
)
from quantlib_reference import oracle
from test_binomial import END, START, market


class VolgaDividendRhoTests(unittest.TestCase):
    def test_units_selection_and_independent_quantlib_references(self):
        engine = AnalyticEuropeanEngine()
        saved = ql.SavedSettings()
        self.addCleanup(lambda: saved)
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction,
                strike=100,
                effective_midnight_utc=START,
                expiry_midnight_utc=END,
            )
            for spot in (80, 100, 120):
                for dividend in (-0.02, 0, 0.02):
                    context = market(spot=spot, rate=0.05, dividend=dividend, sigma=0.2)
                    inputs = {
                        "option": direction,
                        "spot": spot,
                        "strike": 100,
                        "effective_midnight_utc": START.isoformat(),
                        "expiry_midnight_utc": END.isoformat(),
                        "valuation": START.isoformat(),
                        "rate": 0.05,
                        "dividend": dividend,
                        "volatility": 0.2,
                    }
                    reference = oracle.vanilla_option(inputs)
                    expected_dividend = reference.dividendRho() / 100
                    # Differentiate independent QuantLib vega at two resolutions.
                    volga = []
                    for h in (0.0001, 0.00005):
                        up = oracle.vanilla_option({**inputs, "volatility": 0.2 + h})
                        down = oracle.vanilla_option({**inputs, "volatility": 0.2 - h})
                        volga.append((up.vega() - down.vega()) / (20000 * h))
                    self.assertAlmostEqual(volga[0], volga[1], delta=1e-8)
                    expected = {"volga": volga[1], "dividend_rho": expected_dividend}
                    full = engine.price_with_greeks(option, context, "all")
                    for name, value in expected.items():
                        with self.subTest(
                            direction=direction, spot=spot, dividend=dividend, name=name
                        ):
                            selected = engine.price_with_greeks(option, context, name)
                            self.assertAlmostEqual(
                                selected.require(name), value, delta=1e-8
                            )
                            self.assertEqual(selected.require(name), full.require(name))
                            self.assertIsNone(selected.delta)
                            self.assertEqual(selected.price, full.price)
                            other = (
                                "volga" if name == "dividend_rho" else "dividend_rho"
                            )
                            self.assertIsNone(getattr(selected, other))
                    if spot == 100 and dividend == 0.02:
                        self.assertAlmostEqual(
                            full.volga, 0.000236882234437609, delta=1e-14
                        )
                        expected_q = (
                            -0.586851146134764
                            if direction == "call"
                            else 0.393347527171254
                        )
                        self.assertAlmostEqual(
                            full.dividend_rho, expected_q, delta=1e-12
                        )

    def test_parallel_dividend_curves_numerical_bumps_and_availability(self):
        option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_midnight_utc=START,
            expiry_midnight_utc=END,
        )
        curve = RateTermStructure(
            reference_time=START,
            times=[date(2025, 7, 1), END],
            factors=[math.exp(-0.01 * 181 / 365), math.exp(-0.03)],
        )
        context = market().replace(
            model_parameters=market().model_parameters.replace(dividend_yield=curve)
        )
        engine = FiniteDifferenceVanillaEngine(
            asset_step_count=400, time_step_count=400
        )
        exact = AnalyticEuropeanEngine().price_with_greeks(option, context, "all")
        self.assertAlmostEqual(exact.dividend_rho, -exact.delta, delta=1e-12)
        for bump in (0.0001, 0.0002):
            actual = engine.price_with_greeks(
                option,
                context,
                ["volga", "dividend_rho"],
                volatility_shift=bump,
                rate_shift=bump,
            )
            self.assertAlmostEqual(actual.volga, exact.volga, delta=2e-5)
            self.assertAlmostEqual(
                actual.dividend_rho, exact.dividend_rho, delta=0.0002
            )
            shifted_prices = [
                engine.price(
                    option,
                    context.replace(
                        model_parameters=context.model_parameters.replace(
                            dividend_yield=curve.shifted(h)
                        )
                    ),
                )
                for h in (bump, -bump)
            ]
            self.assertAlmostEqual(
                actual.dividend_rho,
                (shifted_prices[0] - shifted_prices[1]) / (200 * bump),
                delta=1e-10,
            )
            self.assertIsNone(actual.delta)
            self.assertIsNone(actual.rho)
        flat = market(dividend=0.02)
        actual = engine.price_with_greeks(
            option, flat, ["volga", "dividend_rho"], volatility_shift=0.2
        )
        self.assertIsNone(actual.volga)
        self.assertIsNotNone(actual.dividend_rho)
        actual = engine.price_with_greeks(
            option, flat, ["volga", "dividend_rho"], rate_shift=math.ulp(0.0)
        )
        self.assertIsNotNone(actual.volga)
        self.assertIsNone(actual.dividend_rho)
        self.assertEqual(context.model_parameters.dividend_curve.factors, curve.factors)

    def test_american_dividend_stencil_crossing_zero_keeps_one_estimator(self):
        option = AmericanOption(
            option_type="call",
            strike=100,
            effective_midnight_utc=START,
            expiry_midnight_utc=END,
        )
        engine = MonteCarloVanillaEngine(path_count=2048, step_count=7, seed=42)
        shift = 1 / 8192
        for dividend in (-shift / 2, 0, shift / 2, shift):
            context = market(rate=0, dividend=dividend)
            actual = engine.price_with_greeks(
                option, context, "dividend_rho", rate_shift=shift
            )
            # A subnormal negative rate forces the exercise grid without changing BSM coefficients.
            up, down = [
                engine.price(
                    option, market(rate=-math.ulp(0.0), dividend=dividend + bump)
                )
                for bump in (shift, -shift)
            ]
            self.assertAlmostEqual(
                actual.dividend_rho, (up - down) / (200 * shift), delta=1e-10
            )
            self.assertEqual(actual.price, engine.price(option, context))
            self.assertIsNone(actual.rho)
