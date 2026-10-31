import math
import sys
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    RateTermStructure,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from test_accumulator import START, contract, market, terminal_reference


class AccumulatorFiniteDifferenceTests(unittest.TestCase):
    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_settings_protocols_defaults_exports_and_ownership(self):
        self.assertIn("FiniteDifferenceAccumulatorEngine", pricing.__all__)
        engine = pricing.FiniteDifferenceAccumulatorEngine()
        self.assertEqual(engine.asset_step_count, 800)
        self.assertEqual(engine.time_step_count, 200)
        self.assertEqual(engine.scheme, "crank_nicolson")
        self.assertIsNone(engine.asset_upper_boundary)
        for field in ("asset_step_count", "time_step_count"):
            for value in (True, 3.5, "800", None):
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(TypeError),
                ):
                    pricing.FiniteDifferenceAccumulatorEngine(**{field: value})
            for value in (2**40, -(2**40)):
                with self.assertRaises(OverflowError):
                    pricing.FiniteDifferenceAccumulatorEngine(**{field: value})
        for value in (True, "440", object()):
            with self.assertRaises(TypeError):
                pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=value)
        with self.assertRaises(OverflowError):
            pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=10**400)
        for fields in (
            {"asset_step_count": 2},
            {"time_step_count": 0},
            {"asset_upper_boundary": 0},
            {"asset_upper_boundary": math.nan},
            {"asset_upper_boundary": math.inf},
        ):
            self.assert_rejection(
                InvalidInputError,
                pricing.FiniteDifferenceAccumulatorEngine,
                **fields,
            )
        with self.assertRaises(ValueError):
            pricing.FiniteDifferenceAccumulatorEngine(scheme="unknown")
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceAccumulatorEngine(scheme=True)
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceAccumulatorEngine(extra=1)
        with self.assertRaises(AttributeError):
            engine.asset_step_count = 400
        wrong = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=START,
            expiry_date=date(2025, 2, 3),
        )
        with self.assertRaises(UnsupportedOperationError):
            engine.price(wrong, market())
        with self.assertRaises(UnsupportedOperationError):
            pricing.implied_volatility(engine, wrong, market(), 1)

    def test_settlement_history_boundaries_and_failure_reuse(self):
        engine = pricing.FiniteDifferenceAccumulatorEngine()
        end = date(2025, 1, 6)
        option = contract(end)
        for spot, quantity in (
            (90, 5),
            (100, 4),
            (109, 4),
            (110, 3),
            (111, 3),
            (math.nextafter(100, 0), 5),
            (math.nextafter(100, 110), 4),
            (math.nextafter(110, 0), 4),
            (math.nextafter(110, 120), 3),
        ):
            with self.subTest(spot=spot):
                result = engine.price_with_greeks(
                    option, market(spot, end), greeks="all"
                )
                self.assertEqual(result.price, quantity * (spot - 100))
                for greek in (
                    "delta",
                    "gamma",
                    "speed",
                    "vega",
                    "rho",
                    "theta",
                    "vanna",
                    "zomma",
                    "charm",
                    "color",
                ):
                    self.assertIsNone(getattr(result, greek))
        self.assertEqual(engine.price(option, market(110, START)), 30)
        for value in (-1, math.nan, math.inf):
            self.assert_rejection(
                InvalidInputError,
                option.replace,
                accumulated_quantity=value,
            )
        for value in (None, True, "3"):
            with self.assertRaises(TypeError):
                option.replace(accumulated_quantity=value)
        self.assertEqual(option.accumulated_quantity, 3)
        self.assertEqual(
            option.replace(accumulated_quantity=0.25).accumulated_quantity, 0.25
        )
        for instrument, context, expected_error in (
            (
                option,
                market(100, START - timedelta(days=1)),
                InvalidInputError,
            ),
            (
                option,
                market(100, datetime(2025, 1, 6, 0, 0, 0, 1, tzinfo=UTC)),
                InvalidInputError,
            ),
            (contract(date(2025, 1, 5)), market(), InvalidInputError),
            (
                contract(end, quantity=sys.float_info.max),
                market(111, end),
                ComputationFailureError,
            ),
        ):
            self.assert_rejection(expected_error, engine.price, instrument, context)
        self.assertEqual(engine.price(option, market(110, end)), 30)

    def test_midnight_intraday_and_calendar_events_are_applied_once(self):
        engine = pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=3200, asset_upper_boundary=440
        )
        option = contract(date(2025, 1, 6))
        for spot, quantity in ((90, 5), (100, 4), (109, 4)):
            at = date(2025, 1, 3)
            expected = terminal_reference(
                option.replace(accumulated_quantity=quantity), market(spot, at)
            )
            self.assertAlmostEqual(
                engine.price(option, market(spot, at)), expected, delta=0.001
            )
        early = market(90, datetime(2025, 1, 3, 0, 0, 0, 1, tzinfo=UTC))
        late = market(90, datetime(2025, 1, 3, 12, tzinfo=UTC))
        for context in (early, late, market(90, date(2025, 1, 4))):
            self.assertAlmostEqual(
                engine.price(option, context),
                terminal_reference(option, context),
                delta=0.001,
            )
        holiday = custom_calendar(
            base=weekdays_calendar(), closed_dates=[date(2025, 1, 3)]
        )
        context = market(90, date(2025, 1, 3)).replace(calendar=holiday)
        self.assertAlmostEqual(
            engine.price(option, context),
            terminal_reference(option, context),
            delta=0.001,
        )
        weekend = contract(date(2025, 1, 5))
        self.assertTrue(
            math.isfinite(
                engine.price(weekend, market().replace(calendar=all_days_calendar()))
            )
        )

    def test_validation_precedes_state_and_bumped_failures_leave_inputs_intact(self):
        option, context = contract(), market()
        engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=110)
        self.assert_rejection(InvalidInputError, engine.price, option, context)
        # Immediate settlement needs no spatial domain.
        self.assertEqual(engine.price(option, market(110, START)), 30)
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            context,
            [],
        )
        engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=110.1)
        context = market(110.05)
        quote = engine.price(option, context)
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            context,
            "delta",
            spot_shift=-1,
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            context,
            "speed",
            spot_shift=0.2,
        )
        self.assertEqual(engine.price(option, context), quote)
        self.assertEqual(engine.asset_upper_boundary, 110.1)
        self.assertEqual(option.accumulated_quantity, 3)
        self.assertEqual(context.spot_price, 110.05)
        unstable = pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=800, time_step_count=1, scheme="explicit_euler"
        )
        self.assert_rejection(InvalidInputError, unstable.price, option, market())

    def test_native_interpolation_mixed_greeks_and_event_availability(self):
        option = contract(date(2025, 4, 1))
        engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=440)
        context = market(100.25, datetime(2025, 1, 4, 12, tzinfo=UTC))
        result = engine.price_with_greeks(option, context, greeks="all")
        h = 0.01
        high, low = (
            engine.price(option, context.replace(spot_price=100.25 + sign * h))
            for sign in (1, -1)
        )
        self.assertAlmostEqual(result.delta, (high - low) / (2 * h), delta=1e-6)
        self.assertAlmostEqual(
            result.gamma, (high - 2 * result.price + low) / h**2, delta=1e-6
        )
        for greek, dependency, field, bump, scale in (
            ("speed", "gamma", "spot_price", h, 1),
            ("vanna", "delta", "volatility", 0.0001, 100),
            ("zomma", "gamma", "volatility", 0.0001, 100),
        ):
            bumped = []
            center = context.spot_price if field == "spot_price" else 0.2
            actual_bump = (center + bump) - center
            for sign in (1, -1):
                changes = {field: center + sign * actual_bump}
                shifted = (
                    context.replace(**changes)
                    if field == "spot_price"
                    else context.replace(
                        model_parameters=context.model_parameters.replace(**changes)
                    )
                )
                bumped.append(
                    getattr(
                        engine.price_with_greeks(option, shifted, dependency),
                        dependency,
                    )
                )
            self.assertAlmostEqual(
                getattr(result, greek),
                (bumped[0] - bumped[1]) / (2 * actual_bump * scale),
                delta=1e-10,
            )
        for greek, dependency in (("charm", "delta"), ("color", "gamma")):
            before, after = [
                engine.price_with_greeks(
                    option,
                    context.replace(
                        valuation_time=context.valuation_time + timedelta(days=offset)
                    ),
                    dependency,
                )
                for offset in (-1, 1)
            ]
            self.assertAlmostEqual(
                getattr(result, greek),
                (getattr(after, dependency) - getattr(before, dependency)) / 2,
                delta=1e-10,
            )
        for bump in (200, 1e-300):
            native = engine.price_with_greeks(
                option, context, ["delta", "gamma", "speed"], spot_shift=bump
            )
            self.assertEqual(native.delta, result.delta)
            self.assertEqual(native.gamma, result.gamma)
            self.assertIsNone(native.speed)
        for spot in (100, 110):
            boundary = engine.price_with_greeks(
                option, market(spot, START), greeks="all"
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
        sunday = engine.price_with_greeks(
            option,
            market(90, datetime(2025, 1, 5, 23, 59, 59, tzinfo=UTC)),
            greeks="all",
        )
        self.assertIsNone(sunday.theta)
        small = engine.price_with_greeks(
            option, market(90, volatility=0.0001), ["vega", "vanna", "zomma"]
        )
        self.assertIsNone(small.vega)
        self.assertIsNone(small.vanna)
        self.assertIsNone(small.zomma)

    def test_node_curves_terminal_forward_and_coverage_order(self):
        end = date(2026, 1, 1)
        risk = RateTermStructure(
            reference_time=START, times=[date(2025, 7, 1), end], factors=[0.97, 0.94]
        )
        dividend = RateTermStructure(
            reference_time=START, times=[date(2025, 7, 1), end], factors=[0.99, 0.98]
        )
        context = market(100, START).replace(
            model_parameters=market().model_parameters.replace(
                risk_free_rate=risk, dividend_yield=dividend
            )
        )
        option = contract(end, knock_out=399, daily=0, quantity=2)
        engine = pricing.FiniteDifferenceAccumulatorEngine(
            asset_upper_boundary=400, time_step_count=800
        )
        self.assertAlmostEqual(
            engine.price(option, context), 2 * (98 - 94), delta=0.002
        )
        short = RateTermStructure(
            reference_time=START, times=[date(2025, 2, 3)], factors=[0.99]
        )
        context = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=short)
        )
        self.assert_rejection(InvalidInputError, engine.price, option, context)
        self.assertEqual(engine.price(option, context.replace(spot_price=399)), 598)

    def test_implied_solver_identifiability_protocols_and_failure_state(self):
        engine = pricing.FiniteDifferenceAccumulatorEngine()
        option = contract(date(2025, 1, 6))
        context = market(100, datetime(2025, 1, 3, 12, tzinfo=UTC), volatility=0.4)
        quote = engine.price(option, context)
        self.assertAlmostEqual(
            pricing.implied_volatility(
                engine, option, context, quote, lower_bound=0.2, upper_bound=0.6
            ),
            0.4,
            delta=1e-6,
        )
        self.assert_rejection(
            RootNotBracketedError,
            pricing.implied_volatility,
            engine,
            option,
            context,
            1e10,
            lower_bound=0.2,
            upper_bound=0.6,
        )
        self.assert_rejection(
            NonConvergenceError,
            pricing.implied_volatility,
            engine,
            option,
            context,
            quote,
            lower_bound=0.2,
            upper_bound=0.6,
            max_iterations=1,
            price_tolerance=1e-30,
            parameter_tolerance=1e-30,
        )
        for instrument, scenario in (
            (option, market(110, START)),
            (option, market(90, option.expiry_date)),
            (contract(daily=0, quantity=0), market()),
            (contract(daily=0), market(90, rate=0, dividend=0)),
        ):
            self.assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                instrument,
                scenario,
                engine.price(instrument, scenario),
            )
        for value in (None, True, "1"):
            with self.assertRaises(TypeError):
                pricing.implied_volatility(engine, option, context, value)
        for value in (True, 1.5):
            with self.assertRaises(TypeError):
                pricing.implied_volatility(
                    engine, option, context, quote, max_iterations=value
                )
        with self.assertRaises(OverflowError):
            pricing.implied_volatility(
                engine, option, context, quote, max_iterations=2**40
            )
        self.assert_rejection(
            InvalidInputError,
            pricing.implied_volatility,
            engine,
            option,
            context,
            math.nan,
        )
        self.assertEqual(engine.price(option, context), quote)
        self.assertEqual(context.model_parameters.volatility, 0.4)

    def test_speed_propagates_nonfinite_shifted_prices_and_preserves_inputs(self):
        engine = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=440)
        option = contract(date(2025, 1, 6), daily=0, quantity=sys.float_info.max)
        context = market(100, datetime(2025, 1, 3, 12, tzinfo=UTC))
        quote = engine.price(option, context)
        for spot in (98, 102):
            self.assert_rejection(
                ComputationFailureError,
                engine.price,
                option,
                context.replace(spot_price=spot),
            )
        self.assert_rejection(
            ComputationFailureError,
            engine.price_with_greeks,
            option,
            context,
            "speed",
            spot_shift=2,
        )
        self.assertEqual(engine.price(option, context), quote)
        self.assertEqual(option.accumulated_quantity, sys.float_info.max)
        self.assertEqual(context.spot_price, 100)

    def test_quantity_affinity_and_concurrent_shared_engine(self):
        engine, context = pricing.FiniteDifferenceAccumulatorEngine(), market(90)
        values = [engine.price(contract(quantity=q), context) for q in (0, 3, 6)]
        self.assertAlmostEqual(
            values[2] - values[1], values[1] - values[0], delta=1e-10
        )
        self.assertEqual(engine.price(contract(daily=0, quantity=0), context), 0)
        self.assertEqual(
            engine.price(contract(daily=0), market(90, rate=0, dividend=0)), -30
        )
        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(
                pool.map(lambda _: engine.price(contract(), context), range(2))
            )
        self.assertEqual(results, [values[1]] * 2)

    def test_quantlib_terminal_portfolios_and_separate_grid_convergence(self):

        from support.structured import contract, market, references, settings

        rows = [
            row
            for row in references()
            if row["engine"] == "FiniteDifferenceAccumulatorEngine"
        ]
        self.assertEqual(len(rows), 18)
        for row in rows:
            option, context, fine = contract(row), market(row), settings(row)
            budget = float(row["tolerances"]["price"]) + float(
                row["inputs"]["uncertainty_price"]
            )
            with self.subTest(case=row["case_id"]):
                refined = pricing.FiniteDifferenceAccumulatorEngine(**fine).price(
                    option, context
                )
                self.assertAlmostEqual(
                    refined, float(row["outputs"]["price"]), delta=budget
                )
                if row["inputs"]["convergence"]:
                    for configuration in (
                        {**fine, "asset_step_count": fine["asset_step_count"] // 2},
                        {**fine, "time_step_count": fine["time_step_count"] // 2},
                        {
                            **fine,
                            "asset_step_count": fine["asset_step_count"] * 2,
                            "asset_upper_boundary": fine["asset_upper_boundary"] * 2,
                        },
                    ):
                        self.assertAlmostEqual(
                            pricing.FiniteDifferenceAccumulatorEngine(
                                **configuration
                            ).price(option, context),
                            refined,
                            delta=budget,
                        )
