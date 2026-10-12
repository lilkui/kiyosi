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
from kiyosi.market import BlackScholesMertonParameters, PricingContext, all_days_calendar


class DiscountUnderflowTests(unittest.TestCase):
    def test_accumulator_monte_carlo_preserves_discounted_quantity_products(self):
        start = date(2025, 1, 1)
        backends = ["cpu"] + (
            ["cuda"] if os.environ.get("KIYOSI_TEST_CUDA") == "enabled" else []
        )
        for backend in backends:
            for spot, quantity, rate in (
                (100, 1e308, 2000),
                (1e-200, 1e-200, -335800),
                (1e-160, 1e-160, -255500),
            ):
                for early in (False, True):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate,
                            dividend_yield=rate - (365 if early else 0),
                            volatility=1e-20,
                        ),
                        spot_price=spot, valuation_time=start,
                        calendar=all_days_calendar(),
                    )
                    terminal_spot = spot * math.exp(1 if early else 0)
                    engine = pricing.MonteCarloAccumulatorEngine(
                        path_count=10, seed=42, backend=backend
                    )
                    for strike_ratio in (0.5, 1, 4):
                        strike = spot * strike_ratio
                        option = Accumulator(
                            strike=strike,
                            knock_out_level=spot * (2 if early else 10),
                            daily_quantity=0, acceleration_factor=1,
                            accumulated_quantity=quantity, effective_date=start,
                            expiry_date=date(2025, 1, 3 if early else 2),
                        )
                        difference = terminal_spot - strike
                        expected = math.copysign(math.exp(
                            math.log(quantity) + math.log(abs(difference)) - rate / 365
                        ), difference) if difference else 0
                        with self.subTest(
                            backend=backend, spot=spot, rate=rate,
                            early=early, strike_ratio=strike_ratio,
                        ):
                            price = engine.price(option, context)
                            self.assertTrue(math.isclose(price, expected, rel_tol=2e-12))
                            for greek in ("delta", "rho"):
                                result = engine.price_with_greeks(
                                    option, context, greek,
                                    spot_shift=spot / 16, rate_shift=0.1,
                                )
                                self.assertEqual(result.price, price)
                                expected_greek = (
                                    math.exp(math.log(quantity) + (1 if early else 0) - rate / 365)
                                    if greek == "delta" else
                                    math.exp(math.log(quantity) + math.log(strike) - rate / 365) / 36500
                                )
                                self.assertTrue(math.isclose(
                                    getattr(result, greek), expected_greek, rel_tol=2e-6
                                ))

    def test_accumulator_monte_carlo_rejects_unrepresentable_discounted_products(self):
        start, end = date(2025, 1, 1), date(2025, 1, 2)
        option = Accumulator(
            strike=1, knock_out_level=1000, daily_quantity=0,
            acceleration_factor=1, accumulated_quantity=1e308,
            effective_date=start, expiry_date=end,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=365, dividend_yield=365, volatility=1e-20
            ),
            spot_price=100, valuation_time=start, calendar=all_days_calendar(),
        )
        with self.assertRaises(KiyosiError) as error:
            pricing.MonteCarloAccumulatorEngine(path_count=10, seed=42).price(option, context)
        self.assertEqual(error.exception.category, ErrorCategory.INVALID_RESULT)

    def test_analytic_delta_and_charm_retain_scaled_normal_tails(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        # Independent high-precision normal-tail references at r = q = -710.
        references = (
            (
                "call", 8, 4.448031813551352e-40,
                -1.8407501816330744e-39, 8.923870780328184e-38,
            ),
            (
                "put", -8, -1.4847155549977004e-43,
                6.144248777637284e-43, -2.9638565030696756e-41,
            ),
        )
        for direction, log_strike, delta, charm, digital_delta in references:
            for rate in (-710, -700):
                for spot in (1, 1e-200):
                    with self.subTest(direction=direction, rate=rate, spot=spot):
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=rate, dividend_yield=rate, volatility=0.2
                            ),
                            spot_price=spot,
                            valuation_time=start,
                        )
                        terms = dict(
                            option_type=direction,
                            strike=spot * math.exp(log_strike),
                            effective_date=start,
                            expiry_date=end,
                        )
                        scale = math.exp(-rate - 710)
                        option = EuropeanOption(**terms)
                        engine = pricing.AnalyticVanillaEngine()
                        for request in ("delta", ["delta", "charm"]):
                            result = engine.price_with_greeks(option, context, request)
                            self.assertTrue(
                                math.isclose(result.delta, delta * scale, rel_tol=1e-10)
                            )
                            self.assertEqual(result.price, engine.price(option, context))
                            if "charm" in request:
                                expected = (charm + (rate + 710) * delta / 365) * scale
                                self.assertTrue(
                                    math.isclose(result.charm, expected, rel_tol=1e-10)
                                )
                        option = AssetOrNothingOption(**terms)
                        engine = pricing.AnalyticDigitalEngine()
                        result = engine.price_with_greeks(option, context, "delta")
                        self.assertTrue(
                            math.isclose(result.delta, digital_delta * scale, rel_tol=1e-10)
                        )
                        self.assertEqual(result.price, engine.price(option, context))

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
