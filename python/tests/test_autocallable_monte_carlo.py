"""Autocallable CPU sampling through the installed Rust extension."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

from kiyosi import (
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import BinarySnowballOption, autocallable_observations
from kiyosi.market import RateTermStructure
from kiyosi.pricing import MonteCarloAutocallableEngine as MC
from kiyosi.pricing import (
    QuadratureAutocallableEngine,
    implied_coupon,
    implied_volatility,
)
from test_phoenix import contract as phoenix
from test_snowball import END, START, contract, market


class AutocallableMonteCarloTests(unittest.TestCase):
    def assert_rejection(self, expected_error, operation):
        with self.assertRaises(expected_error) as error:
            operation()
        self.assertIs(type(error.exception), expected_error)

    def test_terminal_price_matches_quadrature_with_sampling_error(self):
        note, context = contract(), market()
        engine = MC(path_count=32768, seed=42)
        result = engine.price_with_statistics(note, context)
        self.assertEqual(result.price, engine.price(note, context))
        self.assertAlmostEqual(
            result.price,
            QuadratureAutocallableEngine().price(note, context),
            delta=4 * result.standard_error,
        )
        lower, upper = result.confidence_interval
        self.assertLess(lower, result.price)
        self.assertGreater(upper, result.price)

    def test_defaults_strict_representations_and_validation(self):
        engine = MC()
        self.assertEqual(
            (
                engine.path_count,
                engine.seed,
                engine.thread_count,
                engine.sampling,
                engine.antithetic,
            ),
            (100000, None, 0, "pseudo_random", False),
        )
        for field in ("path_count", "thread_count", "seed"):
            for value, error in (
                (True, TypeError),
                (2.5, TypeError),
                (2**80, OverflowError),
            ):
                with self.subTest(field=field, value=value), self.assertRaises(error):
                    MC(**{field: value})
        for fields in ({"antithetic": 1}, {"sampling": 1}, {"backend": "cpu"}):
            with self.assertRaises(TypeError):
                MC(**fields)
        with self.assertRaises(ValueError):
            MC(sampling="invalid")
        for fields in (
            {"path_count": 0},
            {"thread_count": -1},
            {"path_count": 31, "sampling": "sobol"},
        ):
            self.assert_rejection(InvalidInputError, lambda fields=fields: MC(**fields))
        with self.assertRaises(AttributeError):
            engine.seed = 42
        note, context = phoenix(), market()
        for operation in (
            lambda: engine.price(object(), context),
            lambda: engine.price_with_statistics(object(), context),
            lambda: engine.price_with_greeks(note, context, "delta", spot_shift=True),
            lambda: implied_volatility(engine, note, context, True),
            lambda: implied_volatility(engine, note, context, 1, max_iterations=2.5),
            lambda: implied_coupon(engine, note, context, True),
            lambda: implied_coupon(engine, note, context, 1, max_iterations=True),
            lambda: implied_coupon(
                engine, note, context, 1, quote_convention="shift_maturity_coupon"
            ),
        ):
            with self.assertRaises(TypeError):
                operation()
        self.assert_rejection(
            InvalidInputError, lambda: engine.price_with_greeks(note, context, [])
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: engine.price_with_greeks(note, context, "delta", spot_shift=0),
        )
        curve = RateTermStructure(reference_time=START, times=[END], factors=[0.99])
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=curve)
        )
        self.assert_rejection(
            UnsupportedOperationError, lambda: engine.price(note, curved)
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: engine.price_with_greeks(note, curved, []),
        )

    def test_statistics_counts_sobol_and_shared_engine_concurrency(self):
        note, context = phoenix(), market()
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                for paths in (1, 2, 32, 1024):
                    fields = {
                        "path_count": paths,
                        "seed": 2**64 - 1,
                        "sampling": sampling,
                        "antithetic": antithetic,
                    }
                    serial = MC(**fields, thread_count=1).price_with_statistics(
                        note, context
                    )
                    engine = MC(**fields, thread_count=3)
                    with ThreadPoolExecutor(max_workers=2) as pool:
                        results = list(
                            pool.map(
                                lambda _, engine=engine: engine.price_with_statistics(
                                    note, context
                                ),
                                range(2),
                            )
                        )
                    for result in results:
                        self.assertEqual(
                            (
                                result.price,
                                result.standard_error,
                                result.confidence_interval,
                            ),
                            (
                                serial.price,
                                serial.standard_error,
                                serial.confidence_interval,
                            ),
                        )
                    self.assertEqual(
                        serial.standard_error is not None,
                        sampling == "pseudo_random"
                        and paths > (2 if antithetic else 1),
                    )
        self.assertIsNone(
            MC(path_count=31, antithetic=True, seed=42)
            .price_with_statistics(note, context)
            .standard_error
        )

    def test_current_phoenix_coupon_paid_once_and_weekend_does_not_knock_in(self):
        engine = MC(path_count=32768, seed=42)
        # Tiny volatility makes every future observation coupon deterministic.
        note = phoenix(
            observations=autocallable_observations(
                [date(2025, 2, 3), END], [200, 200], coupon_barrier_levels=[90, 90]
            )
        )
        at_event = market(100, date(2025, 2, 3), volatility=1e-12, rate=0, dividend=0)
        self.assertAlmostEqual(
            engine.price(note, at_event), 1 + 0.365 * 90 / 365, delta=1e-14
        )
        after_event = at_event.replace(
            valuation_time=datetime(2025, 2, 3, 0, 0, 0, 1, tzinfo=UTC)
        )
        self.assertAlmostEqual(
            engine.price(note, after_event), 1 + 0.365 * 57 / 365, delta=1e-14
        )
        weekend = datetime(2025, 1, 4, tzinfo=UTC)
        snow = contract(
            expiry_date=date(2025, 1, 6),
            maturity_coupon_rate=0.12,
            knock_in_observation_mode="every_trading_day",
            barrier_state="none",
            observations=autocallable_observations(
                [date(2025, 1, 6)], [120], knock_out_coupon_rates=[0.12]
            ),
        )
        first = engine.price(snow, market(79, weekend, volatility=0.2))
        later = engine.price(
            snow, market(79, weekend + timedelta(microseconds=1), volatility=0.2)
        )
        self.assertAlmostEqual(first, later, delta=1e-7)

    def test_solvers_reject_fixed_cashflows_and_preserve_state(self):
        engine, note, context = MC(path_count=4096, seed=42), phoenix(), market()
        snapshot = repr(note), repr(context)
        quote = engine.price(note, context)
        sigma = implied_volatility(
            engine, note, context, quote, lower_bound=0.2, upper_bound=0.4
        )
        self.assertAlmostEqual(sigma, 0.3, delta=1e-6)
        coupon = implied_coupon(
            engine, note, context, quote, lower_bound=-0.1, upper_bound=0.5
        )
        self.assertAlmostEqual(coupon, note.coupon_rate, delta=1e-8)
        self.assertEqual((repr(note), repr(context)), snapshot)
        # Each unseeded solve needs one consistent affine sampled curve.
        unseeded = MC(path_count=31)
        exposed = phoenix(
            lower_strike=100,
            upper_strike=100,
            observations=autocallable_observations(
                [date(2025, 2, 3), END], [101, 105], coupon_barrier_levels=[0, 0]
            ),
        )
        for _ in range(3):
            solved = implied_coupon(
                unseeded,
                exposed,
                market(rate=0),
                1.01,
                lower_bound=0,
                upper_bound=0.5,
                price_tolerance=1e-12,
            )
            self.assertGreater(solved, 0)
            self.assertLess(solved, 0.5)
        self.assertIsNone(unseeded.seed)
        fixed = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations(
                [END], [110], knock_out_coupon_rates=[0]
            ),
        )
        estimate = MC(path_count=1).price_with_statistics(fixed, market(rate=0))
        self.assertEqual(
            (estimate.price, estimate.standard_error, estimate.confidence_interval),
            (1, 0, (1, 1)),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_volatility(engine, fixed, market(rate=0), 1),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_coupon(
                engine,
                fixed,
                market(100, END),
                1,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            InvalidInputError, lambda: implied_coupon(engine, note, context, math.nan)
        )
        self.assert_rejection(
            RootNotBracketedError, lambda: implied_coupon(engine, note, context, 100)
        )

    def test_quantlib_terminal_payoff_portfolios(self):
        from support.structured import contract, market, references

        rows = [
            row
            for row in references()
            if row["engine"] == "QuadratureAutocallableEngine"
        ]
        self.assertEqual(
            {row["instrument"] for row in rows},
            {
                "SnowballOption",
                "BinarySnowballOption",
                "TernarySnowballOption",
                "PhoenixOption",
            },
        )
        engine = MC(path_count=65536, seed=42, antithetic=True)
        for row in rows:
            with self.subTest(case=row["case_id"]):
                result = engine.price_with_statistics(contract(row), market(row))
                self.assertAlmostEqual(
                    result.price,
                    float(row["outputs"]["price"]),
                    delta=4 * result.standard_error
                    + float(row["inputs"]["uncertainty_price"])
                    + 1e-10,
                )

    def test_sobol_dimension_limit_uses_remaining_monitoring_dates(self):
        end = date(2110, 1, 1)
        fields = {
            "expiry_date": end,
            "observations": autocallable_observations(
                [end], [110], knock_out_coupon_rates=[0.08]
            ),
        }
        engine = MC(path_count=2, seed=42, sampling="sobol")
        self.assert_rejection(
            InvalidInputError,
            lambda: engine.price(
                contract(**fields, knock_in_observation_mode="every_trading_day"),
                market(),
            ),
        )
        self.assertTrue(math.isfinite(engine.price(contract(**fields), market())))
