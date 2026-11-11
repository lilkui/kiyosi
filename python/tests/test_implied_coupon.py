"""Coupon quote validation and pricing through the Python API."""

import inspect
import math
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from datetime import time as clock_time
from typing import get_args

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    RootNotBracketedError,
    UnsupportedOperationError,
    instruments,
)
from kiyosi.instruments import BinarySnowballOption, PhoenixOption
from kiyosi.market import (
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    RateTermStructure,
    weekdays_calendar,
)
from kiyosi.pricing import (
    CouponQuoteConvention,
    FiniteDifferenceAutocallableEngine,
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_phoenix_coupon,
    implied_snowball_coupon,
)
from support.assertions import DomainTestCase
from support.history import dated as dated_history
from support.history import history_fields
from test_phoenix import contract as phoenix
from test_snowball import END, START, autocallable_observations, market
from test_snowball import contract as snowball


def binary(coupon=0.08, maturity=0.06, **changes):
    fields = {
        "maturity_coupon_rate": maturity,
        "effective_time": START,
        "expiry_time": END,
        "observations": autocallable_observations(
            [END], [110], knock_out_coupon_rates=[coupon]
        ),
    }
    fields.update(changes)
    return BinarySnowballOption(
        calendar=weekdays_calendar(), **history_fields(**fields)
    )


class ImpliedCouponTests(DomainTestCase):
    def test_coupon_solvers_reprice_local_volatility_quotes(self):
        context = market().replace(
            model_parameters=LocalVolatilityParameters(
                risk_free_rate=0.04,
                dividend_yield=0.01,
                surface=LocalVolatilitySurface(
                    times=[START, END],
                    spots=[60, 100, 160],
                    values=[[0.32, 0.24, 0.16], [0.28, 0.22, 0.18]],
                ),
            )
        )
        engine = FiniteDifferenceAutocallableEngine(
            asset_step_count=200, time_step_count=100
        )
        note = binary()
        for convention in get_args(CouponQuoteConvention):
            target = engine.price(
                note.with_quoted_coupon(0.12, quote_convention=convention), context
            )
            solved = implied_snowball_coupon(
                engine, note, context, target, quote_convention=convention
            )
            self.assertAlmostEqual(solved["first_observation_coupon"], 0.12, delta=1e-7)
            self.assertEqual(solved["quote_convention"], convention)
            self.assertAlmostEqual(
                engine.price(note.with_quoted_coupon(**solved), context),
                target,
                delta=1e-8,
            )
            with self.assertRaises(UnsupportedOperationError):
                implied_snowball_coupon(
                    QuadratureAutocallableEngine(),
                    note,
                    context,
                    target,
                    quote_convention=convention,
                )
        for field, coupon in (("coupon_per_period", 0.12), ("coupon_per_period", 0.03)):
            note = PhoenixOption(
                calendar=weekdays_calendar(),
                **{field: coupon},
                initial_spot=100,
                knock_in_level=80,
                upper_strike=100,
                lower_strike=60,
                effective_time=START,
                expiry_time=END,
                knock_in_observation_mode="at_expiry",
                observation_clock_time=None,
                observations=[
                    instruments.AutocallableObservation.phoenix(
                        END, 110, coupon_barrier_level=90
                    )
                ],
            )
            target = engine.price(note, context)
            solved = implied_phoenix_coupon(engine, note, context, target)
            self.assertEqual(set(solved), {field})
            self.assertAlmostEqual(solved[field], coupon, delta=1e-7)
            self.assertAlmostEqual(
                engine.price(note.with_quoted_coupon(**solved), context),
                target,
                delta=1e-8,
            )

    def test_monte_carlo_coupon_solvers_require_a_replayable_seed(self):
        unseeded = MonteCarloAutocallableEngine(path_count=1024, max_active_workers=1)
        engine = unseeded.with_resolved_seed()
        context = market()
        convention = "shift_maturity_coupon"
        note = binary()
        target = engine.price(
            note.with_quoted_coupon(0.12, quote_convention=convention), context
        )
        with self.assertRaisesRegex(InvalidInputError, "with_resolved_seed"):
            implied_snowball_coupon(
                unseeded, note, context, target, quote_convention=convention
            )
        solved = implied_snowball_coupon(
            engine, note, context, target, quote_convention=convention
        )
        self.assertAlmostEqual(solved["first_observation_coupon"], 0.12, delta=1e-8)
        self.assertAlmostEqual(
            engine.price(note.with_quoted_coupon(**solved), context),
            target,
            delta=1e-8,
        )
        for note in (phoenix(), phoenix().replace(coupon_per_period=0.02)):
            note = dated_history(note, context)
            target = engine.price(note, context)
            with self.assertRaisesRegex(InvalidInputError, "with_resolved_seed"):
                implied_phoenix_coupon(unseeded, note, context, target)
            quote = implied_phoenix_coupon(engine, note, context, target)
            self.assertAlmostEqual(
                engine.price(note.replace(**quote), context), target, delta=1e-8
            )
        self.assertIsNotNone(engine.seed)
        self.assertIsNone(unseeded.seed)

    def test_ternary_coupon_solver_intersects_bounds_with_valid_maturity_coupons(self):
        context = market()
        for engine in (
            QuadratureAutocallableEngine(asset_step_count=64),
            FiniteDifferenceAutocallableEngine(
                asset_step_count=200, time_step_count=100
            ),
            MonteCarloAutocallableEngine(path_count=512, seed=42, max_active_workers=1),
        ):
            for maturity, minimum, anchor in (
                (0.1, 0.05, 0.1),
                (0.06, 0.02, 0.08),
                (0.07, 0.02, 0.13),
                (0.4047152283389133, 0.002630147007026187, 0.40290962591640395),
            ):
                note = instruments.TernarySnowballOption(
                    calendar=weekdays_calendar(),
                    observation_clock_time=clock_time(),
                    maturity_coupon_rate=maturity,
                    minimum_coupon_rate=minimum,
                    knock_in_level=80,
                    effective_time=START,
                    expiry_time=END,
                    observations=autocallable_observations(
                        [END], [110], knock_out_coupon_rates=[anchor]
                    ),
                )
                quote = engine.price(dated_history(note, context), context)
                for lower in (0, -0.1):
                    with self.subTest(
                        engine=type(engine).__name__, maturity=maturity, lower=lower
                    ):
                        solved = implied_snowball_coupon(
                            engine,
                            dated_history(note, context),
                            context,
                            quote,
                            quote_convention="shift_maturity_coupon",
                            lower_bound=lower,
                        )
                        self.assertAlmostEqual(
                            solved["first_observation_coupon"], anchor, delta=1e-7
                        )
                with self.assertRaises(RootNotBracketedError):
                    implied_snowball_coupon(
                        engine,
                        dated_history(note, context),
                        context,
                        quote,
                        quote_convention="shift_maturity_coupon",
                        lower_bound=-0.1,
                        upper_bound=0.01,
                    )
                with self.assertRaises(InvalidInputError):
                    implied_snowball_coupon(
                        engine,
                        dated_history(note, context),
                        context,
                        quote,
                        quote_convention="shift_maturity_coupon",
                        lower_bound=math.nan,
                    )
                fixed = note.with_quoted_coupon(
                    0, quote_convention="preserve_maturity_coupon"
                )
                self.assertEqual(
                    implied_snowball_coupon(
                        engine,
                        dated_history(note, context),
                        context,
                        engine.price(dated_history(fixed, context), context),
                        quote_convention="preserve_maturity_coupon",
                    )["first_observation_coupon"],
                    0,
                )
                self.assertEqual(note.maturity_coupon_rate, maturity)
                self.assertEqual(note.minimum_coupon_rate, minimum)
            boundary = instruments.TernarySnowballOption(
                calendar=weekdays_calendar(),
                observation_clock_time=clock_time(),
                maturity_coupon_rate=0.05,
                minimum_coupon_rate=0.05,
                knock_in_level=80,
                effective_time=START,
                expiry_time=END,
                observations=autocallable_observations(
                    [END], [110], knock_out_coupon_rates=[0.05]
                ),
            )
            self.assertAlmostEqual(
                implied_snowball_coupon(
                    engine,
                    dated_history(boundary, context),
                    context,
                    engine.price(dated_history(boundary, context), context),
                    quote_convention="shift_maturity_coupon",
                )["first_observation_coupon"],
                0.05,
                delta=1e-15,
            )

    def test_coupon_replacements_apply_solved_quotes_without_changing_notes(self):
        engine = QuadratureAutocallableEngine(asset_step_count=64)
        context = market()
        observations = autocallable_observations(
            [datetime(2025, 2, 3), END], [110, 105], knock_out_coupon_rates=[0.08, 0.12]
        )
        notes = [
            snowball(observations=observations, maturity_coupon_rate=0.06),
            binary(observations=observations),
            instruments.TernarySnowballOption(
                calendar=weekdays_calendar(),
                observation_clock_time=None,
                maturity_coupon_rate=0.06,
                minimum_coupon_rate=-0.2,
                knock_in_level=80,
                knock_in_observation_mode="at_expiry",
                effective_time=START,
                expiry_time=END,
                observations=observations,
            ),
        ]
        for note in notes:
            self.assertEqual(note.replace_history(), note)
            self.assertEqual(
                note.replace_history(
                    barrier_state=Ellipsis,
                ),
                note,
            )
            for convention in get_args(CouponQuoteConvention):
                with self.subTest(note=type(note).__name__, convention=convention):
                    replacement = note.with_quoted_coupon(
                        0.18, quote_convention=convention
                    )
                    self.assertIs(type(replacement), type(note))
                    self.assertIsNot(replacement, note)
                    for actual, expected in zip(
                        replacement.knock_out_coupon_rates, [0.18, 0.22], strict=True
                    ):
                        self.assertAlmostEqual(actual, expected, delta=1e-15)
                    maturity = 0.16 if convention == "shift_maturity_coupon" else 0.06
                    self.assertAlmostEqual(replacement.maturity_coupon_rate, maturity)
                    self.assertEqual(replacement.barrier_state, note.barrier_state)
                    self.assertEqual(
                        replacement.knock_out_levels, note.knock_out_levels
                    )
                    target = engine.price(dated_history(replacement, context), context)
                    solved = implied_snowball_coupon(
                        engine,
                        dated_history(note, context),
                        context,
                        target,
                        quote_convention=convention,
                    )
                    self.assertAlmostEqual(
                        engine.price(
                            dated_history(
                                note.with_quoted_coupon(**solved),
                                context,
                            ),
                            context,
                        ),
                        target,
                        delta=1e-8,
                    )
                    with self.assertRaises(InvalidInputError):
                        note.with_quoted_coupon(math.nan, quote_convention=convention)
                    with self.assertRaises(TypeError):
                        note.with_quoted_coupon(True, quote_convention=convention)
            with self.assertRaises(ValueError):
                note.with_quoted_coupon(0.1, quote_convention="unknown")
            self.assertEqual(note.knock_out_coupon_rates, (0.08, 0.12))
            self.assertEqual(note.maturity_coupon_rate, 0.06)

        for note in (
            phoenix(),
            phoenix(coupon_per_period=0.02),
        ):
            self.assertEqual(note.replace_history(), note)
            self.assertEqual(
                note.replace_history(
                    barrier_state=Ellipsis,
                ),
                note,
            )
            field = "coupon_per_period"
            original = getattr(note, field)
            replacement = note.replace(**{field: 0.18})
            self.assertEqual(getattr(replacement, field), 0.18)
            self.assertEqual(replacement.barrier_state, note.barrier_state)
            self.assertEqual(replacement.observations, note.observations)
            target = engine.price(dated_history(replacement, context), context)
            solved = implied_phoenix_coupon(
                engine, dated_history(note, context), context, target
            )
            self.assertAlmostEqual(
                engine.price(dated_history(note.replace(**solved), context), context),
                target,
                delta=1e-8,
            )
            for value, error in ((math.inf, InvalidInputError), (True, TypeError)):
                with self.assertRaises(error):
                    note.replace(**{field: value})
            self.assertEqual(getattr(note, field), original)

    def test_quoted_phoenix_coupons_preserve_units_and_validate_inputs(self):
        context = market().replace(
            model_parameters=market().model_parameters.replace(
                risk_free_rate=0, dividend_yield=0, volatility=0
            )
        )
        engine = QuadratureAutocallableEngine()
        for field, note in (("coupon_per_period", phoenix(coupon_per_period=0.01)),):
            for coupon in (-0.02, 0, 0.02):
                with self.subTest(field=field, coupon=coupon):
                    changed = note.with_quoted_coupon(**{field: coupon})
                    self.assertEqual(changed, note.replace(**{field: coupon}))
                    quote = engine.price(dated_history(changed, context), context)
                    solved = implied_phoenix_coupon(
                        engine,
                        dated_history(note, context),
                        context,
                        quote,
                        lower_bound=-1,
                    )
                    self.assertEqual(set(solved), {field})
                    self.assertAlmostEqual(solved[field], coupon, delta=1e-12)
                    with self.assertRaises(TypeError):
                        note.replace(coupon_per_period=solved)
                    self.assertAlmostEqual(
                        engine.price(
                            dated_history(note.replace(**solved), context),
                            context,
                        ),
                        quote,
                        delta=1e-12,
                    )
            for value, error in (
                (math.nan, InvalidInputError),
                (math.inf, InvalidInputError),
                (-math.inf, InvalidInputError),
                (True, TypeError),
                (None, TypeError),
            ):
                with self.subTest(field=field, value=value), self.assertRaises(error):
                    note.with_quoted_coupon(**{field: value})
            for keyword in ("coupon_per_period",):
                quoted = note.with_quoted_coupon(**{keyword: 0.12})
                self.assertEqual(getattr(quoted, keyword), 0.12)
                self.assertEqual(quoted.barrier_state, note.barrier_state)
            for choices in ({}, {"coupon_rate": 0.12, "coupon_per_period": 0.01}):
                with self.assertRaises(TypeError):
                    note.with_quoted_coupon(**choices)
            with self.assertRaises(TypeError):
                note.with_quoted_coupon(0.12)
            self.assertEqual(getattr(note, field), 0.01)

    def test_coupon_signatures_express_product_requirements(self):
        engine = QuadratureAutocallableEngine(asset_step_count=64)
        note, context = phoenix(), market()
        quote = engine.price(dated_history(note, context), context)
        signature = inspect.signature(implied_phoenix_coupon)
        self.assertNotIn("quote_convention", signature.parameters)
        bound = signature.bind(engine, dated_history(note, context), context, quote)
        bound.apply_defaults()
        solved = implied_phoenix_coupon(*bound.args, **bound.kwargs)
        self.assertEqual(set(solved), {"coupon_per_period"})
        self.assertAlmostEqual(
            solved["coupon_per_period"], note.coupon_per_period, delta=1e-8
        )
        with self.assertRaises(TypeError):
            implied_phoenix_coupon(
                engine,
                dated_history(note, context),
                context,
                quote,
                quote_convention=None,
            )
        signature = inspect.signature(implied_snowball_coupon)
        self.assertIs(
            signature.parameters["quote_convention"].default, inspect.Parameter.empty
        )
        with self.assertRaises(TypeError):
            signature.bind(engine, binary(), context, 1)
        with self.assertRaises(UnsupportedOperationError):
            implied_snowball_coupon(
                engine,
                dated_history(note, context),
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
            )
        with self.assertRaises(UnsupportedOperationError):
            implied_phoenix_coupon(engine, dated_history(binary(), context), context, 1)

    def test_coupon_endpoints_can_change_daily_knock_in_layers(self):
        end = datetime(2025, 1, 13)
        note = snowball(
            maturity_coupon_rate=0.073,
            knock_in_level=99,
            lower_strike=100,
            knock_in_observation_mode="every_trading_day",
            barrier_state="no_event",
            expiry_time=end,
            observations=autocallable_observations(
                [datetime(2025, 1, 7), end],
                [110, 110],
                knock_out_coupon_rates=[0.073, 0.083],
            ),
        )
        context = market(
            100.25,
            datetime(2025, 1, 1, 12),
            rate=0.03,
            dividend=0,
        )
        engine = QuadratureAutocallableEngine(asset_step_count=17)
        quote = engine.price(dated_history(note, context), context)
        for convention in get_args(CouponQuoteConvention):
            solved = implied_snowball_coupon(
                engine,
                dated_history(note, context),
                context,
                quote,
                quote_convention=convention,
                lower_bound=0,
                upper_bound=0.2,
                price_tolerance=1e-13,
                parameter_tolerance=1e-12,
            )
            self.assertAlmostEqual(
                solved["first_observation_coupon"], 0.073, delta=1e-9
            )

    def test_binary_recovers_both_quote_conventions(self):
        engine = QuadratureAutocallableEngine()
        note, context = binary(), market()
        for convention in ("shift_maturity_coupon", "preserve_maturity_coupon"):
            maturity = 0.16 if convention == "shift_maturity_coupon" else 0.06
            quote = engine.price(
                dated_history(binary(0.18, maturity), context), context
            )
            solved = implied_snowball_coupon(
                engine,
                dated_history(note, context),
                context,
                quote,
                quote_convention=convention,
            )
            self.assertAlmostEqual(solved["first_observation_coupon"], 0.18, delta=1e-7)
            self.assertAlmostEqual(
                engine.price(
                    dated_history(note.with_quoted_coupon(**solved), context), context
                ),
                quote,
                delta=1e-8,
            )
        self.assertEqual(note.knock_out_coupon_rates, (0.08,))
        self.assertEqual(note.maturity_coupon_rate, 0.06)

    def test_all_products_and_presets_recover_signed_quotes_and_residuals(self):
        self.check_all_products(QuadratureAutocallableEngine(asset_step_count=64))
        self.check_all_products(FiniteDifferenceAutocallableEngine(asset_step_count=64))

    def check_all_products(self, engine):
        context = market()
        dates = [datetime(2025, 2, 3), END]

        def note(kind, coupon, convention):
            if kind == "PhoenixOption":
                return phoenix(
                    coupon_per_period=coupon,
                    knock_in_observation_mode="every_trading_day",
                )
            if kind in (
                "SnowballOption",
                "BinarySnowballOption",
                "TernarySnowballOption",
            ):
                fields = {
                    "maturity_coupon_rate": 0.06
                    + (coupon - 0.08 if convention == "shift_maturity_coupon" else 0),
                    "effective_time": START,
                    "expiry_time": END,
                    "observations": autocallable_observations(
                        dates,
                        [110, 105],
                        knock_out_coupon_rates=[coupon, coupon + 0.04],
                    ),
                }
                if kind != "BinarySnowballOption":
                    fields.update(
                        knock_in_level=80,
                        knock_in_observation_mode="every_trading_day",
                        observation_clock_time=clock_time(),
                    )
                if kind == "SnowballOption":
                    fields.update(
                        initial_spot=100,
                        upper_strike=100,
                        lower_strike=60,
                        knock_in_observation_mode="every_trading_day",
                    )
                if kind == "TernarySnowballOption":
                    fields.update(minimum_coupon_rate=-0.2)
                return getattr(instruments, kind)(
                    calendar=weekdays_calendar(), **history_fields(**fields)
                )
            fields = {
                "coupon_rate": coupon,
                "observation_clock_time": None
                if kind == "european_snowball"
                else clock_time(),
                "initial_spot": 100,
                "knock_in_level": 80,
                "knock_out_level": 110,
                "effective_time": START,
                "expiry_time": END,
                "observation_times": dates,
            }
            if kind == "step_down_snowball":
                fields.update(
                    initial_knock_out_level=fields.pop("knock_out_level"),
                    knock_out_level_decrement=2,
                )
            elif kind == "both_down_snowball":
                fields.update(
                    initial_coupon_rate=fields.pop("coupon_rate"),
                    initial_knock_out_level=fields.pop("knock_out_level"),
                    coupon_rate_decrement=0.01,
                    knock_out_level_decrement=2,
                )
            elif kind == "dual_coupon_snowball":
                fields.update(
                    knock_out_coupon_rate=fields.pop("coupon_rate"),
                    maturity_coupon_rate=0.06 + coupon - 0.08,
                )
            elif kind == "parachute_snowball":
                fields.update(final_knock_out_level=95)
            elif kind == "otm_snowball":
                fields.update(upper_strike=90)
            elif kind == "loss_capped_snowball":
                fields.update(lower_strike=60)
            return getattr(instruments, kind)(**history_fields(**fields))

        kinds = (
            "SnowballOption",
            "BinarySnowballOption",
            "TernarySnowballOption",
            "PhoenixOption",
            "standard_snowball",
            "european_snowball",
            "step_down_snowball",
            "both_down_snowball",
            "dual_coupon_snowball",
            "parachute_snowball",
            "otm_snowball",
            "loss_capped_snowball",
        )
        for kind in kinds:
            conventions = (
                (None,)
                if kind == "PhoenixOption"
                else get_args(CouponQuoteConvention)
                if kind.endswith("Option")
                else ("shift_maturity_coupon",)
            )
            for convention in conventions:
                original = note(kind, 0.08, convention)
                snapshot = repr(original), repr(context), repr(engine)
                for coupon in (-0.05, 0.18):
                    with self.subTest(kind=kind, convention=convention, coupon=coupon):
                        target = note(kind, coupon, convention)
                        quote = engine.price(dated_history(target, context), context)
                        kwargs = (
                            {}
                            if convention is None
                            else {"quote_convention": convention}
                        )
                        solved = (
                            implied_phoenix_coupon
                            if isinstance(original, PhoenixOption)
                            else implied_snowball_coupon
                        )(
                            engine,
                            dated_history(original, context),
                            context,
                            quote,
                            lower_bound=-0.1,
                            upper_bound=0.3,
                            price_tolerance=1e-11,
                            **kwargs,
                        )
                        if isinstance(original, PhoenixOption):
                            self.assertEqual(set(solved), {"coupon_per_period"})
                            actual_coupon = solved["coupon_per_period"]
                        else:
                            self.assertEqual(
                                set(solved),
                                {"first_observation_coupon", "quote_convention"},
                            )
                            self.assertEqual(solved["quote_convention"], convention)
                            actual_coupon = solved["first_observation_coupon"]
                        self.assertAlmostEqual(actual_coupon, coupon, delta=1e-8)
                        self.assertAlmostEqual(
                            engine.price(
                                dated_history(
                                    original.with_quoted_coupon(**solved), context
                                ),
                                context,
                            ),
                            quote,
                            delta=1e-11,
                        )
                        self.assertEqual(
                            (repr(original), repr(context), repr(engine)), snapshot
                        )

    def test_independent_quantlib_terminal_quote(self):
        term = 90 / 365
        discount = math.exp(-0.04 * term)
        forward = 100 * math.exp((0.04 - 0.01) * term)
        probability = ql.BlackCalculator(
            ql.CashOrNothingPayoff(ql.Option.Call, 110, 1),
            forward,
            0.3 * math.sqrt(term),
            discount,
        ).value()
        quote = discount * (1 + 0.06 * term) + (0.18 - 0.06) * term * probability
        solved = implied_snowball_coupon(
            QuadratureAutocallableEngine(),
            dated_history(binary(), market()),
            market(),
            quote,
            quote_convention="preserve_maturity_coupon",
        )
        self.assertAlmostEqual(solved["first_observation_coupon"], 0.18, delta=1e-8)

    def test_bounds_defaults_and_validation_order(self):
        engine, note, context = QuadratureAutocallableEngine(), binary(), market()

        def solve(quote, **settings):
            return implied_snowball_coupon(
                engine,
                dated_history(note, context),
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
                **settings,
            )

        for bound in (0, 2):
            self.assertEqual(
                solve(engine.price(dated_history(binary(bound), context), context))[
                    "first_observation_coupon"
                ],
                bound,
            )
        for settings in (
            {"lower_bound": math.nan},
            {"upper_bound": math.inf},
            {"lower_bound": 2},
            {"lower_bound": 3},
            {"price_tolerance": 0},
            {"price_tolerance": math.nan},
            {"parameter_tolerance": -1},
            {"parameter_tolerance": math.inf},
            {"max_iterations": 0},
        ):
            self.assert_rejection(
                InvalidInputError, lambda settings=settings: solve(1, **settings)
            )
        for quote in (math.nan, math.inf):
            self.assert_rejection(InvalidInputError, lambda quote=quote: solve(quote))
        self.assert_rejection(RootNotBracketedError, lambda: solve(10))
        late = market(100, END)
        self.assert_rejection(
            InvalidInputError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(note, late),
                late,
                math.nan,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(note, late),
                late,
                1,
                quote_convention="preserve_maturity_coupon",
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
            InvalidInputError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(note, curved),
                curved,
                math.nan,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(note, curved),
                curved,
                1,
                quote_convention="preserve_maturity_coupon",
            ),
        )

    def test_no_exposure_current_and_past_events_and_ternary_validation(self):
        engine = QuadratureAutocallableEngine()
        known = binary(
            barrier_state="no_event",
            observations=autocallable_observations(
                [datetime(2025, 2, 3), END],
                [110, 110],
                knock_out_coupon_rates=[0.08, 0.08],
            ),
        )
        context = market(100, END)
        quote = engine.price(dated_history(known, context), context)
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(known, context),
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assertAlmostEqual(
            implied_snowball_coupon(
                engine,
                dated_history(known, context),
                context,
                quote,
                quote_convention="shift_maturity_coupon",
            )["first_observation_coupon"],
            0.08,
            delta=1e-8,
        )
        self.assertAlmostEqual(
            implied_snowball_coupon(
                engine,
                dated_history(known, market(110, END)),
                market(110, END),
                1 + 0.18 * 90 / 365,
                quote_convention="preserve_maturity_coupon",
            )["first_observation_coupon"],
            0.18,
            delta=1e-8,
        )
        for note, kwargs in (
            (
                snowball(barrier_state="no_event"),
                {"quote_convention": "shift_maturity_coupon"},
            ),
            (
                phoenix(
                    observations=autocallable_observations(
                        [END], [110], coupon_barrier_levels=[90]
                    )
                ),
                {},
            ),
        ):
            self.assert_rejection(
                UnsupportedOperationError,
                lambda note=note, kwargs=kwargs: (
                    implied_phoenix_coupon
                    if isinstance(note, PhoenixOption)
                    else implied_snowball_coupon
                )(
                    engine,
                    dated_history(note, market(70, END)),
                    market(70, END),
                    engine.price(dated_history(note, market(70, END)), market(70, END)),
                    **kwargs,
                ),
            )
        ternary = instruments.TernarySnowballOption(
            calendar=weekdays_calendar(),
            observation_clock_time=None,
            maturity_coupon_rate=0.06,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            knock_in_observation_mode="at_expiry",
            effective_time=START,
            expiry_time=END,
            observations=known.observations,
            barrier_state="no_event",
            history_time=START,
        )
        coupon = implied_snowball_coupon(
            engine,
            dated_history(ternary, market()),
            market(),
            engine.price(dated_history(ternary, market()), market()),
            quote_convention="shift_maturity_coupon",
        )
        self.assertAlmostEqual(
            coupon["first_observation_coupon"],
            ternary.observations[0].coupon_rate,
            delta=1e-7,
        )
        for note in (
            known,
            snowball(barrier_state="no_event", observations=known.observations),
            ternary,
            phoenix(),
        ):
            settled = note.replace_history(
                barrier_state="knocked_out",
                history_time=note.expiry_time,
            )
            kwargs = (
                {}
                if isinstance(note, PhoenixOption)
                else {"quote_convention": "preserve_maturity_coupon"}
            )
            self.assert_rejection(
                UnsupportedOperationError,
                lambda settled=settled, kwargs=kwargs: (
                    implied_phoenix_coupon
                    if isinstance(settled, PhoenixOption)
                    else implied_snowball_coupon
                )(
                    engine,
                    dated_history(settled, market(100, END)),
                    market(100, END),
                    0,
                    **kwargs,
                ),
            )

    def test_strict_protocols_and_concurrent_solves(self):
        engine, note, context = QuadratureAutocallableEngine(), binary(), market()

        def solve(**settings):
            return implied_snowball_coupon(
                engine, dated_history(note, context), context, 1, **settings
            )

        for value in (True, "0.1", 1j, None):
            for field in (
                "lower_bound",
                "upper_bound",
                "price_tolerance",
                "parameter_tolerance",
            ):
                with self.assertRaises(TypeError):
                    solve(quote_convention="preserve_maturity_coupon", **{field: value})
            with self.assertRaises(TypeError):
                implied_snowball_coupon(
                    engine,
                    dated_history(note, context),
                    context,
                    value,
                    quote_convention="preserve_maturity_coupon",
                )
        for value in (True, 1.5, 1.0, None):
            with self.assertRaises(TypeError):
                solve(quote_convention="preserve_maturity_coupon", max_iterations=value)
        for value in (2**31, -(2**31) - 1):
            with self.assertRaises(OverflowError):
                solve(quote_convention="preserve_maturity_coupon", max_iterations=value)
        for kwargs in (
            {},
            {"quote_convention": Ellipsis},
            {"quote_convention": None},
            {"quote_convention": True},
            {"quote_convention": "preserve_maturity_coupon", "unknown": 1},
        ):
            with self.assertRaises(TypeError):
                solve(**kwargs)
        with self.assertRaises(ValueError):
            solve(quote_convention="unknown")
        with self.assertRaises(TypeError):
            implied_phoenix_coupon(
                engine,
                dated_history(phoenix(), context),
                context,
                1,
                quote_convention="preserve_maturity_coupon",
            )
        with self.assertRaises(TypeError):
            implied_snowball_coupon(
                object(),
                dated_history(note, context),
                context,
                1,
                quote_convention="preserve_maturity_coupon",
            )
        with self.assertRaises(TypeError):
            implied_snowball_coupon(
                engine,
                dated_history(object(), context),
                context,
                1,
                quote_convention="preserve_maturity_coupon",
            )
        quote = engine.price(dated_history(binary(0.18), context), context)

        def recover(_):
            return implied_snowball_coupon(
                engine,
                dated_history(note, context),
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
            )

        with ThreadPoolExecutor(max_workers=2) as pool:
            for coupon in pool.map(recover, range(4)):
                self.assertAlmostEqual(
                    coupon["first_observation_coupon"], 0.18, delta=1e-8
                )

    def test_nonfinite_residual_and_exhausted_bisection_errors(self):
        engine = QuadratureAutocallableEngine(asset_step_count=17)
        note = binary(barrier_state="no_event")
        self.assert_rejection(
            ComputationFailureError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(note, market(110, END)),
                market(110, END),
                -sys.float_info.max,
                quote_convention="preserve_maturity_coupon",
                lower_bound=sys.float_info.max / 2,
                upper_bound=sys.float_info.max,
            ),
        )
        note = snowball(
            knock_in_observation_mode="every_trading_day",
            observations=autocallable_observations(
                [datetime(2025, 2, 3), END],
                [110, 105],
                knock_out_coupon_rates=[0.08, 0.12],
            ),
        )
        original = repr(note)
        self.assert_rejection(
            NonConvergenceError,
            lambda: implied_snowball_coupon(
                engine,
                dated_history(note, market()),
                market(),
                0.97,
                quote_convention="shift_maturity_coupon",
                price_tolerance=1e-30,
                parameter_tolerance=1e-30,
                max_iterations=1,
            ),
        )
        self.assertEqual(repr(note), original)
