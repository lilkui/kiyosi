"""Coupon quote validation and pricing through the Python API."""

import inspect
import math
import sys
import unittest
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
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    CouponQuoteConvention,
    FiniteDifferenceAutocallableEngine,
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_phoenix_coupon,
    implied_snowball_coupon,
)
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
    return BinarySnowballOption(**fields)


class ImpliedCouponTests(unittest.TestCase):
    def test_ternary_coupon_solver_intersects_bounds_with_valid_maturity_coupons(self):
        context = market()
        for engine in (
            QuadratureAutocallableEngine(asset_step_count=64),
            FiniteDifferenceAutocallableEngine(
                asset_step_count=200, time_step_count=100
            ),
            MonteCarloAutocallableEngine(path_count=512, seed=42, thread_count=1),
        ):
            for maturity, minimum, anchor in (
                (0.1, 0.05, 0.1),
                (0.06, 0.02, 0.08),
                (0.07, 0.02, 0.13),
                (0.4047152283389133, 0.002630147007026187, 0.40290962591640395),
            ):
                note = instruments.TernarySnowballOption(
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
                quote = engine.price(note, context)
                for lower in (0, -0.1):
                    with self.subTest(
                        engine=type(engine).__name__, maturity=maturity, lower=lower
                    ):
                        solved = implied_snowball_coupon(
                            engine,
                            note,
                            context,
                            quote,
                            quote_convention="shift_maturity_coupon",
                            lower_bound=lower,
                        )
                        self.assertAlmostEqual(solved, anchor, delta=1e-7)
                with self.assertRaises(RootNotBracketedError):
                    implied_snowball_coupon(
                        engine,
                        note,
                        context,
                        quote,
                        quote_convention="shift_maturity_coupon",
                        lower_bound=-0.1,
                        upper_bound=0.01,
                    )
                with self.assertRaises(InvalidInputError):
                    implied_snowball_coupon(
                        engine,
                        note,
                        context,
                        quote,
                        quote_convention="shift_maturity_coupon",
                        lower_bound=math.nan,
                    )
                fixed = note.with_shifted_coupons(
                    0, quote_convention="preserve_maturity_coupon"
                )
                self.assertEqual(
                    implied_snowball_coupon(
                        engine,
                        note,
                        context,
                        engine.price(fixed, context),
                        quote_convention="preserve_maturity_coupon",
                    ),
                    0,
                )
                self.assertEqual(note.maturity_coupon_rate, maturity)
                self.assertEqual(note.minimum_coupon_rate, minimum)
            boundary = instruments.TernarySnowballOption(
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
                    boundary,
                    context,
                    engine.price(boundary, context),
                    quote_convention="shift_maturity_coupon",
                ),
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
            self.assertEqual(note.replace_history(barrier_state=Ellipsis), note)
            for convention in get_args(CouponQuoteConvention):
                with self.subTest(note=type(note).__name__, convention=convention):
                    replacement = note.with_shifted_coupons(
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
                    target = engine.price(replacement, context)
                    solved = implied_snowball_coupon(
                        engine, note, context, target, quote_convention=convention
                    )
                    self.assertAlmostEqual(
                        engine.price(
                            note.with_shifted_coupons(
                                solved, quote_convention=convention
                            ),
                            context,
                        ),
                        target,
                        delta=1e-8,
                    )
                    with self.assertRaises(InvalidInputError):
                        note.with_shifted_coupons(math.nan, quote_convention=convention)
                    with self.assertRaises(TypeError):
                        note.with_shifted_coupons(True, quote_convention=convention)
            with self.assertRaises(ValueError):
                note.with_shifted_coupons(0.1, quote_convention="unknown")
            self.assertEqual(note.knock_out_coupon_rates, [0.08, 0.12])
            self.assertEqual(note.maturity_coupon_rate, 0.06)

        for note in (
            phoenix(),
            phoenix(coupon_rate=Ellipsis, coupon_per_period=0.02),
        ):
            self.assertEqual(note.replace_history(), note)
            self.assertEqual(note.replace_history(barrier_state=Ellipsis), note)
            field = (
                "coupon_rate" if note.coupon_rate is not None else "coupon_per_period"
            )
            original = getattr(note, field)
            replacement = note.with_coupon(0.18)
            self.assertEqual(getattr(replacement, field), 0.18)
            self.assertEqual(replacement.barrier_state, note.barrier_state)
            self.assertEqual(replacement.observations, note.observations)
            target = engine.price(replacement, context)
            solved = implied_phoenix_coupon(engine, note, context, target)
            self.assertAlmostEqual(
                engine.price(note.with_coupon(solved), context), target, delta=1e-8
            )
            for value, error in ((math.inf, InvalidInputError), (True, TypeError)):
                with self.assertRaises(error):
                    note.with_coupon(value)
            self.assertEqual(getattr(note, field), original)

    def test_coupon_signatures_express_product_requirements(self):
        engine = QuadratureAutocallableEngine(asset_step_count=64)
        note, context = phoenix(), market()
        quote = engine.price(note, context)
        signature = inspect.signature(implied_phoenix_coupon)
        self.assertNotIn("quote_convention", signature.parameters)
        bound = signature.bind(engine, note, context, quote)
        bound.apply_defaults()
        solved = implied_phoenix_coupon(*bound.args, **bound.kwargs)
        self.assertAlmostEqual(solved, note.coupon_rate, delta=1e-8)
        with self.assertRaises(TypeError):
            implied_phoenix_coupon(engine, note, context, quote, quote_convention=None)
        signature = inspect.signature(implied_snowball_coupon)
        self.assertIs(
            signature.parameters["quote_convention"].default, inspect.Parameter.empty
        )
        with self.assertRaises(TypeError):
            signature.bind(engine, binary(), context, 1)
        with self.assertRaises(UnsupportedOperationError):
            implied_snowball_coupon(
                engine,
                note,
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
            )
        with self.assertRaises(UnsupportedOperationError):
            implied_phoenix_coupon(engine, binary(), context, 1)

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
        quote = engine.price(note, context)
        for convention in get_args(CouponQuoteConvention):
            solved = implied_snowball_coupon(
                engine,
                note,
                context,
                quote,
                quote_convention=convention,
                lower_bound=0,
                upper_bound=0.2,
                price_tolerance=1e-13,
                parameter_tolerance=1e-12,
            )
            self.assertAlmostEqual(solved, 0.073, delta=1e-9)

    def assert_rejection(self, expected_error, operation):
        with self.assertRaises(expected_error) as error:
            operation()
        self.assertIs(type(error.exception), expected_error)

    def test_binary_recovers_both_quote_conventions(self):
        engine = QuadratureAutocallableEngine()
        note, context = binary(), market()
        for convention in ("shift_maturity_coupon", "preserve_maturity_coupon"):
            maturity = 0.16 if convention == "shift_maturity_coupon" else 0.06
            quote = engine.price(binary(0.18, maturity), context)
            solved = implied_snowball_coupon(
                engine, note, context, quote, quote_convention=convention
            )
            self.assertAlmostEqual(solved, 0.18, delta=1e-7)
            self.assertAlmostEqual(
                engine.price(binary(solved, maturity), context), quote, delta=1e-8
            )
        self.assertEqual(note.knock_out_coupon_rates, [0.08])
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
                    coupon_rate=coupon,
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
                return getattr(instruments, kind)(**fields)
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
            return getattr(instruments, kind)(**fields)

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
                        quote = engine.price(target, context)
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
                            original,
                            context,
                            quote,
                            lower_bound=-0.1,
                            upper_bound=0.3,
                            price_tolerance=1e-11,
                            **kwargs,
                        )
                        self.assertAlmostEqual(solved, coupon, delta=1e-8)
                        self.assertAlmostEqual(
                            engine.price(note(kind, solved, convention), context),
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
            binary(),
            market(),
            quote,
            quote_convention="preserve_maturity_coupon",
        )
        self.assertAlmostEqual(solved, 0.18, delta=1e-8)

    def test_bounds_defaults_and_validation_order(self):
        engine, note, context = QuadratureAutocallableEngine(), binary(), market()

        def solve(quote, **settings):
            return implied_snowball_coupon(
                engine,
                note,
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
                **settings,
            )

        for bound in (0, 2):
            self.assertEqual(solve(engine.price(binary(bound), context)), bound)
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
                note,
                late,
                math.nan,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_snowball_coupon(
                engine, note, late, 1, quote_convention="preserve_maturity_coupon"
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
                note,
                curved,
                math.nan,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_snowball_coupon(
                engine, note, curved, 1, quote_convention="preserve_maturity_coupon"
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
        quote = engine.price(known, context)
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_snowball_coupon(
                engine,
                known,
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assertAlmostEqual(
            implied_snowball_coupon(
                engine, known, context, quote, quote_convention="shift_maturity_coupon"
            ),
            0.08,
            delta=1e-8,
        )
        self.assertAlmostEqual(
            implied_snowball_coupon(
                engine,
                known,
                market(110, END),
                1 + 0.18 * 90 / 365,
                quote_convention="preserve_maturity_coupon",
            ),
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
                    note,
                    market(70, END),
                    engine.price(note, market(70, END)),
                    **kwargs,
                ),
            )
        ternary = instruments.TernarySnowballOption(
            observation_clock_time=None,
            maturity_coupon_rate=0.06,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            knock_in_observation_mode="at_expiry",
            effective_time=START,
            expiry_time=END,
            observations=known.observations,
            barrier_state="no_event",
        )
        coupon = implied_snowball_coupon(
            engine,
            ternary,
            market(),
            engine.price(ternary, market()),
            quote_convention="shift_maturity_coupon",
        )
        self.assertAlmostEqual(coupon, ternary.observations[0].coupon_rate, delta=1e-7)
        for note in (
            known,
            snowball(barrier_state="no_event", observations=known.observations),
            ternary,
            phoenix(),
        ):
            settled = note.replace_history(barrier_state="knocked_out")
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
                )(engine, settled, market(100, END), 0, **kwargs),
            )

    def test_strict_protocols_and_concurrent_solves(self):
        engine, note, context = QuadratureAutocallableEngine(), binary(), market()

        def solve(**settings):
            return implied_snowball_coupon(engine, note, context, 1, **settings)

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
                    note,
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
                phoenix(),
                context,
                1,
                quote_convention="preserve_maturity_coupon",
            )
        with self.assertRaises(TypeError):
            implied_snowball_coupon(
                object(), note, context, 1, quote_convention="preserve_maturity_coupon"
            )
        with self.assertRaises(TypeError):
            implied_snowball_coupon(
                engine,
                object(),
                context,
                1,
                quote_convention="preserve_maturity_coupon",
            )
        quote = engine.price(binary(0.18), context)

        def recover(_):
            return implied_snowball_coupon(
                engine,
                note,
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
            )

        with ThreadPoolExecutor(max_workers=2) as pool:
            for coupon in pool.map(recover, range(4)):
                self.assertAlmostEqual(coupon, 0.18, delta=1e-8)

    def test_nonfinite_residual_and_exhausted_bisection_errors(self):
        engine = QuadratureAutocallableEngine(asset_step_count=17)
        note = binary(barrier_state="no_event")
        self.assert_rejection(
            ComputationFailureError,
            lambda: implied_snowball_coupon(
                engine,
                note,
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
                note,
                market(),
                0.97,
                quote_convention="shift_maturity_coupon",
                price_tolerance=1e-30,
                parameter_tolerance=1e-30,
                max_iterations=1,
            ),
        )
        self.assertEqual(repr(note), original)
