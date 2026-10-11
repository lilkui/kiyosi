import os
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    BinarySnowballOption,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)

GREEKS = (
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "vanna",
    "zomma",
    "rho",
)


class SharedSampleTests(unittest.TestCase):
    def test_implied_volatility_recovers_seeded_quotes(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction, strike=100, effective_date=start, expiry_date=end
            )
            for paths in (1, 127, 128, 1001):
                for seed in (42, 2**64 - 1):
                    for valuation in (start, datetime(2025, 1, 1, 12, tzinfo=UTC)):
                        with self.subTest(
                            direction=direction,
                            paths=paths,
                            seed=seed,
                            valuation=valuation,
                        ):
                            engine = pricing.MonteCarloVanillaEngine(
                                path_count=paths, seed=seed
                            )
                            context = PricingContext(
                                model_parameters=BlackScholesMertonParameters(
                                    risk_free_rate=0.04,
                                    dividend_yield=0.01,
                                    volatility=0.2,
                                ),
                                spot_price=100.25,
                                valuation_time=valuation,
                            )
                            quote = engine.price(option, context)
                            for bounds in (
                                {"lower_bound": 0.1, "upper_bound": 0.4},
                                {"lower_bound": 0.15, "upper_bound": 0.25},
                            ):
                                volatility = pricing.implied_volatility(
                                    engine, option, context, quote, **bounds
                                )
                                repriced = engine.price(
                                    option,
                                    PricingContext(
                                        model_parameters=BlackScholesMertonParameters(
                                            risk_free_rate=0.04,
                                            dividend_yield=0.01,
                                            volatility=volatility,
                                        ),
                                        spot_price=100.25,
                                        valuation_time=valuation,
                                    ),
                                )
                                self.assertAlmostEqual(repriced, quote, delta=1e-7)

    def test_implied_volatility_failures_and_concurrency(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        )
        context = PricingContext(
            model_parameters=parameters, spot_price=100.25, valuation_time=start
        )
        engine = pricing.MonteCarloVanillaEngine(path_count=1001, seed=42)
        quote = engine.price(option, context)
        expected = pricing.implied_volatility(engine, option, context, quote)
        self.assertAlmostEqual(expected, 0.2, delta=1e-8)
        with ThreadPoolExecutor(max_workers=2) as executor:
            tasks = [
                executor.submit(
                    pricing.implied_volatility, engine, option, context, quote
                )
                for _ in range(2)
            ]
            for task in tasks:
                self.assertEqual(task.result(), expected)
        for observed, settings, category in (
            (float("inf"), {}, kiyosi.ErrorCategory.INVALID_PARAMETER),
            (quote, {"lower_bound": 0}, kiyosi.ErrorCategory.INVALID_PARAMETER),
            (-1, {}, kiyosi.ErrorCategory.UNBRACKETED_VOLATILITY),
            (quote, {"upper_bound": 1000}, kiyosi.ErrorCategory.INVALID_RESULT),
        ):
            with self.subTest(observed=observed, settings=settings):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    pricing.implied_volatility(
                        engine, option, context, observed, **settings
                    )
                self.assertEqual(error.exception.category, category)
        expiry_context = PricingContext(
            model_parameters=parameters, spot_price=100.25, valuation_time=end
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            pricing.implied_volatility(
                engine, option, expiry_context, engine.price(option, expiry_context)
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION
        )
        self.assertEqual(
            pricing.implied_volatility(engine, option, context, quote), expected
        )

    def test_numerical_and_selected_greeks_match_the_engine_api(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        names = GREEKS
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction, strike=100, effective_date=start, expiry_date=end
            )
            for paths in (1, 127, 128, 1001):
                for seed in (42, 2**64 - 1):
                    engine = pricing.MonteCarloVanillaEngine(
                        path_count=paths, seed=seed
                    )
                    for valuation in (
                        start,
                        datetime(2025, 1, 1, 12, tzinfo=UTC),
                        datetime(2026, 1, 1, tzinfo=UTC) - timedelta(microseconds=1),
                        end,
                    ):
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                            ),
                            spot_price=100.25,
                            valuation_time=valuation,
                        )
                        for shifts in (
                            {},
                            {
                                "spot_shift": 0.125,
                                "volatility_shift": 0.02,
                                "rate_shift": 0.005,
                                "time_shift_days": 2,
                            },
                        ):
                            with self.subTest(
                                direction=direction,
                                paths=paths,
                                seed=seed,
                                valuation=valuation,
                                shifts=shifts,
                            ):
                                actual = engine.price_with_greeks(
                                    option, context, GREEKS, **shifts
                                )
                                expected = pricing.calculate_numerical_greeks(
                                    engine, option, context, **shifts
                                )
                                self.assertEqual(actual.price, expected.price)
                                self.assertEqual(
                                    tuple(getattr(actual, name) for name in names),
                                    tuple(getattr(expected, name) for name in names),
                                )
                                selected = engine.price_with_greeks(
                                    option, context, ["delta", "gamma"], **shifts
                                )
                                self.assertEqual(
                                    (selected.delta, selected.gamma),
                                    (actual.delta, actual.gamma),
                                )
                                self.assertIsNone(selected.vega)

    def test_numerical_spot_greeks_equal_scalar_seeded_prices(self):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        )
        spot = 100.25
        valuation = datetime(2025, 1, 1, 12, tzinfo=UTC)
        calendar = all_days_calendar()
        cases = (
            (
                EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_date=start,
                    expiry_date=end,
                ),
                pricing.MonteCarloVanillaEngine(path_count=127, seed=42),
            ),
            (
                Accumulator(
                    strike=100,
                    knock_out_level=115,
                    daily_quantity=1,
                    acceleration_factor=2,
                    effective_date=start,
                    expiry_date=end,
                ),
                pricing.MonteCarloAccumulatorEngine(path_count=31, seed=42),
            ),
        )
        common = {
            "knock_out_levels": [110, 110, 110],
            "observation_dates": [date(2025, 1, 3), date(2025, 1, 8), end],
            "effective_date": start,
            "expiry_date": end,
            "barrier_state": "none",
        }
        coupons = {"knock_out_coupon_rates": [0.08] * 3, "maturity_coupon_rate": 0.06}
        downside = {
            "initial_spot": 100,
            "knock_in_level": 80,
            "upper_strike": 100,
            "lower_strike": 60,
            "knock_in_observation_mode": "every_trading_day",
        }
        cases += tuple(
            (
                AmericanOption(
                    option_type=option_type,
                    strike=100,
                    effective_date=start,
                    expiry_date=end,
                ),
                pricing.MonteCarloVanillaEngine(
                    path_count=paths, step_count=steps, seed=seed
                ),
            )
            for option_type in ("call", "put")
            for paths in (3, 128)
            for steps in (3, 7, 50)
            for seed in (42, 2**64 - 1)
        )
        cases += (
            (
                BinarySnowballOption(**common, **coupons),
                pricing.MonteCarloBinarySnowballEngine(path_count=31, seed=42),
            ),
            (
                TernarySnowballOption(
                    **common,
                    **coupons,
                    knock_in_level=80,
                    minimum_coupon_rate=0.02,
                    knock_in_observation_mode="every_trading_day",
                ),
                pricing.MonteCarloTernarySnowballEngine(path_count=31, seed=42),
            ),
            (
                SnowballOption(**common, **coupons, **downside),
                pricing.MonteCarloSnowballEngine(path_count=31, seed=42),
            ),
            (
                PhoenixOption(
                    **common,
                    **downside,
                    coupon_rate=0.08,
                    coupon_barrier_levels=[90] * 3,
                ),
                pricing.MonteCarloPhoenixEngine(path_count=31, seed=42),
            ),
        )
        if os.environ.get("KIYOSI_TEST_CUDA") == "enabled":
            cases += tuple(
                (
                    option,
                    type(engine)(
                        path_count=31,
                        seed=42,
                        backend="cuda",
                        **(
                            {"step_count": 5}
                            if isinstance(engine, pricing.MonteCarloVanillaEngine)
                            else {}
                        ),
                    ),
                )
                for option, engine in cases
            )
        for option, engine in cases:
            for shift in (0.01, 0.2):
                with self.subTest(engine=type(engine).__name__, shift=shift):
                    h = (spot + shift) - spot
                    contexts = [
                        PricingContext(
                            model_parameters=parameters,
                            spot_price=value,
                            valuation_time=valuation,
                            calendar=calendar,
                        )
                        for value in (spot - h, spot, spot + h)
                    ]
                    low, middle, high = [engine.price(option, ctx) for ctx in contexts]
                    result = pricing.calculate_numerical_greeks(
                        engine, option, contexts[1], spot_shift=shift
                    )
                    self.assertEqual(result.price, middle)
                    self.assertEqual(result.delta, (high - low) / (2 * h))
                    self.assertEqual(result.gamma, (high - 2 * middle + low) / h / h)
                    vol_shift = (0.2 + 0.01) - 0.2
                    stencils = []
                    for vol in (0.2 + vol_shift, 0.2 - vol_shift):
                        model = BlackScholesMertonParameters(
                            risk_free_rate=0.04, dividend_yield=0.01, volatility=vol
                        )
                        stencils.append(
                            [
                                engine.price(
                                    option,
                                    PricingContext(
                                        model_parameters=model,
                                        spot_price=value,
                                        valuation_time=valuation,
                                        calendar=calendar,
                                    ),
                                )
                                for value in (spot, spot + h, spot - h)
                            ]
                        )
                    (center_hi, up_hi, down_hi), (center_lo, up_lo, down_lo) = stencils
                    vanna = ((up_hi - down_hi) - (up_lo - down_lo)) / (
                        4 * h * (100 * vol_shift)
                    )
                    zomma = (
                        (up_hi - 2 * center_hi + down_hi) / h / h
                        - (up_lo - 2 * center_lo + down_lo) / h / h
                    ) / (2 * (100 * vol_shift))
                    for names in (["vanna"], ["zomma"], ["vega", "vanna", "zomma"]):
                        selected = engine.price_with_greeks(
                            option,
                            contexts[1],
                            names,
                            spot_shift=shift,
                            volatility_shift=0.01,
                        )
                        if "vanna" in names:
                            self.assertEqual(selected.vanna, vanna)
                        if "zomma" in names:
                            self.assertEqual(selected.zomma, zomma)

    def test_american_unavailable_stencils_retain_scalar_prices(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.MonteCarloVanillaEngine(path_count=127, step_count=7, seed=42)
        for direction in ("call", "put"):
            option = AmericanOption(
                option_type=direction, strike=100, effective_date=start, expiry_date=end
            )
            scalar = engine.price(option, context)
            for greek in ("delta", "gamma", "speed", "vanna", "zomma"):
                shifts = (
                    {"volatility_shift": 0.3}
                    if greek in ("vanna", "zomma")
                    else {"spot_shift": 200}
                )
                with self.subTest(direction=direction, greek=greek):
                    result = engine.price_with_greeks(option, context, greek, **shifts)
                    self.assertEqual(result.price, scalar)
                    self.assertIsNone(getattr(result, greek))

    def test_failures_and_concurrent_requests_preserve_the_engine(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=start,
        )
        engine = pricing.MonteCarloVanillaEngine(path_count=127, step_count=7, seed=42)
        for option_class in (EuropeanOption, AmericanOption):
            option = option_class(
                option_type="put", strike=100, effective_date=start, expiry_date=end
            )
            with self.subTest(option=option_class.__name__):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price_with_greeks(option, context, GREEKS, rate_shift=1000)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT
                )
                expected = engine.price_with_greeks(option, context, GREEKS)
                with ThreadPoolExecutor(max_workers=2) as executor:
                    tasks = [
                        executor.submit(
                            engine.price_with_greeks, option, context, GREEKS
                        )
                        for _ in range(2)
                    ]
                    for task in tasks:
                        actual = task.result()
                        self.assertEqual(actual.price, expected.price)
                        self.assertEqual(
                            tuple(getattr(actual, name) for name in GREEKS),
                            tuple(getattr(expected, name) for name in GREEKS),
                        )
