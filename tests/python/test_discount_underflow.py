import math
import unittest
from datetime import date

from kiyosi import pricing
from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class DiscountUnderflowTests(unittest.TestCase):
    def test_monte_carlo_preserves_extreme_discount_scales(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.MonteCarloVanillaEngine(path_count=10_000, seed=42)

        def price(direction, amount, rate):
            option = EuropeanOption(
                option_type=direction,
                strike=amount,
                effective_date=start,
                expiry_date=end,
            )
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=rate, volatility=0.2
                ),
                spot_price=amount,
                valuation_time=start,
            )
            return engine.price(option, context)

        for direction in ("call", "put"):
            unit = price(direction, 1, 0)
            for rate in (710, 740, 750, -750):
                with self.subTest(direction=direction, rate=rate):
                    amount = 1e300 if rate > 0 else 1e-300
                    expected = unit * math.exp(math.log(amount) - rate)
                    self.assertTrue(
                        math.isclose(
                            price(direction, amount, rate), expected, rel_tol=2e-12
                        )
                    )

    def test_prices_survive_underflowing_discount_factors(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        amount = 1e300
        for rate in (710, 740, 750):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=rate, volatility=0.2
                ),
                spot_price=amount,
                valuation_time=start,
            )
            discounted = math.exp(math.log(amount) - rate)
            for direction in ("call", "put"):
                sign = 1 if direction == "call" else -1
                terms = {
                    "option_type": direction,
                    "strike": amount,
                    "effective_date": start,
                    "expiry_date": end,
                }
                cases = (
                    (
                        CashOrNothingOption(**terms, payout=amount),
                        discounted * 0.5 * math.erfc(sign * 0.1 / math.sqrt(2)),
                        (
                            pricing.AnalyticDigitalEngine(),
                            pricing.QuadratureDigitalEngine(),
                        ),
                    ),
                    (
                        AssetOrNothingOption(**terms),
                        discounted * 0.5 * math.erfc(-sign * 0.1 / math.sqrt(2)),
                        (
                            pricing.AnalyticDigitalEngine(),
                            pricing.QuadratureDigitalEngine(),
                        ),
                    ),
                    (
                        EuropeanOption(**terms),
                        discounted * math.erf(0.1 / math.sqrt(2)),
                        (
                            pricing.AnalyticVanillaEngine(),
                            pricing.QuadratureVanillaEngine(),
                        ),
                    ),
                )
                for option, expected, engines in cases:
                    for engine in engines:
                        with self.subTest(
                            rate=rate,
                            direction=direction,
                            option=type(option).__name__,
                            engine=type(engine).__name__,
                        ):
                            self.assertTrue(
                                math.isclose(
                                    engine.price(option, context),
                                    expected,
                                    rel_tol=1e-10,
                                    abs_tol=0,
                                )
                            )
