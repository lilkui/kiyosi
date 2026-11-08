"""Snowball variants through the installed Rust public API."""

import inspect
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta
from datetime import time as clock_time

from kiyosi import (
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
    instruments,
)
from kiyosi.instruments import (
    AutocallableObservation,
    BinarySnowballOption,
    SnowballOption,
    TernarySnowballOption,
)
from kiyosi.market import RateTermStructure
from kiyosi.pricing import (
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_snowball_coupon,
    implied_volatility,
    numerical_greeks,
)
from support.history import dated as dated_history
from support.history import history_fields
from test_snowball import END, START, autocallable_observations, market, structured


class SnowballVariantTests(unittest.TestCase):
    def test_european_snowball_signature_defaults_can_be_forwarded(self):
        factory = instruments.european_snowball
        signature = inspect.signature(factory)
        self.assertIsNone(signature.parameters["observation_clock_time"].default)
        fields = {
            "coupon_rate": 0.12,
            "initial_spot": 100,
            "knock_in_level": 80,
            "knock_out_level": 110,
            "observation_times": [END],
            "effective_time": START,
            "expiry_time": END,
        }
        expected = factory(**history_fields(**fields))
        bound = signature.bind(**fields)
        bound.apply_defaults()
        self.assertEqual(factory(**history_fields(**bound.arguments)), expected)
        self.assertIsNone(expected.observation_clock_time)
        self.assertEqual(expected.knock_in_observation_mode, "at_expiry")

    def test_quadrature_covers_drift_into_future_knock_out(self):
        end = datetime(2026, 1, 1)
        note = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_time=START,
            expiry_time=end,
            observations=autocallable_observations(
                [datetime(2025, 6, 30), end],
                [110, 110],
                knock_out_coupon_rates=[0.2, 0.2],
            ),
        )
        context = market(106, volatility=0.001, rate=0.05, dividend=0)
        engine = QuadratureAutocallableEngine()
        expected = 1.2 * math.exp(-0.05)
        self.assertAlmostEqual(
            engine.price(dated_history(note, context), context), expected, delta=1e-7
        )
        self.assertAlmostEqual(
            implied_snowball_coupon(
                engine,
                dated_history(note, context),
                context,
                expected,
                quote_convention="preserve_maturity_coupon",
            )["first_observation_coupon"],
            0.2,
            delta=1e-7,
        )

    def test_quadrature_covers_drift_below_future_knock_out(self):
        end = datetime(2026, 1, 1)
        note = SnowballOption(
            observation_clock_time=clock_time(),
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=START,
            expiry_time=end,
            knock_in_observation_mode="every_trading_day",
            observations=autocallable_observations(
                [end], [110], knock_out_coupon_rates=[0.2]
            ),
        )
        context = market(115, volatility=0.005, rate=0, dividend=0.05)
        d2 = (math.log(115 / 110) - 0.05 - 0.5 * 0.005**2) / 0.005
        expected = 1 + 0.1 * math.erfc(-d2 / math.sqrt(2))
        self.assertAlmostEqual(
            QuadratureAutocallableEngine(asset_step_count=2048).price(
                dated_history(note, context), context
            ),
            expected,
            delta=1e-4,
        )

    def test_history_replacement_preserves_terms_and_rejects_binary_knock_in(self):
        for kind in (BinarySnowballOption, TernarySnowballOption):
            note = self.fixed(kind)
            states = (None, "no_event", "knocked_out")
            if kind is TernarySnowballOption:
                states += ("knocked_in",)
            for state in states:
                with self.subTest(kind=kind.__name__, state=state):
                    replaced = note.replace_history(
                        barrier_state=state,
                        history_time=(note.history_time or note.effective_time)
                        if state is not None
                        else None,
                    )
                    self.assertEqual(replaced, self.fixed(kind, barrier_state=state))
                    self.assertEqual(replaced.replace_history(barrier_state=None), note)
            self.assertEqual(note, self.fixed(kind))
        binary = self.fixed(BinarySnowballOption)
        with self.assertRaises(InvalidInputError):
            binary.replace_history(
                barrier_state="knocked_in",
                history_time=binary.history_time or binary.effective_time,
            )
        self.assertEqual(binary, self.fixed(BinarySnowballOption))

    def test_fixed_terminal_regions_and_no_binary_monitoring(self):
        common = {
            "maturity_coupon_rate": 0.06,
            "principal_redemption_ratio": 0.5,
            "effective_time": START,
            "expiry_time": END,
            "observations": autocallable_observations(
                [END], [110], knock_out_coupon_rates=[0.08]
            ),
        }
        binary = BinarySnowballOption(**history_fields(**common))
        ternary = TernarySnowballOption(
            **history_fields(
                observation_clock_time=None,
                **common,
                minimum_coupon_rate=-0.02,
                knock_in_level=80,
                knock_in_observation_mode="at_expiry",
            )
        )
        engine = QuadratureAutocallableEngine()
        valuation = datetime(2025, 2, 3)
        time = (END - valuation).days / 365
        d2 = (math.log(80 / 110) + (0.04 - 0.01 - 0.3**2 / 2) * time) / (
            0.3 * math.sqrt(time)
        )
        knock_out_probability = math.erfc(-d2 / math.sqrt(2)) / 2
        expected = math.exp(-0.04 * time) * (
            0.5 + (0.06 + 0.02 * knock_out_probability) * 90 / 365
        )
        self.assertAlmostEqual(
            engine.price(
                dated_history(binary, market(80, valuation)), market(80, valuation)
            ),
            expected,
            delta=1e-12,
        )
        for spot in (79.999, 80, 109, 110):
            for note in (binary, ternary):
                coupon = (
                    0.08
                    if spot >= 110
                    else -0.02
                    if spot < 80 and note is ternary
                    else 0.06
                )
                self.assertEqual(
                    engine.price(
                        dated_history(note, market(spot, END)), market(spot, END)
                    ),
                    0.5 + coupon * 90 / 365,
                )

    def assert_rejection(self, expected, operation, *args, **kwargs):
        with self.assertRaises(expected) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected)

    def fixed(self, kind, **changes):
        fields = {
            "maturity_coupon_rate": 0.06,
            "effective_time": START,
            "expiry_time": END,
            "observations": autocallable_observations(
                [END], [110], knock_out_coupon_rates=[0.08]
            ),
        }
        if kind is TernarySnowballOption:
            fields.update(
                minimum_coupon_rate=0.02,
                knock_in_level=80,
                knock_in_observation_mode="at_expiry",
            )
        fields.update(changes)
        if kind is TernarySnowballOption:
            fields.setdefault(
                "observation_clock_time",
                clock_time()
                if fields["knock_in_observation_mode"] == "every_trading_day"
                else None,
            )
        return kind(**history_fields(**fields))

    def test_existing_quantlib_terminal_portfolios(self):
        engine = QuadratureAutocallableEngine()
        for name, kind, inputs in structured.scenarios():
            if kind not in ("BinarySnowballOption", "TernarySnowballOption"):
                continue
            with self.subTest(name=name):
                fields = {
                    "effective_time": datetime.fromisoformat(inputs["effective_time"]),
                    "expiry_time": datetime.fromisoformat(inputs["expiry_time"]),
                    "barrier_state": inputs["history"],
                    "principal_redemption_ratio": inputs["principal_redemption_ratio"],
                    "maturity_coupon_rate": inputs["maturity_coupon_rate"],
                    "observations": autocallable_observations(
                        [datetime.fromisoformat(inputs["expiry_time"])],
                        [110],
                        knock_out_coupon_rates=[inputs["coupon_rate"]],
                    ),
                }
                if kind == "TernarySnowballOption":
                    fields.update(
                        minimum_coupon_rate=inputs["minimum_coupon_rate"],
                        knock_in_level=inputs["knock_in_level"],
                        knock_in_observation_mode=inputs["knock_in_mode"],
                        observation_clock_time=clock_time()
                        if inputs["knock_in_mode"] == "every_trading_day"
                        else None,
                    )
                note = getattr(instruments, kind)(**history_fields(**fields))
                ctx = market(
                    inputs["spot"],
                    datetime.fromisoformat(inputs["valuation"]),
                    volatility=inputs["volatility"],
                    rate=inputs["rate"],
                )
                self.assertAlmostEqual(
                    engine.price(dated_history(note, ctx), ctx),
                    structured.price(dated_history(kind, inputs), inputs),
                    delta=1e-10,
                )

    def test_all_eight_presets_match_explicit_terms_prices_and_analysis(self):
        dates = [datetime(2025, 2, 3), datetime(2025, 3, 3), END]
        common = {
            "initial_spot": 100,
            "knock_in_level": 80,
            "observation_times": dates,
            "effective_time": START,
            "expiry_time": END,
            "principal_redemption_ratio": 0.5,
            "barrier_state": "no_event",
        }
        cases = [
            (
                "standard_snowball",
                {"coupon_rate": 0.125, "knock_out_level": 110},
                [110] * 3,
                [0.125] * 3,
                0.125,
                100,
                0,
                "every_trading_day",
            ),
            (
                "european_snowball",
                {"coupon_rate": 0.125, "knock_out_level": 110},
                [110] * 3,
                [0.125] * 3,
                0.125,
                100,
                0,
                "at_expiry",
            ),
            (
                "step_down_snowball",
                {
                    "coupon_rate": 0.125,
                    "initial_knock_out_level": 110,
                    "knock_out_level_decrement": 2,
                },
                [110, 108, 106],
                [0.125] * 3,
                0.125,
                100,
                0,
                "every_trading_day",
            ),
            (
                "both_down_snowball",
                {
                    "initial_coupon_rate": 0.125,
                    "initial_knock_out_level": 110,
                    "knock_out_level_decrement": 2,
                    "coupon_rate_decrement": 0.03125,
                },
                [110, 108, 106],
                [0.125, 0.09375, 0.0625],
                0.0625,
                100,
                0,
                "every_trading_day",
            ),
            (
                "dual_coupon_snowball",
                {
                    "knock_out_coupon_rate": 0.125,
                    "knock_out_level": 110,
                    "maturity_coupon_rate": -0.02,
                },
                [110] * 3,
                [0.125] * 3,
                -0.02,
                100,
                0,
                "every_trading_day",
            ),
            (
                "parachute_snowball",
                {
                    "coupon_rate": 0.125,
                    "knock_out_level": 110,
                    "final_knock_out_level": 95,
                },
                [110, 110, 95],
                [0.125] * 3,
                0.125,
                100,
                0,
                "every_trading_day",
            ),
            (
                "otm_snowball",
                {"coupon_rate": 0.125, "knock_out_level": 110, "upper_strike": 90},
                [110] * 3,
                [0.125] * 3,
                0.125,
                90,
                0,
                "every_trading_day",
            ),
            (
                "loss_capped_snowball",
                {"coupon_rate": 0.125, "knock_out_level": 110, "lower_strike": 60},
                [110] * 3,
                [0.125] * 3,
                0.125,
                100,
                60,
                "every_trading_day",
            ),
        ]
        engine = QuadratureAutocallableEngine(asset_step_count=128)
        ctx = market(100, datetime(2025, 1, 15))
        for name, extra, levels, coupons, maturity, upper, lower, mode in cases:
            with self.subTest(preset=name):
                actual = getattr(instruments, name)(
                    **history_fields(
                        observation_clock_time=clock_time()
                        if mode == "every_trading_day"
                        else None,
                        **common,
                        **extra,
                    )
                )
                explicit = SnowballOption(
                    observation_clock_time=clock_time()
                    if mode == "every_trading_day"
                    else None,
                    maturity_coupon_rate=maturity,
                    initial_spot=100,
                    knock_in_level=80,
                    upper_strike=upper,
                    lower_strike=lower,
                    knock_in_observation_mode=mode,
                    barrier_state="no_event",
                    principal_redemption_ratio=0.5,
                    effective_time=START,
                    expiry_time=END,
                    observations=autocallable_observations(
                        dates, levels, knock_out_coupon_rates=coupons
                    ),
                    history_time=START,
                )
                self.assertEqual(actual, explicit)
                self.assertEqual(
                    engine.price(dated_history(actual, ctx), ctx),
                    engine.price(dated_history(explicit, ctx), ctx),
                )
                mc = MonteCarloAutocallableEngine(path_count=128, seed=42)
                self.assertEqual(
                    mc.price(dated_history(actual, ctx), ctx),
                    mc.price(dated_history(explicit, ctx), ctx),
                )
                for note in (actual, explicit):
                    result = numerical_greeks(
                        engine,
                        dated_history(note, ctx),
                        ctx,
                        ["delta", "vega"],
                        spot_shift=0.25,
                    )
                    self.assertIsNotNone(result.delta)
                    self.assertIsNotNone(result.vega)
                self.assertEqual(
                    actual.replace_history(
                        barrier_state="knocked_in",
                        history_time=actual.history_time or actual.effective_time,
                    ),
                    explicit.replace_history(
                        barrier_state="knocked_in",
                        history_time=explicit.history_time or explicit.effective_time,
                    ),
                )
                self.assertEqual(
                    eval(
                        repr(actual),
                        vars(instruments) | {"datetime": __import__("datetime")},
                    ),
                    actual,
                )
        defaults = instruments.standard_snowball(
            observation_clock_time=clock_time(),
            coupon_rate=0.08,
            initial_spot=100,
            knock_in_level=80,
            knock_out_level=110,
            observation_times=[END],
            effective_time=START,
            expiry_time=END,
        )
        self.assertEqual(defaults.principal_redemption_ratio, 1)
        self.assertIsNone(defaults.barrier_state)

    def test_native_representations_validation_timing_and_ownership(self):
        engine = QuadratureAutocallableEngine()
        for kind in (BinarySnowballOption, TernarySnowballOption):
            note = self.fixed(kind)
            self.assertEqual(note.principal_redemption_ratio, 1)
            self.assertIsNone(note.barrier_state)
            self.assertEqual(
                eval(
                    repr(note), vars(instruments) | {"datetime": __import__("datetime")}
                ),
                note,
            )
            records = list(note.observations)
            records.clear()
            self.assertEqual(len(note.observations), 1)
            for field in ("maturity_coupon_rate", "principal_redemption_ratio"):
                for value in (True, "0.1", object()):
                    with self.assertRaises(TypeError):
                        self.fixed(kind, **{field: value})
                for value in (math.nan, math.inf):
                    self.assert_rejection(
                        InvalidInputError, self.fixed, kind, **{field: value}
                    )
            for records, expected_error in (
                ([], InvalidInputError),
                (
                    [AutocallableObservation.phoenix(END, 110, coupon_barrier_level=1)],
                    InvalidInputError,
                ),
            ):
                self.assert_rejection(
                    expected_error, self.fixed, kind, observations=records
                )
            with self.assertRaises(TypeError):
                self.fixed(kind, observations=[object()])
            with self.assertRaises(TypeError):
                self.fixed(kind, effective_time=datetime(2025, 1, 1).date())
            self.assert_rejection(
                InvalidInputError, self.fixed, kind, expiry_time=datetime(2024, 12, 31)
            )
            self.assert_rejection(
                InvalidInputError,
                self.fixed,
                kind,
                observations=[],
                maturity_coupon_rate=math.nan,
            )
            with self.assertRaises(ValueError):
                note.replace_history(
                    barrier_state="unknown",
                    history_time=note.history_time or note.effective_time,
                )
            with self.assertRaises(TypeError):
                note.replace_history(
                    barrier_state=True,
                    history_time=note.history_time or note.effective_time,
                )
            self.assertIsNone(note.barrier_state)
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                dated_history(
                    note.replace_history(
                        barrier_state="knocked_out",
                        history_time=note.history_time or note.effective_time,
                    ),
                    market(),
                ),
                market(),
            )
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                dated_history(note, market(100, datetime(2025, 4, 2))),
                market(100, datetime(2025, 4, 2)),
            )
        binary = self.fixed(BinarySnowballOption)
        self.assert_rejection(
            InvalidInputError, binary.replace_history, barrier_state="knocked_in"
        )
        self.assertIsNone(binary.barrier_state)
        for value in (True, "0.02"):
            with self.assertRaises(TypeError):
                self.fixed(TernarySnowballOption, minimum_coupon_rate=value)
        for value in (0.07, math.inf, math.nan):
            self.assert_rejection(
                InvalidInputError,
                self.fixed,
                TernarySnowballOption,
                minimum_coupon_rate=value,
            )
        self.assert_rejection(
            InvalidInputError,
            self.fixed,
            TernarySnowballOption,
            observations=[],
            knock_in_level=0,
        )
        daily = self.fixed(
            TernarySnowballOption, knock_in_observation_mode="every_trading_day"
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            dated_history(daily, market(100, datetime(2025, 1, 2))),
            market(100, datetime(2025, 1, 2)),
        )
        self.assertEqual(
            engine.price(
                dated_history(
                    daily.replace_history(
                        barrier_state="knocked_in",
                        history_time=daily.history_time or daily.effective_time,
                    ),
                    market(100, END),
                ),
                market(100, END),
            ),
            1 + 0.02 * 90 / 365,
        )
        # Calendar applicability is checked before history and expiry settlement.
        curve = RateTermStructure(
            reference_time=START,
            times=[END, END + (END - START)],
            factors=[0.99, (0.99) * 0.5],
        )
        self.assert_rejection(
            UnsupportedOperationError,
            engine.price,
            dated_history(binary, market(rate=curve)),
            market(rate=curve),
        )

    def test_preset_rejections_and_strict_numeric_conversion(self):
        common = {
            "coupon_rate": 0.08,
            "initial_spot": 100,
            "knock_in_level": 80,
            "knock_out_level": 110,
            "observation_times": [END],
            "effective_time": START,
            "expiry_time": END,
        }
        for field in (
            "coupon_rate",
            "initial_spot",
            "knock_in_level",
            "knock_out_level",
            "principal_redemption_ratio",
        ):
            for value in (True, "100"):
                with self.assertRaises(TypeError):
                    instruments.standard_snowball(
                        **history_fields(
                            observation_clock_time=clock_time(),
                            **(common | {field: value}),
                        )
                    )
        self.assert_rejection(
            InvalidInputError,
            instruments.standard_snowball,
            observation_clock_time=clock_time(),
            **(common | {"observation_times": []}),
        )
        self.assert_rejection(
            InvalidInputError,
            instruments.loss_capped_snowball,
            observation_clock_time=clock_time(),
            **common,
            lower_strike=101,
        )
        self.assert_rejection(
            InvalidInputError,
            instruments.parachute_snowball,
            observation_clock_time=clock_time(),
            **common,
            final_knock_out_level=0,
        )
        self.assert_rejection(
            InvalidInputError,
            instruments.otm_snowball,
            observation_clock_time=clock_time(),
            **common,
            upper_strike=math.nan,
        )
        stepped = common.copy()
        stepped["initial_knock_out_level"] = stepped.pop("knock_out_level")
        self.assert_rejection(
            InvalidInputError,
            instruments.step_down_snowball,
            observation_clock_time=clock_time(),
            **stepped,
            knock_out_level_decrement=math.inf,
        )
        # A negative decrement is supported if the expanded levels remain positive.
        note = instruments.step_down_snowball(
            **history_fields(
                observation_clock_time=clock_time(),
                **(stepped | {"observation_times": [datetime(2025, 2, 3), END]}),
                knock_out_level_decrement=-2,
            )
        )
        self.assertEqual(note.knock_out_levels, (110, 112))

    def test_fixed_variants_support_greeks_solving_current_events_and_concurrent_reads(
        self,
    ):
        engine = QuadratureAutocallableEngine()
        for kind in (BinarySnowballOption, TernarySnowballOption):
            note = self.fixed(kind)
            ctx = market(103)
            original = repr(note)
            result = numerical_greeks(
                engine, dated_history(note, ctx), ctx, greeks="all", spot_shift=0.25
            )
            for name in (
                "delta",
                "gamma",
                "speed",
                "vega",
                "vanna",
                "zomma",
                "rho",
                "theta",
                "charm",
                "color",
            ):
                self.assertTrue(math.isfinite(getattr(result, name)), name)
            sigma = implied_volatility(
                engine,
                dated_history(note, ctx),
                ctx,
                result.price,
                lower_bound=0.2,
                upper_bound=0.4,
            )
            self.assertAlmostEqual(sigma, 0.3, delta=1e-6)
            self.assert_rejection(
                RootNotBracketedError,
                implied_volatility,
                engine,
                dated_history(note, ctx),
                ctx,
                10,
            )
            self.assertEqual(repr(note), original)
            self.assertEqual(ctx.model_parameters.volatility, 0.3)
            with ThreadPoolExecutor(max_workers=3) as pool:
                prices = list(
                    pool.map(
                        lambda s, note=note: engine.price(
                            dated_history(note, market(s)), market(s)
                        ),
                        [80, 100, 120],
                    )
                )
            self.assertEqual(
                prices,
                [
                    engine.price(dated_history(note, market(s)), market(s))
                    for s in [80, 100, 120]
                ],
            )
            middle = datetime(2025, 2, 3)
            note = self.fixed(
                kind,
                barrier_state="no_event",
                observations=autocallable_observations(
                    [middle, END], [110, 105], knock_out_coupon_rates=[0.08, 0.06]
                ),
            )
            current = engine.price_with_greeks(
                dated_history(note, market(110, middle)),
                market(110, middle),
                greeks="all",
            )
            self.assertAlmostEqual(current.price, 1 + 0.08 * 33 / 365)
            self.assertIsNone(current.delta)
            self.assertIsNone(current.theta)
            self.assertEqual(
                (current.vega, current.rho),
                (0, 0),
            )
            self.assert_rejection(
                UnsupportedOperationError,
                implied_volatility,
                engine,
                dated_history(note, market(110, middle)),
                market(110, middle),
                current.price,
            )
            intraday = datetime(2025, 2, 3, 0, 0, 0, 1)
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                dated_history(
                    note.replace_history(barrier_state=None), market(110, intraday)
                ),
                market(110, intraday),
            )
            self.assertEqual(
                engine.price(
                    dated_history(
                        note.replace_history(
                            barrier_state="knocked_out",
                            history_time=note.history_time or note.effective_time,
                        ),
                        market(100, END),
                    ),
                    market(100, END),
                ),
                0,
            )
            fixed = self.fixed(
                kind,
                maturity_coupon_rate=0,
                observations=autocallable_observations(
                    [END], [110], knock_out_coupon_rates=[0]
                ),
                **({"minimum_coupon_rate": 0} if kind is TernarySnowballOption else {}),
            )
            self.assertAlmostEqual(
                engine.price(dated_history(fixed, ctx), ctx),
                math.exp(-0.04 * 90 / 365),
                delta=1e-15,
            )
            self.assert_rejection(
                UnsupportedOperationError,
                implied_volatility,
                engine,
                dated_history(fixed, ctx),
                ctx,
                engine.price(dated_history(fixed, ctx), ctx),
            )

    def test_multi_event_convergence_for_fixed_variants_and_both_monitoring_modes(self):
        dates = [datetime(2025, 2, 3), datetime(2025, 3, 3), END]
        ctx = market()
        for kind, mode in (
            (BinarySnowballOption, None),
            (TernarySnowballOption, "at_expiry"),
            (TernarySnowballOption, "every_trading_day"),
        ):
            note = self.fixed(
                kind,
                observations=autocallable_observations(
                    dates, [110, 108, 105], knock_out_coupon_rates=[0.08] * 3
                ),
                **(
                    {"knock_in_observation_mode": mode, "knock_in_level": 95}
                    if mode
                    else {}
                ),
            )
            prices = [
                QuadratureAutocallableEngine(
                    asset_step_count=n, integration_standard_deviations=e
                ).price(dated_history(note, ctx), ctx)
                for n, e in ((512, 8), (1024, 8), (1024, 10))
            ]
            self.assertAlmostEqual(prices[0], prices[1], delta=1e-4)
            self.assertAlmostEqual(prices[1], prices[2], delta=1e-4)

    def test_signed_coupon_and_one_second_digital_regressions(self):
        end = datetime(2026, 1, 1)
        engine = QuadratureAutocallableEngine()
        for coupon in (0.1, -4):
            for mode in ("at_expiry", "every_trading_day"):
                for kind in (BinarySnowballOption, TernarySnowballOption):
                    note = self.fixed(
                        kind,
                        expiry_time=end,
                        maturity_coupon_rate=0,
                        observations=autocallable_observations(
                            [end], [100], knock_out_coupon_rates=[coupon]
                        ),
                        **(
                            {
                                "minimum_coupon_rate": 0,
                                "knock_in_level": 100,
                                "knock_in_observation_mode": mode,
                            }
                            if kind is TernarySnowballOption
                            else {}
                        ),
                    )
                    d2 = (0.03 - 0.8**2 / 2) / 0.8
                    expected = math.exp(-0.04) * (
                        1 + coupon * math.erfc(-d2 / math.sqrt(2)) / 2
                    )
                    self.assertAlmostEqual(
                        engine.price(
                            dated_history(note, market(volatility=0.8)),
                            market(volatility=0.8),
                        ),
                        expected,
                        delta=1e-12,
                    )
        valuation = datetime(2026, 1, 1) - timedelta(seconds=1)
        t = 1 / (365 * 86400)
        note = self.fixed(
            BinarySnowballOption,
            expiry_time=end,
            maturity_coupon_rate=0,
            observations=autocallable_observations(
                [end], [100], knock_out_coupon_rates=[0.1]
            ),
        )
        for sigma in (0.01, 0.2):
            for spot in (100, 100 + 50 * sigma * math.sqrt(t)):
                d2 = (math.log(spot / 100) + (0.03 - sigma**2 / 2) * t) / (
                    sigma * math.sqrt(t)
                )
                expected = math.exp(-0.04 * t) * (
                    1 + 0.1 * math.erfc(-d2 / math.sqrt(2)) / 2
                )
                self.assertAlmostEqual(
                    engine.price(
                        dated_history(note, market(spot, valuation, volatility=sigma)),
                        market(spot, valuation, volatility=sigma),
                    ),
                    expected,
                    delta=1e-9,
                )
