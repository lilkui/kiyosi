"""Volga and parallel dividend-yield sensitivities through the public API."""

import math
import unittest
from datetime import datetime

import QuantLib as ql
from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    FiniteDifferenceVanillaEngine,
    MonteCarloVanillaEngine,
    numerical_greeks,
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
                option_type=direction, strike=100, effective_time=START, expiry_time=END
            )
            for spot in (80, 100, 120):
                for dividend in (-0.02, 0, 0.02):
                    context = market(spot=spot, rate=0.05, dividend=dividend, sigma=0.2)
                    inputs = {
                        "option": direction,
                        "spot": spot,
                        "strike": 100,
                        "effective_time": START.date().isoformat(),
                        "expiry_time": END.date().isoformat(),
                        "valuation": START.date().isoformat(),
                        "rate": 0.05,
                        "dividend": dividend,
                        "volatility": 0.2,
                    }
                    reference = oracle.vanilla_option(inputs)
                    expected_dividend = reference.dividendRho() / 100
                    # Differentiate independent QuantLib vega_per_percentage_point at two resolutions.
                    volga_per_squared_percentage_point = []
                    for h in (0.0001, 0.00005):
                        up = oracle.vanilla_option({**inputs, "volatility": 0.2 + h})
                        down = oracle.vanilla_option({**inputs, "volatility": 0.2 - h})
                        volga_per_squared_percentage_point.append(
                            (up.vega() - down.vega()) / (20000 * h)
                        )
                    self.assertAlmostEqual(
                        volga_per_squared_percentage_point[0],
                        volga_per_squared_percentage_point[1],
                        delta=1e-8,
                    )
                    expected = {
                        "volga_per_squared_percentage_point": volga_per_squared_percentage_point[
                            1
                        ],
                        "dividend_rho_per_percentage_point": expected_dividend,
                    }
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
                                "volga_per_squared_percentage_point"
                                if name == "dividend_rho_per_percentage_point"
                                else "dividend_rho_per_percentage_point"
                            )
                            self.assertIsNone(getattr(selected, other))
                    if spot == 100 and dividend == 0.02:
                        self.assertAlmostEqual(
                            full.volga_per_squared_percentage_point,
                            0.000236882234437609,
                            delta=1e-14,
                        )
                        expected_q = (
                            -0.586851146134764
                            if direction == "call"
                            else 0.393347527171254
                        )
                        self.assertAlmostEqual(
                            full.dividend_rho_per_percentage_point,
                            expected_q,
                            delta=1e-12,
                        )

    def test_parallel_dividend_curves_numerical_bumps_and_availability(self):
        option = EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        curve = RateTermStructure(
            reference_time=START,
            times=[datetime(2025, 7, 1), END],
            factors=[math.exp(-0.01 * 181 / 365), math.exp(-0.03)],
        )
        context = market().replace(
            model_parameters=market().model_parameters.replace(dividend_yield=curve)
        )
        engine = FiniteDifferenceVanillaEngine(
            asset_step_count=400, time_step_count=400
        )
        exact = AnalyticEuropeanEngine().price_with_greeks(option, context, "all")
        self.assertAlmostEqual(
            exact.dividend_rho_per_percentage_point, -exact.delta, delta=1e-12
        )
        for bump in (0.0001, 0.0002):
            actual = numerical_greeks(
                engine,
                option,
                context,
                [
                    "volga_per_squared_percentage_point",
                    "dividend_rho_per_percentage_point",
                ],
                volatility_shift=bump,
                rate_shift=bump,
            )
            self.assertAlmostEqual(
                actual.volga_per_squared_percentage_point,
                exact.volga_per_squared_percentage_point,
                delta=2e-5,
            )
            self.assertAlmostEqual(
                actual.dividend_rho_per_percentage_point,
                exact.dividend_rho_per_percentage_point,
                delta=0.0002,
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
                actual.dividend_rho_per_percentage_point,
                (shifted_prices[0] - shifted_prices[1]) / (200 * bump),
                delta=1e-10,
            )
            self.assertIsNone(actual.delta)
            self.assertIsNone(actual.rho_per_percentage_point)
        flat = market(dividend=0.02)
        actual = numerical_greeks(
            engine,
            option,
            flat,
            ["volga_per_squared_percentage_point", "dividend_rho_per_percentage_point"],
            volatility_shift=0.2,
        )
        self.assertIsNone(actual.volga_per_squared_percentage_point)
        self.assertIsNotNone(actual.dividend_rho_per_percentage_point)
        actual = numerical_greeks(
            engine,
            option,
            flat,
            ["volga_per_squared_percentage_point", "dividend_rho_per_percentage_point"],
            rate_shift=math.ulp(0.0),
        )
        self.assertIsNotNone(actual.volga_per_squared_percentage_point)
        self.assertIsNone(actual.dividend_rho_per_percentage_point)
        self.assertEqual(context.model_parameters.dividend_curve.factors, curve.factors)

    def test_american_dividend_stencil_crossing_zero_keeps_one_estimator(self):
        option = AmericanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        engine = MonteCarloVanillaEngine(path_count=2048, step_count=6, seed=42)
        requested = 0.0001
        for dividend in (-requested / 2, 0, requested / 2, requested):
            context = market(rate=0, dividend=dividend)
            actual = engine.price_with_greeks(
                option, context, "dividend_rho_per_percentage_point"
            )
            shift = (dividend + requested) - dividend
            if dividend - (dividend - shift) != shift:
                self.assertIsNone(actual.dividend_rho_per_percentage_point)
                continue
            # A subnormal negative rate forces the exercise grid without changing BSM coefficients.
            up, down = [
                engine.price(
                    option, market(rate=-math.ulp(0.0), dividend=dividend + bump)
                )
                for bump in (shift, -shift)
            ]
            self.assertAlmostEqual(
                actual.dividend_rho_per_percentage_point,
                (up - down) / (200 * shift),
                delta=1e-10,
            )
            self.assertEqual(actual.price, engine.price(option, context))
            self.assertIsNone(actual.rho_per_percentage_point)
