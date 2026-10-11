import unittest
from datetime import date

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    CashOrNothingOption,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)


class AutocallableRegressionTests(unittest.TestCase):
    def test_fd_event_skipping_preserves_path_dependent_prices_and_greeks(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        # Prices and sensitivities recorded before skipping non-event node scans.
        expected = iter(
            (
                (0.9851073493143833, 0.0033442626878479875, -0.0003388868715960558),
                (0.985121983280094, 0.0033434330011221924, -0.0003390186073992385),
                (0.985092706060584, 0.003345091076280171, -0.00033875459128074326),
                (0.9741963514308023, 0.004327134341220007, -0.00039946361635084804),
                (0.9742415857695996, 0.004324391125183735, -0.00039959520738663823),
                (0.9741507369014278, 0.004329894964145986, -0.00039933080007825694),
            )
        )
        for mode in ("at_expiry", "every_trading_day"):
            note = PhoenixOption(
                coupon_rate=0.08,
                initial_spot=100,
                knock_in_level=80,
                knock_out_levels=[110, 108, 106, 104],
                coupon_barrier_levels=[90] * 4,
                upper_strike=100,
                lower_strike=60,
                observation_dates=[
                    date(2025, 4, 1),
                    date(2025, 7, 1),
                    date(2025, 10, 1),
                    end,
                ],
                knock_in_observation_mode=mode,
                effective_date=start,
                expiry_date=end,
            )
            for scheme in ("crank_nicolson", "implicit_euler", "explicit_euler"):
                with self.subTest(mode=mode, scheme=scheme):
                    result = pricing.FiniteDifferencePhoenixEngine(
                        asset_step_count=200,
                        time_step_count=2000,
                        scheme=scheme,
                        asset_upper_boundary=400,
                    ).price_with_greeks(note, context, ["delta", "gamma"])
                    for actual, value in zip(
                        (result.price, result.delta, result.gamma),
                        next(expected),
                        strict=True,
                    ):
                        self.assertAlmostEqual(actual, value, delta=1e-12)

    def test_expiry_only_knock_in_matches_analytic_terminal_portfolios(self):
        import math

        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        common = {
            "knock_in_level": 100,
            "knock_out_levels": [110],
            "observation_dates": [end],
            "knock_in_observation_mode": "at_expiry",
            "effective_date": start,
            "expiry_date": end,
        }
        snowball = SnowballOption(
            **common,
            knock_out_coupon_rates=[0.06],
            maturity_coupon_rate=0.06,
            initial_spot=100,
            upper_strike=100,
            lower_strike=60,
        )
        phoenix = PhoenixOption(
            **common,
            coupon_rate=0.08,
            coupon_barrier_levels=[0],
            initial_spot=100,
            upper_strike=100,
            lower_strike=60,
        )
        ternary = TernarySnowballOption(
            **common,
            knock_out_coupon_rates=[0.06],
            maturity_coupon_rate=0.06,
            minimum_coupon_rate=0.02,
        )
        put_prices = [
            pricing.AnalyticVanillaEngine().price(
                EuropeanOption(
                    option_type="put",
                    strike=strike,
                    effective_date=start,
                    expiry_date=end,
                ),
                context,
            )
            for strike in (100, 60)
        ]
        discount = math.exp(-0.04)
        downside = discount - (put_prices[0] - put_prices[1]) / 100
        digital = pricing.AnalyticDigitalEngine().price(
            CashOrNothingOption(
                option_type="call",
                strike=100,
                payout=1,
                effective_date=start,
                expiry_date=end,
            ),
            context,
        )
        cases = (
            (
                snowball,
                pricing.FiniteDifferenceSnowballEngine,
                downside + 0.06 * digital,
            ),
            (
                phoenix,
                pricing.FiniteDifferencePhoenixEngine,
                downside + 0.08 * discount,
            ),
            (
                ternary,
                pricing.FiniteDifferenceTernarySnowballEngine,
                1.02 * discount + 0.04 * digital,
            ),
        )
        for scheme in ("crank_nicolson", "implicit_euler", "explicit_euler"):
            for note, engine_type, expected in cases:
                with self.subTest(scheme=scheme, note=type(note).__name__):
                    actual = engine_type(
                        asset_step_count=200,
                        time_step_count=2000,
                        scheme=scheme,
                        asset_upper_boundary=400,
                    ).price(note, context)
                    self.assertAlmostEqual(actual, expected, delta=0.002)

    def test_protected_snowball_uses_observation_paths_with_stochastic_knock_out(self):
        start, middle, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        terms = {
            "knock_out_coupon_rates": [0.08, 0.08],
            "maturity_coupon_rate": 0,
            "initial_spot": 100,
            "knock_in_level": 75,
            "knock_out_levels": [110, 104],
            "upper_strike": 100,
            "lower_strike": 100,
            "observation_dates": [middle, end],
            "effective_date": start,
            "expiry_date": end,
        }
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.MonteCarloSnowballEngine(path_count=2_000, seed=42)
        self.assertEqual(
            engine.price(
                SnowballOption(**terms, knock_in_observation_mode="every_trading_day"),
                context,
            ),
            engine.price(
                SnowballOption(**terms, knock_in_observation_mode="at_expiry"), context
            ),
        )

    def test_fixed_autocallable_prices_skip_numerical_work_after_validation(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = SnowballOption(
            knock_out_coupon_rates=[0],
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=75,
            knock_out_levels=[110],
            upper_strike=100,
            lower_strike=100,
            observation_dates=[end],
            effective_date=start,
            expiry_date=end,
            knock_in_observation_mode="every_trading_day",
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                self.assertEqual(
                    pricing.MonteCarloSnowballEngine(
                        path_count=20_000, seed=42, backend=backend
                    ).price(note, context),
                    1,
                )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            pricing.MonteCarloSnowballEngine(path_count=0).price(note, context)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        engine = pricing.FiniteDifferenceSnowballEngine(asset_upper_boundary=1)
        result = engine.price_with_greeks(note, context, ["delta", "gamma"])
        self.assertEqual((result.price, result.delta, result.gamma), (1, 0, 0))
        with self.assertRaises(kiyosi.KiyosiError) as error:
            pricing.FiniteDifferenceSnowballEngine(time_step_count=0).price(
                note, context
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )

    def test_fixed_finite_difference_autocallables_retain_rate_sensitivity(self):
        import math

        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = BinarySnowballOption(
            knock_out_coupon_rates=[0],
            maturity_coupon_rate=0,
            knock_out_levels=[110],
            observation_dates=[end],
            effective_date=start,
            expiry_date=end,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        result = pricing.FiniteDifferenceBinarySnowballEngine().price_with_greeks(
            note, context, ["delta", "gamma", "vega", "rho"]
        )
        self.assertEqual(result.price, math.exp(-0.04))
        self.assertEqual((result.delta, result.gamma, result.vega), (0, 0, 0))
        self.assertAlmostEqual(result.rho, -math.exp(-0.04) / 100, delta=1e-8)

    def test_fixed_implied_volatility_preserves_non_finite_pricing_errors(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = BinarySnowballOption(
            knock_out_coupon_rates=[1e308],
            maturity_coupon_rate=1e308,
            knock_out_levels=[110],
            observation_dates=[end],
            principal_ratio=1e308,
            effective_date=start,
            expiry_date=end,
        )
        for valuation in (start, end):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=0.2
                ),
                spot_price=100,
                valuation_time=valuation,
            )
            with self.subTest(valuation=valuation):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    pricing.implied_volatility(
                        pricing.MonteCarloBinarySnowballEngine(path_count=2, seed=1),
                        note,
                        context,
                        1,
                    )
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT
                )

    def test_event_only_monte_carlo_ignores_unobserved_trading_days(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0, volatility=0.3
        )
        for observations in ([date(2025, 7, 1)], [date(2025, 7, 1), end]):
            terms = {
                "knock_out_coupon_rates": [0.08] * len(observations),
                "maturity_coupon_rate": 0.04,
                "knock_out_levels": [110] * len(observations),
                "observation_dates": observations,
                "effective_date": start,
                "expiry_date": end,
            }
            for mode in ("at_expiry", "every_trading_day"):
                snowball = SnowballOption(
                    **terms,
                    initial_spot=100,
                    knock_in_level=90,
                    upper_strike=100,
                    lower_strike=60,
                    knock_in_observation_mode=mode,
                )
                for note, engine in (
                    (
                        BinarySnowballOption(**terms),
                        pricing.MonteCarloBinarySnowballEngine(
                            path_count=2000, seed=42
                        ),
                    ),
                    (
                        snowball,
                        pricing.MonteCarloSnowballEngine(path_count=2000, seed=42),
                    ),
                ):
                    with self.subTest(
                        observations=observations, note=type(note).__name__, mode=mode
                    ):
                        prices = [
                            engine.price(
                                note,
                                PricingContext(
                                    model_parameters=parameters,
                                    spot_price=100,
                                    valuation_time=start,
                                    calendar=calendar,
                                ),
                            )
                            for calendar in (weekdays_calendar(), all_days_calendar())
                        ]
                        if (
                            isinstance(note, SnowballOption)
                            and mode == "every_trading_day"
                        ):
                            self.assertNotEqual(*prices)
                        else:
                            self.assertEqual(*prices)
