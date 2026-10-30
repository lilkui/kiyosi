"""Rust FD defaults, representation boundaries, history and solver contracts."""

import math
import unittest
from datetime import date

from kiyosi import InvalidInputError, UnsupportedOperationError
from kiyosi.instruments import BinarySnowballOption, autocallable_observations
from kiyosi.market import RateTermStructure
from kiyosi.pricing import FiniteDifferenceAutocallableEngine as FD
from kiyosi.pricing import implied_coupon, implied_volatility
from test_phoenix import contract as phoenix
from test_snowball import END, START, market
from test_snowball import contract as snowball


class AutocallableFiniteDifferenceTests(unittest.TestCase):
    def assert_rejection(self, expected_error, operation):
        with self.assertRaises(expected_error) as error:
            operation()
        self.assertIs(type(error.exception), expected_error)

    def test_engine_defaults_and_representation_errors(self):
        engine = FD()
        self.assertEqual((engine.asset_step_count, engine.time_step_count), (800, 2000))
        self.assertEqual(engine.scheme, "crank_nicolson")
        self.assertIsNone(engine.asset_upper_boundary)
        for field in ("asset_step_count", "time_step_count"):
            for value, error in (
                (True, TypeError),
                (2.5, TypeError),
                (2**40, OverflowError),
            ):
                with self.subTest(field=field, value=value), self.assertRaises(error):
                    FD(**{field: value})
        for fields in ({"scheme": 1}, {"asset_upper_boundary": True}, {"unknown": 1}):
            with self.assertRaises(TypeError):
                FD(**fields)
        with self.assertRaises(ValueError):
            FD(scheme="unknown")
        for fields in (
            {"time_step_count": 0},
            {"asset_step_count": 2},
            {"asset_upper_boundary": math.nan},
        ):
            self.assert_rejection(InvalidInputError, lambda fields=fields: FD(**fields))
        with self.assertRaises(AttributeError):
            engine.time_step_count = 1
        with self.assertRaises(TypeError):
            engine.price(object(), market())
        note = phoenix()
        for operation in (
            lambda: engine.price_with_greeks(note, market(), "delta", spot_shift=True),
            lambda: implied_volatility(engine, note, market(), True),
            lambda: implied_volatility(engine, note, market(), 1, max_iterations=2.5),
            lambda: implied_coupon(engine, note, market(), True),
            lambda: implied_coupon(engine, note, market(), 1, max_iterations=True),
            lambda: implied_coupon(
                engine, note, market(), 1, quote_convention="shift_maturity_coupon"
            ),
        ):
            with self.assertRaises(TypeError):
                operation()
        with self.assertRaises(OverflowError):
            implied_coupon(engine, note, market(), 1, max_iterations=2**40)

    def test_native_spot_greeks_survive_infeasible_scalar_bumps(self):
        for coupon in (0, 0.1):
            note = BinarySnowballOption(
                maturity_coupon_rate=0,
                effective_date=START,
                expiry_date=END,
                observations=autocallable_observations(
                    [END], [100], knock_out_coupon_rates=[coupon]
                ),
            )
            engine, context = FD(), market()
            reference = engine.price_with_greeks(note, context, ["delta", "gamma"])
            for shift in (200, 1e-300):
                result = engine.price_with_greeks(
                    note, context, ["delta", "gamma", "speed"], spot_shift=shift
                )
                self.assertEqual(
                    (result.delta, result.gamma), (reference.delta, reference.gamma)
                )
                self.assertIsNone(result.speed)

    def test_explicit_uniform_stability_uses_the_top_node_bound(self):
        end = date(2026, 1, 1)
        note = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=START,
            expiry_date=end,
            observations=autocallable_observations(
                [date(2025, 2, 3), end], [100, 100], knock_out_coupon_rates=[0.1, 0.1]
            ),
        )
        engine = FD(
            asset_step_count=11,
            time_step_count=10,
            scheme="explicit_euler",
            asset_upper_boundary=400,
        )
        self.assert_rejection(InvalidInputError, lambda: engine.price(note, market()))

    def test_event_greeks_node_curves_and_failed_replacement(self):
        engine = FD()
        note = phoenix()
        context = market(90, date(2025, 2, 3))
        result = engine.price_with_greeks(note, context, all_greeks=True)
        self.assertIsNone(result.delta)
        self.assertIsNone(result.gamma)
        self.assertIsNone(result.speed)
        self.assertIsNone(result.theta)
        self.assertIsNotNone(result.vega)
        self.assertIsNotNone(result.rho)
        snapshot = note.observations
        with self.assertRaises(ValueError):
            note.replace(barrier_state="invalid")
        self.assertEqual(note.observations, snapshot)
        context = market()
        curve = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.04 * 90 / 365)]
        )
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=curve)
        )
        self.assertAlmostEqual(
            engine.price(note, curved), engine.price(note, context), delta=1e-12
        )
        short = RateTermStructure(
            reference_time=START, times=[date(2025, 2, 3)], factors=[0.99]
        )
        original_price = engine.price(note, context)
        invalid = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=short)
        )
        self.assert_rejection(InvalidInputError, lambda: engine.price(note, invalid))
        self.assertEqual(engine.price(note, context), original_price)

    def test_fixed_prices_and_unidentifiable_parameters_validate_first(self):
        note = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations(
                [END], [110], knock_out_coupon_rates=[0]
            ),
        )
        context = market(rate=0)
        engine = FD(asset_upper_boundary=1)
        result = engine.price_with_greeks(note, context, ["delta", "gamma", "vega"])
        self.assertEqual(
            (result.price, result.delta, result.gamma, result.vega), (1, 0, 0, 0)
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_volatility(engine, note, context, 1),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_coupon(
                engine,
                note,
                market(100, END),
                1,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: implied_coupon(
                engine,
                note,
                context,
                math.nan,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        missing = snowball(
            knock_in_observation_mode="every_trading_day", barrier_state=None
        )
        later = market(100, date(2025, 2, 3))
        self.assert_rejection(InvalidInputError, lambda: engine.price(missing, later))
        self.assertIsNone(missing.barrier_state)


class AutocallableQuantLibTests(unittest.TestCase):
    def test_terminal_portfolios_and_separate_grid_convergence(self):
        from support.structured import contract, market, references, settings

        rows = [
            row
            for row in references()
            if row["engine"] == "FiniteDifferenceAutocallableEngine"
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
        for row in rows:
            note, context, fine = contract(row), market(row), settings(row)
            budget = float(row["tolerances"]["price"]) + float(
                row["inputs"]["uncertainty_price"]
            )
            with self.subTest(case=row["case_id"]):
                refined = FD(**fine).price(note, context)
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
                        with self.subTest(settings=configuration):
                            self.assertAlmostEqual(
                                FD(**configuration).price(note, context),
                                refined,
                                delta=budget,
                            )
