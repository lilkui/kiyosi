"""Autocallable finite-difference numerical regressions."""

import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

from kiyosi import InvalidInputError, RootNotBracketedError, pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    CashOrNothingOption,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    autocallable_observations,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)


class AutocallableRegressionTests(unittest.TestCase):
    def test_finite_difference_greek_workspaces_preserve_independent_prices(self):
        from price_differences import price_differences

        start, end = date(2025, 1, 1), date(2025, 1, 13)
        engine = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=80, time_step_count=80, asset_upper_boundary=400
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=-0.02, dividend_yield=0.01, volatility=0.3
            ),
            spot_price=100.25,
            valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
            calendar=all_days_calendar(),
        )
        shifts = {"spot_shift": 0.25, "volatility_shift": 0.002, "rate_shift": 0.001}
        for mode in ("every_trading_day", "at_expiry"):
            note = SnowballOption(
                maturity_coupon_rate=0.06,
                initial_spot=100,
                knock_in_level=80,
                upper_strike=100,
                lower_strike=60,
                knock_in_observation_mode=mode,
                barrier_state="none",
                effective_date=start,
                expiry_date=end,
                observations=autocallable_observations(
                    [date(2025, 1, 3), end],
                    [110, 105],
                    knock_out_coupon_rates=[0.08, 0.08],
                ),
            )
            expected = price_differences(
                engine, note, context, ["vega", "rho"], **shifts
            )
            actual = engine.price_with_greeks(note, context, greeks="all", **shifts)
            self.assertEqual(actual.price, expected.price)
            self.assertEqual(actual.vega, expected.vega)
            self.assertEqual(actual.rho, expected.rho)
            for greek in ("speed", "vanna", "zomma", "charm", "color"):
                with self.subTest(mode=mode, greek=greek):
                    single = engine.price_with_greeks(note, context, greek, **shifts)
                    self.assertEqual(getattr(single, greek), getattr(actual, greek))

    def test_automatic_domains_converge_to_volatile_terminal_portfolios(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=2
            ),
            spot_price=100,
            valuation_time=start,
        )
        binary = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [100], knock_out_coupon_rates=[0.1]
            ),
        )
        digital = CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=0.1,
            effective_date=start,
            expiry_date=end,
        )
        expected = 1 + pricing.AnalyticDigitalEngine().price(digital, context)
        for steps in (200, 2000):
            engine = pricing.FiniteDifferenceAutocallableEngine(
                asset_step_count=steps, time_step_count=2000
            )
            with self.subTest(steps=steps):
                self.assertAlmostEqual(
                    engine.price(binary, context), expected, delta=0.0003
                )
        common = {
            "effective_date": start,
            "expiry_date": end,
            "knock_in_level": 100,
            "knock_in_observation_mode": "every_trading_day",
        }
        downside = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
        put = lambda strike: pricing.AnalyticEuropeanEngine().price(
            EuropeanOption(
                option_type="put", strike=strike, effective_date=start, expiry_date=end
            ),
            context,
        )
        expected_downside = expected - (put(100) - put(60)) / 100
        for note, value in (
            (
                SnowballOption(
                    **common,
                    **downside,
                    maturity_coupon_rate=0,
                    observations=binary.observations,
                ),
                expected_downside,
            ),
            (
                PhoenixOption(
                    **common,
                    **downside,
                    coupon_rate=0.1,
                    observations=autocallable_observations(
                        [end], [100], coupon_barrier_levels=[100]
                    ),
                ),
                expected_downside,
            ),
            (
                TernarySnowballOption(
                    **common,
                    maturity_coupon_rate=0.1,
                    minimum_coupon_rate=0,
                    observations=binary.observations,
                ),
                expected,
            ),
        ):
            with self.subTest(product=type(note).__name__):
                self.assertAlmostEqual(engine.price(note, context), value, delta=0.0003)
        result = engine.price_with_greeks(
            binary, context, ["delta", "gamma", "vega", "rho"]
        )
        reference = pricing.AnalyticDigitalEngine().price_with_greeks(
            digital, context, ["delta", "gamma", "vega", "rho"]
        )
        for greek in ("delta", "gamma", "vega", "rho"):
            value = getattr(reference, greek) - (0.01 if greek == "rho" else 0)
            self.assertAlmostEqual(
                getattr(result, greek), value, delta=abs(value) * 0.02 + 1e-8
            )
        quote = engine.price(binary, context)
        fixed = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=2000, time_step_count=2000, asset_upper_boundary=1e6
        )
        delta = (
            fixed.price(binary, context.replace(spot_price=100.1))
            - fixed.price(binary, context.replace(spot_price=99.9))
        ) / 0.2
        self.assertAlmostEqual(
            fixed.price_with_greeks(binary, context, "delta").delta,
            delta,
            delta=abs(delta) * 0.02,
        )
        volatility = pricing.implied_volatility(
            engine,
            binary,
            context,
            quote,
            lower_bound=1.5,
            upper_bound=2.5,
            price_tolerance=1e-10,
            parameter_tolerance=1e-10,
        )
        recovered = context.replace(
            model_parameters=context.model_parameters.replace(volatility=volatility)
        )
        self.assertAlmostEqual(engine.price(binary, recovered), quote, delta=1e-8)
        explicit = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=200, time_step_count=2000, scheme="explicit_euler"
        )
        self.assertAlmostEqual(explicit.price(binary, context), expected, delta=0.0003)

    def test_finite_difference_volatility_trials_refresh_operators_and_isolate_requests(
        self,
    ):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        observation_dates = [date(2025, 1, 3), date(2025, 1, 8), end]
        knock_out_levels = [101, 100, 99]
        common = {"effective_date": start, "expiry_date": end, "barrier_state": "none"}
        coupons = {
            "maturity_coupon_rate": 0.06,
            "observations": autocallable_observations(
                observation_dates, knock_out_levels, knock_out_coupon_rates=[0.08] * 3
            ),
        }

        def context(volatility):
            return PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=0.01, volatility=volatility
                ),
                spot_price=100.25,
                valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
                calendar=all_days_calendar(),
            )

        base = context(0.27)
        bounds = {"lower_bound": 0.1, "upper_bound": 0.4, "price_tolerance": 1e-12}
        for mode in ("at_expiry", "every_trading_day"):
            knock_in = {"knock_in_level": 99, "knock_in_observation_mode": mode}
            cases = (
                (
                    PhoenixOption,
                    dict(
                        **knock_in,
                        initial_spot=100,
                        upper_strike=100,
                        lower_strike=60,
                        coupon_rate=0.08,
                        observations=autocallable_observations(
                            observation_dates,
                            knock_out_levels,
                            coupon_barrier_levels=[100, 99, 98],
                        ),
                    ),
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
                (
                    SnowballOption,
                    dict(
                        **knock_in,
                        **coupons,
                        initial_spot=100,
                        upper_strike=100,
                        lower_strike=60,
                    ),
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
                (
                    BinarySnowballOption,
                    coupons,
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
                (
                    TernarySnowballOption,
                    dict(
                        **knock_in,
                        **coupons,
                        minimum_coupon_rate=0.02,
                    ),
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
            )
            for kind, terms, engine_type in cases:
                note = kind(**common, **terms)
                for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
                    with self.subTest(kind=kind, mode=mode, scheme=scheme):
                        engine = engine_type(
                            asset_step_count=32, time_step_count=64, scheme=scheme
                        )
                        quotes = [
                            engine.price(note, context(volatility))
                            for volatility in (0.17, 0.33)
                        ]

                        def solve(quote, *, engine=engine, note=note):
                            return pricing.implied_volatility(
                                engine, note, base, quote, **bounds
                            )

                        expected = [solve(quote) for quote in quotes]
                        for quote, volatility in zip(quotes, expected):
                            self.assertAlmostEqual(
                                engine.price(note, context(volatility)),
                                quote,
                                delta=1e-10,
                            )
                        with self.assertRaises(RootNotBracketedError) as error:
                            solve(1e10)
                        self.assertIs(
                            type(error.exception),
                            RootNotBracketedError,
                        )
                        with ThreadPoolExecutor(max_workers=2) as pool:
                            recovered = list(pool.map(solve, quotes))
                        self.assertEqual(recovered, expected)

    def test_coupon_endpoint_batches_preserve_layer_changes_and_isolation(
        self,
    ):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        observation_dates = [date(2025, 1, 7), end]
        knock_out_levels = [110, 110]
        common = {"effective_date": start, "expiry_date": end, "barrier_state": "none"}
        coupons = {
            "maturity_coupon_rate": 0.073,
            "observations": autocallable_observations(
                observation_dates,
                knock_out_levels,
                knock_out_coupon_rates=[0.073, 0.083],
            ),
        }
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0, volatility=0.3
            ),
            spot_price=100.25,
            valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
            calendar=all_days_calendar(),
        )
        for mode in ("at_expiry", "every_trading_day"):
            knock_in = {"knock_in_level": 99, "knock_in_observation_mode": mode}
            cases = (
                (
                    PhoenixOption,
                    dict(
                        **knock_in,
                        initial_spot=100,
                        upper_strike=100,
                        lower_strike=60,
                        coupon_rate=0.073,
                        observations=autocallable_observations(
                            observation_dates,
                            knock_out_levels,
                            coupon_barrier_levels=[90, 90],
                        ),
                    ),
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
                (
                    SnowballOption,
                    dict(
                        **knock_in,
                        **coupons,
                        initial_spot=100,
                        upper_strike=100,
                        lower_strike=100,
                    ),
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
                (
                    BinarySnowballOption,
                    coupons,
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
                (
                    TernarySnowballOption,
                    dict(
                        **knock_in,
                        **coupons,
                        minimum_coupon_rate=0,
                    ),
                    pricing.FiniteDifferenceAutocallableEngine,
                ),
            )
            for kind, terms, engine_type in cases:
                note = kind(**common, **terms)
                engines = [
                    (
                        engine_type(
                            asset_step_count=16,
                            time_step_count=8,
                            scheme=scheme,
                            asset_upper_boundary=400,
                        ),
                        scheme,
                    )
                    for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson")
                ]
                engines.extend(
                    (
                        pricing.QuadratureAutocallableEngine(asset_step_count=steps),
                        f"quadrature_{steps}",
                    )
                    for steps in (17, 128)
                )
                engines.append(
                    (
                        pricing.MonteCarloAutocallableEngine(path_count=1024, seed=42),
                        "monte_carlo",
                    )
                )
                for engine, scheme in engines:
                    quote = engine.price(note, context)
                    conventions = (
                        ({},)
                        if kind is PhoenixOption
                        else (
                            {"quote_convention": "shift_maturity_coupon"},
                            {"quote_convention": "preserve_maturity_coupon"},
                        )
                    )
                    for convention in conventions:
                        alternate_terms = dict(terms)
                        if kind is PhoenixOption:
                            alternate_terms["coupon_rate"] = 0.12
                        else:
                            alternate_terms["observations"] = autocallable_observations(
                                observation_dates,
                                knock_out_levels,
                                knock_out_coupon_rates=[0.12, 0.13],
                            )
                            if (
                                convention["quote_convention"]
                                == "shift_maturity_coupon"
                            ):
                                alternate_terms["maturity_coupon_rate"] = 0.12
                        alternate_quote = engine.price(
                            kind(**common, **alternate_terms), context
                        )

                        def solve(
                            observed_price,
                            *,
                            convention=convention,
                            engine=engine,
                            note=note,
                        ):
                            # Weak coupon exposure needs tighter price accuracy to recover the quote.
                            return pricing.implied_coupon(
                                engine,
                                note,
                                context,
                                observed_price,
                                lower_bound=0,
                                upper_bound=0.2,
                                price_tolerance=1e-14,
                                parameter_tolerance=1e-12,
                                **convention,
                            )

                        with self.subTest(
                            kind=kind, mode=mode, scheme=scheme, convention=convention
                        ):
                            with ThreadPoolExecutor(max_workers=2) as pool:
                                results = list(
                                    pool.map(solve, (quote, alternate_quote))
                                )
                            self.assertAlmostEqual(results[0], 0.073, delta=1e-9)
                            self.assertAlmostEqual(results[1], 0.12, delta=1e-9)
                            with self.assertRaises(InvalidInputError) as error:
                                pricing.implied_coupon(
                                    engine,
                                    note,
                                    context,
                                    quote,
                                    lower_bound=0.2,
                                    upper_bound=0,
                                    **convention,
                                )
                            self.assertIs(
                                type(error.exception),
                                InvalidInputError,
                            )
                            self.assertEqual(solve(quote), results[0])

    def test_autocallable_requests_revalidate_contracts_and_calendars(self):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        dates = [start + timedelta(days=index) for index in range(1, 13)]
        observation_dates = dates
        knock_out_levels = [110] * len(dates)
        common = {"effective_date": start, "expiry_date": end}
        coupons = {
            "maturity_coupon_rate": 0.06,
            "observations": autocallable_observations(
                observation_dates,
                knock_out_levels,
                knock_out_coupon_rates=[0.08] * len(dates),
            ),
        }
        downside = {
            "initial_spot": 100,
            "knock_in_level": 99,
            "upper_strike": 100,
            "lower_strike": 60,
            "knock_in_observation_mode": "every_trading_day",
        }
        cases = (
            (BinarySnowballOption, coupons),
            (SnowballOption, {**coupons, **downside}),
            (
                TernarySnowballOption,
                {
                    **coupons,
                    "knock_in_level": 99,
                    "minimum_coupon_rate": 0.02,
                    "knock_in_observation_mode": "every_trading_day",
                },
            ),
            (
                PhoenixOption,
                {
                    **downside,
                    "coupon_rate": 0.08,
                    "observations": autocallable_observations(
                        observation_dates,
                        knock_out_levels,
                        coupon_barrier_levels=[100] * len(dates),
                    ),
                },
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
        for option_type, terms in cases:
            note = option_type(**common, **terms, barrier_state="none")
            missing_history = option_type(**common, **terms)
            engines = (
                pricing.FiniteDifferenceAutocallableEngine(
                    asset_step_count=80, time_step_count=1
                ),
                pricing.QuadratureAutocallableEngine(asset_step_count=16),
            )
            for engine in engines:
                with self.subTest(engine=type(engine).__name__):
                    shifts = {"volatility_shift": 0.01, "rate_shift": 0.01}
                    result = engine.price_with_greeks(
                        note, base, ["vega", "rho"], **shifts
                    )
                    self.assertEqual(result.price, engine.price(note, base))
                    vol_shift = (0.2 + 0.01) - 0.2
                    self.assertEqual(
                        result.vega,
                        (
                            engine.price(note, context(volatility=0.2 + vol_shift))
                            - engine.price(note, context(volatility=0.2 - vol_shift))
                        )
                        / (200 * vol_shift),
                    )
                    rate_shift = (0.04 + 0.01) - 0.04
                    self.assertEqual(
                        result.rho,
                        (
                            engine.price(note, context(rate=0.04 + rate_shift))
                            - engine.price(note, context(rate=0.04 - rate_shift))
                        )
                        / (200 * rate_shift),
                    )
                    for option, shifted, expected_error in (
                        (
                            note,
                            context(calendar=weekdays_calendar()),
                            InvalidInputError,
                        ),
                        (missing_history, base, InvalidInputError),
                    ):
                        with self.assertRaises(expected_error) as error:
                            engine.price_with_greeks(option, shifted, ["vega", "rho"])
                        self.assertIs(type(error.exception), expected_error)
                    with ThreadPoolExecutor(max_workers=2) as pool:
                        results = list(
                            pool.map(
                                lambda _, *, engine=engine, note=note, shifts=shifts: (
                                    engine.price_with_greeks(
                                        note, base, ["vega", "rho"], **shifts
                                    )
                                ),
                                range(2),
                            )
                        )
                    for recovered in results:
                        self.assertEqual(
                            (recovered.price, recovered.vega, recovered.rho),
                            (result.price, result.vega, result.rho),
                        )

    def test_irrelevant_knock_in_events_preserve_spot_and_time_greeks(self):
        import math

        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = SnowballOption(
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=75,
            upper_strike=100,
            lower_strike=100,
            barrier_state="none",
            knock_in_observation_mode="every_trading_day",
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [110], knock_out_coupon_rates=[0]
            ),
        )
        for engine in (pricing.FiniteDifferenceAutocallableEngine(),):
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
        for engine in (pricing.FiniteDifferenceAutocallableEngine(),):
            for state in ("none", "knocked_in"):
                with self.subTest(engine=type(engine).__name__, state=state):
                    note = SnowballOption(
                        maturity_coupon_rate=0.06,
                        initial_spot=100,
                        knock_in_level=75,
                        upper_strike=100,
                        lower_strike=60,
                        barrier_state=state,
                        effective_date=start,
                        expiry_date=end,
                        knock_in_observation_mode="every_trading_day",
                        observations=autocallable_observations(
                            [end], [110], knock_out_coupon_rates=[0.08]
                        ),
                    )
                    result = engine.price_with_greeks(
                        note, context, ["theta", "charm", "color"]
                    )
                    for value in (result.theta, result.charm, result.color):
                        self.assertEqual(value is not None, state == "knocked_in")

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
        observation_dates = [end]
        knock_out_levels = [110]
        common = {
            "knock_in_level": 100,
            "knock_in_observation_mode": "at_expiry",
            "effective_date": start,
            "expiry_date": end,
        }
        snowball = SnowballOption(
            **common,
            maturity_coupon_rate=0.06,
            initial_spot=100,
            upper_strike=100,
            lower_strike=60,
            observations=autocallable_observations(
                observation_dates, knock_out_levels, knock_out_coupon_rates=[0.06]
            ),
        )
        phoenix = PhoenixOption(
            **common,
            coupon_rate=0.08,
            initial_spot=100,
            upper_strike=100,
            lower_strike=60,
            observations=autocallable_observations(
                observation_dates, knock_out_levels, coupon_barrier_levels=[0]
            ),
        )
        ternary = TernarySnowballOption(
            **common,
            maturity_coupon_rate=0.06,
            minimum_coupon_rate=0.02,
            observations=autocallable_observations(
                observation_dates, knock_out_levels, knock_out_coupon_rates=[0.06]
            ),
        )
        put_prices = [
            pricing.AnalyticEuropeanEngine().price(
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
                pricing.FiniteDifferenceAutocallableEngine,
                downside + 0.06 * digital,
            ),
            (
                phoenix,
                pricing.FiniteDifferenceAutocallableEngine,
                downside + 0.08 * discount,
            ),
            (
                ternary,
                pricing.FiniteDifferenceAutocallableEngine,
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
            "maturity_coupon_rate": 0,
            "initial_spot": 100,
            "knock_in_level": 75,
            "upper_strike": 100,
            "lower_strike": 100,
            "effective_date": start,
            "expiry_date": end,
            "observations": autocallable_observations(
                [middle, end], [110, 104], knock_out_coupon_rates=[0.08, 0.08]
            ),
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
            pricing.FiniteDifferenceAutocallableEngine(scheme="crank_nicolson"),
            pricing.FiniteDifferenceAutocallableEngine(scheme="implicit_euler"),
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
        explicit = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=80,
            time_step_count=200,
            scheme="explicit_euler",
            asset_upper_boundary=400,
        )
        actual = explicit.price(
            SnowballOption(**terms, knock_in_observation_mode="every_trading_day"),
            context,
        )
        reference = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=80,
            time_step_count=2000,
            scheme="explicit_euler",
            asset_upper_boundary=400,
        ).price(SnowballOption(**terms, knock_in_observation_mode="at_expiry"), context)
        self.assertAlmostEqual(actual, reference, delta=0.0002)
        with self.assertRaises(InvalidInputError) as error:
            explicit.price(
                SnowballOption(**terms, knock_in_observation_mode="at_expiry"), context
            )
        self.assertIs(type(error.exception), InvalidInputError)

    def test_fixed_finite_difference_autocallables_retain_rate_sensitivity(self):
        import math

        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [110], knock_out_coupon_rates=[0]
            ),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        result = pricing.FiniteDifferenceAutocallableEngine().price_with_greeks(
            note, context, ["delta", "gamma", "vega", "rho"]
        )
        self.assertEqual(result.price, math.exp(-0.04))
        self.assertEqual((result.delta, result.gamma, result.vega), (0, 0, 0))
        self.assertAlmostEqual(result.rho, -math.exp(-0.04) / 100, delta=1e-8)
