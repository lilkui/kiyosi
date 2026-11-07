"""Autocallable finite-difference engine validation precedence."""

import unittest
from datetime import datetime

from kiyosi import InvalidInputError, pricing
from kiyosi.instruments import AutocallableObservation, BinarySnowballOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from support.history import dated as dated_history


class AutocallableValidationTests(unittest.TestCase):
    def test_autocallable_engines_share_domain_validation_precedence(self):
        start, end = datetime(2025, 1, 1), datetime(2025, 1, 4)
        for history in (None, "no_event"):
            fields = {
                "maturity_coupon_rate": 0.05,
                "barrier_state": history,
                "effective_time": start,
                "observations": [
                    AutocallableObservation.snowball(
                        datetime(2025, 1, 2), 110, coupon_rate=0.08
                    )
                ],
            }
            with self.assertRaises(InvalidInputError):
                BinarySnowballOption(
                    **(
                        fields
                        | {
                            "observations": [
                                AutocallableObservation.snowball(
                                    end, 110, coupon_rate=0
                                )
                            ]
                        }
                    ),
                    expiry_time=end,
                )
            note = BinarySnowballOption(**fields, expiry_time=datetime(2025, 1, 3))
            valuations = [datetime(2024, 12, 31), datetime(2025, 1, 4)]
            if history is None:
                valuations.append(datetime(2025, 1, 3))
            for valuation in valuations:
                expected = InvalidInputError
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
                            dated_history(note, context), context
                        ),
                        lambda engine=engine, note=note, context=context: (
                            engine.price_with_greeks(
                                dated_history(note, context), context, "delta"
                            )
                        ),
                        lambda engine=engine, note=note, context=context: (
                            pricing.implied_volatility(
                                engine, dated_history(note, context), context, 1
                            )
                        ),
                    ):
                        with self.subTest(
                            history=history, valuation=valuation, engine=type(engine)
                        ):
                            with self.assertRaises(expected) as error:
                                operation()
                            self.assertIs(type(error.exception), expected)
