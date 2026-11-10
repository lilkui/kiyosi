"""Autocallable CPU sampling through the installed Rust extension."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

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
    implied_phoenix_coupon,
    implied_snowball_coupon,
    implied_volatility,
    numerical_greeks,
)
from support.history import dated as dated_history
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
        result = engine.price_with_statistics(dated_history(note, context), context)
        self.assertEqual(
            result.price, engine.price(dated_history(note, context), context)
        )
        self.assertAlmostEqual(
            result.price,
            QuadratureAutocallableEngine().price(dated_history(note, context), context),
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
                engine.max_active_workers,
                engine.sampling,
                engine.antithetic,
            ),
            (131072, None, 0, "pseudo_random", False),
        )
        for field in ("path_count", "max_active_workers", "seed"):
            for value, error in (
                (True, TypeError),
                (2.5, TypeError),
                (2**80, OverflowError),
            ):
                with self.subTest(field=field, value=value), self.assertRaises(error):
                    MC(**{field: value})
        for fields in ({"antithetic": 1}, {"sampling": 1}, {"unknown": 1}):
            with self.assertRaises(TypeError):
                MC(**fields)
        with self.assertRaises(ValueError):
            MC(sampling="invalid")
        for fields in (
            {"path_count": 0},
            {"max_active_workers": -1},
            {"path_count": 31, "sampling": "sobol"},
        ):
            self.assert_rejection(InvalidInputError, lambda fields=fields: MC(**fields))
        with self.assertRaises(AttributeError):
            engine.seed = 42
        note, context = phoenix(), market()
        for operation in (
            lambda: engine.price(dated_history(object(), context), context),
            lambda: engine.price_with_statistics(
                dated_history(object(), context), context
            ),
            lambda: numerical_greeks(
                engine, dated_history(note, context), context, "delta", spot_shift=True
            ),
            lambda: implied_volatility(
                engine,
                dated_history(note, context),
                context,
                True,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
            lambda: implied_volatility(
                engine,
                dated_history(note, context),
                context,
                1,
                max_iterations=2.5,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
            lambda: implied_phoenix_coupon(
                engine, dated_history(note, context), context, True
            ),
            lambda: implied_phoenix_coupon(
                engine, dated_history(note, context), context, 1, max_iterations=True
            ),
            lambda: implied_phoenix_coupon(
                engine,
                dated_history(note, context),
                context,
                1,
                quote_convention="shift_maturity_coupon",
            ),
        ):
            with self.assertRaises(TypeError):
                operation()
        self.assert_rejection(
            InvalidInputError,
            lambda: numerical_greeks(
                engine, dated_history(note, context), context, "delta", spot_shift=0
            ),
        )
        curve = RateTermStructure(
            reference_time=START,
            times=[END, END + (END - START)],
            factors=[0.99, (0.99) * 0.5],
        )
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=curve)
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: engine.price(dated_history(note, curved), curved),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: engine.price_with_greeks(dated_history(note, curved), curved, []),
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
                    serial = MC(**fields, max_active_workers=1).price_with_statistics(
                        dated_history(note, context), context
                    )
                    engine = MC(**fields, max_active_workers=3)
                    with ThreadPoolExecutor(max_workers=2) as pool:
                        results = list(
                            pool.map(
                                lambda _, engine=engine: engine.price_with_statistics(
                                    dated_history(note, context), context
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
            .price_with_statistics(dated_history(note, context), context)
            .standard_error
        )

    def test_current_phoenix_coupon_paid_once_and_weekend_does_not_knock_in(self):
        engine = MC(path_count=32768, seed=42)
        # Tiny volatility makes every future observation coupon deterministic.
        note = phoenix(
            observations=autocallable_observations(
                [datetime(2025, 2, 3), END], [200, 200], coupon_barrier_levels=[90, 90]
            )
        )
        at_event = market(
            100, datetime(2025, 2, 3), volatility=1e-12, rate=0, dividend=0
        )
        self.assertAlmostEqual(
            engine.price(dated_history(note, at_event), at_event),
            1 + 2 * 0.01,
            delta=1e-14,
        )
        after_event = at_event.replace(valuation_time=datetime(2025, 2, 3, 0, 0, 0, 1))
        self.assertAlmostEqual(
            engine.price(dated_history(note, after_event), after_event),
            1 + 0.01,
            delta=1e-14,
        )
        weekend = datetime(2025, 1, 4)
        snow = contract(
            expiry_time=datetime(2025, 1, 6),
            maturity_coupon_rate=0.12,
            knock_in_observation_mode="every_trading_day",
            barrier_state="no_event",
            observations=autocallable_observations(
                [datetime(2025, 1, 6)], [120], knock_out_coupon_rates=[0.12]
            ),
        )
        first = engine.price(
            dated_history(snow, market(79, weekend, volatility=0.2)),
            market(79, weekend, volatility=0.2),
        )
        later = engine.price(
            dated_history(
                snow, market(79, weekend + timedelta(microseconds=1), volatility=0.2)
            ),
            market(79, weekend + timedelta(microseconds=1), volatility=0.2),
        )
        self.assertAlmostEqual(first, later, delta=1e-7)

    def test_solvers_reject_fixed_cashflows_and_preserve_state(self):
        engine, note, context = MC(path_count=4096, seed=42), phoenix(), market()
        snapshot = repr(note), repr(context)
        quote = engine.price(dated_history(note, context), context)
        sigma = implied_volatility(
            engine,
            dated_history(note, context),
            context,
            quote,
            lower_bound=0.2,
            upper_bound=0.4,
        )
        self.assertAlmostEqual(sigma, 0.3, delta=1e-6)
        coupon = implied_phoenix_coupon(
            engine,
            dated_history(note, context),
            context,
            quote,
            lower_bound=-0.1,
            upper_bound=0.5,
        )
        self.assertEqual(set(coupon), {"coupon_per_period"})
        self.assertAlmostEqual(
            coupon["coupon_per_period"], note.coupon_per_period, delta=1e-8
        )
        self.assertEqual((repr(note), repr(context)), snapshot)
        unseeded = MC(path_count=31)
        exposed = phoenix(
            lower_strike=100,
            upper_strike=100,
            observations=autocallable_observations(
                [datetime(2025, 2, 3), END], [101, 105], coupon_barrier_levels=[0, 0]
            ),
        )
        with self.assertRaises(InvalidInputError):
            implied_phoenix_coupon(
                unseeded, dated_history(exposed, market(rate=0)), market(rate=0), 1.01
            )
        for _ in range(3):
            replay = unseeded.with_resolved_seed()
            solved = implied_phoenix_coupon(
                replay,
                dated_history(exposed, market(rate=0)),
                market(rate=0),
                1.01,
                lower_bound=0,
                upper_bound=0.5,
                price_tolerance=1e-12,
            )
            self.assertGreater(solved["coupon_per_period"], 0)
            self.assertLess(solved["coupon_per_period"], 0.5)
            self.assertAlmostEqual(
                replay.price(
                    dated_history(exposed.replace(**solved), market(rate=0)),
                    market(rate=0),
                ),
                1.01,
                delta=1e-12,
            )
        self.assertIsNone(unseeded.seed)
        fixed = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_time=START,
            expiry_time=END,
            observations=autocallable_observations(
                [END], [110], knock_out_coupon_rates=[0]
            ),
        )
        estimate = MC(path_count=1).price_with_statistics(
            dated_history(fixed, market(rate=0)), market(rate=0)
        )
        self.assertEqual(
            (estimate.price, estimate.standard_error, estimate.confidence_interval),
            (1, 0, (1, 1)),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_volatility(
                engine,
                dated_history(fixed, market(rate=0)),
                market(rate=0),
                1,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(fixed, market(100, END)),
                market(100, END),
                1,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: implied_phoenix_coupon(
                engine, dated_history(note, context), context, math.nan
            ),
        )
        self.assert_rejection(
            RootNotBracketedError,
            lambda: implied_phoenix_coupon(
                engine, dated_history(note, context), context, 100
            ),
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
                result = engine.price_with_statistics(
                    dated_history(contract(row), market(row)), market(row)
                )
                self.assertAlmostEqual(
                    result.price,
                    float(row["outputs"]["price"]),
                    delta=4 * result.standard_error
                    + float(row["inputs"]["uncertainty_price"])
                    + 1e-10,
                )

    def test_sobol_dimension_limit_uses_remaining_monitoring_dates(self):
        end = datetime(2110, 1, 1)
        fields = {
            "expiry_time": end,
            "observations": autocallable_observations(
                [end], [110], knock_out_coupon_rates=[0.08]
            ),
        }
        engine = MC(path_count=2, seed=42, sampling="sobol")
        self.assert_rejection(
            InvalidInputError,
            lambda: engine.price(
                dated_history(
                    contract(**fields, knock_in_observation_mode="every_trading_day"),
                    market(),
                ),
                market(),
            ),
        )
        self.assertTrue(
            math.isfinite(
                engine.price(dated_history(contract(**fields), market()), market())
            )
        )
