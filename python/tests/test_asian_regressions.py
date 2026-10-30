import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import date, timedelta

from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import ArithmeticAveragePriceOption, GeometricAveragePriceOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import implied_volatility


class AsianRegressionTests(unittest.TestCase):
    def test_arithmetic_prices_survive_adjusted_strike_overflow(self):
        start, end = date(2023, 1, 1), date(2026, 1, 1)
        engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        for valuation in (date(2025, 1, 1), end - timedelta(days=1)):
            for volatility in (math.ulp(0.0), 1e-200, 1e-6, 0.2, 1.0):
                for rate in (-0.05, 0, 0.05):
                    parameters = BlackScholesMertonParameters(
                        risk_free_rate=rate, dividend_yield=rate, volatility=volatility
                    )
                    for direction in ("call", "put"):
                        with self.subTest(
                            valuation=valuation,
                            volatility=volatility,
                            rate=rate,
                            direction=direction,
                        ):
                            values = []
                            for scale in (1, 1e306):
                                option = ArithmeticAveragePriceOption(
                                    option_type=direction,
                                    strike=100 * scale,
                                    effective_date=start,
                                    averaging_start_date=start,
                                    expiry_date=end,
                                    realized_average=50 * scale,
                                )
                                context = PricingContext(
                                    model_parameters=parameters,
                                    spot_price=50 * scale,
                                    valuation_time=valuation,
                                )
                                values.append(engine.price(option, context) / scale)
                            self.assertTrue(
                                math.isclose(values[0], values[1], rel_tol=1e-10),
                                values,
                            )

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
                    with self.assertRaises(InvalidInputError) as error:
                        engine.price_with_greeks(
                            option, context, **selection, spot_shift=0
                        )
                    self.assertIs(type(error.exception), InvalidInputError)

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
        with self.assertRaises(ComputationFailureError) as error:
            engine.price(option, context)
        self.assertIs(type(error.exception), ComputationFailureError)

    def test_asian_terms_preserve_validation_precedence(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for option_type in (GeometricAveragePriceOption, ArithmeticAveragePriceOption):
            for strike, average, expected_error in (
                (0, -1, InvalidInputError),
                (100, -1, InvalidInputError),
                (100, 0, InvalidInputError),
            ):
                with self.subTest(
                    option_type=option_type, strike=strike, average=average
                ):
                    with self.assertRaises(expected_error) as error:
                        option_type(
                            option_type="call",
                            strike=strike,
                            realized_average=average,
                            effective_date=end,
                            averaging_start_date=start,
                            expiry_date=start,
                        )
                    self.assertIs(type(error.exception), expected_error)

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

    def test_asian_analytic_prices_retain_scaled_normal_tails(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.4
        )
        for direction, spot, strike, geometric, arithmetic in (
            ("call", 1e300, 1e304, 3.016612743105038e-48, 3.859991844527151e-40),
            ("put", 1e304, 1e300, 1.6951408750594615e-48, 2.369019512512863e-42),
        ):
            for scale in (1, 1e-100):
                context = PricingContext(
                    model_parameters=parameters,
                    spot_price=spot * scale,
                    valuation_time=start,
                )
                terms = {
                    "option_type": direction,
                    "strike": strike * scale,
                    "effective_date": start,
                    "averaging_start_date": start,
                    "expiry_date": end,
                }
                for engine, option, expected in (
                    (
                        pricing.AnalyticGeometricAveragePriceEngine(),
                        GeometricAveragePriceOption(**terms),
                        geometric,
                    ),
                    (
                        pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
                        ArithmeticAveragePriceOption(**terms),
                        arithmetic,
                    ),
                ):
                    with self.subTest(
                        direction=direction, scale=scale, engine=type(engine).__name__
                    ):
                        actual = engine.price(option, context)
                        self.assertTrue(
                            math.isclose(actual, expected * scale, rel_tol=1e-8)
                        )
                        self.assertEqual(
                            engine.price_with_greeks(option, context, "delta").price,
                            actual,
                        )

    def test_arithmetic_asian_implied_volatility_rejects_locked_realized_payoffs(self):
        start = date(2025, 1, 1)
        end, valuation = start + timedelta(days=364), start + timedelta(days=182)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                volatility=0.2,
            ),
            spot_price=100,
            valuation_time=valuation,
        )
        engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        for direction in ("call", "put"):
            terms = {
                "option_type": direction,
                "strike": 100,
                "effective_date": start,
                "averaging_start_date": start,
                "expiry_date": end,
            }
            for realized in (100, 200, 220):
                with self.subTest(direction=direction, realized=realized):
                    option = ArithmeticAveragePriceOption(
                        **terms, realized_average=realized
                    )
                    quote = engine.price(option, context)
                    if realized < 200:
                        self.assertAlmostEqual(
                            implied_volatility(
                                engine,
                                option,
                                context,
                                quote,
                                lower_bound=0.05,
                                upper_bound=0.4,
                            ),
                            0.2,
                            delta=1e-6,
                        )
                    else:
                        for observed in (quote, quote + 1):
                            with self.assertRaises(UnsupportedOperationError) as error:
                                implied_volatility(engine, option, context, observed)
                            self.assertIs(
                                type(error.exception),
                                UnsupportedOperationError,
                            )
                        if direction == "put":
                            self.assertEqual(quote, 0)
            geometric = GeometricAveragePriceOption(**terms, realized_average=200)
            geometric_engine = pricing.AnalyticGeometricAveragePriceEngine()
            self.assertAlmostEqual(
                implied_volatility(
                    geometric_engine,
                    geometric,
                    context,
                    geometric_engine.price(geometric, context),
                    lower_bound=0.05,
                    upper_bound=0.4,
                    price_tolerance=1e-30,
                ),
                0.2,
                delta=1e-6,
            )

    def test_arithmetic_asians_retain_small_moment_variance_across_averaging_windows(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        for direction in ("call", "put"):
            for spot in (100, 1e8, 1e14):
                for sigma in (1e-13, 1e-8, 1e-4, 0.009999, 0.01, 0.010001, 0.2):
                    for window in (0, 1, 2):
                        with self.subTest(
                            direction=direction,
                            spot=spot,
                            sigma=sigma,
                            window=window,
                        ):
                            valuation = (
                                start + timedelta(days=120) if window == 2 else start
                            )
                            averaging = (
                                start + timedelta(days=120) if window == 1 else start
                            )
                            delta = 1 if window == 0 else 245 / 365
                            lead = 120 / 365 if window == 1 else 0
                            scale = delta if window == 2 else 1
                            h = sigma * sigma * delta
                            term = excess = h / 3
                            for n in range(2, 17):
                                term *= h / (n + 2)
                                excess += term
                            variance = sigma * sigma * lead + math.log1p(excess)
                            expected = (
                                scale
                                * spot
                                * math.erf(math.sqrt(variance) / (2 * math.sqrt(2)))
                            )
                            context = PricingContext(
                                model_parameters=BlackScholesMertonParameters(
                                    risk_free_rate=0,
                                    dividend_yield=0,
                                    volatility=sigma,
                                ),
                                spot_price=spot,
                                valuation_time=valuation,
                            )
                            option = ArithmeticAveragePriceOption(
                                option_type=direction,
                                strike=spot,
                                effective_date=start,
                                averaging_start_date=averaging,
                                expiry_date=end,
                                realized_average=spot if window == 2 else 0,
                            )
                            actual = engine.price(option, context)
                            self.assertAlmostEqual(
                                actual, expected, delta=max(1e-24, expected * 1e-9)
                            )

    def test_arithmetic_asian_small_variance_prices_preserve_nonzero_carry(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        # Independent 75-digit evaluation of the integrated moments and normal CDF.
        cases = (
            (0.02, 0, 101006700.13377905, 0.22861156554028925, 0.2286115636758159),
            (-0.02, 0, 99006633.4662235, 0.23206658954210946, 0.2320665939728782),
            (0.3, 0, 116619602.52533437, 0.2064505517413919, 0.20645055603932333),
            (-0.3, 0, 86393926.43942738, 0.2585644971473165, 0.25856449358383626),
            (1.4, 1, 262916196.58168963, 0.20160068921082872, 0.2016006909574677),
            (-1.4, 2, 76395650.08983347, 0.1859456384724795, 0.18594565344673475),
        )
        for rate, window, strike, call, put in cases:
            for direction, expected in (("call", call), ("put", put)):
                with self.subTest(rate=rate, window=window, direction=direction):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate,
                            dividend_yield=0,
                            volatility=1e-8,
                        ),
                        spot_price=1e8,
                        valuation_time=(
                            start + timedelta(days=120) if window == 2 else start
                        ),
                    )
                    option = ArithmeticAveragePriceOption(
                        option_type=direction,
                        strike=strike,
                        effective_date=start,
                        averaging_start_date=(
                            start + timedelta(days=120) if window == 1 else start
                        ),
                        expiry_date=end,
                        realized_average=1e8 if window == 2 else 0,
                    )
                    actual = (
                        pricing.TurnbullWakemanArithmeticAveragePriceEngine().price(
                            option, context
                        )
                    )
                    self.assertAlmostEqual(actual, expected, delta=5e-7)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0,
                dividend_yield=0,
                volatility=1e-8,
            ),
            spot_price=100,
            valuation_time=end - timedelta(days=1),
        )
        oversized = ArithmeticAveragePriceOption(
            option_type="call",
            strike=1e308,
            effective_date=start,
            averaging_start_date=start,
            expiry_date=end,
            realized_average=1,
        )
        self.assertEqual(
            pricing.TurnbullWakemanArithmeticAveragePriceEngine().price(
                oversized, context
            ),
            0,
        )

    def test_geometric_asians_retain_small_positive_variance_across_averaging_windows(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction in ("call", "put"):
            for spot in (100, 1e8, 1e14):
                for sigma in (1e-160, 1e-13, 1e-12, 1e-8, 1e-4, 0.2):
                    for window in (0, 1, 2):
                        with self.subTest(
                            direction=direction, spot=spot, sigma=sigma, window=window
                        ):
                            valuation = (
                                start + timedelta(days=120) if window == 2 else start
                            )
                            averaging = (
                                start + timedelta(days=120) if window == 1 else start
                            )
                            future = 1 if window == 0 else 245 / 365
                            lead = 120 / 365 if window == 1 else 0
                            weight = future if window == 2 else 1
                            rate = (
                                0.5
                                * sigma
                                * sigma
                                * (
                                    1
                                    - weight * (lead + future / 3) / (lead + future / 2)
                                )
                            )
                            deviation = sigma * weight * math.sqrt(lead + future / 3)
                            expected = (
                                spot
                                * math.exp(-rate * (lead + future))
                                * math.erf(deviation / (2 * math.sqrt(2)))
                            )
                            context = PricingContext(
                                model_parameters=BlackScholesMertonParameters(
                                    risk_free_rate=rate,
                                    dividend_yield=0,
                                    volatility=sigma,
                                ),
                                spot_price=spot,
                                valuation_time=valuation,
                            )
                            option = GeometricAveragePriceOption(
                                option_type=direction,
                                strike=spot,
                                effective_date=start,
                                averaging_start_date=averaging,
                                expiry_date=end,
                                realized_average=spot if window == 2 else 0,
                            )
                            price = pricing.AnalyticGeometricAveragePriceEngine().price(
                                option, context
                            )
                            self.assertAlmostEqual(
                                price, expected, delta=max(expected * 1e-9, 1e-170)
                            )

    def test_single_fixing_geometric_asians_retain_european_time_value(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticGeometricAveragePriceEngine()
        for direction in ("call", "put"):
            for sigma in (1e-13, 0.2):
                with self.subTest(direction=direction, sigma=sigma):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0,
                            dividend_yield=0,
                            volatility=sigma,
                        ),
                        spot_price=1e14,
                        valuation_time=start,
                    )
                    terms = {
                        "option_type": direction,
                        "strike": 1e14,
                        "effective_date": start,
                        "averaging_start_date": end,
                        "expiry_date": end,
                    }
                    actual = engine.price(GeometricAveragePriceOption(**terms), context)
                    expected = 1e14 * math.erf(sigma / (2 * math.sqrt(2)))
                    self.assertAlmostEqual(
                        actual, expected, delta=max(1e-12, expected * 1e-12)
                    )
                    with self.assertRaises(InvalidInputError) as error:
                        engine.price(
                            GeometricAveragePriceOption(**terms, realized_average=1e14),
                            context,
                        )
                    self.assertIs(type(error.exception), InvalidInputError)
            expired = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.05,
                    dividend_yield=0.02,
                    volatility=0.2,
                ),
                spot_price=110,
                valuation_time=end,
            )
            settled = engine.price(
                GeometricAveragePriceOption(
                    option_type=direction,
                    strike=100,
                    effective_date=start,
                    averaging_start_date=end,
                    expiry_date=end,
                ),
                expired,
            )
            self.assertEqual(settled, 10 if direction == "call" else 0)
