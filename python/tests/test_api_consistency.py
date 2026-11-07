"""Public API contracts shared by Rust and Python callers."""

import math
import pickle
import unittest
from datetime import datetime, timedelta
from datetime import time as clock_time
from decimal import Decimal, FloatOperation, Inexact, Rounded, localcontext
from fractions import Fraction

from kiyosi import InvalidInputError, UnsupportedOperationError, market, pricing
from kiyosi import instruments as ins
from kiyosi.pricing import numerical_greeks
from support.history import dated as dated_history


class ApiConsistencyTests(unittest.TestCase):
    def test_current_barrier_touches_retain_cashflow_parameter_greeks(self):
        start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
        for volatility in (0, 0.2):
            for timing in ("at_hit", "at_expiry"):
                option = ins.BarrierOption(
                    option_type="call",
                    strike=100,
                    effective_time=start,
                    expiry_time=end,
                    barrier_level=120,
                    barrier_type="up_and_out",
                    rebate=10,
                    rebate_timing=timing,
                )
                context = market.PricingContext(
                    model_parameters=market.BlackScholesMertonParameters(
                        risk_free_rate=0.05,
                        dividend_yield=0.02,
                        volatility=volatility,
                    ),
                    spot_price=120,
                    valuation_time=start,
                )
                result = pricing.AnalyticBarrierEngine().price_with_greeks(
                    option,
                    context,
                    ["delta", "rho", "dividend_rho", "vega"],
                )
                expected = -10 * math.exp(-0.05) if timing == "at_expiry" else 0
                self.assertAlmostEqual(result.require("rho"), expected, delta=2e-8)
                self.assertEqual(result.require("dividend_rho"), 0)
                self.assertEqual(result.status("delta"), "undefined")
                if volatility:
                    self.assertEqual(result.require("vega"), 0)
                else:
                    self.assertEqual(result.status("vega"), "undefined")

    def test_european_digitals_allow_pre_effective_pricing_greeks_and_solves(self):
        start = datetime(2025, 1, 1)
        effective, expiry = start + timedelta(days=30), start + timedelta(days=365)
        mc = pricing.MonteCarloDigitalEngine(path_count=128, seed=42, thread_count=1)
        fd = pricing.FiniteDifferenceDigitalEngine(
            asset_step_count=200, time_step_count=100
        )
        engines = [
            pricing.AnalyticDigitalEngine(),
            pricing.CoxRossRubinsteinDigitalEngine(),
            fd,
            pricing.QuadratureDigitalEngine(),
            mc,
        ]
        for kind in (ins.CashOrNothingOption, ins.AssetOrNothingOption):
            for direction in ("call", "put"):
                future = kind(
                    option_type=direction,
                    strike=100,
                    effective_time=effective,
                    expiry_time=expiry,
                    **({"payout": 10} if kind is ins.CashOrNothingOption else {}),
                )
                existing = future.replace(effective_time=start)
                for volatility in (0, 0.2):
                    model = market.BlackScholesMertonParameters(
                        risk_free_rate=0.05, dividend_yield=0.02, volatility=volatility
                    )
                    for at in (start, effective - timedelta(microseconds=1), effective):
                        context = market.PricingContext(
                            model_parameters=model, spot_price=105, valuation_time=at
                        )
                        for engine in engines:
                            with self.subTest(
                                kind=kind.__name__,
                                direction=direction,
                                volatility=volatility,
                                at=at,
                                engine=type(engine).__name__,
                            ):
                                price = engine.price(
                                    dated_history(future, context), context
                                )
                                self.assertEqual(
                                    price,
                                    engine.price(
                                        dated_history(existing, context), context
                                    ),
                                )
                                native = engine.price_with_greeks(
                                    dated_history(future, context), context, "delta"
                                )
                                self.assertEqual(
                                    native,
                                    engine.price_with_greeks(
                                        dated_history(existing, context),
                                        context,
                                        "delta",
                                    ),
                                )
                                self.assertTrue(math.isfinite(native.require("delta")))
                                numerical = numerical_greeks(
                                    engine,
                                    dated_history(future, context),
                                    context,
                                    "theta",
                                )
                                self.assertTrue(
                                    math.isfinite(numerical.require("theta"))
                                )
                                self.assertEqual(
                                    numerical,
                                    numerical_greeks(
                                        engine,
                                        dated_history(existing, context),
                                        context,
                                        "theta",
                                    ),
                                )
                                if volatility:
                                    self.assertAlmostEqual(
                                        pricing.implied_volatility(
                                            engine,
                                            dated_history(future, context),
                                            context,
                                            price,
                                            lower_bound=0.2,
                                            upper_bound=0.3,
                                        ),
                                        0.2,
                                        delta=1e-12,
                                    )
                        self.assertEqual(
                            mc.price_with_statistics(
                                dated_history(future, context), context
                            ),
                            mc.price_with_statistics(
                                dated_history(existing, context), context
                            ),
                        )
                    short = market.RateTermStructure.bounded_flat(
                        0.05, reference_time=start, max_time=expiry - timedelta(days=1)
                    )
                    uncovered = context.replace(
                        model_parameters=model.replace(risk_free_rate=short),
                        valuation_time=start,
                    )
                    expired = context.replace(
                        valuation_time=expiry + timedelta(microseconds=1)
                    )
                    for engine in engines:
                        for invalid in (uncovered, expired):
                            with self.assertRaises(InvalidInputError):
                                engine.price(dated_history(future, invalid), invalid)
                surface = market.LocalVolatilitySurface(
                    times=[start, expiry],
                    spots=[50, 150],
                    values=[[0.2, 0.2], [0.2, 0.2]],
                )
                local = context.replace(
                    model_parameters=market.LocalVolatilityParameters(
                        risk_free_rate=0.05, dividend_yield=0.02, surface=surface
                    ),
                    valuation_time=start,
                )
                self.assertEqual(
                    fd.price(dated_history(future, local), local),
                    fd.price(dated_history(existing, local), local),
                )
                self.assertEqual(future.effective_time, effective)

    def test_european_pre_effective_pricing_preserves_contract_and_model_coverage(self):
        valuation = datetime(2025, 1, 1)
        effective, expiry = (
            valuation + timedelta(days=30),
            valuation + timedelta(days=365),
        )
        future = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=effective, expiry_time=expiry
        )
        existing = future.replace(effective_time=valuation)
        engines = [
            pricing.AnalyticEuropeanEngine(),
            pricing.CoxRossRubinsteinVanillaEngine(),
            pricing.FiniteDifferenceVanillaEngine(),
            pricing.QuadratureVanillaEngine(),
            pricing.MonteCarloVanillaEngine(path_count=128, seed=42, thread_count=1),
        ]
        for volatility in (0, 0.2):
            model = market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=volatility
            )
            context = market.PricingContext(
                model_parameters=model, spot_price=100, valuation_time=valuation
            )
            for engine in engines:
                with self.subTest(engine=type(engine).__name__, volatility=volatility):
                    self.assertEqual(
                        engine.price(dated_history(future, context), context),
                        engine.price(dated_history(existing, context), context),
                    )
                    self.assertTrue(
                        math.isfinite(
                            engine.price_with_greeks(
                                dated_history(future, context), context, "delta"
                            ).require("delta")
                        )
                    )
            if volatility:
                engine = engines[0]
                self.assertAlmostEqual(
                    pricing.implied_volatility(
                        engine,
                        dated_history(future, context),
                        context,
                        engine.price(dated_history(future, context), context),
                    ),
                    volatility,
                    delta=1e-7,
                )
        heston = market.HestonParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            initial_variance=0.04,
            long_run_variance=0.04,
            mean_reversion_speed=1.5,
            vol_of_vol=0,
            correlation=-0.7,
        )
        context = context.replace(model_parameters=heston)
        for engine in (
            pricing.AnalyticHestonEuropeanEngine(),
            pricing.FiniteDifferenceHestonVanillaEngine(
                asset_step_count=40, variance_step_count=10, time_step_count=20
            ),
        ):
            self.assertEqual(
                engine.price(dated_history(future, context), context),
                engine.price(dated_history(existing, context), context),
            )
        surface = market.LocalVolatilitySurface(
            times=[valuation, expiry], spots=[50, 150], values=[[0.2, 0.2], [0.2, 0.2]]
        )
        local = market.LocalVolatilityParameters(
            risk_free_rate=0.05, dividend_yield=0.02, surface=surface
        )
        context = context.replace(model_parameters=local)
        self.assertEqual(
            engines[2].price(dated_history(future, context), context),
            engines[2].price(dated_history(existing, context), context),
        )
        bounded = market.RateTermStructure.bounded_flat(
            0.05, reference_time=effective, max_time=expiry
        )
        with self.assertRaises(InvalidInputError):
            engines[0].price(
                dated_history(
                    future,
                    context.replace(
                        model_parameters=model.replace(risk_free_rate=bounded)
                    ),
                ),
                context.replace(model_parameters=model.replace(risk_free_rate=bounded)),
            )
        self.assertEqual(future.effective_time, effective)

    def test_snowball_coupon_diagnostics_identify_the_supplied_field(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        common = {
            "effective_time": start,
            "expiry_time": end,
            "observations": ins.autocallable_observations([end], [110]),
        }
        for invalid in (math.nan, math.inf, -math.inf):
            for factory, fields, field in (
                (
                    ins.BinarySnowballOption,
                    {"maturity_coupon_rate": invalid},
                    "maturity_coupon_rate",
                ),
                (
                    ins.TernarySnowballOption,
                    {
                        "maturity_coupon_rate": invalid,
                        "minimum_coupon_rate": 0,
                        "knock_in_level": 80,
                        "observation_clock_time": clock_time(),
                    },
                    "maturity_coupon_rate",
                ),
                (
                    ins.TernarySnowballOption,
                    {
                        "maturity_coupon_rate": 0.1,
                        "minimum_coupon_rate": invalid,
                        "knock_in_level": 80,
                        "observation_clock_time": clock_time(),
                    },
                    "minimum_coupon_rate",
                ),
            ):
                with self.subTest(contract=factory.__name__, field=field):
                    with self.assertRaises(InvalidInputError) as error:
                        factory(**common, **fields)
                    self.assertIn(field, str(error.exception))

    def test_structured_engines_require_curve_coverage_only_for_remaining_cashflows(
        self,
    ):
        start, valuation, end = (
            datetime(2025, 1, 1),
            datetime(2025, 1, 3),
            datetime(2026, 1, 1),
        )
        curve = market.RateTermStructure(
            reference_time=start, times=[datetime(2025, 2, 1)], factors=[1]
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=curve, dividend_yield=curve, volatility=0.2
            ),
            spot_price=125,
            valuation_time=valuation,
        )
        accumulator = ins.Accumulator(
            knock_out_settled=False,
            observation_clock_time=clock_time(),
            strike=100,
            knock_out_level=120,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
            effective_time=start,
            expiry_time=end,
        )
        note = ins.BinarySnowballOption(
            maturity_coupon_rate=0.1,
            effective_time=start,
            expiry_time=end,
            barrier_state="no_event",
            observations=ins.autocallable_observations(
                [datetime(2025, 1, 2), valuation, end], [120, 120, 120]
            ),
        )
        for kind, contract, settled, active, current_value in (
            (
                "Accumulator",
                accumulator,
                accumulator.replace(knock_out_settled=True),
                accumulator.replace(
                    knock_out_level=150,
                    accumulated_quantity=accumulator.accumulated_quantity,
                    knock_out_settled=accumulator.knock_out_settled,
                ),
                75,
            ),
            (
                "Autocallable",
                note,
                note.replace_history(barrier_state="knocked_out"),
                note.replace(
                    barrier_state=note.barrier_state,
                    observations=ins.autocallable_observations(
                        [datetime(2025, 1, 2), valuation, end], [150, 150, 150]
                    ),
                ),
                1,
            ),
        ):
            for family in ("FiniteDifference", "Quadrature", "MonteCarlo"):
                engine = getattr(pricing, family + kind + "Engine")()
                with self.subTest(kind=kind, engine=family):
                    self.assertEqual(
                        engine.price(dated_history(settled, context), context), 0
                    )
                    self.assertEqual(
                        engine.price(dated_history(contract, context), context),
                        current_value,
                    )
                    self.assertEqual(
                        engine.price_with_greeks(
                            dated_history(settled, context), context, "delta"
                        ).delta,
                        0,
                    )
                    with self.assertRaises(InvalidInputError):
                        engine.price(dated_history(active, context), context)
                    if family == "MonteCarlo":
                        estimate = engine.price_with_statistics(
                            dated_history(settled, context), context
                        )
                        self.assertEqual(estimate.price, 0)
                        self.assertEqual(estimate.standard_error, 0)

    def test_accumulator_settlement_requires_a_strictly_prior_knock_out_observation(
        self,
    ):
        start, first_explicit, end = (datetime(2025, 1, day) for day in (6, 8, 10))
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        )
        for times, first in (
            (None, start),
            ([first_explicit], first_explicit),
            ([], None),
        ):
            original = ins.Accumulator(
                observation_clock_time=clock_time(),
                strike=100,
                knock_out_level=120,
                daily_quantity=1,
                acceleration_factor=2,
                effective_time=start,
                expiry_time=end,
                knock_out_observation_times=times,
            )
            settled = original.replace_history(knock_out_settled=True)
            for valuation in (
                start,
                first_explicit,
                first_explicit + timedelta(microseconds=1),
                end,
            ):
                context = market.PricingContext(
                    model_parameters=model, spot_price=100, valuation_time=valuation
                )
                for family in ("FiniteDifference", "Quadrature", "MonteCarlo"):
                    engine = getattr(pricing, family + "AccumulatorEngine")()
                    with self.subTest(
                        schedule=times, valuation=valuation, engine=family
                    ):
                        if first is not None and first < valuation:
                            self.assertEqual(
                                engine.price(dated_history(settled, context), context),
                                0,
                            )
                            self.assertEqual(
                                engine.price_with_greeks(
                                    dated_history(settled, context), context, "delta"
                                ).delta,
                                0,
                            )
                            if family == "MonteCarlo":
                                self.assertEqual(
                                    engine.price_with_statistics(
                                        dated_history(settled, context), context
                                    ).price,
                                    0,
                                )
                        else:
                            with self.assertRaises(InvalidInputError):
                                engine.price(dated_history(settled, context), context)
                            with self.assertRaises(InvalidInputError):
                                engine.price_with_greeks(
                                    dated_history(settled, context), context, "delta"
                                )
                            if family == "MonteCarlo":
                                with self.assertRaises(InvalidInputError):
                                    engine.price_with_statistics(
                                        dated_history(settled, context), context
                                    )
            self.assertFalse(original.knock_out_settled)
            self.assertIsNone(original.accumulated_quantity)
            self.assertEqual(original.knock_out_observation_times, times)

    def test_numeric_rejections_identify_the_failing_field(self):
        defaults = {"risk_free_rate": 0.02, "dividend_yield": 0, "volatility": 0.2}
        for field, invalid in (
            ("risk_free_rate", math.nan),
            ("dividend_yield", math.inf),
            ("volatility", -0.2),
        ):
            with (
                self.subTest(field=field),
                self.assertRaises(InvalidInputError) as error,
            ):
                market.BlackScholesMertonParameters(**(defaults | {field: invalid}))
            self.assertIn(field, str(error.exception))
        model = market.BlackScholesMertonParameters(**defaults)
        with self.assertRaises(InvalidInputError) as error:
            model.replace(volatility=-0.1)
        self.assertIn("volatility", str(error.exception))
        self.assertEqual(model.volatility, 0.2)

    def test_contractual_calendars_are_validated_before_pricing(self):
        start, closed, end = (datetime(2025, 1, day) for day in (3, 4, 6))
        life = {"effective_time": start, "expiry_time": end}
        monitoring = {"observation_mode": "scheduled", "observation_times": [closed]}
        single = {"barrier_level": 120, "barrier_type": "up_and_out"}
        vanilla = {"option_type": "call", "strike": 100}
        factories = [
            (ins.BarrierOption, dict(life, **vanilla, **single, **monitoring)),
            (
                ins.DoubleBarrierOption,
                dict(
                    life,
                    **vanilla,
                    lower_barrier=80,
                    upper_barrier=120,
                    barrier_type="knock_out",
                    **monitoring,
                ),
            ),
            (
                ins.cash_binary_barrier_option,
                dict(life, **vanilla, **single, payout=10, **monitoring),
            ),
            (
                ins.asset_binary_barrier_option,
                dict(life, **vanilla, **single, **monitoring),
            ),
            (ins.cash_touch_option, dict(life, **single, payout=10, **monitoring)),
            (ins.asset_touch_option, dict(life, **single, **monitoring)),
            (
                ins.cash_double_touch,
                dict(
                    life, lower_barrier=80, upper_barrier=120, payout=10, **monitoring
                ),
            ),
            (
                ins.Accumulator,
                dict(
                    life,
                    strike=90,
                    knock_out_level=110,
                    daily_quantity=1,
                    acceleration_factor=2,
                    knock_out_observation_times=[closed],
                    observation_clock_time=clock_time(),
                ),
            ),
            (
                ins.BinarySnowballOption,
                dict(
                    life,
                    maturity_coupon_rate=0.1,
                    observations=[ins.AutocallableObservation(closed, 110)],
                ),
            ),
            (
                ins.standard_snowball,
                dict(
                    life,
                    initial_spot=100,
                    knock_in_level=80,
                    knock_out_level=110,
                    coupon_rate=0.1,
                    observation_times=[closed],
                    observation_clock_time=clock_time(),
                ),
            ),
            (
                ins.RangeAccrualOption,
                {
                    "effective_time": start,
                    "expiry_time": closed,
                    "notional": 100,
                    "annual_coupon_rate": 0.1,
                    "lower_bound": 80,
                    "upper_bound": 120,
                    "observation_clock_time": clock_time(),
                },
            ),
        ]
        for factory, fields in factories:
            with self.subTest(contract=factory.__name__):
                with self.assertRaises(InvalidInputError):
                    factory(**fields)
                option = factory(**fields, calendar=market.all_days_calendar())
                with self.assertRaises(InvalidInputError):
                    option.replace(calendar=market.weekdays_calendar())
                self.assertEqual(option.calendar, market.all_days_calendar())
                self.assertEqual(option.replace(), option)
                self.assertEqual(pickle.loads(pickle.dumps(option)), option)

    def test_bounded_barrier_grids_keep_resolution_when_unused_upper_bound_changes(
        self,
    ):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.02, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for direction in ("call", "put"):
            terms = {
                "option_type": direction,
                "strike": 100,
                "effective_time": start,
                "expiry_time": end,
            }
            single = ins.BarrierOption(
                **terms, barrier_level=120, barrier_type="up_and_out"
            )
            double = ins.DoubleBarrierOption(
                **terms, lower_barrier=80, upper_barrier=120, barrier_type="knock_out"
            )
            expected = pricing.AnalyticBarrierEngine().price(
                dated_history(single, context), context
            )
            previous = None
            for upper in (121, 400, 10_000):
                settings = {
                    "asset_step_count": 200,
                    "time_step_count": 200,
                    "asset_upper_boundary": upper,
                }
                actual = (
                    pricing.FiniteDifferenceBarrierEngine(**settings).price(
                        dated_history(single, context), context
                    ),
                    pricing.FiniteDifferenceDoubleBarrierEngine(**settings).price(
                        dated_history(double, context), context
                    ),
                )
                with self.subTest(direction=direction, upper=upper):
                    self.assertAlmostEqual(actual[0], expected, delta=0.003)
                    if previous is not None:
                        self.assertEqual(actual, previous)
                previous = actual

    def test_barrier_engines_require_curve_coverage_only_for_remaining_cashflows(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        curve = market.RateTermStructure(
            reference_time=start, times=[datetime(2025, 2, 1)], factors=[1]
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=curve, dividend_yield=curve, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime(2025, 1, 2),
        )
        settled = ins.BarrierOption(
            option_type="call",
            strike=100,
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_out",
            touch_state="touched",
            rebate_timing="at_hit",
        )
        for factory in (
            pricing.AnalyticBarrierEngine,
            pricing.CoxRossRubinsteinBarrierEngine,
            pricing.QuadratureBarrierEngine,
            pricing.FiniteDifferenceBarrierEngine,
            pricing.MonteCarloBarrierEngine,
        ):
            engine = factory()
            with self.subTest(engine=factory.__name__):
                self.assertEqual(
                    engine.price(dated_history(settled, context), context), 0
                )
                self.assertEqual(
                    engine.price_with_greeks(
                        dated_history(settled, context), context, "delta"
                    ).delta,
                    0,
                )
                for changes in (
                    {"touch_state": "untouched"},
                    {"rebate": 10, "rebate_timing": "at_expiry"},
                    {"barrier_type": "up_and_in", "rebate_timing": "at_expiry"},
                    {"touch_state": None},
                ):
                    with self.assertRaises(InvalidInputError):
                        engine.price(
                            dated_history(settled.replace(**changes), context), context
                        )

    def test_structured_contract_calendars_control_observations_and_survive_reconstruction(
        self,
    ):
        start, end = datetime(2025, 1, 3), datetime(2025, 1, 6)
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=1e-12
        )
        context = market.PricingContext(
            model_parameters=model, spot_price=100, valuation_time=start
        )
        accrual = ins.RangeAccrualOption(
            observation_clock_time=clock_time(),
            notional=1000,
            annual_coupon_rate=0.12,
            lower_bound=90,
            upper_bound=110,
            effective_time=start,
            expiry_time=end,
            realized_hit_count=0,
        )
        expiry = context.replace(valuation_time=end)
        engine = pricing.AnalyticRangeAccrualEngine()
        price = engine.price(dated_history(accrual, expiry), expiry)
        self.assertAlmostEqual(price, 360 / 365, delta=1e-12)
        daily = accrual.replace(
            calendar=market.all_days_calendar(),
            realized_hit_count=accrual.realized_hit_count,
        )
        self.assertAlmostEqual(
            engine.price(dated_history(daily, expiry), expiry), price / 3, delta=1e-12
        )
        accumulator = ins.Accumulator(
            observation_clock_time=clock_time(),
            strike=90,
            knock_out_level=200,
            daily_quantity=1,
            acceleration_factor=1,
            effective_time=start,
            expiry_time=end,
        )
        note = ins.standard_snowball(
            observation_clock_time=clock_time(),
            coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            knock_out_level=110,
            observation_times=[end],
            effective_time=start,
            expiry_time=end,
            calendar=market.all_days_calendar(),
        )
        for value in (daily, accumulator, note):
            self.assertEqual(value.replace().calendar, value.calendar)
            self.assertEqual(pickle.loads(pickle.dumps(value)).calendar, value.calendar)
            self.assertEqual(value.replace_history().calendar, value.calendar)
        self.assertEqual(
            note.with_quoted_coupon(
                0.2, quote_convention="shift_maturity_coupon"
            ).calendar,
            note.calendar,
        )

    def test_constant_node_curves_match_scalars_and_keep_coverage(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=start, expiry_time=end
        )
        curve = market.RateTermStructure(
            reference_time=start, times=[end, end + timedelta(days=365)], factors=[1, 1]
        )
        self.assertEqual(curve.exact_flat_rate, 0)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        nodes = context.replace(
            model_parameters=context.model_parameters.replace(
                risk_free_rate=curve, dividend_yield=curve
            )
        )
        short = context.replace(
            model_parameters=context.model_parameters.replace(
                risk_free_rate=market.RateTermStructure(
                    reference_time=start,
                    times=[start + timedelta(days=180)],
                    factors=[1],
                )
            )
        )
        for engine in (
            pricing.AnalyticEuropeanEngine(),
            pricing.CoxRossRubinsteinVanillaEngine(),
            pricing.QuadratureVanillaEngine(),
            pricing.FiniteDifferenceVanillaEngine(),
            pricing.MonteCarloVanillaEngine(path_count=512, seed=42, thread_count=1),
        ):
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(dated_history(option, context), context),
                    engine.price(dated_history(option, nodes), nodes),
                    delta=1e-9,
                )
                with self.assertRaises(InvalidInputError):
                    engine.price(dated_history(option, short), short)

    def test_heston_greeks_check_model_before_request_controls(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=start, expiry_time=end
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for engine in (
            pricing.AnalyticHestonEuropeanEngine(),
            pricing.FiniteDifferenceHestonVanillaEngine(),
        ):
            for request in ([], "all"):
                with (
                    self.subTest(engine=type(engine).__name__, request=request),
                    self.assertRaises(UnsupportedOperationError),
                ):
                    numerical_greeks(
                        engine,
                        dated_history(option, context),
                        context,
                        request,
                        time_shift=timedelta(days=0),
                    )

    def test_built_in_pricing_requires_valuation_within_inclusive_contract_life(self):
        start, end = datetime(2025, 1, 1), datetime(2025, 1, 2)
        life = {"effective_time": start, "expiry_time": end}
        cases = [
            (
                pricing.AnalyticEuropeanEngine(),
                ins.EuropeanOption(option_type="call", strike=100, **life),
            )
        ]
        cases.extend(
            (
                pricing.AnalyticRangeAccrualEngine(),
                ins.RangeAccrualOption(
                    observation_clock_time=clock_time(),
                    notional=100,
                    annual_coupon_rate=coupon,
                    lower_bound=90,
                    upper_bound=110,
                    **life,
                ),
            )
            for coupon in (0, 0.1)
        )
        for engine, option in cases:
            for at, valid in (
                (
                    start - timedelta(microseconds=1),
                    isinstance(option, ins.EuropeanOption),
                ),
                (start, True),
                (end, True),
                (end + timedelta(microseconds=1), False),
            ):
                context = market.PricingContext(
                    model_parameters=market.BlackScholesMertonParameters(
                        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
                    ),
                    spot_price=100,
                    valuation_time=at,
                )
                with self.subTest(instrument=type(option).__name__, at=at):
                    if valid:
                        self.assertGreaterEqual(
                            engine.price(dated_history(option, context), context), 0
                        )
                        self.assertGreaterEqual(
                            engine.price_with_greeks(
                                dated_history(option, context), context, "delta"
                            ).price,
                            0,
                        )
                    else:
                        with self.assertRaises(InvalidInputError):
                            engine.price(dated_history(option, context), context)
                        with self.assertRaises(InvalidInputError):
                            engine.price_with_greeks(
                                dated_history(option, context), context, "delta"
                            )

    def test_observation_construction_validates_numeric_terms_before_note_creation(
        self,
    ):
        time = datetime(2026, 1, 1)
        for level in (0, -1, float("nan"), float("inf")):
            with self.subTest(level=level):
                with self.assertRaises(InvalidInputError):
                    ins.AutocallableObservation(time, level)
                with self.assertRaises(InvalidInputError):
                    ins.autocallable_observations([time], [level])
        for field, keyword, invalids in (
            (
                "coupon_rate",
                "knock_out_coupon_rates",
                (float("nan"), float("inf"), -float("inf")),
            ),
            (
                "coupon_barrier_level",
                "coupon_barrier_levels",
                (-1, float("nan"), float("inf")),
            ),
        ):
            for invalid in invalids:
                with self.subTest(field=field, invalid=invalid):
                    with self.assertRaises(InvalidInputError):
                        getattr(
                            ins.AutocallableObservation,
                            "snowball" if field == "coupon_rate" else "phoenix",
                        )(time, 110, **{field: invalid})
                    with self.assertRaises(InvalidInputError):
                        ins.autocallable_observations(
                            [time], [110], **{keyword: [invalid]}
                        )
        records = ins.autocallable_observations(
            [time], [110], knock_out_coupon_rates=[-0.01], coupon_barrier_levels=[0]
        )
        self.assertEqual(records[0].coupon_rate, -0.01)
        self.assertEqual(records[0].coupon_barrier_level, 0)

    def test_asian_history_uses_optional_positive_averages(self):
        start, end = datetime(2025, 1, 1), datetime(2025, 1, 11)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime(2025, 1, 2),
        )
        for kind, engine in (
            (
                ins.GeometricAveragePriceOption,
                pricing.AnalyticGeometricAveragePriceEngine(),
            ),
            (
                ins.ArithmeticAveragePriceOption,
                pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
            ),
        ):
            with self.subTest(kind=kind.__name__):
                original = kind(
                    option_type="call",
                    strike=100,
                    effective_time=start,
                    averaging_start_time=start,
                    expiry_time=end,
                )
                self.assertIsNone(original.realized_average)
                with self.assertRaises(InvalidInputError):
                    engine.price(dated_history(original, context), context)
                seasoned = original.replace_history(realized_average=110)
                self.assertEqual(seasoned.realized_average, 110)
                self.assertGreater(
                    engine.price(dated_history(seasoned, context), context), 0
                )
                self.assertEqual(seasoned.replace_history(), seasoned)
                self.assertEqual(
                    seasoned.replace_history(realized_average=None), original
                )
                self.assertEqual(seasoned.replace(realized_average=None), original)
                self.assertEqual(pickle.loads(pickle.dumps(original)), original)
                self.assertEqual(pickle.loads(pickle.dumps(seasoned)), seasoned)
                for invalid in (0, -1, float("nan"), float("inf")):
                    with self.assertRaises(InvalidInputError):
                        seasoned.replace_history(realized_average=invalid)
                self.assertEqual(seasoned.realized_average, 110)

    def test_integer_inputs_share_the_index_protocol(self):
        class Index:
            def __init__(self, value):
                self.value = value

            def __index__(self):
                return self.value

        schedule = market.fixed_interval_schedule(
            observation_clock_time=clock_time(),
            start_time=datetime(2025, 1, 1),
            end_time=datetime(2025, 1, 10),
            interval_days=Index(2),
        )
        self.assertEqual(schedule[Index(1)], datetime(2025, 1, 6))
        self.assertEqual(
            pricing.CoxRossRubinsteinVanillaEngine(step_count=Index(8)).step_count, 8
        )
        self.assertEqual(pricing.MonteCarloVanillaEngine(seed=Index(42)).seed, 42)
        self.assertEqual(
            pricing.AnalyticHestonEuropeanEngine(
                max_evaluations=Index(32)
            ).max_evaluations,
            32,
        )
        for invalid in (True, 2.5):
            with self.subTest(invalid=invalid), self.assertRaises(TypeError):
                pricing.CoxRossRubinsteinVanillaEngine(step_count=invalid)
        with self.assertRaises(OverflowError):
            pricing.CoxRossRubinsteinVanillaEngine(step_count=Index(2**31))
        with self.assertRaises(OverflowError):
            pricing.MonteCarloVanillaEngine(seed=Index(2**64))
        failure = RuntimeError("index conversion failed")

        class FailingIndex:
            def __index__(self):
                raise failure

        with self.assertRaises(RuntimeError) as caught:
            pricing.MonteCarloVanillaEngine(seed=FailingIndex())
        self.assertIs(caught.exception, failure)

    def test_exact_numbers_reject_implicit_rounding(self):
        class CoercingInteger(int):
            def __eq__(self, other):
                return float(self) == other

        fields = {
            "option_type": "call",
            "effective_time": datetime(2025, 1, 1),
            "expiry_time": datetime(2026, 1, 1),
        }
        original = ins.EuropeanOption(strike=100, **fields)
        for value in (
            2**53 + 1,
            CoercingInteger(2**53 + 1),
            CoercingInteger(2**63 - 1),
            Fraction(2**53 + 1, 2**53),
            Fraction(1, 10),
            Decimal("0.1"),
            Decimal(2**53 + 1),
        ):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    ins.EuropeanOption(strike=value, **fields)
                with self.assertRaises(ValueError):
                    original.replace(strike=value)
                self.assertEqual(original.strike, 100)
                self.assertEqual(
                    original.replace(strike=float(value)).strike, float(value)
                )
        for value in (
            2**53 + 2,
            CoercingInteger(2**53 + 2),
            Fraction(1, 2),
            Decimal(100),
            Decimal("0.125"),
            Decimal.from_float(0.1),
        ):
            with self.subTest(value=value):
                self.assertEqual(
                    ins.EuropeanOption(strike=value, **fields).strike, value
                )

    def test_decimal_conversion_preserves_context_and_error_categories(self):
        start, expiry = datetime(2025, 1, 1), datetime(2026, 1, 1)
        with localcontext() as decimal_context:
            for signal in (FloatOperation, Inexact, Rounded):
                decimal_context.traps[signal] = True
            decimal_context.clear_flags()
            model = market.BlackScholesMertonParameters(
                risk_free_rate=Decimal("0.125"),
                dividend_yield=Decimal(0),
                volatility=Decimal("0.25"),
            )
            context = market.PricingContext(
                model_parameters=model,
                spot_price=Decimal(100),
                valuation_time=start,
            )
            option = ins.EuropeanOption(
                option_type="call",
                strike=Decimal(100),
                effective_time=start,
                expiry_time=expiry,
            )
            engine = pricing.AnalyticEuropeanEngine()
            quote = engine.price(dated_history(option, context), context)
            self.assertAlmostEqual(
                pricing.implied_volatility(
                    engine,
                    dated_history(option, context),
                    context,
                    Decimal.from_float(quote),
                    lower_bound=Decimal("0.125"),
                    upper_bound=Decimal("0.5"),
                ),
                0.25,
                delta=1e-8,
            )
            for invalid in (Decimal("1e400"), Decimal("-1e400")):
                with self.assertRaises(OverflowError):
                    option.replace(strike=invalid)
            for invalid in (Decimal("NaN"), Decimal("Infinity"), Decimal("-Infinity")):
                with self.assertRaises(InvalidInputError):
                    option.replace(strike=invalid)
            for invalid in (Decimal("0.1"), Decimal("1e-400")):
                with self.assertRaises(ValueError):
                    option.replace(strike=invalid)
            self.assertEqual(
                model.replace(
                    risk_free_rate=float(Decimal("1e-400"))
                ).flat_risk_free_rate,
                0,
            )
            with self.assertRaises(TypeError):
                option.replace(strike=True)
            self.assertEqual(option.strike, 100)
            self.assertFalse(any(decimal_context.flags.values()))

    def test_settled_accumulators_have_no_remaining_cashflows(self):
        from kiyosi import InvalidInputError

        start, end = datetime(2025, 1, 1), datetime(2025, 1, 10)
        option = ins.Accumulator(
            observation_clock_time=clock_time(),
            strike=90,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            effective_time=start,
            expiry_time=end,
        )
        settled = option.replace_history(knock_out_settled=True)
        self.assertFalse(option.knock_out_settled)
        self.assertTrue(settled.replace(strike=95).knock_out_settled)
        self.assertTrue(
            settled.replace_history(accumulated_quantity=3).knock_out_settled
        )
        self.assertEqual(pickle.loads(pickle.dumps(settled)), settled)
        self.assertTrue(
            ins.Accumulator(
                observation_clock_time=clock_time(),
                strike=90,
                knock_out_level=110,
                daily_quantity=1,
                acceleration_factor=2,
                effective_time=start,
                expiry_time=end,
                knock_out_settled=True,
            ).knock_out_settled
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime(2025, 1, 3),
        )
        for factory in (
            pricing.QuadratureAccumulatorEngine,
            pricing.FiniteDifferenceAccumulatorEngine,
            pricing.MonteCarloAccumulatorEngine,
        ):
            engine = factory()
            with self.subTest(engine=factory.__name__):
                self.assertEqual(
                    engine.price(dated_history(settled, context), context), 0
                )
                self.assertEqual(
                    engine.price(
                        dated_history(
                            settled.replace_history(accumulated_quantity=3), context
                        ),
                        context,
                    ),
                    0,
                )
                active = settled.replace_history(
                    knock_out_settled=False, accumulated_quantity=0
                )
                self.assertGreater(
                    engine.price(dated_history(active, context), context), 0
                )
                with self.assertRaises(InvalidInputError):
                    engine.price(
                        dated_history(
                            settled.replace_history(knock_out_settled=False), context
                        ),
                        context,
                    )
                result = engine.price_with_greeks(
                    dated_history(settled, context), context, "all"
                )
                for greek in (
                    "delta",
                    "gamma",
                    "theta",
                    "vega",
                    "rho",
                ):
                    self.assertEqual(result.require(greek), 0)
                with self.assertRaises(UnsupportedOperationError):
                    pricing.implied_volatility(
                        engine, dated_history(settled, context), context, 0
                    )
        for invalid in (0, 1, "yes"):
            with self.subTest(invalid=invalid), self.assertRaises(TypeError):
                option.replace_history(knock_out_settled=invalid)

    def test_smaller_volatility_bumps_recover_unavailable_stencils(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=start, expiry_time=end
        )
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.00005
        )
        context = market.PricingContext(
            model_parameters=model, spot_price=100, valuation_time=start
        )
        engine = pricing.AnalyticEuropeanEngine()
        unavailable = numerical_greeks(
            engine, dated_history(option, context), context, "vega"
        )
        self.assertEqual(unavailable.status("vega"), "stencil_unavailable")
        self.assertIsNone(unavailable.vega)
        restored = pickle.loads(pickle.dumps(unavailable))
        self.assertEqual(restored, unavailable)
        self.assertEqual(restored.status("vega"), "stencil_unavailable")
        recovered = numerical_greeks(
            engine,
            dated_history(option, context),
            context,
            "vega",
            volatility_shift=0.00001,
        )
        exact = engine.price_with_greeks(
            dated_history(option, context), context, "vega"
        )
        self.assertEqual(recovered.status("vega"), "available")
        self.assertAlmostEqual(recovered.vega, exact.vega, delta=1e-7)
        zero = context.replace(model_parameters=model.replace(volatility=0))
        self.assertEqual(
            numerical_greeks(engine, dated_history(option, zero), zero, "vega").status(
                "vega"
            ),
            "undefined",
        )

    def test_greek_status_distinguishes_requests_support_and_stencils(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=start, expiry_time=end
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        result = pricing.AnalyticEuropeanEngine().price_with_greeks(
            dated_history(option, context), context, "delta"
        )
        self.assertEqual(result.status("delta"), "available")
        self.assertEqual(result.status("gamma"), "not_requested")
        self.assertEqual(result.requested, ["delta"])
        terminal = pricing.AnalyticEuropeanEngine().price_with_greeks(
            dated_history(option, context.replace(valuation_time=end)),
            context.replace(valuation_time=end),
            "delta",
        )
        self.assertEqual(terminal.status("delta"), "undefined")
        collapsed = numerical_greeks(
            pricing.QuadratureVanillaEngine(),
            dated_history(option, context),
            context,
            "delta",
            spot_shift=400,
        )
        self.assertEqual(collapsed.status("delta"), "stencil_unavailable")
        heston = context.replace(
            model_parameters=market.HestonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                initial_variance=0.04,
                long_run_variance=0.04,
                mean_reversion_speed=1.5,
                vol_of_vol=0.3,
                correlation=-0.7,
            )
        )
        unsupported = pricing.AnalyticHestonEuropeanEngine().price_with_greeks(
            dated_history(option, heston), heston, "delta"
        )
        self.assertEqual(unsupported.status("delta"), "unsupported")
        self.assertEqual(unsupported.status("gamma"), "not_requested")
        local = context.replace(
            model_parameters=market.LocalVolatilityParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                surface=market.LocalVolatilitySurface(
                    times=[start, end], spots=[80, 120], values=[[0.2, 0.2], [0.2, 0.2]]
                ),
            )
        )
        partial = pricing.FiniteDifferenceVanillaEngine().price_with_greeks(
            dated_history(option, local), local, ["delta", "vega"]
        )
        self.assertEqual(partial.status("delta"), "available")
        self.assertEqual(partial.status("vega"), "unsupported")
        self.assertEqual(partial.requested, ["delta", "vega"])
        for original in (result, terminal, collapsed, unsupported, partial):
            restored = pickle.loads(pickle.dumps(original))
            self.assertEqual(restored.price, original.price)
            self.assertEqual(restored.requested, original.requested)
            self.assertEqual(restored.supported, original.supported)
            for greek in ("delta", "gamma", "vega"):
                self.assertEqual(restored.status(greek), original.status(greek))
                self.assertEqual(getattr(restored, greek), getattr(original, greek))
        with self.assertRaises(ValueError):
            result.status("unknown")

    def test_flat_rate_query_is_independent_of_reference_time(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        for reference in (None, start):
            curve = market.RateTermStructure.flat(0.05, reference_time=reference)
            self.assertEqual(curve.exact_flat_rate, 0.05)
            self.assertAlmostEqual(curve.shifted(0.01).exact_flat_rate, 0.06)
            self.assertEqual(curve.reference_time, reference)
            restored = pickle.loads(pickle.dumps(curve))
            self.assertEqual(restored.exact_flat_rate, 0.05)
            self.assertEqual(restored.reference_time, reference)
        nodes = market.RateTermStructure(
            reference_time=start, times=[end], factors=[0.95]
        )
        self.assertAlmostEqual(nodes.exact_flat_rate, -math.log(0.95))

    def test_numerical_greeks_has_uniform_controls_across_engines(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        option = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=start, expiry_time=end
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        expected = (
            pricing.AnalyticEuropeanEngine()
            .price_with_greeks(dated_history(option, context), context, "delta")
            .require("delta")
        )
        for engine in (
            pricing.AnalyticEuropeanEngine(),
            pricing.QuadratureVanillaEngine(),
            pricing.FiniteDifferenceVanillaEngine(
                asset_step_count=100, time_step_count=100
            ),
            pricing.MonteCarloVanillaEngine(path_count=4096, seed=42, thread_count=1),
        ):
            with self.subTest(engine=type(engine).__name__):
                result = pricing.numerical_greeks(
                    engine,
                    dated_history(option, context),
                    context,
                    "delta",
                    spot_shift=0.2,
                )
                self.assertEqual(
                    result.price, engine.price(dated_history(option, context), context)
                )
                self.assertAlmostEqual(result.require("delta"), expected, delta=0.03)
                self.assertEqual(result.requested, ["delta"])
                with self.assertRaises(InvalidInputError):
                    pricing.numerical_greeks(
                        engine,
                        dated_history(option, context),
                        context,
                        "delta",
                        rate_shift=0,
                    )
        with self.assertRaises(TypeError):
            pricing.numerical_greeks(
                pricing.AnalyticEuropeanEngine(),
                dated_history(option, context),
                context,
                "delta",
                spot_shift=True,
            )
        with self.assertRaises(UnsupportedOperationError):
            pricing.numerical_greeks(
                pricing.AnalyticEuropeanEngine(),
                dated_history(
                    ins.AmericanOption(
                        option_type="call",
                        strike=100,
                        effective_time=start,
                        expiry_time=end,
                    ),
                    context,
                ),
                context,
                "unknown",
            )
        with self.assertRaises(InvalidInputError):
            pricing.numerical_greeks(
                pricing.AnalyticEuropeanEngine(),
                dated_history(option, context),
                context,
                [],
            )

    def test_double_touch_barrier_type_roundtrips_through_replacement(self):
        for factory, expected in (
            (ins.cash_double_touch, "knock_in"),
            (ins.cash_double_no_touch, "knock_out"),
        ):
            option = factory(
                effective_time=datetime(2025, 1, 1),
                expiry_time=datetime(2026, 1, 1),
                lower_barrier=80,
                upper_barrier=120,
                payout=10,
            )
            self.assertEqual(option.barrier_type, expected)
            self.assertEqual(option.replace(barrier_type=option.barrier_type), option)
            other = "knock_out" if expected == "knock_in" else "knock_in"
            self.assertEqual(option.replace(barrier_type=other).barrier_type, other)
            self.assertEqual(pickle.loads(pickle.dumps(option)).barrier_type, expected)


class GreekShiftValidationTests(unittest.TestCase):
    def test_shift_validation_is_independent_of_estimator_and_valuation_time(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        life = {"effective_time": start, "expiry_time": end}
        bsm = market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        vanilla = ins.EuropeanOption(option_type="call", strike=100, **life)
        digital = ins.CashOrNothingOption(
            option_type="call", strike=100, payout=10, **life
        )
        sampling = {"path_count": 32, "seed": 42, "thread_count": 1}
        cases = [
            (pricing.AnalyticEuropeanEngine(), vanilla, bsm),
            (pricing.QuadratureVanillaEngine(), vanilla, bsm),
            (pricing.CoxRossRubinsteinDigitalEngine(), digital, bsm),
            (
                pricing.CoxRossRubinsteinBarrierEngine(),
                ins.BarrierOption(
                    option_type="call",
                    strike=100,
                    barrier_level=120,
                    barrier_type="up_and_out",
                    touch_state="untouched",
                    **life,
                ),
                bsm,
            ),
            (pricing.MonteCarloDigitalEngine(**sampling), digital, bsm),
            (
                pricing.MonteCarloBarrierEngine(**sampling),
                ins.BarrierOption(
                    option_type="call",
                    strike=100,
                    barrier_level=120,
                    barrier_type="up_and_out",
                    touch_state="untouched",
                    **life,
                ),
                bsm,
            ),
            (
                pricing.MonteCarloDoubleBarrierEngine(**sampling),
                ins.DoubleBarrierOption(
                    option_type="call",
                    strike=100,
                    lower_barrier=80,
                    upper_barrier=120,
                    barrier_type="knock_out",
                    touch_state="untouched",
                    **life,
                ),
                bsm,
            ),
            *(
                (
                    pricing.MonteCarloAsianEngine(**sampling),
                    kind(
                        option_type="call",
                        strike=100,
                        averaging_start_time=end,
                        **life,
                    ),
                    bsm,
                )
                for kind in (
                    ins.GeometricAveragePriceOption,
                    ins.ArithmeticAveragePriceOption,
                )
            ),
        ]
        for engine, option, model in cases:
            for at in (start, end):
                context = market.PricingContext(
                    model_parameters=model, spot_price=110, valuation_time=at
                )
                with self.subTest(engine=type(engine).__name__, at=at):
                    self.assertGreaterEqual(
                        engine.price_with_greeks(
                            dated_history(option, context), context, "delta"
                        ).price,
                        0,
                    )
                    for field in (
                        "spot_shift",
                        "volatility_shift",
                        "rate_shift",
                        "time_shift",
                    ):
                        invalids = (
                            (timedelta(), timedelta(days=-1))
                            if field == "time_shift"
                            else (0, -1, float("nan"), float("inf"))
                        )
                        for bad in invalids:
                            with (
                                self.subTest(field=field, invalid=bad),
                                self.assertRaises(InvalidInputError),
                            ):
                                numerical_greeks(
                                    engine,
                                    dated_history(option, context),
                                    context,
                                    "delta",
                                    **{field: bad},
                                )


class SettingsDiagnosticTests(unittest.TestCase):
    def test_settings_diagnostics_identify_invalid_fields(self):
        cases = [
            (pricing.CoxRossRubinsteinVanillaEngine, "step_count", 0),
            (pricing.CoxRossRubinsteinVanillaEngine, "step_count", 1_000_001),
            (pricing.MonteCarloVanillaEngine, "path_count", 0),
            (pricing.MonteCarloVanillaEngine, "path_count", 10_000_001),
            (pricing.MonteCarloVanillaEngine, "thread_count", -1),
            (pricing.FiniteDifferenceVanillaEngine, "asset_step_count", 2),
            (pricing.FiniteDifferenceVanillaEngine, "time_step_count", 0),
            (pricing.FiniteDifferenceVanillaEngine, "asset_upper_boundary", 0.0),
            (pricing.FiniteDifferenceVanillaEngine, "asset_upper_boundary", math.nan),
            (pricing.FiniteDifferenceHestonVanillaEngine, "variance_step_count", 2),
            (pricing.FiniteDifferenceHestonVanillaEngine, "asset_step_count", 2),
            (pricing.FiniteDifferenceHestonVanillaEngine, "time_step_count", 0),
            (
                pricing.FiniteDifferenceHestonVanillaEngine,
                "variance_upper_boundary",
                -1.0,
            ),
            (
                pricing.FiniteDifferenceHestonVanillaEngine,
                "variance_upper_boundary",
                math.inf,
            ),
            (pricing.FiniteDifferenceHestonVanillaEngine, "asset_upper_boundary", 0.0),
            (pricing.QuadratureBarrierEngine, "asset_step_count", 3),
            (pricing.QuadratureBarrierEngine, "integration_standard_deviations", 0.0),
            (
                pricing.QuadratureBarrierEngine,
                "integration_standard_deviations",
                math.nan,
            ),
            (pricing.QuadratureVanillaEngine, "time_step_count", 0),
            (pricing.AnalyticHestonEuropeanEngine, "absolute_tolerance", -1.0),
            (pricing.AnalyticHestonEuropeanEngine, "absolute_tolerance", math.nan),
            (pricing.AnalyticHestonEuropeanEngine, "relative_tolerance", -1.0),
            (pricing.AnalyticHestonEuropeanEngine, "relative_tolerance", math.inf),
            (pricing.AnalyticHestonEuropeanEngine, "max_evaluations", 0),
        ]
        for factory, field, invalid in cases:
            with self.subTest(engine=factory.__name__, field=field):
                with self.assertRaises(InvalidInputError) as caught:
                    factory(**{field: invalid})
                self.assertIn(field, str(caught.exception))

    def test_surface_diagnostics_identify_the_invalid_node(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        with self.assertRaises(InvalidInputError) as caught:
            market.LocalVolatilitySurface(
                times=[start, end], spots=[80, 100], values=[[0.2, -0.1], [0.2, 0.2]]
            )
        self.assertIn("values[0][1]", str(caught.exception))


class GreekMethodTests(unittest.TestCase):
    def test_greek_methods_check_curve_support_without_rejecting_asian_settlement(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        life = {"effective_time": start, "expiry_time": end}
        european = ins.EuropeanOption(option_type="call", strike=100, **life)
        american = ins.AmericanOption(option_type="put", strike=100, **life)
        digital = ins.CashOrNothingOption(
            option_type="call", strike=100, payout=10, **life
        )
        geometric = ins.GeometricAveragePriceOption(
            option_type="call", strike=100, averaging_start_time=start, **life
        )
        arithmetic = ins.ArithmeticAveragePriceOption(
            option_type="call", strike=100, averaging_start_time=start, **life
        )
        mc = pricing.MonteCarloAsianEngine(path_count=16, seed=42, thread_count=1)
        curve = market.RateTermStructure(
            reference_time=start,
            times=[start + timedelta(days=180), end],
            factors=[0.98, 0.94],
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for engine, option, method in [
            (pricing.CoxRossRubinsteinVanillaEngine(), european, "mixed"),
            (pricing.CoxRossRubinsteinVanillaEngine(), american, "mixed"),
            (pricing.CoxRossRubinsteinDigitalEngine(), digital, "native"),
            (pricing.BjerksundStenslandVanillaEngine(), american, "numerical"),
            (mc, geometric, "native"),
            (
                pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
                arithmetic,
                "numerical",
            ),
        ]:
            with self.subTest(
                engine=type(engine).__name__, option=type(option).__name__
            ):
                self.assertEqual(engine.greek_method(option, context), method)
                self.assertTrue(
                    math.isfinite(engine.price(dated_history(option, context), context))
                )
                for dividend in (False, True):
                    for volatility in (0, 0.2):
                        unsupported = context.replace(
                            model_parameters=market.BlackScholesMertonParameters(
                                risk_free_rate=0.05 if dividend else curve,
                                dividend_yield=curve if dividend else 0.02,
                                volatility=volatility,
                            )
                        )
                        with self.assertRaises(UnsupportedOperationError):
                            engine.price(
                                dated_history(option, unsupported), unsupported
                            )
                        with self.assertRaises(UnsupportedOperationError):
                            engine.greek_method(option, unsupported)
                        with self.assertRaises(UnsupportedOperationError):
                            engine.active_greek_controls(option, unsupported)
        curved = context.replace(
            model_parameters=context.model_parameters.replace(risk_free_rate=curve)
        )
        self.assertEqual(mc.greek_method(arithmetic, curved), "native")
        self.assertTrue(
            math.isfinite(mc.price(dated_history(arithmetic, curved), curved))
        )
        analytic = pricing.AnalyticGeometricAveragePriceEngine()
        self.assertEqual(analytic.greek_method(geometric, curved), "numerical")
        self.assertTrue(
            math.isfinite(analytic.price(dated_history(geometric, curved), curved))
        )
        terminal = curved.replace(valuation_time=end)
        for engine, option in (
            (mc, geometric),
            (pricing.TurnbullWakemanArithmeticAveragePriceEngine(), arithmetic),
        ):
            option = option.replace_history(realized_average=110)
            self.assertEqual(engine.greek_method(option, terminal), "terminal")
            self.assertEqual(
                engine.price(dated_history(option, terminal), terminal), 10
            )

    def test_greek_methods_report_estimators_and_active_controls(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        life = {"effective_time": start, "expiry_time": end}
        option = ins.EuropeanOption(option_type="call", strike=100, **life)
        digital = ins.CashOrNothingOption(
            option_type="call", strike=100, payout=10, **life
        )
        barrier = ins.BarrierOption(
            option_type="call",
            strike=100,
            barrier_level=120,
            barrier_type="up_and_out",
            **life,
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        all_controls = [
            "spot_shift",
            "volatility_shift",
            "rate_shift",
            "time_shift",
        ]
        for engine, contract, method, controls in [
            (pricing.AnalyticEuropeanEngine(), option, "native", []),
            (pricing.QuadratureVanillaEngine(), option, "numerical", all_controls),
            (pricing.CoxRossRubinsteinVanillaEngine(), option, "mixed", all_controls),
            (pricing.MonteCarloVanillaEngine(), option, "numerical", all_controls),
            (pricing.MonteCarloDigitalEngine(), digital, "native", []),
            (
                pricing.MonteCarloBarrierEngine(),
                barrier,
                "native_with_spot_control",
                ["spot_shift"],
            ),
        ]:
            with self.subTest(engine=type(engine).__name__):
                self.assertEqual(engine.greek_method(contract, context), method)
                self.assertEqual(
                    engine.active_greek_controls(contract, context), controls
                )
                terminal = context.replace(valuation_time=end)
                exact = isinstance(engine, pricing.AnalyticEuropeanEngine)
                self.assertEqual(
                    engine.greek_method(contract, terminal),
                    "native" if exact else "terminal",
                )
                self.assertEqual(
                    engine.active_greek_controls(contract, terminal),
                    [] if exact else ["spot_shift"],
                )
        zero = context.replace(
            model_parameters=context.model_parameters.replace(volatility=0)
        )
        self.assertEqual(
            pricing.AnalyticEuropeanEngine().greek_method(option, zero), "numerical"
        )
        local = context.replace(
            model_parameters=market.LocalVolatilityParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                surface=market.LocalVolatilitySurface(
                    times=[start, end], spots=[80, 120], values=[[0.2, 0.2], [0.2, 0.2]]
                ),
            )
        )
        self.assertEqual(
            pricing.FiniteDifferenceVanillaEngine().active_greek_controls(
                option, local
            ),
            ["spot_shift"],
        )
        with self.assertRaises(UnsupportedOperationError):
            pricing.AnalyticEuropeanEngine().greek_method(option, local)
        heston = context.replace(
            model_parameters=market.HestonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                initial_variance=0.04,
                long_run_variance=0.04,
                mean_reversion_speed=1.5,
                vol_of_vol=0.3,
                correlation=-0.7,
            )
        )
        self.assertEqual(
            pricing.AnalyticHestonEuropeanEngine().greek_method(option, heston),
            "unsupported",
        )
        self.assertEqual(
            pricing.AnalyticHestonEuropeanEngine().active_greek_controls(
                option, heston
            ),
            [],
        )
        with self.assertRaises(UnsupportedOperationError):
            pricing.AnalyticEuropeanEngine().greek_method(digital, context)


class ObservationReplacementTests(unittest.TestCase):
    def test_observation_replacement_preserves_validity_and_original_values(self):
        time = datetime(2026, 1, 1)
        original = ins.AutocallableObservation.snowball(time, 110, coupon_rate=-0.01)
        phoenix = ins.AutocallableObservation.phoenix(
            time, 110, coupon_barrier_level=90
        )
        self.assertEqual(phoenix.coupon_rate, 0)
        self.assertEqual(pickle.loads(pickle.dumps(phoenix)), phoenix)
        self.assertIn("AutocallableObservation.phoenix", repr(phoenix))
        with self.assertRaises(InvalidInputError):
            phoenix.replace(coupon_rate=0.12)
        with self.assertRaises(InvalidInputError):
            ins.autocallable_observations(
                [time], [110], knock_out_coupon_rates=[0.12], coupon_barrier_levels=[90]
            )
        changed = original.replace(
            time=time + timedelta(days=1), knock_out_level=120, coupon_rate=0.08
        )
        self.assertEqual(changed.time, time + timedelta(days=1))
        self.assertEqual(changed.knock_out_level, 120)
        self.assertEqual(changed.coupon_rate, 0.08)
        self.assertEqual(changed.coupon_barrier_level, 0)
        self.assertEqual(original.replace(), original)
        self.assertEqual(original.replace(coupon_rate=...), original)
        self.assertEqual(pickle.loads(pickle.dumps(changed)), changed)
        for fields in (
            {"knock_out_level": 0},
            {"knock_out_level": math.inf},
            {"coupon_rate": math.nan},
            {"coupon_barrier_level": -1},
            {"coupon_barrier_level": math.inf},
        ):
            with self.subTest(fields=fields), self.assertRaises(InvalidInputError):
                original.replace(**fields)
            self.assertEqual(
                (
                    original.time,
                    original.knock_out_level,
                    original.coupon_rate,
                    original.coupon_barrier_level,
                ),
                (time, 110, -0.01, 0),
            )
        for value in (None, True):
            with self.subTest(value=value), self.assertRaises(TypeError):
                original.replace(coupon_rate=value)
