import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

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
    def test_autocallable_requests_revalidate_contracts_and_calendars(self):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        dates = [start + timedelta(days=index) for index in range(1, 13)]
        common = dict(
            knock_out_levels=[110] * len(dates),
            observation_dates=dates,
            effective_date=start,
            expiry_date=end,
        )
        coupons = dict(
            knock_out_coupon_rates=[0.08] * len(dates), maturity_coupon_rate=0.06
        )
        downside = dict(
            initial_spot=100, knock_in_level=99, upper_strike=100, lower_strike=60,
            knock_in_observation_mode="every_trading_day",
        )
        cases = (
            (BinarySnowballOption, coupons, "BinarySnowball"),
            (SnowballOption, {**coupons, **downside}, "Snowball"),
            (
                TernarySnowballOption,
                {**coupons, "knock_in_level": 99, "minimum_coupon_rate": 0.02,
                 "knock_in_observation_mode": "every_trading_day"},
                "TernarySnowball",
            ),
            (
                PhoenixOption,
                {**downside, "coupon_rate": 0.08, "coupon_barrier_levels": [100] * len(dates)},
                "Phoenix",
            ),
        )

        def context(rate=0.04, volatility=0.2, calendar=None):
            return PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=0.01, volatility=volatility
                ),
                spot_price=100.25,
                valuation_time=datetime(2025, 1, 12, 12, tzinfo=UTC),
                calendar=calendar or all_days_calendar(),
            )

        base = context()
        for option_type, terms, name in cases:
            note = option_type(**common, **terms, barrier_state="none")
            missing_history = option_type(**common, **terms)
            engines = (
                getattr(pricing, f"MonteCarlo{name}Engine")(path_count=31, seed=42),
                getattr(pricing, f"FiniteDifference{name}Engine")(
                    asset_step_count=80, time_step_count=1
                ),
            )
            for engine in engines:
                with self.subTest(engine=type(engine).__name__):
                    shifts = dict(volatility_shift=0.01, rate_shift=0.01)
                    result = engine.price_with_greeks(note, base, ["vega", "rho"], **shifts)
                    self.assertEqual(result.price, engine.price(note, base))
                    vol_shift = (0.2 + 0.01) - 0.2
                    self.assertEqual(
                        result.vega,
                        (
                            engine.price(note, context(volatility=0.2 + vol_shift))
                            - engine.price(note, context(volatility=0.2 - vol_shift))
                        ) / (200 * vol_shift),
                    )
                    rate_shift = (0.04 + 0.01) - 0.04
                    self.assertEqual(
                        result.rho,
                        (
                            engine.price(note, context(rate=0.04 + rate_shift))
                            - engine.price(note, context(rate=0.04 - rate_shift))
                        ) / (200 * rate_shift),
                    )
                    for option, shifted, category in (
                        (note, context(calendar=weekdays_calendar()), kiyosi.ErrorCategory.INVALID_DATE),
                        (missing_history, base, kiyosi.ErrorCategory.INVALID_PARAMETER),
                    ):
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            engine.price_with_greeks(option, shifted, ["vega", "rho"])
                        self.assertEqual(error.exception.category, category)
                    with ThreadPoolExecutor(max_workers=2) as pool:
                        results = list(pool.map(
                            lambda _: engine.price_with_greeks(note, base, ["vega", "rho"], **shifts),
                            range(2),
                        ))
                    for recovered in results:
                        self.assertEqual(
                            (recovered.price, recovered.vega, recovered.rho),
                            (result.price, result.vega, result.rho),
                        )

    def test_shared_spot_simulations_match_separate_seeded_prices(self):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        terms = {
            "knock_out_levels": [101, 100, 99],
            "observation_dates": [date(2025, 1, 3), date(2025, 1, 8), end],
            "barrier_state": "none",
            "effective_date": start,
            "expiry_date": end,
        }
        cases = [
            (
                BinarySnowballOption(
                    knock_out_coupon_rates=[0.08] * 3,
                    maturity_coupon_rate=0.05,
                    **terms,
                ),
                pricing.MonteCarloBinarySnowballEngine,
            ),
        ]
        for mode in ("every_trading_day", "at_expiry"):
            common = {**terms, "knock_in_level": 99, "knock_in_observation_mode": mode}
            cases.extend(
                [
                    (
                        SnowballOption(
                            knock_out_coupon_rates=[0.08] * 3,
                            maturity_coupon_rate=0.06,
                            initial_spot=100,
                            upper_strike=100,
                            lower_strike=60,
                            **common,
                        ),
                        pricing.MonteCarloSnowballEngine,
                    ),
                    (
                        PhoenixOption(
                            coupon_rate=0.08,
                            initial_spot=100,
                            coupon_barrier_levels=[100, 99, 98],
                            upper_strike=100,
                            lower_strike=60,
                            **common,
                        ),
                        pricing.MonteCarloPhoenixEngine,
                    ),
                    (
                        TernarySnowballOption(
                            knock_out_coupon_rates=[0.08] * 3,
                            maturity_coupon_rate=0.06,
                            minimum_coupon_rate=0.02,
                            **common,
                        ),
                        pricing.MonteCarloTernarySnowballEngine,
                    ),
                ]
            )
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.8
        )
        spot, shift = 100.25, 0.2
        h = (spot + shift) - spot
        contexts = [
            PricingContext(
                model_parameters=parameters,
                spot_price=spot + offset * h,
                valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
                calendar=all_days_calendar(),
            )
            for offset in (0, 1, -1, 2, -2)
        ]
        for note, engine_type in cases:
            for paths in (1, 31, 32):
                for seed in (42, 2**64 - 1):
                    with self.subTest(note=type(note).__name__, paths=paths, seed=seed):
                        engine = engine_type(path_count=paths, seed=seed)
                        p, up, down, up2, down2 = [
                            engine.price(note, ctx) for ctx in contexts
                        ]
                        result = engine.price_with_greeks(
                            note,
                            contexts[0],
                            ["delta", "gamma", "speed"],
                            spot_shift=shift,
                        )
                        self.assertEqual(result.price, p)
                        self.assertEqual(result.delta, (up - down) / (2 * h))
                        self.assertEqual(result.gamma, (up - 2 * p + down) / h / h)
                        self.assertEqual(
                            result.speed,
                            (up2 - 2 * up + 2 * down - down2) / h / h / (2 * h),
                        )
                        numerical = pricing.calculate_numerical_greeks(
                            engine, note, contexts[0], spot_shift=shift
                        )
                        self.assertEqual(
                            (result.price, result.delta, result.gamma, result.speed),
                            (
                                numerical.price,
                                numerical.delta,
                                numerical.gamma,
                                numerical.speed,
                            ),
                        )
                        self.assertIsNone(result.vega)

    def test_irrelevant_knock_in_events_preserve_spot_and_time_greeks(self):
        import math

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
            barrier_state="none",
            knock_in_observation_mode="every_trading_day",
            effective_date=start,
            expiry_date=end,
        )
        for engine in (
            pricing.FiniteDifferenceSnowballEngine(),
            pricing.MonteCarloSnowballEngine(path_count=64, seed=42),
        ):
            for time in (
                datetime(2025, 1, 1, tzinfo=UTC),
                datetime(2025, 1, 2, 12, tzinfo=UTC),
            ):
                with self.subTest(engine=type(engine).__name__, time=time):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                        ),
                        spot_price=75,
                        valuation_time=time,
                    )
                    result = engine.price_with_greeks(
                        note, context, ["delta", "gamma", "theta", "charm", "color"]
                    )
                    years = (
                        datetime(2026, 1, 1, tzinfo=UTC) - time
                    ).total_seconds() / (365 * 86400)
                    self.assertAlmostEqual(
                        result.price, math.exp(-0.04 * years), delta=1e-12
                    )
                    self.assertEqual(
                        (result.delta, result.gamma, result.charm, result.color),
                        (0, 0, 0, 0),
                    )
                    self.assertIsNotNone(result.theta)
                    self.assertAlmostEqual(
                        result.theta, 0.04 / 365 * result.price, delta=1e-8
                    )

    def test_completed_knock_in_preserves_time_greeks_without_hiding_live_events(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=75,
            valuation_time=datetime(2025, 1, 2, 12, tzinfo=UTC),
        )
        for engine in (
            pricing.FiniteDifferenceSnowballEngine(),
            pricing.MonteCarloSnowballEngine(path_count=64, seed=42),
        ):
            for state in ("none", "knocked_in"):
                with self.subTest(engine=type(engine).__name__, state=state):
                    note = SnowballOption(
                        knock_out_coupon_rates=[0.08],
                        maturity_coupon_rate=0.06,
                        initial_spot=100,
                        knock_in_level=75,
                        knock_out_levels=[110],
                        upper_strike=100,
                        lower_strike=60,
                        observation_dates=[end],
                        barrier_state=state,
                        effective_date=start,
                        expiry_date=end,
                        knock_in_observation_mode="every_trading_day",
                    )
                    result = engine.price_with_greeks(
                        note, context, ["theta", "charm", "color"]
                    )
                    for value in (result.theta, result.charm, result.color):
                        self.assertEqual(value is not None, state == "knocked_in")

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
            calendar=all_days_calendar(),
        )
        for engine in (
            pricing.MonteCarloSnowballEngine(path_count=2_000, seed=42),
            pricing.FiniteDifferenceSnowballEngine(scheme="crank_nicolson"),
            pricing.FiniteDifferenceSnowballEngine(scheme="implicit_euler"),
        ):
            with self.subTest(engine=type(engine).__name__):
                self.assertEqual(
                    engine.price(
                        SnowballOption(
                            **terms, knock_in_observation_mode="every_trading_day"
                        ),
                        context,
                    ),
                    engine.price(
                        SnowballOption(**terms, knock_in_observation_mode="at_expiry"),
                        context,
                    ),
                )
        explicit = pricing.FiniteDifferenceSnowballEngine(
            asset_step_count=80,
            time_step_count=200,
            scheme="explicit_euler",
            asset_upper_boundary=400,
        )
        actual = explicit.price(
            SnowballOption(**terms, knock_in_observation_mode="every_trading_day"),
            context,
        )
        reference = pricing.FiniteDifferenceSnowballEngine(
            asset_step_count=80,
            time_step_count=2000,
            scheme="explicit_euler",
            asset_upper_boundary=400,
        ).price(SnowballOption(**terms, knock_in_observation_mode="at_expiry"), context)
        self.assertAlmostEqual(actual, reference, delta=0.0002)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            explicit.price(
                SnowballOption(**terms, knock_in_observation_mode="at_expiry"), context
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
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
