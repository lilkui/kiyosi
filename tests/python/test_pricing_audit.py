import math
import unittest
from datetime import date, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    CashOrNothingOption,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import implied_volatility


class PricingAuditTests(unittest.TestCase):
    def test_analytic_vanilla_greeks_preserve_scaled_normal_tails(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        )
        engine = pricing.AnalyticVanillaEngine()
        for direction in ("call", "put"):
            context = PricingContext(
                model_parameters=parameters, spot_price=1e-300, valuation_time=start
            )
            option = EuropeanOption(
                option_type=direction,
                strike=1e-296,
                effective_date=start,
                expiry_date=end,
            )
            expected = {
                "gamma": 6.035176823596672e-159,
                "speed": 1.3805980535242903e144,
                "color": -1.7524741795041444e-158,
                "zomma": 6.396530755190127e-157,
            }
            result = engine.price_with_greeks(option, context, greeks=list(expected))
            for name, value in expected.items():
                self.assertTrue(
                    math.isclose(result.require(name), value, rel_tol=1e-10)
                )
            context = PricingContext(
                model_parameters=parameters, spot_price=1e300, valuation_time=start
            )
            option = EuropeanOption(
                option_type=direction,
                strike=1e304,
                effective_date=start,
                expiry_date=end,
            )
            result = engine.price_with_greeks(option, context, greeks=["vega", "theta"])
            self.assertTrue(
                math.isclose(result.vega, 1.2070353647193343e-161, rel_tol=1e-10)
            )
            self.assertTrue(
                math.isclose(result.theta, -3.306946204710505e-163, rel_tol=1e-10)
            )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=2
            ),
            spot_price=1e308,
            valuation_time=start,
        )
        option = EuropeanOption(
            option_type="call", strike=1e308, effective_date=start, expiry_date=end
        )
        result = engine.price_with_greeks(option, context, greeks=["gamma"])
        self.assertTrue(
            math.isclose(result.gamma, 1.2098536225957167e-309, rel_tol=1e-10)
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=1e155
            ),
            spot_price=100,
            valuation_time=start,
        )
        option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=start + timedelta(days=1),
        )
        result = engine.price_with_greeks(option, context, all_greeks=True)
        self.assertEqual(result.price, 100)
        self.assertEqual(result.color, 0)

    def test_quadrature_retains_scaled_prices_beyond_the_former_tail_cutoff(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        )
        for direction, sign, spot, strike in (
            ("call", 1, 1e100, 1.5e101),
            ("put", -1, 1.5e101, 1e100),
        ):
            context = PricingContext(
                model_parameters=parameters, spot_price=spot, valuation_time=start
            )
            option = EuropeanOption(
                option_type=direction,
                strike=strike,
                effective_date=start,
                expiry_date=end,
            )
            value = pricing.QuadratureVanillaEngine().price(option, context)
            self.assertTrue(math.isclose(value, 2.5478923549273412e57, rel_tol=1e-7))
            for threshold in (11.0, 11.9, 11.99, 12.0, 13.6, 30.0):
                strike = spot * math.exp(sign * 0.2 * threshold - 0.02)
                digital = CashOrNothingOption(
                    option_type=direction,
                    strike=strike,
                    payout=1e100,
                    effective_date=start,
                    expiry_date=end,
                )
                value = pricing.QuadratureDigitalEngine().price(digital, context)
                expected = 1e100 * 0.5 * math.erfc(threshold / math.sqrt(2))
                self.assertTrue(math.isclose(value, expected, rel_tol=1e-8))

    def test_implied_volatility_rejects_fixed_participation_and_zero_accrual(self):
        start, fixing, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)

        def check(engine, option, context, exposed):
            quote = engine.price(option, context)
            if exposed:
                self.assertEqual(
                    implied_volatility(
                        engine,
                        option,
                        context,
                        quote,
                        lower_bound=0.05,
                        upper_bound=0.4,
                    ),
                    0.05,
                )
            else:
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    implied_volatility(
                        engine,
                        option,
                        context,
                        quote,
                        lower_bound=0.05,
                        upper_bound=0.4,
                    )
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION
                )

        for rate in (0, 0.04):
            for lower in (100, 60):
                for coupon in (0, 0.1):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate, dividend_yield=0, volatility=0.05
                        ),
                        spot_price=100,
                        valuation_time=start,
                    )
                    terms = {
                        "initial_spot": 100,
                        "knock_in_level": 80,
                        "knock_out_levels": [120, 120],
                        "upper_strike": 100,
                        "lower_strike": lower,
                        "observation_dates": [fixing, end],
                        "knock_in_observation_mode": "at_expiry",
                        "effective_date": start,
                        "expiry_date": end,
                    }
                    snowball = SnowballOption(
                        **terms,
                        knock_out_coupon_rates=[coupon, coupon],
                        maturity_coupon_rate=coupon,
                    )
                    phoenix = PhoenixOption(
                        **terms, coupon_rate=coupon, coupon_barrier_levels=[90, 90]
                    )
                    for engine, option in (
                        (
                            pricing.MonteCarloSnowballEngine(path_count=64, seed=73),
                            snowball,
                        ),
                        (pricing.FiniteDifferenceSnowballEngine(), snowball),
                        (
                            pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
                            phoenix,
                        ),
                        (pricing.FiniteDifferencePhoenixEngine(), phoenix),
                    ):
                        with self.subTest(
                            engine=type(engine).__name__,
                            rate=rate,
                            lower=lower,
                            coupon=coupon,
                        ):
                            check(
                                engine,
                                option,
                                context,
                                rate != 0 or lower != 100 or coupon != 0,
                            )
        for historical in (False, True):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=0.05
                ),
                spot_price=100 if historical else 70,
                valuation_time=fixing if historical else start,
            )
            note = SnowballOption(
                knock_out_coupon_rates=[0, 0],
                maturity_coupon_rate=0.1,
                initial_spot=100,
                knock_in_level=80,
                knock_out_levels=[120, 120],
                upper_strike=100,
                lower_strike=100,
                observation_dates=[fixing, end],
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in" if historical else "none",
                effective_date=start,
                expiry_date=end,
            )
            for engine in (
                pricing.MonteCarloSnowballEngine(path_count=64, seed=73),
                pricing.FiniteDifferenceSnowballEngine(),
            ):
                check(engine, note, context, False)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.05
            ),
            spot_price=80,
            valuation_time=start,
        )
        for knock_out in (90, 100, 120):
            for acceleration in (0, 1):
                for quantity in (0, 1):
                    option = Accumulator(
                        strike=100,
                        knock_out_level=knock_out,
                        daily_quantity=1,
                        acceleration_factor=acceleration,
                        accumulated_quantity=quantity,
                        effective_date=start,
                        expiry_date=end,
                    )
                    for engine in (
                        pricing.MonteCarloAccumulatorEngine(path_count=64, seed=73),
                        pricing.FiniteDifferenceAccumulatorEngine(),
                    ):
                        with self.subTest(
                            engine=type(engine).__name__,
                            knock_out=knock_out,
                            acceleration=acceleration,
                            quantity=quantity,
                        ):
                            check(
                                engine,
                                option,
                                context,
                                quantity != 0 or acceleration != 0 or knock_out > 100,
                            )
