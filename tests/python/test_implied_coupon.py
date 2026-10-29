"""Coupon quotes through the installed Rust extension public API."""

import math
import sys
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime
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
from kiyosi.market import RateTermStructure, all_days_calendar
from kiyosi.pricing import (
    CouponQuoteConvention,
    FiniteDifferenceAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_coupon,
)
from test_phoenix import contract as phoenix
from test_snowball import END, START, autocallable_observations, market
from test_snowball import contract as snowball


def binary(coupon=0.08, maturity=0.06, **changes):
    fields = {
        "maturity_coupon_rate": maturity,
        "effective_date": START,
        "expiry_date": END,
        "observations": autocallable_observations(
            [END], [110], knock_out_coupon_rates=[coupon]
        ),
    }
    fields.update(changes)
    return BinarySnowballOption(**fields)


class ImpliedCouponTests(unittest.TestCase):
    def test_coupon_endpoints_can_change_daily_knock_in_layers(self):
        end = date(2025, 1, 13)
        note = snowball(
            maturity_coupon_rate=0.073,
            knock_in_level=99,
            lower_strike=100,
            knock_in_observation_mode="every_trading_day",
            barrier_state="none",
            expiry_date=end,
            observations=autocallable_observations(
                [date(2025, 1, 7), end],
                [110, 110],
                knock_out_coupon_rates=[0.073, 0.083],
            ),
        )
        context = market(
            100.25,
            datetime(2025, 1, 1, 12, tzinfo=UTC),
            rate=0.03,
            dividend=0,
            calendar=all_days_calendar(),
        )
        engine = QuadratureAutocallableEngine(asset_step_count=17)
        quote = engine.price(note, context)
        for convention in get_args(CouponQuoteConvention):
            solved = implied_coupon(
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
            solved = implied_coupon(
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
        dates = [date(2025, 2, 3), END]

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
                    "effective_date": START,
                    "expiry_date": END,
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
                "initial_spot": 100,
                "knock_in_level": 80,
                "knock_out_level": 110,
                "effective_date": START,
                "expiry_date": END,
                "observation_dates": dates,
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
                        solved = implied_coupon(
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
        solved = implied_coupon(
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
            return implied_coupon(
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
            lambda: implied_coupon(
                engine,
                note,
                late,
                math.nan,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_coupon(
                engine, note, late, 1, quote_convention="preserve_maturity_coupon"
            ),
        )
        curve = RateTermStructure(reference_time=START, times=[END], factors=[0.99])
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=curve)
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: implied_coupon(
                engine,
                note,
                curved,
                math.nan,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_coupon(
                engine, note, curved, 1, quote_convention="preserve_maturity_coupon"
            ),
        )

    def test_no_exposure_current_and_past_events_and_ternary_validation(self):
        engine = QuadratureAutocallableEngine()
        known = binary(
            barrier_state="none",
            observations=autocallable_observations(
                [date(2025, 2, 3), END], [110, 110], knock_out_coupon_rates=[0.08, 0.08]
            ),
        )
        context = market(100, END)
        quote = engine.price(known, context)
        self.assert_rejection(
            UnsupportedOperationError,
            lambda: implied_coupon(
                engine,
                known,
                context,
                quote,
                quote_convention="preserve_maturity_coupon",
            ),
        )
        self.assertAlmostEqual(
            implied_coupon(
                engine, known, context, quote, quote_convention="shift_maturity_coupon"
            ),
            0.08,
            delta=1e-8,
        )
        self.assertAlmostEqual(
            implied_coupon(
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
                snowball(barrier_state="none"),
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
                lambda note=note, kwargs=kwargs: implied_coupon(
                    engine,
                    note,
                    market(70, END),
                    engine.price(note, market(70, END)),
                    **kwargs,
                ),
            )
        ternary = instruments.TernarySnowballOption(
            maturity_coupon_rate=0.06,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            knock_in_observation_mode="at_expiry",
            effective_date=START,
            expiry_date=END,
            observations=known.observations,
            barrier_state="none",
        )
        self.assert_rejection(
            InvalidInputError,
            lambda: implied_coupon(
                engine, ternary, market(), 1, quote_convention="shift_maturity_coupon"
            ),
        )
        for note in (
            known,
            snowball(barrier_state="none", observations=known.observations),
            ternary,
            phoenix(),
        ):
            settled = note.replace(barrier_state="knocked_out")
            kwargs = (
                {}
                if isinstance(note, PhoenixOption)
                else {"quote_convention": "preserve_maturity_coupon"}
            )
            self.assert_rejection(
                UnsupportedOperationError,
                lambda settled=settled, kwargs=kwargs: implied_coupon(
                    engine, settled, market(100, END), 0, **kwargs
                ),
            )

    def test_strict_protocols_and_concurrent_solves(self):
        engine, note, context = QuadratureAutocallableEngine(), binary(), market()

        def solve(**settings):
            return implied_coupon(engine, note, context, 1, **settings)

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
                implied_coupon(
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
            {"quote_convention": None},
            {"quote_convention": True},
            {"quote_convention": "preserve_maturity_coupon", "unknown": 1},
        ):
            with self.assertRaises(TypeError):
                solve(**kwargs)
        with self.assertRaises(ValueError):
            solve(quote_convention="unknown")
        with self.assertRaises(TypeError):
            implied_coupon(
                engine,
                phoenix(),
                context,
                1,
                quote_convention="preserve_maturity_coupon",
            )
        with self.assertRaises(TypeError):
            implied_coupon(
                object(), note, context, 1, quote_convention="preserve_maturity_coupon"
            )
        with self.assertRaises(TypeError):
            implied_coupon(
                engine,
                object(),
                context,
                1,
                quote_convention="preserve_maturity_coupon",
            )
        quote = engine.price(binary(0.18), context)

        def recover(_):
            return implied_coupon(
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
        note = binary(barrier_state="none")
        self.assert_rejection(
            ComputationFailureError,
            lambda: implied_coupon(
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
                [date(2025, 2, 3), END], [110, 105], knock_out_coupon_rates=[0.08, 0.12]
            ),
        )
        original = repr(note)
        self.assert_rejection(
            NonConvergenceError,
            lambda: implied_coupon(
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
