import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import Accumulator, EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext

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
        for option, engine in cases:
            for shift in (0.01, 0.2):
                with self.subTest(engine=type(engine).__name__, shift=shift):
                    h = (spot + shift) - spot
                    contexts = [
                        PricingContext(
                            model_parameters=parameters,
                            spot_price=value,
                            valuation_time=valuation,
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

    def test_failures_and_concurrent_requests_preserve_the_engine(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=start,
        )
        engine = pricing.MonteCarloVanillaEngine(path_count=127, seed=42)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(option, context, ["rho"], rate_shift=1000)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)
        expected = engine.price_with_greeks(option, context, GREEKS)
        with ThreadPoolExecutor(max_workers=2) as executor:
            tasks = [
                executor.submit(engine.price_with_greeks, option, context, GREEKS)
                for _ in range(2)
            ]
            for task in tasks:
                actual = task.result()
                self.assertEqual(actual.price, expected.price)
                self.assertEqual(
                    (actual.delta, actual.gamma, actual.rho),
                    (expected.delta, expected.gamma, expected.rho),
                )
