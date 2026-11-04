"""Public API contracts shared by Rust and Python callers."""

import pickle
import unittest
from datetime import datetime, timedelta
from fractions import Fraction

from kiyosi import InvalidInputError, UnsupportedOperationError, market, pricing
from kiyosi import instruments as ins


class ApiConsistencyTests(unittest.TestCase):
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
                (start - timedelta(microseconds=1), False),
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
                        self.assertGreaterEqual(engine.price(option, context), 0)
                        self.assertGreaterEqual(
                            engine.price_with_greeks(option, context, "delta").price, 0
                        )
                    else:
                        with self.assertRaises(InvalidInputError):
                            engine.price(option, context)
                        with self.assertRaises(InvalidInputError):
                            engine.price_with_greeks(option, context, "delta")

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
                        ins.AutocallableObservation(time, 110, **{field: invalid})
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
                    engine.price(original, context)
                seasoned = original.replace_history(realized_average=110)
                self.assertEqual(seasoned.realized_average, 110)
                self.assertGreater(engine.price(seasoned, context), 0)
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
        for value in (2**53 + 2, CoercingInteger(2**53 + 2), Fraction(1, 2)):
            with self.subTest(value=value):
                self.assertEqual(
                    ins.EuropeanOption(strike=value, **fields).strike, value
                )

    def test_terminated_accumulators_have_no_remaining_cashflows(self):
        from kiyosi import InvalidInputError

        start, end = datetime(2025, 1, 1), datetime(2025, 1, 10)
        option = ins.Accumulator(
            strike=90,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            effective_time=start,
            expiry_time=end,
        )
        settled = option.replace_history(terminated=True)
        self.assertFalse(option.terminated)
        self.assertTrue(settled.replace(strike=95).terminated)
        self.assertTrue(settled.replace_history(accumulated_quantity=3).terminated)
        self.assertEqual(pickle.loads(pickle.dumps(settled)), settled)
        self.assertTrue(
            ins.Accumulator(
                strike=90,
                knock_out_level=110,
                daily_quantity=1,
                acceleration_factor=2,
                effective_time=start,
                expiry_time=end,
                terminated=True,
            ).terminated
        )
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime(2025, 1, 3),
            calendar=market.all_days_calendar(),
        )
        for factory in (
            pricing.QuadratureAccumulatorEngine,
            pricing.FiniteDifferenceAccumulatorEngine,
            pricing.MonteCarloAccumulatorEngine,
        ):
            engine = factory()
            with self.subTest(engine=factory.__name__):
                self.assertEqual(engine.price(settled, context), 0)
                self.assertEqual(
                    engine.price(
                        settled.replace_history(accumulated_quantity=3), context
                    ),
                    0,
                )
                active = settled.replace_history(
                    terminated=False, accumulated_quantity=0
                )
                self.assertGreater(engine.price(active, context), 0)
                with self.assertRaises(InvalidInputError):
                    engine.price(settled.replace_history(terminated=False), context)
                result = engine.price_with_greeks(settled, context, "all")
                for greek in ("delta", "gamma", "theta", "vega", "rho"):
                    self.assertEqual(result.require(greek), 0)
                with self.assertRaises(UnsupportedOperationError):
                    pricing.implied_volatility(engine, settled, context, 0)
        for invalid in (None, 0, 1, "yes"):
            with self.subTest(invalid=invalid), self.assertRaises(TypeError):
                option.replace_history(terminated=invalid)

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
            option, context, "delta"
        )
        self.assertEqual(result.status("delta"), "available")
        self.assertEqual(result.status("gamma"), "not_requested")
        self.assertEqual(result.requested, ["delta"])
        terminal = pricing.AnalyticEuropeanEngine().price_with_greeks(
            option, context.replace(valuation_time=end), "delta"
        )
        self.assertEqual(terminal.status("delta"), "undefined")
        collapsed = pricing.QuadratureVanillaEngine().price_with_greeks(
            option, context, "delta", spot_shift=400
        )
        self.assertEqual(collapsed.status("delta"), "undefined")
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
            option, heston, "delta"
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
            option, local, ["delta", "vega"]
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
            self.assertEqual(curve.flat_rate, 0.05)
            self.assertAlmostEqual(curve.shifted(0.01).flat_rate, 0.06)
            self.assertEqual(curve.reference_time, reference)
            restored = pickle.loads(pickle.dumps(curve))
            self.assertEqual(restored.flat_rate, 0.05)
            self.assertEqual(restored.reference_time, reference)
        nodes = market.RateTermStructure(
            reference_time=start, times=[end], factors=[0.95]
        )
        self.assertIsNone(nodes.flat_rate)

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
            .price_with_greeks(option, context, "delta")
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
                    engine, option, context, "delta", spot_shift=0.2
                )
                self.assertEqual(result.price, engine.price(option, context))
                self.assertAlmostEqual(result.require("delta"), expected, delta=0.03)
                self.assertEqual(result.requested, ["delta"])
                with self.assertRaises(InvalidInputError):
                    pricing.numerical_greeks(
                        engine, option, context, "delta", rate_shift=0
                    )
        with self.assertRaises(TypeError):
            pricing.numerical_greeks(
                pricing.AnalyticEuropeanEngine(),
                option,
                context,
                "delta",
                spot_shift=True,
            )
        with self.assertRaises(UnsupportedOperationError):
            pricing.numerical_greeks(
                pricing.AnalyticEuropeanEngine(),
                ins.AmericanOption(
                    option_type="call",
                    strike=100,
                    effective_time=start,
                    expiry_time=end,
                ),
                context,
                "unknown",
            )
        with self.assertRaises(InvalidInputError):
            pricing.numerical_greeks(
                pricing.AnalyticEuropeanEngine(), option, context, []
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
