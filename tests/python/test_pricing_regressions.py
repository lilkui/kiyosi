import unittest
from datetime import date

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import AmericanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class PricingRegressionTests(unittest.TestCase):
    def test_bjerksund_retains_small_volatility_early_exercise_value(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        engine = pricing.BjerksundStenslandVanillaEngine()
        for direction in ("call", "put"):
            call = direction == "call"
            option = AmericanOption(
                option_type=direction,
                strike=100 if call else 150,
                effective_date=start,
                expiry_date=end,
            )
            for sigma in (1e-6, 1e-10, 1e-20, 1e-100, 1e-150, 1e-200, 1e200):
                with self.subTest(direction=direction, sigma=sigma):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.1 if call else 0.05,
                            dividend_yield=0.05 if call else 0.1,
                            volatility=sigma,
                        ),
                        spot_price=150 if call else 100,
                        valuation_time=start,
                    )
                    if sigma in (1e-200, 1e200):
                        for method, extra in (
                            (engine.price, ()),
                            (engine.price_with_greeks, ("delta",)),
                        ):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                method(option, context, *extra)
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.INVALID_RESULT,
                            )
                    else:
                        actual = engine.price(option, context)
                        self.assertAlmostEqual(actual, 56.25, delta=1e-8)
                        self.assertEqual(
                            engine.price_with_greeks(option, context, "delta").price,
                            actual,
                        )

    def test_american_fd_domains_preserve_long_expiry_exercise_value(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0.05, volatility=1.0
            ),
            spot_price=100,
            valuation_time=start,
        )
        option = AmericanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        # QuantLib 1.43 FD, 3200 time/asset steps and 2 damping steps.
        reference = 69.73204196706111
        coarse = pricing.FiniteDifferenceVanillaEngine().price(option, context)
        fine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=800, time_step_count=1600
        )
        refined = fine.price(option, context)
        self.assertAlmostEqual(coarse, reference, delta=0.2)
        self.assertAlmostEqual(refined, reference, delta=0.03)
        self.assertLess(abs(refined - reference), abs(coarse - reference))
        joint = fine.price_with_greeks(option, context, "vega")
        self.assertEqual(joint.price, refined)
        self.assertIsNotNone(joint.vega)
