"""Shared European analytics contracts, through the Rust extension."""

import importlib
import json
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta
from fractions import Fraction
from pathlib import Path

import QuantLib as ql
from kiyosi import (
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import AnalyticEuropeanEngine, implied_volatility


class AnalyticsTests(unittest.TestCase):
    def setUp(self):
        self.option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        self.context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=date(2025, 1, 1),
        )
        self.engine = AnalyticEuropeanEngine()

    def test_greeks_and_volatility_recovery(self):
        result = self.engine.price_with_greeks(
            self.option, self.context, all_greeks=True
        )
        self.assertAlmostEqual(result.delta, 0.586851146134764, delta=1e-12)
        self.assertAlmostEqual(result.gamma, 0.0189505787550087, delta=1e-12)
        self.assertAlmostEqual(result.vega, 0.379011575100175, delta=1e-12)
        self.assertAlmostEqual(
            implied_volatility(self.engine, self.option, self.context, result.price),
            0.2,
            delta=1e-8,
        )

    def test_live_quantlib_greeks_scalar_differences_and_recovery(self):
        root = Path(__file__).resolve().parent

        oracle = importlib.import_module("quantlib_reference.oracle")
        differences = importlib.import_module("price_differences").price_differences
        oracle.require_version()
        config = json.loads((root / "quantlib_reference/scenarios.json").read_text())
        for scenario in config["scenarios"]:
            inputs = dict(config["defaults"]["inputs"], **scenario["inputs"])
            option = EuropeanOption(
                option_type=inputs["option"],
                strike=inputs["strike"],
                effective_date=date.fromisoformat(inputs["effective_date"]),
                expiry_date=date.fromisoformat(inputs["expiry_date"]),
            )
            context = self.context.replace(
                spot_price=inputs["spot"],
                valuation_time=date.fromisoformat(inputs["valuation"]),
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=inputs["rate"],
                    dividend_yield=inputs["dividend"],
                    volatility=inputs["volatility"],
                ),
            )
            saved = ql.SavedSettings()
            try:
                reference, metadata = oracle.reference(inputs)
            finally:
                del saved
            result = self.engine.price_with_greeks(option, context, all_greeks=True)
            budget = dict(
                config["defaults"]["tolerances"], **scenario.get("tolerances", {})
            )
            for name, expected in reference.items():
                with self.subTest(case=scenario["case_id"], measure=name):
                    self.assertAlmostEqual(
                        getattr(result, name),
                        expected,
                        delta=budget[name] + metadata[f"uncertainty_{name}"],
                    )
            # Reuse the existing scalar oracle and unchanged numerical acceptance budgets.
            numeric_budget = dict(
                config["defaults"]["numerical_tolerances"],
                **scenario.get("numerical_tolerances", {}),
            )
            bumps = (
                (0.008, 0.01)
                if (option.expiry_date - context.valuation_time.date()).days <= 2
                else (0.01, 0.012)
            )
            for bump in bumps:
                numeric = differences(
                    self.engine,
                    option,
                    context,
                    sorted(set(reference) - {"price"}),
                    spot_shift=bump,
                )
                for name, expected in reference.items():
                    with self.subTest(
                        case=scenario["case_id"], measure=name, bump=bump
                    ):
                        self.assertAlmostEqual(
                            getattr(numeric, name),
                            expected,
                            delta=numeric_budget[name]
                            + metadata[f"uncertainty_{name}"],
                        )
            self.assertAlmostEqual(
                implied_volatility(self.engine, option, context, result.price),
                inputs["volatility"],
                delta=1e-7,
            )

    def test_requests_availability_and_expiry(self):
        full = self.engine.price_with_greeks(self.option, self.context, all_greeks=True)
        for request in (
            "delta",
            ["delta", "vega", "delta"],
            (name for name in ("delta", "vega")),
        ):
            selected = self.engine.price_with_greeks(self.option, self.context, request)
            self.assertEqual(selected.delta, full.delta)
            self.assertEqual(selected.require("delta"), full.delta)
            self.assertIsNone(selected.gamma)
            with self.assertRaises(UnsupportedOperationError) as error:
                selected.require("gamma")
            self.assertIs(type(error.exception), UnsupportedOperationError)
        expiry = self.context.replace(
            spot_price=110, valuation_time=self.option.expiry_date
        )
        result = self.engine.price_with_greeks(self.option, expiry, all_greeks=True)
        self.assertEqual(result.price, 10)
        for name in (
            "delta",
            "gamma",
            "speed",
            "theta",
            "charm",
            "color",
            "vega",
            "vanna",
            "zomma",
            "rho",
        ):
            self.assertIsNone(getattr(result, name))
            with self.assertRaises(UnsupportedOperationError):
                result.require(name)
        for changes in (
            {},
            {"all_greeks": 1},
            {"greeks": "delta", "all_greeks": True},
            {"greeks": True},
            {"greeks": [1]},
        ):
            with self.subTest(changes=changes), self.assertRaises(TypeError):
                self.engine.price_with_greeks(self.option, self.context, **changes)
        for request in ("Delta", "unknown"):
            with self.assertRaises(ValueError):
                self.engine.price_with_greeks(self.option, self.context, request)
        with self.assertRaises(InvalidInputError) as error:
            self.engine.price_with_greeks(self.option, self.context, [])
        self.assertIs(type(error.exception), InvalidInputError)
        for value in (1, "unknown"):
            with self.assertRaises(TypeError if value == 1 else ValueError):
                full.require(value)
        with self.assertRaises(AttributeError):
            full.delta = 0
        del self.context
        self.assertEqual(full.delta, 0.586851146134764)

    def test_strict_shift_and_solver_representations(self):
        for field in (
            "spot_shift",
            "volatility_shift",
            "rate_shift",
            "time_shift_days",
        ):
            for bad in (True, "1", None, complex(1)):
                with self.subTest(field=field, bad=bad), self.assertRaises(TypeError):
                    self.engine.price_with_greeks(
                        self.option, self.context, "delta", **{field: bad}
                    )
        for field in (
            "lower_bound",
            "upper_bound",
            "price_tolerance",
            "parameter_tolerance",
            "max_iterations",
        ):
            for bad in (True, "1", None, complex(1)):
                with self.subTest(field=field, bad=bad), self.assertRaises(TypeError):
                    implied_volatility(
                        self.engine, self.option, self.context, 9.227, **{field: bad}
                    )
        for field, call in (
            (
                "time_shift_days",
                lambda **kw: self.engine.price_with_greeks(
                    self.option, self.context, "delta", **kw
                ),
            ),
            (
                "max_iterations",
                lambda **kw: implied_volatility(
                    self.engine, self.option, self.context, 9.227, **kw
                ),
            ),
        ):
            for value in (1.5, 1.0, Fraction(3, 2)):
                with self.assertRaises(TypeError):
                    call(**{field: value})
            with self.assertRaises(OverflowError):
                call(**{field: 2**40})
        for quote in (True, "1", None):
            with self.assertRaises(TypeError):
                implied_volatility(self.engine, self.option, self.context, quote)
        with self.assertRaises(OverflowError):
            implied_volatility(self.engine, self.option, self.context, 10**1000)
        with self.assertRaises(TypeError):
            self.engine.price_with_greeks(self.option, self.context, "delta", unknown=1)
        result = self.engine.price_with_greeks(
            self.option, self.context, "delta", spot_shift=Fraction(1, 100)
        )
        self.assertAlmostEqual(result.delta, 0.586851146134764, delta=1e-12)

    def test_domain_errors_validation_order_and_failure_state(self):
        quote = self.engine.price(self.option, self.context)

        def solve(context=None, observed=quote, **kw):
            return implied_volatility(
                self.engine, self.option, context or self.context, observed, **kw
            )

        for field in (
            "spot_shift",
            "volatility_shift",
            "rate_shift",
            "time_shift_days",
        ):
            for bad in (0, -1):
                with self.assertRaises(InvalidInputError) as error:
                    self.engine.price_with_greeks(
                        self.option, self.context, "delta", **{field: bad}
                    )
                self.assertIs(type(error.exception), InvalidInputError)
        for field in ("spot_shift", "volatility_shift", "rate_shift"):
            for bad in (math.nan, math.inf):
                with self.assertRaises(InvalidInputError) as error:
                    self.engine.price_with_greeks(
                        self.option, self.context, "delta", **{field: bad}
                    )
                self.assertIs(type(error.exception), InvalidInputError)
        for controls in (
            {"lower_bound": 0},
            {"lower_bound": 4},
            {"upper_bound": math.inf},
            {"price_tolerance": 0},
            {"parameter_tolerance": math.nan},
            {"max_iterations": 0},
        ):
            with self.assertRaises(InvalidInputError) as error:
                solve(**controls)
            self.assertIs(type(error.exception), InvalidInputError)
        for observed, expected_error in (
            (math.nan, InvalidInputError),
            (math.inf, InvalidInputError),
            (-1, RootNotBracketedError),
            (101, RootNotBracketedError),
        ):
            with self.assertRaises(expected_error) as error:
                solve(observed=observed)
            self.assertIs(type(error.exception), expected_error)
        with self.assertRaises(NonConvergenceError) as error:
            solve(max_iterations=1, price_tolerance=1e-15, parameter_tolerance=1e-15)
        self.assertIs(type(error.exception), NonConvergenceError)
        expiry = self.context.replace(valuation_time=self.option.expiry_date)
        with self.assertRaises(UnsupportedOperationError) as error:
            solve(expiry, 0)
        self.assertIs(type(error.exception), UnsupportedOperationError)
        outside = self.context.replace(
            valuation_time=datetime(2026, 1, 1, 0, 0, 0, 1, tzinfo=UTC)
        )
        with self.assertRaises(InvalidInputError) as error:
            solve(outside, 0)
        self.assertIs(type(error.exception), InvalidInputError)
        with self.assertRaises(InvalidInputError) as error:
            self.engine.price_with_greeks(self.option, outside, [])
        self.assertIs(type(error.exception), InvalidInputError)
        self.assertEqual(self.context.model_parameters.volatility, 0.2)
        self.assertEqual(self.engine.price(self.option, self.context), quote)

    def test_solver_endpoints_curves_and_concurrent_calls(self):
        for vol in (0.0001, 0.03, 0.2, 1.5, 4):
            context = self.context.replace(
                model_parameters=self.context.model_parameters.replace(volatility=vol)
            )
            quote = self.engine.price(self.option, context)
            self.assertAlmostEqual(
                implied_volatility(self.engine, self.option, self.context, quote),
                vol,
                delta=1e-8,
            )
        reference = datetime(2025, 1, 1, tzinfo=UTC)
        end = datetime(2026, 1, 1, tzinfo=UTC)
        knot = reference + timedelta(days=180)
        curve = RateTermStructure(
            reference_time=reference,
            times=[knot, end],
            factors=[
                math.exp(-0.02 * 180 / 365),
                math.exp(-0.02 * 180 / 365 - 0.08 * 185 / 365),
            ],
        )
        context = self.context.replace(
            model_parameters=self.context.model_parameters.replace(
                risk_free_rate=curve
            ),
            valuation_time=knot,
        )
        result = self.engine.price_with_greeks(self.option, context, all_greeks=True)
        after = self.context.replace(
            model_parameters=context.model_parameters,
            valuation_time=knot + timedelta(seconds=1),
        )
        self.assertAlmostEqual(
            result.theta,
            (self.engine.price(self.option, after) - result.price) * 86400,
            delta=1e-7,
        )
        quote = result.price

        def recover(_):
            return implied_volatility(self.engine, self.option, context, quote)

        with ThreadPoolExecutor(max_workers=4) as pool:
            for vol in pool.map(recover, range(16)):
                self.assertAlmostEqual(vol, 0.2, delta=1e-8)

    def test_extreme_greeks_preserve_scaling_and_selected_results(self):
        for vol, spot, strike in (
            (1e-200, 100, 100),
            (0.2, 1e300, 1e304),
            (1e308, 100, 100),
        ):
            option = EuropeanOption(
                option_type="call",
                strike=strike,
                effective_date=self.option.effective_date,
                expiry_date=self.option.expiry_date,
            )
            context = self.context.replace(
                spot_price=spot,
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=vol
                ),
            )
            result = self.engine.price_with_greeks(
                option, context, ["delta", "vega", "gamma"]
            )
            for name in ("delta", "vega", "gamma"):
                self.assertTrue(math.isfinite(getattr(result, name)))
                self.assertEqual(
                    getattr(result, name),
                    getattr(self.engine.price_with_greeks(option, context, name), name),
                )
        # Normal density scaled before exponentiation; unscaled tail density underflows.
        self.assertEqual(result.gamma, 0)
