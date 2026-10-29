"""Public API checks, through the Rust extension."""

import importlib
import json
import math
import numbers
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta, timezone
from fractions import Fraction
from pathlib import Path

import QuantLib as ql
from kiyosi import ComputationFailureError, InvalidInputError
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    weekdays_calendar,
)
from kiyosi.pricing import AnalyticEuropeanEngine


class EuropeanTests(unittest.TestCase):
    def setUp(self):
        self.parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )

    def option(self, **changes):
        values = {
            "option_type": "call",
            "strike": 100.0,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        values.update(changes)
        return EuropeanOption(**values)

    def context(self, **changes):
        values = {
            "model_parameters": self.parameters,
            "spot_price": 100.0,
            "valuation_time": date(2025, 1, 1),
        }
        values.update(changes)
        return PricingContext(**values)

    def test_call_and_put_prices(self):
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        context = PricingContext(
            model_parameters=parameters,
            spot_price=100.0,
            valuation_time=date(2025, 1, 1),
        )
        for direction, expected in (
            ("call", 9.227005508154036),
            ("put", 6.330080627549918),
        ):
            with self.subTest(direction=direction):
                option = EuropeanOption(
                    option_type=direction,
                    strike=100.0,
                    effective_date=date(2025, 1, 1),
                    expiry_date=date(2026, 1, 1),
                )
                self.assertAlmostEqual(
                    AnalyticEuropeanEngine().price(option, context),
                    expected,
                    delta=1e-12,
                )

    def test_existing_scenarios_against_live_quantlib(self):
        test_root = Path(__file__).resolve().parent

        oracle = importlib.import_module("quantlib_reference.oracle")
        oracle.require_version()
        config = json.loads(
            (test_root / "quantlib_reference/scenarios.json").read_text()
        )
        for scenario in config["scenarios"]:
            inputs = dict(config["defaults"]["inputs"], **scenario["inputs"])
            with self.subTest(case=scenario["case_id"]), ql.SavedSettings():
                expected = oracle.vanilla_option(inputs).NPV()
                model = BlackScholesMertonParameters(
                    risk_free_rate=inputs["rate"],
                    dividend_yield=inputs["dividend"],
                    volatility=inputs["volatility"],
                )
                option = self.option(
                    option_type=inputs["option"],
                    strike=inputs["strike"],
                    effective_date=date.fromisoformat(inputs["effective_date"]),
                    expiry_date=date.fromisoformat(inputs["expiry_date"]),
                )
                context = self.context(
                    model_parameters=model,
                    spot_price=inputs["spot"],
                    valuation_time=date.fromisoformat(inputs["valuation"]),
                )
                tolerance = scenario.get("tolerances", {}).get(
                    "price", config["defaults"]["tolerances"]["price"]
                )
                self.assertAlmostEqual(
                    AnalyticEuropeanEngine().price(option, context),
                    expected,
                    delta=tolerance,
                )

    def test_valuation_and_expiry_boundaries(self):
        option, engine = self.option(), AnalyticEuropeanEngine()
        noon = self.context(valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC))
        self.assertLess(
            engine.price(option, noon), engine.price(option, self.context())
        )
        expiry = self.context(spot_price=110, valuation_time=option.expiry_date)
        self.assertEqual(engine.price(option, expiry), 10)
        same_day = self.option(effective_date=option.expiry_date)
        self.assertEqual(engine.price(same_day, expiry), 10)
        for at in (
            date(2024, 12, 31),
            datetime(2026, 1, 1, 0, 0, 0, 1, tzinfo=UTC),
            date(2026, 1, 2),
        ):
            context = self.context(valuation_time=at)
            with self.subTest(at=at), self.assertRaises(InvalidInputError) as error:
                engine.price(option, context)
            self.assertIs(type(error.exception), InvalidInputError)

    def test_dates_and_datetimes_use_stored_components(self):
        class ShadowedDate(date):
            year = property(lambda self: 67562)
            month = property(lambda self: 257)
            day = property(lambda self: 257)

        class ShadowedTime(datetime):
            year = property(lambda self: 67562)
            hour = property(lambda self: 25)
            microsecond = property(lambda self: 1000001)

            def astimezone(self, tz=None):
                raise AssertionError("must use builtin conversion")

            def utcoffset(self):
                return None

        start = ShadowedDate(2025, 1, 1)
        self.assertEqual(
            self.option(effective_date=start).effective_date, date(2025, 1, 1)
        )
        local = ShadowedTime(
            2025, 1, 1, 8, 9, 10, 123456, tzinfo=timezone(timedelta(hours=8))
        )
        self.assertEqual(
            self.context(valuation_time=local).valuation_time,
            datetime(2025, 1, 1, 0, 9, 10, 123456, tzinfo=UTC),
        )
        for day in (
            date(1, 1, 1),
            date(1600, 1, 1),
            date(2500, 1, 1),
            date(9999, 12, 31),
        ):
            self.assertEqual(
                self.context(valuation_time=day).valuation_time,
                datetime.combine(day, datetime.min.time(), UTC),
            )
        latest = datetime(9999, 12, 31, 23, 59, 59, 999999, tzinfo=UTC)
        self.assertEqual(self.context(valuation_time=latest).valuation_time, latest)
        for bad in ("2025-01-01", None, 1, datetime(2025, 1, 1)):
            with self.subTest(bad=bad), self.assertRaises(TypeError):
                self.context(valuation_time=bad)
        with self.assertRaises(TypeError):
            self.option(effective_date=datetime(2025, 1, 1, tzinfo=UTC))

    def test_strict_numeric_protocol_and_native_conversion_failures(self):
        for field in ("risk_free_rate", "dividend_yield", "volatility"):
            for bad in (True, False, "0.2", None, complex(1)):
                values = {
                    "risk_free_rate": 0.05,
                    "dividend_yield": 0.02,
                    "volatility": 0.2,
                }
                values[field] = bad
                with self.subTest(field=field, bad=bad), self.assertRaises(TypeError):
                    BlackScholesMertonParameters(**values)
        for bad in (True, "100", None):
            with self.assertRaises(TypeError):
                self.option(strike=bad)
            with self.assertRaises(TypeError):
                self.context(spot_price=bad)
        with self.assertRaises(OverflowError):
            self.option(strike=10**1000)
        self.assertEqual(self.option(strike=Fraction(201, 2)).strike, 100.5)

        @numbers.Real.register
        class BrokenReal:
            def __float__(self):
                raise MemoryError("conversion failed")

        with self.assertRaises(MemoryError):
            self.option(strike=BrokenReal())
        with self.assertRaises(TypeError):
            self.option(option_type=1)
        with self.assertRaises(ValueError):
            self.option(option_type="Call")
        with self.assertRaises(TypeError):
            EuropeanOption("call", 100, date(2025, 1, 1), date(2026, 1, 1))

    def test_domain_errors_and_failed_construction_preserve_inputs(self):
        context, option = self.context(), self.option()
        for bad in (0, -1, math.nan, math.inf, -math.inf):
            for make, expected_error in (
                (
                    lambda bad=bad: self.option(strike=bad),
                    InvalidInputError,
                ),
                (
                    lambda bad=bad: self.context(spot_price=bad),
                    InvalidInputError,
                ),
                (
                    lambda bad=bad: BlackScholesMertonParameters(
                        risk_free_rate=0, dividend_yield=0, volatility=bad
                    ),
                    InvalidInputError,
                ),
            ):
                with (
                    self.subTest(bad=bad, expected_error=expected_error),
                    self.assertRaises(expected_error) as error,
                ):
                    make()
                self.assertIs(type(error.exception), expected_error)
        for field, expected_error in (
            ("risk_free_rate", InvalidInputError),
            ("dividend_yield", InvalidInputError),
        ):
            values = {"risk_free_rate": 0, "dividend_yield": 0, "volatility": 0.2}
            values[field] = math.inf
            with self.assertRaises(expected_error) as error:
                BlackScholesMertonParameters(**values)
            self.assertIs(type(error.exception), expected_error)
        with self.assertRaises(InvalidInputError) as error:
            self.option(effective_date=date(2027, 1, 1))
        self.assertIs(type(error.exception), InvalidInputError)
        self.assertAlmostEqual(
            AnalyticEuropeanEngine().price(option, context), 9.227005508154036
        )

    def test_owned_readonly_values_and_concurrent_pricing(self):
        context, option, engine = (
            self.context(),
            self.option(),
            AnalyticEuropeanEngine(),
        )
        self.assertEqual(
            context.calendar.trading_days_per_year,
            weekdays_calendar().trading_days_per_year,
        )
        self.assertEqual(context.calendar.trading_days_per_year, 252)
        saved = context.model_parameters
        with self.assertRaises(AttributeError):
            context.spot_price = 200
        with self.assertRaises(AttributeError):
            saved.volatility = 0
        with self.assertRaises(AttributeError):
            option.strike = 0
        expected = engine.price(option, context)
        with ThreadPoolExecutor(max_workers=4) as pool:
            futures = [pool.submit(engine.price, option, context) for _ in range(16)]
            prices = [future.result() for future in futures]
            distinct = list(
                pool.map(
                    lambda _: AnalyticEuropeanEngine().price(
                        self.option(), self.context()
                    ),
                    range(16),
                )
            )
        self.assertEqual(prices, [expected] * 16)
        self.assertEqual(distinct, prices)
        del context
        self.assertEqual(saved.volatility, 0.2)

    def test_extreme_numerical_regressions(self):
        # Independent high-precision cases retained from the existing analytic suite.
        for direction, spot, strike, sigma, expected in (
            ("call", 100, 100.00000000013, 1e-13, 4.6459080591698985e-51),
            ("put", 100, 99.99999999987, 1e-13, 4.645908058131258e-51),
            ("call", 100, 100.00099900499002, 1e-6, 8.276374696508508e-29),
            ("call", 100, 100.00100100501001, 1e-6, 6.749830430787083e-29),
            ("put", 100, 99.99700004499955, 1e-6, 1.6319322561070434e-203),
            ("call", 1e300, 1e304, 0.2, 1.1367038364232515e-163),
            ("put", 1e304, 1e300, 0.2, 1.1367038364232515e-163),
            ("call", 100, 100, 1e-200, 3.989422804014327e-199),
        ):
            model = BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=sigma
            )
            with self.subTest(direction=direction, strike=strike, sigma=sigma):
                actual = AnalyticEuropeanEngine().price(
                    self.option(option_type=direction, strike=strike),
                    self.context(model_parameters=model, spot_price=spot),
                )
                self.assertTrue(math.isclose(actual, expected, rel_tol=1e-8))
        model = BlackScholesMertonParameters(
            risk_free_rate=-1000, dividend_yield=0, volatility=0.3
        )
        with self.assertRaises(ComputationFailureError) as error:
            AnalyticEuropeanEngine().price(
                self.option(option_type="put"), self.context(model_parameters=model)
            )
        self.assertIs(type(error.exception), ComputationFailureError)
