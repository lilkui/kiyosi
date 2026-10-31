"""FCN/DCN coupon units and staged snowballs through the public Python API."""

import datetime
import inspect
import math
import unittest
from datetime import date

from kiyosi import InvalidInputError
from kiyosi.instruments import (
    AutocallableObservation,
    PhoenixOption,
    autocallable_observations,
    butterfly_snowball,
    digital_coupon_note,
    early_profit_snowball,
    fixed_coupon_note,
)
from kiyosi.pricing import (
    FiniteDifferenceAutocallableEngine,
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_coupon,
)
from test_snowball import market

START, MIDDLE, END = date(2025, 1, 1), date(2025, 2, 3), date(2025, 4, 1)


def terms(**changes):
    fields = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "knock_out_level": 110,
        "observation_dates": [MIDDLE, END],
        "effective_date": START,
        "expiry_date": END,
    }
    fields.update(changes)
    return fields


def phoenix_fields(**changes):
    fields = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 80,
        "lower_strike": 0,
        "knock_in_observation_mode": "at_expiry",
        "effective_date": START,
        "expiry_date": END,
        "barrier_state": "none",
        "observations": autocallable_observations([MIDDLE, END], [110, 110]),
    }
    fields.update(changes)
    return fields


def engines():
    return (
        QuadratureAutocallableEngine(),
        FiniteDifferenceAutocallableEngine(),
        MonteCarloAutocallableEngine(path_count=4096, seed=42),
    )


class AutocallablePresetTests(unittest.TestCase):
    def test_fcn_dcn_expand_to_plain_phoenix_and_own_their_terms(self):
        for factory, extra, barrier in (
            (fixed_coupon_note, {}, 0),
            (digital_coupon_note, {"coupon_barrier_level": 90}, 90),
        ):
            dates = [MIDDLE, END]
            note = factory(
                coupon_per_period=0.01, **terms(observation_dates=dates), **extra
            )
            expected = PhoenixOption(
                coupon_per_period=0.01,
                **phoenix_fields(
                    barrier_state=None,
                    observations=autocallable_observations(
                        dates, [110, 110], coupon_barrier_levels=[barrier, barrier]
                    ),
                ),
            )
            self.assertIs(type(note), PhoenixOption)
            self.assertEqual(note, expected)
            self.assertIsNone(note.coupon_rate)
            self.assertEqual(note.coupon_per_period, 0.01)
            self.assertEqual(note.upper_strike, 80)
            self.assertEqual(note.lower_strike, 0)
            self.assertEqual(note.knock_in_observation_mode, "at_expiry")
            self.assertIsNone(note.barrier_state)
            self.assertEqual(note.principal_ratio, 1)
            dates.clear()
            self.assertEqual(note.observation_dates, [MIDDLE, END])
            self.assertEqual(note.replace(barrier_state="none").coupon_per_period, 0.01)
            self.assertEqual(
                note.replace(barrier_state="none").replace(barrier_state=None), note
            )

    def test_phoenix_coupon_choice_signatures_getters_and_repr_roundtrip(self):
        signature = inspect.signature(PhoenixOption)
        for key in ("coupon_rate", "coupon_per_period"):
            self.assertIs(signature.parameters[key].default, Ellipsis)
        for key in ("coupon_rate", "coupon_per_period"):
            fields = {key: -0.01}
            note = PhoenixOption(**fields, **phoenix_fields())
            unused = (
                "coupon_rate" if key == "coupon_per_period" else "coupon_per_period"
            )
            self.assertEqual(getattr(note, key), -0.01)
            self.assertIsNone(getattr(note, unused))
            self.assertIn(f"{key}=-0.01", repr(note))
            self.assertNotIn(f"{unused}=", repr(note).split(", initial_spot=", 1)[0])
            self.assertEqual(
                eval(
                    repr(note),
                    {
                        "datetime": datetime,
                        "PhoenixOption": PhoenixOption,
                        "AutocallableObservation": AutocallableObservation,
                    },
                ),
                note,
            )
            forwarded = {"coupon_rate": Ellipsis, "coupon_per_period": Ellipsis}
            forwarded.update(fields)
            self.assertEqual(PhoenixOption(**forwarded, **phoenix_fields()), note)
            with self.assertRaises(AttributeError):
                setattr(note, key, 0.02)
        for fields in ({}, {"coupon_rate": 0.1, "coupon_per_period": 0.01}):
            with self.subTest(fields=fields), self.assertRaises(TypeError):
                PhoenixOption(**fields, **phoenix_fields())
        for key in ("coupon_rate", "coupon_per_period"):
            for value in (True, "0.01", None):
                with self.subTest(key=key, value=value), self.assertRaises(TypeError):
                    PhoenixOption(**{key: value}, **phoenix_fields())
            for value in (math.nan, math.inf, -math.inf):
                with (
                    self.subTest(key=key, value=value),
                    self.assertRaises(InvalidInputError),
                ):
                    PhoenixOption(**{key: value}, **phoenix_fields())

    def test_shared_fixed_coupon_and_downside_boundary_scenarios_in_all_engines(self):
        for factory, extra, barrier in (
            (fixed_coupon_note, {}, 0),
            (digital_coupon_note, {"coupon_barrier_level": 90}, 90),
        ):
            for coupon in (0.01, -0.01):
                note = factory(
                    coupon_per_period=coupon,
                    **terms(principal_ratio=0.25, barrier_state="none"),
                    **extra,
                )
                for engine in engines():
                    for spot, base in (
                        (70, 0.15),
                        (80, 0.25),
                        (89.999, 0.25),
                        (90, 0.25),
                        (110, 0.25),
                    ):
                        with self.subTest(
                            factory=factory, engine=engine, spot=spot, coupon=coupon
                        ):
                            expected = base + (coupon if spot >= barrier else 0)
                            self.assertAlmostEqual(
                                engine.price(note, market(spot, END)),
                                expected,
                                places=14,
                            )
                    self.assertAlmostEqual(
                        engine.price(note, market(110, MIDDLE)),
                        0.25 + coupon,
                        places=14,
                    )
        note = digital_coupon_note(
            coupon_per_period=0.01,
            coupon_barrier_level=90,
            **terms(knock_out_level=85, barrier_state="none"),
        )
        for engine in engines():
            self.assertEqual(engine.price(note, market(85, MIDDLE)), 1)

    def test_multiple_fixed_payments_do_not_accrue_days_or_repay_past_coupons(self):
        for factory, extra, spot, expected in (
            (fixed_coupon_note, {}, 100, 1.02),
            (fixed_coupon_note, {}, 70, 0.92),
            (digital_coupon_note, {"coupon_barrier_level": 90}, 100, 1.02),
            (digital_coupon_note, {"coupon_barrier_level": 90}, 85, 1.0),
        ):
            note = factory(
                coupon_per_period=0.01, **terms(barrier_state="none"), **extra
            )
            for engine in engines():
                with self.subTest(factory=factory, spot=spot, engine=engine):
                    context = market(spot, START, rate=0, dividend=0, volatility=1e-8)
                    self.assertAlmostEqual(
                        engine.price(note, context), expected, places=9
                    )
                    after = market(
                        spot, date(2025, 2, 4), rate=0, dividend=0, volatility=1e-8
                    )
                    remaining = expected - (
                        0.01 if factory is fixed_coupon_note or spot >= 90 else 0
                    )
                    self.assertAlmostEqual(
                        engine.price(note, after), remaining, places=9
                    )
                    self.assertEqual(
                        engine.price(note.replace(barrier_state="knocked_out"), after),
                        0,
                    )
        annual = PhoenixOption(coupon_rate=0.365, **phoenix_fields())
        fixed = PhoenixOption(coupon_per_period=0.01, **phoenix_fields())
        engine = QuadratureAutocallableEngine()
        self.assertAlmostEqual(
            engine.price(annual, market(110, MIDDLE)), 1 + 0.365 * 33 / 365
        )
        self.assertAlmostEqual(engine.price(fixed, market(110, MIDDLE)), 1.01)

    def test_implied_coupon_bounds_tolerance_and_result_use_contract_units(self):
        for factory, extra in (
            (fixed_coupon_note, {}),
            (digital_coupon_note, {"coupon_barrier_level": 90}),
        ):
            note = factory(coupon_per_period=0.01, **terms(), **extra)
            target = factory(coupon_per_period=-0.02, **terms(), **extra)
            for engine in engines():
                context = market()
                quote = engine.price(target, context)
                solved = implied_coupon(
                    engine, note, context, quote, lower_bound=-0.05, upper_bound=0.05
                )
                self.assertAlmostEqual(solved, -0.02, places=7)
                self.assertEqual(note.coupon_per_period, 0.01)
                self.assertIsNone(note.coupon_rate)

    def test_staged_snowballs_use_inclusive_cutoffs_and_whole_holding_coupons(self):
        dates = [MIDDLE, date(2025, 2, 4), date(2025, 3, 3), date(2025, 3, 4), END]
        cases = (
            (early_profit_snowball, {}, [0.2, 0.05, 0.05, 0.05, 0.05]),
            (
                butterfly_snowball,
                {"third_coupon_rate": -0.02, "second_stage_end_date": date(2025, 3, 3)},
                [0.2, 0.05, 0.05, -0.02, -0.02],
            ),
        )
        for factory, extra, rates in cases:
            note = factory(
                first_coupon_rate=0.2,
                second_coupon_rate=0.05,
                first_stage_end_date=MIDDLE,
                **terms(observation_dates=dates, barrier_state="none"),
                **extra,
            )
            self.assertEqual(note.knock_out_coupon_rates, rates)
            self.assertEqual(note.maturity_coupon_rate, rates[-1])
            for day, rate in zip(dates, rates, strict=True):
                expected = 1 + rate * (day - START).days / 365
                for engine in engines():
                    self.assertAlmostEqual(
                        engine.price(note, market(110, day)), expected, places=14
                    )
            self.assertAlmostEqual(
                QuadratureAutocallableEngine().price(note, market(100, END)),
                1 + rates[-1] * 90 / 365,
            )

    def test_stage_validation_checks_unused_rates_and_accepts_empty_stages(self):
        fields = {
            "first_coupon_rate": 0.2,
            "second_coupon_rate": -0.02,
            "first_stage_end_date": date(2025, 2, 1),
        }
        note = early_profit_snowball(**fields, **terms(observation_dates=[END]))
        self.assertEqual(note.knock_out_coupon_rates, [-0.02])
        increasing = butterfly_snowball(
            first_coupon_rate=-0.02,
            second_coupon_rate=0,
            third_coupon_rate=0.2,
            first_stage_end_date=date(2025, 2, 1),
            second_stage_end_date=date(2025, 3, 1),
            **terms(),
        )
        self.assertEqual(increasing.knock_out_coupon_rates, [0, 0.2])
        self.assertEqual(increasing.maturity_coupon_rate, 0.2)
        for cutoff in (date(2024, 12, 31), END, date(2025, 4, 2)):
            with self.assertRaises(InvalidInputError):
                early_profit_snowball(
                    **(fields | {"first_stage_end_date": cutoff}), **terms()
                )
        for cutoff in (date(2025, 1, 31), date(2025, 2, 1), END):
            with self.assertRaises(InvalidInputError):
                butterfly_snowball(
                    **fields,
                    third_coupon_rate=0.01,
                    second_stage_end_date=cutoff,
                    **terms(),
                )
        for key in ("first_coupon_rate", "second_coupon_rate"):
            for value in (math.nan, math.inf, -math.inf):
                with (
                    self.subTest(key=key, value=value),
                    self.assertRaises(InvalidInputError),
                ):
                    early_profit_snowball(
                        **(fields | {key: value}), **terms(observation_dates=[END])
                    )
        for value in (math.nan, math.inf, -math.inf):
            with self.assertRaises(InvalidInputError):
                butterfly_snowball(
                    **fields,
                    third_coupon_rate=value,
                    second_stage_end_date=date(2025, 3, 1),
                    **terms(),
                )
        for value in (True, "2025-02-01", datetime.datetime(2025, 2, 1)):
            with self.assertRaises(TypeError):
                early_profit_snowball(
                    **(fields | {"first_stage_end_date": value}), **terms()
                )

    def test_preset_representation_and_domain_errors_use_native_categories(self):
        for value in (True, "0.01", None):
            with self.assertRaises(TypeError):
                fixed_coupon_note(coupon_per_period=value, **terms())
        for value in (math.nan, math.inf, -math.inf):
            with self.assertRaises(InvalidInputError):
                fixed_coupon_note(coupon_per_period=value, **terms())
        for barrier in (-1, math.nan, math.inf):
            with self.assertRaises(InvalidInputError):
                digital_coupon_note(
                    coupon_per_period=0.01, coupon_barrier_level=barrier, **terms()
                )
        for schedule in ([], [END, MIDDLE], [MIDDLE, MIDDLE], [date(2025, 4, 2)]):
            with self.assertRaises(InvalidInputError):
                fixed_coupon_note(
                    coupon_per_period=0.01, **terms(observation_dates=schedule)
                )
        with self.assertRaises(TypeError):
            digital_coupon_note(coupon_per_period=0.01, **terms())
        for factory, parameter in (
            (fixed_coupon_note, "coupon_per_period"),
            (digital_coupon_note, "coupon_barrier_level"),
            (early_profit_snowball, "first_stage_end_date"),
            (butterfly_snowball, "second_stage_end_date"),
        ):
            self.assertEqual(
                inspect.signature(factory).parameters[parameter].kind,
                inspect.Parameter.KEYWORD_ONLY,
            )
