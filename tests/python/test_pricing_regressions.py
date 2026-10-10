import math
import unittest
from datetime import date

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    BarrierOption,
    EuropeanOption,
    SnowballOption,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class PricingRegressionTests(unittest.TestCase):
    def test_zero_exposure_accumulators_skip_numerical_work_after_validation(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=0,
            acceleration_factor=2,
            accumulated_quantity=0,
            effective_date=start,
            expiry_date=end,
        )
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        )
        context = PricingContext(
            model_parameters=parameters, spot_price=90, valuation_time=start
        )
        for engine in (
            pricing.MonteCarloAccumulatorEngine(path_count=2000, seed=42),
            pricing.MonteCarloAccumulatorEngine(
                path_count=2000, seed=42, backend="cuda"
            ),
            pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=1),
        ):
            with self.subTest(engine=type(engine).__name__):
                self.assertEqual(engine.price(option, context), 0)
                result = engine.price_with_greeks(
                    option, context, ["delta", "gamma", "vega", "rho"]
                )
                self.assertEqual(
                    (result.price, result.delta, result.gamma, result.vega, result.rho),
                    (0, 0, 0, 0, 0),
                )
                expired = PricingContext(
                    model_parameters=parameters,
                    spot_price=90,
                    valuation_time=date(2026, 1, 2),
                )
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(option, expired)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE
                )
        for engine in (
            pricing.MonteCarloAccumulatorEngine(path_count=0),
            pricing.FiniteDifferenceAccumulatorEngine(time_step_count=0),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine.price(option, context)
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )

    def test_structured_finite_difference_spot_greeks_reuse_native_layers(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=70,
            valuation_time=date(2025, 1, 2),
        )
        accumulator = Accumulator(
            strike=50,
            knock_out_level=200,
            daily_quantity=0,
            acceleration_factor=1,
            accumulated_quantity=3,
            effective_date=start,
            expiry_date=end,
        )
        note = SnowballOption(
            knock_out_coupon_rates=[0],
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=75,
            knock_out_levels=[200],
            upper_strike=100,
            lower_strike=0,
            observation_dates=[end],
            knock_in_observation_mode="every_trading_day",
            barrier_state="knocked_in",
            effective_date=start,
            expiry_date=end,
        )
        put = EuropeanOption(
            option_type="put", strike=100, effective_date=start, expiry_date=end
        )
        expected = pricing.AnalyticVanillaEngine().price_with_greeks(
            put, context, ["delta", "gamma"]
        )
        time = (end - date(2025, 1, 2)).days / 365
        for engine, option, delta, gamma in (
            (
                pricing.FiniteDifferenceAccumulatorEngine(),
                accumulator,
                3 * math.exp(-0.01 * time),
                0,
            ),
            (
                pricing.FiniteDifferenceSnowballEngine(
                    asset_step_count=800, asset_upper_boundary=800
                ),
                note,
                -expected.delta / 100,
                -expected.gamma / 100,
            ),
        ):
            with self.subTest(engine=type(engine).__name__):
                result = engine.price_with_greeks(option, context, ["delta", "gamma"])
                shifted = engine.price_with_greeks(
                    option, context, ["delta", "gamma"], spot_shift=0.1
                )
                self.assertEqual(result.price, engine.price(option, context))
                self.assertEqual(
                    (result.delta, result.gamma), (shifted.delta, shifted.gamma)
                )
                self.assertAlmostEqual(result.delta, delta, delta=1e-5)
                self.assertAlmostEqual(result.gamma, gamma, delta=1e-6)
                self.assertIsNone(result.vega)

    def test_analytic_barriers_retain_tiny_positive_volatility_time_value(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticBarrierEngine()
        for kind in ("down_and_in", "down_and_out", "up_and_in", "up_and_out"):
            for direction in ("call", "put"):
                for spot in (1e16, 1e150):
                    for sigma in (1e-12, 1e-16, 1e-20, 1e-200):
                        with self.subTest(
                            kind=kind, direction=direction, spot=spot, sigma=sigma
                        ):
                            context = PricingContext(
                                model_parameters=BlackScholesMertonParameters(
                                    risk_free_rate=0, dividend_yield=0, volatility=sigma
                                ),
                                spot_price=spot,
                                valuation_time=start,
                            )
                            option = BarrierOption(
                                option_type=direction,
                                strike=spot,
                                effective_date=start,
                                expiry_date=end,
                                barrier_level=(1.2 if kind.startswith("up") else 0.8)
                                * spot,
                                barrier_type=kind,
                            )
                            actual = engine.price(option, context)
                            expected = (
                                0
                                if kind.endswith("in")
                                else spot * sigma / math.sqrt(2 * math.pi)
                            )
                            self.assertTrue(
                                math.isclose(actual, expected, rel_tol=1e-12)
                            )
                            self.assertEqual(
                                engine.price_with_greeks(
                                    option, context, "delta"
                                ).price,
                                actual,
                            )

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
