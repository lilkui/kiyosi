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
    BinarySnowballOption,
    SnowballOption,
    asset_binary_barrier_option,
    cash_binary_barrier_option,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext


class AuditRegressionTests(unittest.TestCase):
    def test_resolved_barriers_retain_time_greeks_across_inactive_observations(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
            ),
            spot_price=100,
            valuation_time=date(2025, 7, 1),
        )
        terms = dict(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        expected = pricing.calculate_numerical_greeks(
            pricing.AnalyticVanillaEngine(), EuropeanOption(**terms), context
        )
        for state in ("touched", "untouched"):
            option = BarrierOption(
                **terms,
                barrier_level=80,
                barrier_type="down_and_in",
                observation_mode="scheduled",
                observation_dates=[date(2025, 3, 3), date(2025, 7, 2), end],
                touch_state=state,
            )
            for engine in (
                pricing.AnalyticBarrierEngine(),
                pricing.FiniteDifferenceBarrierEngine(),
            ):
                with self.subTest(state=state, engine=type(engine).__name__):
                    result = engine.price_with_greeks(
                        option, context, ["theta", "charm", "color"]
                    )
                    for name in ("theta", "charm", "color"):
                        if state == "touched":
                            self.assertEqual(result.price, expected.price)
                            self.assertAlmostEqual(
                                getattr(result, name),
                                getattr(expected, name),
                                delta=1e-10,
                            )
                        else:
                            self.assertIsNone(getattr(result, name))

    def test_knocked_out_autocallables_retain_zero_time_greeks_after_valid_history(
        self,
    ):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        terms = dict(
            knock_out_coupon_rates=[0.08] * 3,
            maturity_coupon_rate=0.06,
            knock_out_levels=[110] * 3,
            observation_dates=[date(2025, 3, 3), date(2025, 7, 2), end],
            barrier_state="knocked_out",
            effective_date=start,
            expiry_date=end,
        )
        binary = BinarySnowballOption(**terms)
        snowball = SnowballOption(
            **terms,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=60,
            knock_in_observation_mode="every_trading_day",
        )
        for option, engines in (
            (
                binary,
                (
                    pricing.FiniteDifferenceAutocallableEngine(),
                    pricing.MonteCarloAutocallableEngine(path_count=32, seed=42),
                ),
            ),
            (
                snowball,
                (
                    pricing.FiniteDifferenceAutocallableEngine(),
                    pricing.MonteCarloAutocallableEngine(path_count=32, seed=42),
                ),
            ),
        ):
            for valuation in (date(2025, 3, 4), date(2025, 7, 1)):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
                    ),
                    spot_price=100,
                    valuation_time=valuation,
                )
                for engine in engines:
                    with self.subTest(
                        option=type(option).__name__,
                        engine=type(engine).__name__,
                        valuation=valuation,
                    ):
                        result = engine.price_with_greeks(
                            option, context, ["theta", "charm", "color"]
                        )
                        self.assertEqual(result.price, 0)
                        for name in ("theta", "charm", "color"):
                            if valuation == date(2025, 7, 1):
                                self.assertEqual(getattr(result, name), 0)
                            else:
                                self.assertIsNone(getattr(result, name))

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
