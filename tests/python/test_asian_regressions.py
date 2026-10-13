import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import date, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import ArithmeticAveragePriceOption, GeometricAveragePriceOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class AsianRegressionTests(unittest.TestCase):
    def test_vanna_survives_underflow_of_combined_stencil_scales(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=1e-200
            ),
            spot_price=1e-200,
            valuation_time=start,
        )
        for kind, engine in (
            (
                GeometricAveragePriceOption,
                pricing.AnalyticGeometricAveragePriceEngine(),
            ),
            (
                ArithmeticAveragePriceOption,
                pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
            ),
        ):
            with self.subTest(kind=kind):
                option = kind(
                    option_type="call",
                    strike=5e-201,
                    effective_date=start,
                    averaging_start_date=start,
                    expiry_date=end,
                )
                result = engine.price_with_greeks(
                    option,
                    context,
                    greeks="vanna",
                    spot_shift=1e-201,
                    volatility_shift=1e-201,
                )
                self.assertAlmostEqual(result.price, 5e-201, delta=5e-213)
                self.assertEqual(result.vanna, 0)

    def test_concurrent_asian_delta_requests_preserve_results_and_errors(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for kind, engine, expected_price, expected_delta in (
            (
                GeometricAveragePriceOption,
                pricing.AnalyticGeometricAveragePriceEngine(),
                5.035867546515384,
                0.5473455532045122,
            ),
            (
                ArithmeticAveragePriceOption,
                pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
                5.246501860362777,
                0.5602339572242674,
            ),
        ):
            option = kind(
                option_type="call",
                strike=100,
                effective_date=start,
                averaging_start_date=start,
                expiry_date=end,
            )

            def calculate(selection, engine=engine, option=option):
                return engine.price_with_greeks(option, context, **selection)

            selections = [
                {"greeks": "delta"},
                {"greeks": ["delta", "delta"]},
                {"all_greeks": True},
            ] * 4
            with self.subTest(kind=kind), ThreadPoolExecutor(max_workers=4) as pool:
                for selection, result in zip(
                    selections, pool.map(calculate, selections)
                ):
                    self.assertAlmostEqual(result.price, expected_price, delta=1e-12)
                    self.assertAlmostEqual(result.delta, expected_delta, delta=1e-12)
                    if "greeks" in selection:
                        self.assertIsNone(result.gamma)
                        self.assertIsNone(result.vega)
                    else:
                        self.assertGreater(result.gamma, 0)
                        self.assertGreater(result.vega, 0)
                for selection in selections[:3]:
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        engine.price_with_greeks(
                            option, context, **selection, spot_shift=0
                        )
                    self.assertEqual(
                        error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                    )

    def test_accrued_arithmetic_calls_retain_representable_discounted_prices(self):
        start = date(2025, 1, 1)
        # Independent 80-digit discounted expected arithmetic payoffs.
        cases = (
            (2, 1e308, 0, 0, 0.25, 7.5e307),
            (2, 1e308, 0.05, 0.05, 0.25, 7.134220683755355e307),
            (2, 1e308, 2, 0, 0.25, 2.0050048747581985e307),
            (1, 1e300, 750, 750, 0.25, 1.4262637226062548e-26),
            (1, 1e-300, -750, -750, 0.25, 3.943870906091103e25),
            (1, 100, 0.04, 0.01, 0.5, 48.76732432807474),
        )
        engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        for elapsed, amount, rate, dividend, strike_fraction, expected in cases:
            for direction in ("call", "put"):
                with self.subTest(amount=amount, rate=rate, direction=direction):
                    valuation = start + timedelta(days=365 * elapsed)
                    option = ArithmeticAveragePriceOption(
                        option_type=direction,
                        strike=amount * strike_fraction,
                        effective_date=start,
                        averaging_start_date=start,
                        expiry_date=valuation + timedelta(days=365),
                        realized_average=amount,
                    )
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate,
                            dividend_yield=dividend,
                            volatility=0.2,
                        ),
                        spot_price=amount,
                        valuation_time=valuation,
                    )
                    actual = engine.price(option, context)
                    if direction == "call":
                        self.assertAlmostEqual(actual, expected, delta=expected * 1e-12)
                    else:
                        self.assertEqual(actual, 0)
        option = ArithmeticAveragePriceOption(
            option_type="call",
            strike=25,
            effective_date=start,
            averaging_start_date=start,
            expiry_date=start + timedelta(days=730),
            realized_average=100,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=-750, dividend_yield=-750, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start + timedelta(days=365),
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(option, context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)

    def test_asian_terms_preserve_validation_precedence(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for option_type in (GeometricAveragePriceOption, ArithmeticAveragePriceOption):
            for strike, average, category in (
                (0, -1, kiyosi.ErrorCategory.INVALID_STRIKE),
                (100, -1, kiyosi.ErrorCategory.INVALID_PARAMETER),
                (100, 0, kiyosi.ErrorCategory.INVALID_TIME_RANGE),
            ):
                with self.subTest(
                    option_type=option_type, strike=strike, average=average
                ):
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        option_type(
                            option_type="call",
                            strike=strike,
                            realized_average=average,
                            effective_date=end,
                            averaging_start_date=start,
                            expiry_date=start,
                        )
                    self.assertEqual(error.exception.category, category)

    def test_arithmetic_asians_retain_time_value_when_squared_volatility_underflows(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction in ("call", "put"):
            for spot, sigma in ((100, 1e-170), (1e160, 1e-160), (1e300, 1e-200)):
                for window in (0, 1, 2):
                    with self.subTest(
                        direction=direction, spot=spot, sigma=sigma, window=window
                    ):
                        future = 1 if window == 0 else 245 / 365
                        lead = 120 / 365 if window == 1 else 0
                        scale = future if window == 2 else 1
                        expected = (
                            scale
                            * spot
                            * sigma
                            * math.sqrt((lead + future / 3) / (2 * math.pi))
                        )
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0,
                                dividend_yield=0,
                                volatility=sigma,
                            ),
                            spot_price=spot,
                            valuation_time=start + timedelta(days=120)
                            if window == 2
                            else start,
                        )
                        option = ArithmeticAveragePriceOption(
                            option_type=direction,
                            strike=spot,
                            effective_date=start,
                            averaging_start_date=start + timedelta(days=120)
                            if window == 1
                            else start,
                            expiry_date=end,
                            realized_average=spot if window == 2 else 0,
                        )
                        actual = (
                            pricing.TurnbullWakemanArithmeticAveragePriceEngine().price(
                                option, context
                            )
                        )
                        self.assertAlmostEqual(actual, expected, delta=expected * 1e-12)
