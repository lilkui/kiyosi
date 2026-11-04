"""Term-structure validation and pricing through the Python API."""

import math
import numbers
import unittest
from datetime import UTC, datetime, timedelta, timezone

from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    HestonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    RateTermStructure,
)

REFERENCE = datetime(2025, 1, 1)
MIDDLE = REFERENCE + timedelta(days=180)
MATURITY = REFERENCE + timedelta(days=365)
START, END = REFERENCE, MATURITY


def example_curve():
    first = math.exp(-0.02 * 180 / 365)
    return RateTermStructure(
        reference_time=REFERENCE,
        times=[MIDDLE, MATURITY],
        factors=[first, first * math.exp(-0.08 * 185 / 365)],
    )


def market(rate=None, dividend=0.01, valuation=REFERENCE, volatility=0.2):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=example_curve() if rate is None else rate,
            dividend_yield=dividend,
            volatility=volatility,
        ),
        spot_price=100,
        valuation_time=valuation,
    )


def vanilla(direction="call", strike=103):
    return EuropeanOption(
        option_type=direction,
        strike=strike,
        effective_time=START,
        expiry_time=END,
    )


class RateTermStructureTests(unittest.TestCase):
    def test_flat_rate_queries_and_curve_roundtrips_preserve_coverage(self):
        curve = RateTermStructure.flat(0.03, reference_time=REFERENCE)
        rates = {"risk_free_rate": curve, "dividend_yield": curve}
        models = [
            BlackScholesMertonParameters(**rates, volatility=0.2),
            HestonParameters(
                **rates,
                initial_variance=0.04,
                long_run_variance=0.04,
                mean_reversion_speed=1,
                vol_of_vol=0.2,
                correlation=-0.5,
            ),
            LocalVolatilityParameters(
                **rates,
                surface=LocalVolatilitySurface(
                    times=[REFERENCE - timedelta(days=365), MATURITY],
                    spots=[50, 150],
                    values=[[0.2, 0.2], [0.2, 0.2]],
                ),
            ),
        ]
        for model in models:
            with self.subTest(model=type(model).__name__):
                for field in ("flat_risk_free_rate", "flat_dividend_yield"):
                    self.assertEqual(getattr(model, field), 0.03)
                for field in ("risk_free_curve", "dividend_curve"):
                    copied = getattr(model, field)
                    self.assertIsInstance(copied, RateTermStructure)
                    self.assertEqual(copied, curve)
                    self.assertIsNot(copied, curve)
                replay = model.replace(
                    risk_free_rate=model.risk_free_curve,
                    dividend_yield=model.dividend_curve,
                )
                self.assertEqual(replay, model)
                with self.assertRaises(InvalidInputError):
                    PricingContext(
                        model_parameters=replay,
                        spot_price=100,
                        valuation_time=REFERENCE - timedelta(days=1),
                    )
                undated = model.replace(risk_free_rate=0.03, dividend_yield=0.03)
                self.assertEqual(undated.flat_risk_free_rate, model.flat_risk_free_rate)
                self.assertEqual(undated.flat_dividend_yield, model.flat_dividend_yield)
                nodes = model.replace(
                    risk_free_rate=example_curve(), dividend_yield=example_curve()
                )
                for field in ("flat_risk_free_rate", "flat_dividend_yield"):
                    with self.assertRaises(UnsupportedOperationError):
                        getattr(nodes, field)

    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_log_interpolation_interval_factors_and_right_hand_forwards(self):
        curve = example_curve()
        self.assertEqual(curve.reference_time, REFERENCE)
        self.assertEqual(curve.max_time, MATURITY)
        self.assertEqual(curve.nodes, [MIDDLE, MATURITY])
        self.assertEqual(curve.factors[0], math.exp(-0.02 * 180 / 365))
        self.assertAlmostEqual(
            curve.factor(REFERENCE, REFERENCE + timedelta(days=90, hours=12)),
            math.exp(-0.02 * 90.5 / 365),
            delta=1e-14,
        )
        self.assertAlmostEqual(
            curve.log_factor(
                REFERENCE + timedelta(days=90), MIDDLE + timedelta(days=30)
            ),
            -(0.02 * 90 + 0.08 * 30) / 365,
            delta=1e-14,
        )
        self.assertAlmostEqual(
            curve.forward_rate(MIDDLE - timedelta(microseconds=1)), 0.02, delta=1e-13
        )
        self.assertAlmostEqual(curve.forward_rate(MIDDLE), 0.08, delta=1e-13)
        self.assertAlmostEqual(curve.forward_rate(MATURITY), 0.08, delta=1e-13)
        self.assertEqual(curve.factor(REFERENCE, REFERENCE), 1)
        self.assertEqual(curve.factor(MATURITY, MATURITY), 1)
        self.assertAlmostEqual(
            curve.factor(REFERENCE, MATURITY),
            curve.factor(REFERENCE, MIDDLE) * curve.factor(MIDDLE, MATURITY),
            delta=1e-14,
        )

    def test_node_validation_rejects_bad_factors_order_and_counts(self):
        for factor in (0, -0.5, math.nan, math.inf):
            with self.subTest(factor=factor):
                self.assert_rejection(
                    InvalidInputError,
                    RateTermStructure,
                    reference_time=REFERENCE,
                    times=[MIDDLE],
                    factors=[factor],
                )
        for times in (
            [],
            [REFERENCE],
            [REFERENCE - timedelta(microseconds=1)],
            [MIDDLE, MIDDLE],
            [MATURITY, MIDDLE],
        ):
            with self.subTest(times=times):
                self.assert_rejection(
                    InvalidInputError,
                    RateTermStructure,
                    reference_time=REFERENCE,
                    times=times,
                    factors=[0.95] * len(times),
                )
        self.assert_rejection(
            InvalidInputError,
            RateTermStructure,
            reference_time=REFERENCE,
            times=[MIDDLE, MATURITY],
            factors=[0.95],
        )

    def test_negative_node_rates_are_valid_without_monotonic_factors(self):
        curve = RateTermStructure(
            reference_time=REFERENCE, times=[MIDDLE, MATURITY], factors=[1.02, 0.99]
        )
        self.assertEqual(curve.factor(REFERENCE, MIDDLE), 1.02)
        self.assertLess(curve.forward_rate(REFERENCE), 0)
        self.assertGreater(curve.forward_rate(MIDDLE), 0)
        self.assertAlmostEqual(curve.factor(MIDDLE, MATURITY), 0.99 / 1.02, delta=1e-14)

    def test_microsecond_intervals_remain_distinct_far_from_reference(self):
        origin = datetime(1, 1, 1)
        second = datetime(9998, 1, 1)
        first = second - timedelta(microseconds=1)
        curve = RateTermStructure(
            reference_time=origin,
            times=[first, second],
            factors=[math.exp(-100), math.exp(-101)],
        )
        self.assertAlmostEqual(curve.log_factor(first, second), -1, delta=1e-14)
        self.assertAlmostEqual(curve.factor(first, second), math.exp(-1), delta=1e-14)
        long_years = (first - origin).total_seconds() / (365 * 86400)
        self.assertAlmostEqual(
            curve.forward_rate(first - timedelta(microseconds=1)),
            100 / long_years,
            delta=1e-14,
        )
        rate = 365 * 86400 * 1e6
        self.assertAlmostEqual(curve.forward_rate(first) / rate, 1, delta=1e-13)
        context = market(rate=curve, dividend=0, valuation=first)
        equivalent = market(rate=rate, dividend=0, valuation=first)
        option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_time=first,
            expiry_time=second,
        )
        engine = pricing.AnalyticEuropeanEngine()
        self.assertAlmostEqual(
            engine.price(option, context), 100 * (1 - math.exp(-1)), delta=1e-11
        )
        self.assertAlmostEqual(
            engine.price(option, context), engine.price(option, equivalent), delta=1e-11
        )
        distant_context = market(
            rate=curve, dividend=0, valuation=origin, volatility=1e-8
        )
        distant_option = EuropeanOption(
            option_type="call",
            strike=math.exp(100),
            effective_time=origin,
            expiry_time=second,
        )
        self.assert_rejection(
            ComputationFailureError, engine.price, distant_option, distant_context
        )

    def test_queries_reject_extrapolation_and_reversed_intervals(self):
        curve = example_curve()
        for start, end in (
            (REFERENCE - timedelta(microseconds=1), MIDDLE),
            (REFERENCE, MATURITY + timedelta(microseconds=1)),
            (MIDDLE, REFERENCE),
        ):
            with self.subTest(start=start, end=end):
                self.assert_rejection(InvalidInputError, curve.factor, start, end)
                self.assert_rejection(InvalidInputError, curve.log_factor, start, end)
        for time in (
            REFERENCE - timedelta(microseconds=1),
            MATURITY + timedelta(microseconds=1),
        ):
            with self.subTest(time=time):
                self.assert_rejection(InvalidInputError, curve.forward_rate, time)

    def test_flat_curves_and_parallel_shifts_preserve_coverage(self):
        flat = RateTermStructure.flat(-0.03)
        dated = RateTermStructure.flat(-0.03, reference_time=REFERENCE)
        self.assertIsNone(flat.reference_time)
        self.assertIsNone(flat.max_time)
        self.assertEqual(dated.reference_time, REFERENCE)
        self.assertIsNone(dated.max_time)
        before = REFERENCE - timedelta(days=365)
        self.assertAlmostEqual(
            flat.factor(before, REFERENCE), math.exp(0.03), delta=1e-14
        )
        self.assert_rejection(InvalidInputError, dated.factor, before, REFERENCE)
        curve = example_curve()
        shifted = curve.shifted(0.01)
        self.assertEqual(shifted.reference_time, curve.reference_time)
        self.assertEqual(shifted.max_time, curve.max_time)
        self.assertEqual(shifted.nodes, curve.nodes)
        self.assertEqual(curve.shifted(0), curve)
        self.assertAlmostEqual(
            shifted.factor(MIDDLE, MATURITY),
            curve.factor(MIDDLE, MATURITY) * math.exp(-0.01 * 185 / 365),
            delta=1e-14,
        )
        self.assertAlmostEqual(
            flat.shifted(0.01).forward_rate(REFERENCE), -0.02, delta=1e-15
        )
        self.assert_rejection(InvalidInputError, curve.shifted, math.inf)
        for rate in (math.nan, math.inf):
            with self.subTest(rate=rate):
                self.assert_rejection(InvalidInputError, RateTermStructure.flat, rate)

    def test_python_representation_errors_precede_core_validation(self):
        for value in (True, "0.03", None):
            with self.subTest(value=value), self.assertRaises(TypeError):
                RateTermStructure.flat(value)
        with self.assertRaises(OverflowError):
            RateTermStructure.flat(1 << 10000)
        for factor in (True, "0.95", None):
            with self.subTest(factor=factor), self.assertRaises(TypeError):
                RateTermStructure(
                    reference_time=REFERENCE, times=[MIDDLE], factors=[factor]
                )
        for time in (True, "2025-01-01", datetime(2025, 1, 1, tzinfo=UTC)):
            with self.subTest(time=time), self.assertRaises(TypeError):
                RateTermStructure(reference_time=time, times=[MIDDLE], factors=[0.95])
        with self.assertRaises(TypeError):
            example_curve().shifted(True)

    def test_timezone_free_timestamps_and_owned_nodes(self):
        times = [MIDDLE, MATURITY]
        factors = [0.98, 0.94]
        curve = RateTermStructure(
            reference_time=REFERENCE, times=times, factors=factors
        )
        times.clear()
        factors[0] = 9
        self.assertEqual(curve.reference_time, REFERENCE)
        self.assertEqual(curve.nodes, [MIDDLE, MATURITY])
        self.assertEqual(curve.factors, [0.98, 0.94])
        copied_nodes = curve.nodes
        copied_factors = curve.factors
        copied_nodes.clear()
        copied_factors[0] = 7
        self.assertEqual(curve.nodes, [MIDDLE, MATURITY])
        self.assertEqual(curve.factors, [0.98, 0.94])
        with self.assertRaises(AttributeError):
            curve.reference_time = MATURITY
        date_curve = RateTermStructure(
            reference_time=START, times=[END], factors=[0.94]
        )
        self.assertEqual(date_curve.reference_time, REFERENCE)
        self.assertEqual(date_curve.nodes, [MATURITY])

    def test_context_coverage_and_replace_preserve_independent_curves(self):
        risk_free = example_curve()
        dividend = RateTermStructure(
            reference_time=REFERENCE - timedelta(days=30),
            times=[REFERENCE + timedelta(days=90), MATURITY + timedelta(days=30)],
            factors=[0.99, 0.96],
        )
        context = market(rate=risk_free, dividend=dividend)
        model = context.model_parameters
        self.assertEqual(model.risk_free_curve, risk_free)
        self.assertEqual(model.dividend_curve, dividend)
        with self.assertRaises(UnsupportedOperationError):
            _ = model.flat_risk_free_rate
        with self.assertRaises(UnsupportedOperationError):
            _ = model.flat_dividend_yield
        rolled = context.replace(valuation_time=MIDDLE)
        self.assertEqual(rolled.valuation_time, MIDDLE)
        self.assertEqual(rolled.model_parameters, model)
        for time in (
            REFERENCE - timedelta(microseconds=1),
            MATURITY + timedelta(microseconds=1),
        ):
            with self.subTest(time=time):
                self.assert_rejection(
                    InvalidInputError,
                    context.replace,
                    valuation_time=time,
                )
        self.assertEqual(context.valuation_time, REFERENCE)
        self.assertEqual(context.model_parameters, model)
        replaced = model.replace(volatility=0.25)
        self.assertEqual(replaced.risk_free_curve, risk_free)
        self.assertEqual(replaced.dividend_curve, dividend)
        self.assertEqual(replaced.volatility, 0.25)
        self.assert_rejection(InvalidInputError, model.replace, volatility=0)
        self.assertEqual(model.volatility, 0.2)

    def test_curve_prices_match_quantlib_and_discounted_parity(self):
        import QuantLib as ql

        saved = ql.SavedSettings()
        try:
            ql.Settings.instance().evaluationDate = ql.Date(1, 1, 2025)
            ql_dates = [ql.Date(1, 1, 2025), ql.Date(30, 6, 2025), ql.Date(1, 1, 2026)]
            risk_factors = [
                1.0,
                math.exp(-0.02 * 180 / 365),
                math.exp(-(0.02 * 180 + 0.08 * 185) / 365),
            ]
            dividend_factors = [
                1.0,
                math.exp(-0.01 * 180 / 365),
                math.exp(-(0.01 * 180 + 0.03 * 185) / 365),
            ]
            risk = RateTermStructure(
                reference_time=REFERENCE,
                times=[MIDDLE, MATURITY],
                factors=risk_factors[1:],
            )
            dividend = RateTermStructure(
                reference_time=REFERENCE,
                times=[MIDDLE, MATURITY],
                factors=dividend_factors[1:],
            )
            actual_context = market(rate=risk, dividend=dividend)
            process = ql.BlackScholesMertonProcess(
                ql.QuoteHandle(ql.SimpleQuote(100)),
                ql.YieldTermStructureHandle(
                    ql.DiscountCurve(ql_dates, dividend_factors, ql.Actual365Fixed())
                ),
                ql.YieldTermStructureHandle(
                    ql.DiscountCurve(ql_dates, risk_factors, ql.Actual365Fixed())
                ),
                ql.BlackVolTermStructureHandle(
                    ql.BlackConstantVol(
                        ql_dates[0], ql.NullCalendar(), 0.2, ql.Actual365Fixed()
                    )
                ),
            )
            engine = pricing.AnalyticEuropeanEngine()
            prices = []
            for direction, ql_direction in (
                ("call", ql.Option.Call),
                ("put", ql.Option.Put),
            ):
                reference = ql.VanillaOption(
                    ql.PlainVanillaPayoff(ql_direction, 103),
                    ql.EuropeanExercise(ql_dates[-1]),
                )
                reference.setPricingEngine(ql.AnalyticEuropeanEngine(process))
                price = engine.price(vanilla(direction), actual_context)
                self.assertAlmostEqual(price, reference.NPV(), delta=1e-11)
                prices.append(price)
                equivalent = market(
                    rate=(0.02 * 180 + 0.08 * 185) / 365,
                    dividend=(0.01 * 180 + 0.03 * 185) / 365,
                )
                self.assertAlmostEqual(
                    price, engine.price(vanilla(direction), equivalent), delta=1e-11
                )
            self.assertAlmostEqual(
                prices[0] - prices[1],
                100 * dividend_factors[-1] - 103 * risk_factors[-1],
                delta=1e-11,
            )
            scalar = market(rate=0.03)
            explicit = market(
                rate=RateTermStructure.flat(0.03, reference_time=REFERENCE)
            )
            self.assertEqual(
                engine.price(vanilla(), scalar), engine.price(vanilla(), explicit)
            )
        finally:
            del saved

    def test_replace_signatures_validation_timing_and_failure_state(self):
        model = market().model_parameters
        context = market()
        for obj, fields in (
            (model, ("risk_free_rate", "dividend_yield", "volatility")),
            (context, ("model_parameters", "spot_price", "valuation_time", "calendar")),
        ):
            replacement = obj.replace()
            if isinstance(obj, PricingContext):
                self.assertEqual(replacement.model_parameters, obj.model_parameters)
                self.assertEqual(replacement.spot_price, obj.spot_price)
                self.assertEqual(replacement.valuation_time, obj.valuation_time)
                self.assertEqual(
                    replacement.calendar.trading_days_per_year,
                    obj.calendar.trading_days_per_year,
                )
            else:
                self.assertEqual(replacement, obj)
            self.assertIsNot(replacement, obj)
            for field in fields:
                for value in (None, True, "invalid"):
                    with (
                        self.subTest(field=field, value=value),
                        self.assertRaises(TypeError),
                    ):
                        obj.replace(**{field: value})
            with self.assertRaises(TypeError):
                obj.replace(1)
            with self.assertRaises(TypeError):
                obj.replace(unknown=1)
        for field, value, expected_error in (
            ("risk_free_rate", math.nan, InvalidInputError),
            ("dividend_yield", math.inf, InvalidInputError),
            ("volatility", 0, InvalidInputError),
        ):
            self.assert_rejection(expected_error, model.replace, **{field: value})
        for spot in (0, math.nan, math.inf):
            self.assert_rejection(InvalidInputError, context.replace, spot_price=spot)
        with self.assertRaises(OverflowError):
            model.replace(volatility=10**1000)
        with self.assertRaises(TypeError):
            context.replace(valuation_time=datetime(2025, 1, 6, tzinfo=UTC))
        changed = context.replace(
            spot_price=105,
            valuation_time=datetime(2025, 1, 6, 9),
        )
        self.assertEqual(changed.spot_price, 105)
        self.assertEqual(changed.valuation_time, datetime(2025, 1, 6, 9))
        self.assertEqual(
            changed.calendar.trading_days_per_year,
            context.calendar.trading_days_per_year,
        )
        self.assertEqual(context.valuation_time, REFERENCE)
        self.assertEqual(context.spot_price, 100)
        self.assertEqual(context.model_parameters, model)
        zero = model.replace(risk_free_rate=0, dividend_yield=0)
        self.assertEqual((zero.flat_risk_free_rate, zero.flat_dividend_yield), (0, 0))
        short = RateTermStructure(
            reference_time=REFERENCE, times=[MIDDLE], factors=[0.98]
        )
        shorter = context.replace(model_parameters=model.replace(risk_free_rate=short))
        self.assert_rejection(
            InvalidInputError,
            pricing.AnalyticEuropeanEngine().price,
            vanilla(),
            shorter,
        )
        for time in (
            REFERENCE - timedelta(microseconds=1),
            MATURITY + timedelta(microseconds=1),
        ):
            self.assert_rejection(InvalidInputError, market, valuation=time)
        for risk, dividend in ((0, short), (short, 0)):
            candidate = market(rate=risk, dividend=dividend)
            self.assert_rejection(
                InvalidInputError,
                candidate.replace,
                valuation_time=MATURITY,
            )

    def test_protocol_conversion_errors_and_datetime_boundaries(self):
        from fractions import Fraction

        class ConversionFailure(Exception):
            pass

        @numbers.Real.register
        class BadFloat:
            def __float__(self):
                raise ConversionFailure

        def broken_iterable():
            yield MIDDLE
            raise ConversionFailure

        with self.assertRaises(ConversionFailure):
            RateTermStructure.flat(BadFloat())
        with self.assertRaises(ConversionFailure):
            RateTermStructure(
                reference_time=REFERENCE, times=broken_iterable(), factors=[0.95]
            )
        with self.assertRaises(OverflowError):
            RateTermStructure(
                reference_time=REFERENCE, times=[MIDDLE], factors=[1 << 10000]
            )
        curve = RateTermStructure(
            reference_time=START,
            times=(time for time in [MIDDLE, MATURITY]),
            factors=(float(value) for value in (Fraction(49, 50), Fraction(47, 50))),
        )
        self.assertEqual(curve.factors, [0.98, 0.94])
        for time in (
            datetime.min.replace(tzinfo=timezone(timedelta(hours=1))),
            datetime.max.replace(tzinfo=timezone(-timedelta(hours=1))),
        ):
            with self.subTest(time=time), self.assertRaises(TypeError):
                RateTermStructure.flat(0.03, reference_time=time)
            with self.subTest(time=time), self.assertRaises(TypeError):
                curve.forward_rate(time)
        for operation in (curve.factor, curve.log_factor):
            with self.assertRaises(TypeError):
                operation(True, MATURITY)
        for times, factors in (
            (None, [0.95]),
            ([MIDDLE], None),
            ([True], [0.95]),
            ([MIDDLE], [False]),
        ):
            with (
                self.subTest(times=times, factors=factors),
                self.assertRaises(TypeError),
            ):
                RateTermStructure(
                    reference_time=REFERENCE, times=times, factors=factors
                )
        for obj, field, value in (
            (curve, "factors", [1]),
            (market().model_parameters, "risk_free_curve", curve),
            (market(), "spot_price", 101),
        ):
            with self.assertRaises(AttributeError):
                setattr(obj, field, value)

    def test_bulk_curve_factors_follow_scalar_overflow_and_underflow_rules(self):
        import pickle

        curve = RateTermStructure(
            reference_time=REFERENCE, times=[MATURITY], factors=[0.95]
        )
        overflowing = curve.shifted(-1000)
        self.assert_rejection(ComputationFailureError, lambda: overflowing.factors)
        self.assert_rejection(
            ComputationFailureError, overflowing.factor, REFERENCE, MATURITY
        )
        underflowing = curve.shifted(1000)
        self.assertEqual(underflowing.factors, [0.0])
        self.assertEqual(underflowing.factor(REFERENCE, MATURITY), 0.0)
        self.assertEqual(curve.factors, [0.95])
        for extreme in (overflowing, underflowing):
            self.assertEqual(pickle.loads(pickle.dumps(extreme)), extreme)
        self.assert_rejection(
            InvalidInputError,
            RateTermStructure,
            reference_time=REFERENCE,
            times=[MATURITY],
            factors=underflowing.factors,
        )

    def test_curve_numerical_limits_ownership_and_concurrency(self):
        from concurrent.futures import ThreadPoolExecutor

        self.assertEqual(RateTermStructure.flat(1000).factor(REFERENCE, MATURITY), 0)
        self.assert_rejection(
            ComputationFailureError,
            RateTermStructure.flat(-1000).factor,
            REFERENCE,
            MATURITY,
        )
        self.assert_rejection(
            ComputationFailureError,
            RateTermStructure.flat(1e308).log_factor,
            REFERENCE,
            MATURITY + timedelta(days=365),
        )
        self.assert_rejection(
            InvalidInputError,
            RateTermStructure.flat(1e308).shifted,
            1e308,
        )
        curve = example_curve()
        shifted = curve.shifted(0.01)
        self.assertAlmostEqual(
            shifted.factors[1], curve.factors[1] * math.exp(-0.01), delta=1e-14
        )
        self.assert_rejection(InvalidInputError, curve.shifted(1e308).shifted, 1e308)
        context = market()
        model = context.model_parameters
        owned_curve = model.risk_free_curve
        replaced = context.replace(spot_price=105)
        del context, model
        self.assertEqual(owned_curve, example_curve())
        self.assertEqual(replaced.model_parameters.risk_free_curve, example_curve())
        engine = pricing.AnalyticEuropeanEngine()
        option = vanilla()
        expected = engine.price(option, replaced)
        with ThreadPoolExecutor(max_workers=4) as pool:
            prices = list(pool.map(lambda _: engine.price(option, replaced), range(8)))
        self.assertEqual(prices, [expected] * 8)
