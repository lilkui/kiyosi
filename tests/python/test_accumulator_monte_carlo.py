"""Accumulator CPU Monte Carlo through the installed Rust extension."""

import math
import sys
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

from kiyosi import ErrorCategory as E
from kiyosi import KiyosiError, pricing
from kiyosi.instruments import Accumulator
from kiyosi.market import RateTermStructure, custom_calendar, weekdays_calendar
from kiyosi.pricing import MonteCarloAccumulatorEngine as MC
from test_accumulator import START, contract, market, terminal_reference


class AccumulatorMonteCarloTests(unittest.TestCase):
    def category(self, category, operation):
        with self.assertRaises(KiyosiError) as error:
            operation()
        self.assertEqual(error.exception.category, category)

    def test_current_events_settle_before_sampling(self):
        end = date(2025, 1, 6)
        engine = MC()
        for spot, expected in ((90, -50), (100, 0), (105, 20), (110, 30)):
            result = engine.price_with_statistics(contract(end), market(spot, end))
            self.assertEqual(result.price, expected)
            self.assertEqual(result.standard_error, 0)
            self.assertEqual(result.confidence_interval, (expected, expected))

    def test_terminal_and_multidate_prices_match_independent_references(self):
        engine = MC(path_count=32768, seed=42)
        for option, context, reference in (
            (
                contract(date(2025, 1, 6)),
                market(valuation=datetime(2025, 1, 3, 12, tzinfo=UTC)),
                terminal_reference,
            ),
            (
                contract(),
                market(99, START),
                pricing.QuadratureAccumulatorEngine().price,
            ),
            (contract(), market(111), pricing.QuadratureAccumulatorEngine().price),
        ):
            result = engine.price_with_statistics(option, context)
            self.assertEqual(result.price, engine.price(option, context))
            self.assertGreater(result.standard_error, 0)
            self.assertAlmostEqual(
                result.price,
                reference(option, context),
                delta=4 * result.standard_error + 0.02,
            )
            lower, upper = result.confidence_interval
            self.assertAlmostEqual(
                upper - lower,
                2 * 1.959963984540054 * result.standard_error,
                delta=1e-12,
            )

    def test_strict_settings_protocols_defaults_and_ownership(self):
        self.assertIn("MonteCarloAccumulatorEngine", pricing.__all__)
        engine = MC()
        self.assertEqual(
            (
                engine.path_count,
                engine.step_count,
                engine.seed,
                engine.thread_count,
                engine.sampling,
                engine.antithetic,
            ),
            (100000, 50, None, 0, "pseudo_random", False),
        )
        for field in ("path_count", "step_count", "thread_count", "seed"):
            for value, error in (
                (True, TypeError),
                (2.5, TypeError),
                (2**80, OverflowError),
            ):
                with self.subTest(field=field, value=value), self.assertRaises(error):
                    MC(**{field: value})
        with self.assertRaises(OverflowError):
            MC(seed=-1)
        for fields in ({"antithetic": 1}, {"sampling": 1}, {"backend": "cpu"}):
            with self.assertRaises(TypeError):
                MC(**fields)
        with self.assertRaises(ValueError):
            MC(sampling="invalid")
        for fields in (
            {"path_count": 0},
            {"path_count": 10000001},
            {"step_count": 1},
            {"thread_count": -1},
            {"path_count": 31, "sampling": "sobol"},
        ):
            self.category(E.INVALID_PARAMETER, lambda fields=fields: MC(**fields))
        with self.assertRaises(AttributeError):
            engine.seed = 42
        note, context = contract(), market()
        for operation in (
            lambda: engine.price(object(), context),
            lambda: engine.price_with_statistics(object(), context),
            lambda: engine.price_with_greeks(note, context, "delta", spot_shift=True),
            lambda: pricing.implied_volatility(engine, object(), context, 1),
            lambda: pricing.implied_volatility(engine, note, context, True),
            lambda: pricing.implied_volatility(
                engine, note, context, 1, max_iterations=2.5
            ),
        ):
            with self.assertRaises(TypeError):
                operation()

    def test_counts_sobol_antithetic_and_shared_engine_threads(self):
        note, context = contract(), market()
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                for paths in (1, 2, 4, 1024):
                    fields = {
                        "path_count": paths,
                        "seed": 2**64 - 1,
                        "sampling": sampling,
                        "antithetic": antithetic,
                    }
                    serial = MC(**fields, thread_count=1).price_with_statistics(
                        note, context
                    )
                    engine = MC(**fields, thread_count=3, step_count=2)
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
        odd = MC(path_count=31, seed=42, antithetic=True).price_with_statistics(
            note, context
        )
        self.assertIsNone(odd.standard_error)
        self.assertIsNone(odd.confidence_interval)
        reference = pricing.QuadratureAccumulatorEngine().price(note, context)
        for antithetic in (False, True):
            result = MC(
                path_count=16384, seed=42, sampling="sobol", antithetic=antithetic
            ).price_with_statistics(note, context)
            self.assertAlmostEqual(result.price, reference, delta=1)
            self.assertIsNone(result.standard_error)

    def test_midnight_intraday_calendar_and_omitted_history(self):
        engine = MC(path_count=2, seed=42)
        note = contract(date(2025, 1, 6))
        holiday = custom_calendar(
            base=weekdays_calendar(), closed_dates=[date(2025, 1, 3)]
        )
        for context, expected in (
            (market(90, date(2025, 1, 3), 1e-20, 0, 0), -70),
            (
                market(90, datetime(2025, 1, 3, 0, 0, 0, 1, tzinfo=UTC), 1e-20, 0, 0),
                -50,
            ),
            (market(90, date(2025, 1, 4), 1e-20, 0, 0), -50),
            (market(90, date(2025, 1, 3), 1e-20, 0, 0, daily=True), -110),
            (market(90, date(2025, 1, 3), 1e-20, 0, 0).replace(calendar=holiday), -50),
        ):
            self.assertEqual(engine.price(note, context), expected)
        omitted = Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            effective_date=START,
            expiry_date=date(2025, 1, 6),
        )
        self.assertEqual(engine.price(omitted, market(90, omitted.expiry_date)), -20)
        self.assertEqual(omitted.accumulated_quantity, 0)

    def test_shared_greeks_scalar_bumps_and_implied_volatility(self):
        note = contract(date(2025, 1, 6))
        context = market(valuation=datetime(2025, 1, 3, 12, tzinfo=UTC))
        engine = MC(path_count=4096, seed=42)
        result = engine.price_with_greeks(
            note, context, all_greeks=True, spot_shift=0.25
        )
        self.assertEqual(result.price, engine.price(note, context))
        up = engine.price(note, context.replace(spot_price=100.5))
        down = engine.price(note, context.replace(spot_price=100))
        self.assertEqual(result.delta, (up - down) / 0.5)
        # Equivalent second-difference groupings can round differently.
        rounding = (
            4
            * sys.float_info.epsilon
            * (abs(up) + 2 * abs(result.price) + abs(down))
            / 0.25**2
        )
        self.assertAlmostEqual(
            result.gamma, (up - 2 * result.price + down) / 0.25**2, delta=rounding
        )
        for greek in ("speed", "vega", "vanna", "zomma", "rho"):
            self.assertTrue(math.isfinite(result.require(greek)))
        for greek in ("theta", "charm", "color"):
            self.assertIsNone(getattr(result, greek))
        sigma = pricing.implied_volatility(
            engine, note, context, result.price, lower_bound=0.1, upper_bound=0.3
        )
        self.assertAlmostEqual(sigma, 0.2, delta=1e-6)
        for spot in (100, 110):
            boundary = engine.price_with_greeks(
                note, market(spot, START), all_greeks=True
            )
            for greek in (
                "delta",
                "gamma",
                "speed",
                "vanna",
                "zomma",
                "theta",
                "charm",
                "color",
            ):
                self.assertIsNone(getattr(boundary, greek))
            self.assertIsNotNone(boundary.vega)
            self.assertIsNotNone(boundary.rho)
        expired = engine.price_with_greeks(
            note, market(90, note.expiry_date), all_greeks=True
        )
        for greek in (
            "delta",
            "gamma",
            "speed",
            "vega",
            "rho",
            "vanna",
            "zomma",
            "theta",
            "charm",
            "color",
        ):
            self.assertIsNone(getattr(expired, greek))
        self.assertEqual(engine.seed, 42)
        self.assertEqual(note.accumulated_quantity, 3)
        self.assertEqual(context.model_parameters.volatility, 0.2)
        unseeded = MC(path_count=1024)
        smooth = contract(date(2025, 1, 6), knock_out=1e300, acceleration=1, quantity=0)
        shared = unseeded.price_with_greeks(
            smooth, context, ["delta", "gamma"], spot_shift=0.25
        )
        self.assertAlmostEqual(shared.gamma, 0, delta=1e-10)
        self.assertIsNone(unseeded.seed)

    def test_validation_fixed_payoffs_and_failure_reuse(self):
        engine = MC(path_count=32, seed=42)
        note, context = contract(date(2025, 1, 6)), market()
        for instrument, invalid, category in (
            (note, market(110, START - timedelta(days=1)), E.INVALID_TIME_RANGE),
            (
                note,
                market(110, datetime(2025, 1, 6, 0, 0, 0, 1, tzinfo=UTC)),
                E.INVALID_TIME_RANGE,
            ),
            (contract(date(2025, 1, 5)), market(110, START), E.INVALID_DATE),
            (
                contract(quantity=sys.float_info.max),
                market(111, START),
                E.INVALID_RESULT,
            ),
        ):
            self.category(
                category,
                lambda instrument=instrument, invalid=invalid: engine.price(
                    instrument, invalid
                ),
            )
        self.category(
            E.INVALID_PARAMETER, lambda: engine.price_with_greeks(note, context, [])
        )
        self.category(
            E.INVALID_PARAMETER,
            lambda: engine.price_with_greeks(note, context, "delta", spot_shift=0),
        )
        curve = RateTermStructure(
            reference_time=START, times=[note.expiry_date], factors=[0.99]
        )
        for field in ("risk_free_rate", "dividend_yield"):
            curved = context.replace(
                model_parameters=context.model_parameters.replace(**{field: curve})
            )
            self.category(
                E.UNSUPPORTED_OPERATION,
                lambda curved=curved: engine.price(note, curved),
            )
            self.category(
                E.UNSUPPORTED_OPERATION,
                lambda curved=curved: engine.price_with_greeks(note, curved, []),
            )
            self.category(
                E.UNSUPPORTED_OPERATION,
                lambda curved=curved: pricing.implied_volatility(
                    engine, note, curved, math.nan
                ),
            )
        for instrument, fixed, expected in (
            (note, market(110, START), 30),
            (note, market(90, note.expiry_date), -50),
            (contract(daily=0), market(90, START, rate=0, dividend=0), -30),
            (contract(daily=0, quantity=0), context, 0),
            (contract(knock_out=100, acceleration=0, quantity=0), context, 0),
        ):
            result = engine.price_with_statistics(instrument, fixed)
            self.assertEqual(result.price, expected)
            self.assertEqual(result.standard_error, 0)
            self.category(
                E.UNSUPPORTED_OPERATION,
                lambda instrument=instrument, fixed=fixed, expected=expected: (
                    pricing.implied_volatility(engine, instrument, fixed, expected)
                ),
            )
        for quote, category in (
            (math.nan, E.INVALID_PARAMETER),
            (1e100, E.UNBRACKETED_VOLATILITY),
        ):
            self.category(
                category,
                lambda quote=quote: pricing.implied_volatility(
                    engine, note, context, quote
                ),
            )
        self.assertEqual(engine.price(note, market(110, START)), 30)
        self.assertEqual(note.accumulated_quantity, 3)

    def test_sobol_dimension_limit_preserves_known_settlements(self):
        engine = MC(path_count=2, seed=42, sampling="sobol")
        note = contract(date(2090, 1, 2))
        context = market(valuation=START, daily=True)
        self.category(E.INVALID_PARAMETER, lambda: engine.price(note, context))
        result = engine.price_with_statistics(note, context.replace(spot_price=110))
        self.assertEqual(result.price, 30)
        self.assertIsNone(result.standard_error)
        self.assertIsNone(result.confidence_interval)
        zero = contract(date(2090, 1, 2), daily=0, quantity=0)
        self.assertEqual(engine.price(zero, context), 0)

    def test_rate_volatility_and_mixed_greeks_match_seeded_scalar_repricing(self):
        note = contract(date(2025, 1, 13))
        context = market(valuation=datetime(2025, 1, 3, 12, tzinfo=UTC))
        engine = MC(path_count=1024, seed=42, antithetic=True)
        h = 0.25
        v = (0.2 + 0.002) - 0.2
        r = (0.04 + 0.001) - 0.04

        def price(spot=100.25, volatility=0.2, rate=0.04):
            return engine.price(
                note,
                context.replace(
                    spot_price=spot,
                    model_parameters=context.model_parameters.replace(
                        volatility=volatility, risk_free_rate=rate
                    ),
                ),
            )

        result = engine.price_with_greeks(
            note,
            context,
            all_greeks=True,
            spot_shift=h,
            volatility_shift=0.002,
            rate_shift=0.001,
        )
        high, low = price(volatility=0.2 + v), price(volatility=0.2 - v)
        high_up = price(100.5, 0.2 + v)
        high_down = price(100.0, 0.2 + v)
        low_up = price(100.5, 0.2 - v)
        low_down = price(100.0, 0.2 - v)
        expected = {
            "rho": (price(rate=0.04 + r) - price(rate=0.04 - r)) / (200 * r),
            "vega": (high - low) / (200 * v),
            "vanna": (high_up - high_down - low_up + low_down) / (400 * h * v),
            "zomma": ((high_up - 2 * high + high_down) - (low_up - 2 * low + low_down))
            / (200 * h**2 * v),
            "speed": (price(100.75) - 2 * price(100.5) + 2 * price(100) - price(99.75))
            / (2 * h**3),
        }
        for greek, value in expected.items():
            with self.subTest(greek=greek):
                self.assertAlmostEqual(result.require(greek), value, delta=1e-9)


if __name__ == "__main__":
    unittest.main()
