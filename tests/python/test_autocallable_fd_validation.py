"""Migrated autocallable engine validation precedence from the legacy suite."""

import unittest
from datetime import date

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import AutocallableObservation, BinarySnowballOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class AutocallableValidationTests(unittest.TestCase):
    def test_autocallable_engines_share_domain_validation_precedence(self):
        start, end = date(2025, 1, 1), date(2025, 1, 4)
        for history in (None, "none"):
            note = BinarySnowballOption(
                maturity_coupon_rate=0.05,
                barrier_state=history,
                effective_date=start,
                expiry_date=end,
                observations=[
                    AutocallableObservation(date(2025, 1, 2), 110, coupon_rate=0.08)
                ],
            )
            for valuation, expected in (
                (start, kiyosi.ErrorCategory.INVALID_DATE),
                (
                    date(2025, 1, 3),
                    kiyosi.ErrorCategory.INVALID_PARAMETER
                    if history is None
                    else kiyosi.ErrorCategory.INVALID_DATE,
                ),
                (date(2025, 1, 5), kiyosi.ErrorCategory.INVALID_TIME_RANGE),
            ):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.03,
                        dividend_yield=0,
                        volatility=0.2,
                    ),
                    spot_price=100,
                    valuation_time=valuation,
                )
                for engine in (
                    pricing.FiniteDifferenceAutocallableEngine(),
                    pricing.MonteCarloAutocallableEngine(path_count=2, seed=7),
                ):
                    for operation in (
                        lambda engine=engine, note=note, context=context: engine.price(
                            note, context
                        ),
                        lambda engine=engine, note=note, context=context: (
                            engine.price_with_greeks(note, context, "delta")
                        ),
                        lambda engine=engine, note=note, context=context: (
                            pricing.implied_volatility(engine, note, context, 1)
                        ),
                    ):
                        with self.subTest(
                            history=history, valuation=valuation, engine=type(engine)
                        ):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                operation()
                            self.assertEqual(error.exception.category, expected)
