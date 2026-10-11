import math
import os
import unittest
from datetime import date

from kiyosi import ErrorCategory, KiyosiError, pricing
from kiyosi.instruments import (
    Accumulator,
    AssetOrNothingOption,
    BarrierOption,
    BinarySnowballOption,
    CashOrNothingOption,
    EuropeanOption,
    PhoenixOption,
    cash_one_touch_up,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class DiscountUnderflowTests(unittest.TestCase):
    def test_trading_day_monte_carlo_preserves_signed_discounted_payments(self):
        start, middle, end = date(2025, 1, 1), date(2026, 1, 1), date(2027, 1, 1)
        backends = ["cpu"] + (
            ["cuda"] if os.environ.get("KIYOSI_TEST_CUDA") == "enabled" else []
        )
        for backend in backends:
            for rate in (740, 750, -750):
                amount = 1e300 if rate > 0 else 1e-300
                discounted = math.exp(math.log(amount) - rate)
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=rate, dividend_yield=rate, volatility=1e-20
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                for early in (False, True):
                    for sign in (-1, 0, 1):
                        note = BinarySnowballOption(
                            knock_out_coupon_rates=(
                                [(sign - 1) * amount, 0] if early else [0]
                            ),
                            maturity_coupon_rate=0 if early else (sign - 1) * amount,
                            knock_out_levels=[80, 80] if early else [200],
                            observation_dates=[middle, end] if early else [middle],
                            principal_ratio=amount,
                            effective_date=start,
                            expiry_date=end if early else middle,
                        )
                        engine = pricing.MonteCarloBinarySnowballEngine(
                            path_count=10, seed=42, backend=backend
                        )
                        with self.subTest(
                            backend=backend, rate=rate, early=early, sign=sign
                        ):
                            self.assertTrue(
                                math.isclose(
                                    engine.price(note, context),
                                    sign * discounted,
                                    rel_tol=2e-12,
                                )
                            )
                for strike in (90, 110):
                    option = Accumulator(
                        strike=strike,
                        knock_out_level=120,
                        daily_quantity=0,
                        acceleration_factor=1,
                        accumulated_quantity=amount,
                        effective_date=start,
                        expiry_date=middle,
                    )
                    engine = pricing.MonteCarloAccumulatorEngine(
                        path_count=10, seed=42, backend=backend
                    )
                    with self.subTest(backend=backend, rate=rate, strike=strike):
                        price = engine.price(option, context)
                        self.assertTrue(
                            math.isclose(
                                price, (100 - strike) * discounted, rel_tol=2e-12
                            )
                        )
                        result = engine.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        self.assertEqual(result.price, price)
                        self.assertTrue(
                            math.isclose(result.delta, discounted, rel_tol=1e-8)
                        )
                if rate > 0:
                    phoenix = PhoenixOption(
                        coupon_rate=amount,
                        initial_spot=100,
                        knock_in_level=80,
                        knock_out_levels=[200, 200],
                        coupon_barrier_levels=[0, 0],
                        upper_strike=100,
                        lower_strike=100,
                        observation_dates=[middle, end],
                        knock_in_observation_mode="every_trading_day",
                        principal_ratio=0,
                        effective_date=start,
                        expiry_date=end,
                    )
                    engine = pricing.MonteCarloPhoenixEngine(
                        path_count=10, seed=42, backend=backend
                    )
                    with self.subTest(backend=backend, rate=rate, instrument="phoenix"):
                        self.assertTrue(
                            math.isclose(
                                engine.price(phoenix, context),
                                discounted,
                                rel_tol=2e-12,
                            )
                        )

    def test_trading_day_monte_carlo_rejects_unrepresentable_discounted_payments(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = BinarySnowballOption(
            knock_out_coupon_rates=[0],
            maturity_coupon_rate=0,
            knock_out_levels=[200],
            observation_dates=[end],
            effective_date=start,
            expiry_date=end,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=-750, dividend_yield=-750, volatility=1e-20
            ),
            spot_price=100,
            valuation_time=start,
        )
        with self.assertRaises(KiyosiError) as error:
            pricing.MonteCarloBinarySnowballEngine(path_count=10, seed=42).price(
                note, context
            )
        self.assertEqual(error.exception.category, ErrorCategory.INVALID_RESULT)

    def test_resolved_barrier_settlements_preserve_extreme_discount_scales(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for rate in (740, 750, -750):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=rate, volatility=0.2
                ),
                spot_price=150,
                valuation_time=start,
            )
            for amount in (0, 1e300 if rate > 0 else 1e-300):
                terms = {
                    "effective_date": start,
                    "expiry_date": end,
                    "barrier_level": 120,
                }
                barrier = BarrierOption(
                    **terms,
                    option_type="call",
                    strike=100,
                    barrier_type="up_and_out",
                    rebate=amount,
                )
                expected = math.exp(math.log(amount) - rate) if amount else 0
                cases = [
                    (pricing.AnalyticBarrierEngine(), barrier),
                    (pricing.FiniteDifferenceBarrierEngine(), barrier),
                ]
                if amount:
                    cases.append(
                        (
                            pricing.AnalyticBinaryBarrierEngine(),
                            cash_one_touch_up(
                                **terms, payout=amount, settlement_timing="at_expiry"
                            ),
                        )
                    )
                for engine, option in cases:
                    with self.subTest(
                        rate=rate, amount=amount, engine=type(engine).__name__
                    ):
                        self.assertTrue(
                            math.isclose(
                                engine.price(option, context), expected, rel_tol=2e-12
                            )
                        )

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
