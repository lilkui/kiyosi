import math
import unittest
from datetime import date

from kiyosi import ErrorCategory, KiyosiError, pricing
from kiyosi.instruments import (
    AmericanOption,
    AssetOrNothingOption,
    BarrierOption,
    CashOrNothingOption,
    EuropeanOption,
    asset_binary_barrier_option,
    cash_binary_barrier_option,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class AuditRegressionTests(unittest.TestCase):
    def test_binomial_implied_volatility_respects_the_tree_domain(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        for rate in (-0.01, 0.02, 0.05):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate,
                    dividend_yield=0.02,
                    volatility=0.2,
                ),
                spot_price=100,
                valuation_time=start,
            )
            for engine in (
                pricing.CoxRossRubinsteinVanillaEngine(1),
                pricing.CoxRossRubinsteinVanillaEngine(),
            ):
                for direction in ("call", "put"):
                    for option_type in (EuropeanOption, AmericanOption):
                        with self.subTest(
                            rate=rate,
                            steps=engine.step_count,
                            direction=direction,
                            option=option_type.__name__,
                        ):
                            option = option_type(
                                option_type=direction,
                                strike=100,
                                effective_date=start,
                                expiry_date=end,
                            )
                            quote = engine.price(option, context)
                            self.assertAlmostEqual(
                                pricing.implied_volatility(
                                    engine, option, context, quote
                                ),
                                0.2,
                                delta=1e-7,
                            )
                            with self.assertRaises(KiyosiError) as excluded:
                                pricing.implied_volatility(
                                    engine,
                                    option,
                                    context,
                                    quote,
                                    lower_bound=0.3,
                                    upper_bound=0.4,
                                )
                            self.assertEqual(
                                excluded.exception.category,
                                ErrorCategory.UNBRACKETED_VOLATILITY,
                            )
                            if rate != 0.02:
                                with self.assertRaises(KiyosiError) as infeasible:
                                    pricing.implied_volatility(
                                        engine,
                                        option,
                                        context,
                                        quote,
                                        upper_bound=0.001,
                                    )
                                self.assertEqual(
                                    infeasible.exception.category,
                                    ErrorCategory.UNBRACKETED_VOLATILITY,
                                )
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        for steps in (0, 1000001):
            with self.assertRaises(KiyosiError) as invalid:
                pricing.implied_volatility(
                    pricing.CoxRossRubinsteinVanillaEngine(steps), option, context, 10
                )
            self.assertEqual(
                invalid.exception.category, ErrorCategory.INVALID_PARAMETER
            )
        collapsed_context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=1e-20
            ),
            spot_price=100,
            valuation_time=start,
        )
        with self.assertRaises(KiyosiError) as collapsed:
            pricing.CoxRossRubinsteinVanillaEngine().price(option, collapsed_context)
        self.assertEqual(collapsed.exception.category, ErrorCategory.INVALID_RESULT)

    def test_barrier_prices_retain_scaled_normal_tails(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        )
        for spot in (1e200, 1e300):
            for multiple in (1000, 1500):
                barrier = 0.8 * spot
                terms = {
                    "option_type": "call",
                    "strike": multiple * spot,
                    "effective_date": start,
                    "expiry_date": end,
                }
                barrier_terms = {
                    **terms,
                    "barrier_level": barrier,
                    "barrier_type": "down_and_in",
                }
                context = PricingContext(
                    model_parameters=parameters,
                    spot_price=spot,
                    valuation_time=start,
                )
                reflected = PricingContext(
                    model_parameters=parameters,
                    spot_price=barrier * (barrier / spot),
                    valuation_time=start,
                )
                for engine, option, vanilla_engine, vanilla in (
                    (
                        pricing.AnalyticBarrierEngine(),
                        BarrierOption(**barrier_terms),
                        pricing.AnalyticVanillaEngine(),
                        EuropeanOption(**terms),
                    ),
                    (
                        pricing.AnalyticBinaryBarrierEngine(),
                        asset_binary_barrier_option(**barrier_terms),
                        pricing.AnalyticDigitalEngine(),
                        AssetOrNothingOption(**terms),
                    ),
                    (
                        pricing.AnalyticBinaryBarrierEngine(),
                        cash_binary_barrier_option(**barrier_terms, payout=spot),
                        pricing.AnalyticDigitalEngine(),
                        CashOrNothingOption(**terms, payout=spot),
                    ),
                ):
                    with self.subTest(
                        spot=spot, multiple=multiple, option=type(option).__name__
                    ):
                        expected = (spot / barrier) * vanilla_engine.price(
                            vanilla, reflected
                        )
                        actual = engine.price(option, context)
                        self.assertGreater(expected, 0)
                        self.assertTrue(math.isclose(actual, expected, rel_tol=1e-8))
                        self.assertEqual(
                            engine.price_with_greeks(option, context, "delta").price,
                            actual,
                        )

    def test_numerical_greeks_use_representable_symmetric_bumps(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0,
            dividend_yield=0,
            volatility=0.01,
        )
        for spot in (100, 1e12, 1e13, 1e14):
            with self.subTest(spot=spot):
                option = EuropeanOption(
                    option_type="call",
                    strike=spot * 0.01,
                    effective_date=start,
                    expiry_date=end,
                )
                context = PricingContext(
                    model_parameters=parameters,
                    spot_price=spot,
                    valuation_time=start,
                )
                for result in (
                    pricing.calculate_numerical_greeks(
                        pricing.AnalyticVanillaEngine(), option, context
                    ),
                    pricing.QuadratureVanillaEngine().price_with_greeks(
                        option,
                        context,
                        ["delta", "gamma", "speed"],
                    ),
                ):
                    self.assertAlmostEqual(result.delta, 1, delta=1e-12)
                    self.assertEqual(result.gamma, 0)
                    self.assertAlmostEqual(result.speed, 0, delta=1e-6)
                if spot == 1e14:
                    collapsed = pricing.calculate_numerical_greeks(
                        pricing.AnalyticVanillaEngine(),
                        option,
                        context,
                        spot_shift=0.004,
                    )
                    self.assertIsNone(collapsed.delta)
                    self.assertIsNone(collapsed.gamma)
                    self.assertIsNone(collapsed.speed)
                    self.assertEqual(collapsed.vega, 0)
