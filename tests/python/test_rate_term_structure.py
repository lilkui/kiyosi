import math
import unittest
from datetime import UTC, datetime, timedelta, timezone

from kiyosi import ErrorCategory, KiyosiError, pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    ArithmeticAveragePriceOption,
    AssetOrNothingOption,
    BarrierOption,
    BinarySnowballOption,
    CashOrNothingOption,
    DoubleBarrierOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    asset_binary_barrier_option,
    asset_no_touch_up,
    asset_one_touch_down,
    asset_one_touch_up,
    autocallable_observations,
    cash_binary_barrier_option,
    cash_double_no_touch,
    cash_double_touch,
    cash_no_touch_up,
    cash_one_touch_down,
    cash_one_touch_up,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)

REFERENCE = datetime(2025, 1, 1, tzinfo=UTC)
MIDDLE = REFERENCE + timedelta(days=180)
MATURITY = REFERENCE + timedelta(days=365)
START, END = REFERENCE.date(), MATURITY.date()


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
        effective_date=START,
        expiry_date=END,
    )


class RateTermStructureTests(unittest.TestCase):
    def assert_category(self, category, operation, *args, **kwargs):
        with self.assertRaises(KiyosiError) as error:
            operation(*args, **kwargs)
        self.assertEqual(error.exception.category, category)

    def test_log_interpolation_interval_factors_and_right_hand_forwards(self):
        curve = example_curve()
        self.assertFalse(curve.is_flat)
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
                self.assert_category(
                    ErrorCategory.INVALID_PARAMETER,
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
                self.assert_category(
                    ErrorCategory.INVALID_PARAMETER,
                    RateTermStructure,
                    reference_time=REFERENCE,
                    times=times,
                    factors=[0.95] * len(times),
                )
        self.assert_category(
            ErrorCategory.INVALID_PARAMETER,
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
        origin = datetime(1, 1, 1, tzinfo=UTC)
        second = datetime(9998, 1, 1, tzinfo=UTC)
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
            effective_date=first.date(),
            expiry_date=second.date(),
        )
        engine = pricing.AnalyticVanillaEngine()
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
            effective_date=origin.date(),
            expiry_date=second.date(),
        )
        self.assert_category(
            ErrorCategory.INVALID_RESULT, engine.price, distant_option, distant_context
        )

    def test_queries_reject_extrapolation_and_reversed_intervals(self):
        curve = example_curve()
        for start, end in (
            (REFERENCE - timedelta(microseconds=1), MIDDLE),
            (REFERENCE, MATURITY + timedelta(microseconds=1)),
            (MIDDLE, REFERENCE),
        ):
            with self.subTest(start=start, end=end):
                self.assert_category(
                    ErrorCategory.INVALID_TIME_RANGE, curve.factor, start, end
                )
                self.assert_category(
                    ErrorCategory.INVALID_TIME_RANGE, curve.log_factor, start, end
                )
        for time in (
            REFERENCE - timedelta(microseconds=1),
            MATURITY + timedelta(microseconds=1),
        ):
            with self.subTest(time=time):
                self.assert_category(
                    ErrorCategory.INVALID_TIME_RANGE, curve.forward_rate, time
                )

    def test_flat_curves_and_parallel_shifts_preserve_coverage(self):
        flat = RateTermStructure.flat(-0.03)
        dated = RateTermStructure.flat(-0.03, reference_time=REFERENCE)
        self.assertTrue(flat.is_flat)
        self.assertIsNone(flat.reference_time)
        self.assertIsNone(flat.max_time)
        self.assertEqual(dated.reference_time, REFERENCE)
        self.assertIsNone(dated.max_time)
        before = REFERENCE - timedelta(days=365)
        self.assertAlmostEqual(
            flat.factor(before, REFERENCE), math.exp(0.03), delta=1e-14
        )
        self.assert_category(
            ErrorCategory.INVALID_TIME_RANGE, dated.factor, before, REFERENCE
        )
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
        self.assert_category(ErrorCategory.INVALID_PARAMETER, curve.shifted, math.inf)
        for rate in (math.nan, math.inf):
            with self.subTest(rate=rate):
                self.assert_category(
                    ErrorCategory.INVALID_PARAMETER, RateTermStructure.flat, rate
                )

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
        for time in (True, "2025-01-01", datetime(2025, 1, 1)):
            with self.subTest(time=time), self.assertRaises(TypeError):
                RateTermStructure(reference_time=time, times=[MIDDLE], factors=[0.95])
        with self.assertRaises(TypeError):
            example_curve().shifted(True)

    def test_timestamp_normalization_and_owned_nodes(self):
        offset = timezone(timedelta(hours=8))
        times = [MIDDLE.astimezone(offset), MATURITY.astimezone(offset)]
        factors = [0.98, 0.94]
        curve = RateTermStructure(
            reference_time=REFERENCE.astimezone(offset), times=times, factors=factors
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
        self.assertEqual(model.risk_free_rate, risk_free)
        self.assertEqual(model.dividend_yield, dividend)
        rolled = context.replace(valuation_time=MIDDLE)
        self.assertEqual(rolled.valuation_time, MIDDLE)
        self.assertEqual(rolled.model_parameters, model)
        for time in (
            REFERENCE - timedelta(microseconds=1),
            MATURITY + timedelta(microseconds=1),
        ):
            with self.subTest(time=time):
                self.assert_category(
                    ErrorCategory.INVALID_TIME_RANGE,
                    context.replace,
                    valuation_time=time,
                )
        self.assertEqual(context.valuation_time, REFERENCE)
        self.assertEqual(context.model_parameters, model)
        replaced = model.replace(volatility=0.25)
        self.assertEqual(replaced.risk_free_curve, risk_free)
        self.assertEqual(replaced.dividend_curve, dividend)
        self.assertEqual(replaced.volatility, 0.25)
        self.assert_category(
            ErrorCategory.INVALID_VOLATILITY, model.replace, volatility=0
        )
        self.assertEqual(model.volatility, 0.2)

    def test_scalar_and_explicit_flat_inputs_price_equally(self):
        engine = pricing.AnalyticVanillaEngine()
        scalar = market(rate=0.03)
        explicit = market(rate=RateTermStructure.flat(0.03, reference_time=REFERENCE))
        self.assertEqual(scalar.model_parameters.risk_free_rate, 0.03)
        self.assertEqual(
            scalar.model_parameters.risk_free_curve.forward_rate(REFERENCE), 0.03
        )
        self.assertEqual(
            engine.price(vanilla(), scalar), engine.price(vanilla(), explicit)
        )
        for flat_engine in (
            pricing.QuadratureVanillaEngine(),
            pricing.CoxRossRubinsteinVanillaEngine(),
        ):
            with self.subTest(engine=type(flat_engine).__name__):
                self.assertEqual(
                    flat_engine.price(vanilla(), scalar),
                    flat_engine.price(vanilla(), explicit),
                )

    def test_analytic_prices_preserve_parity_digital_decomposition_and_rho(self):
        dividend = RateTermStructure(
            reference_time=REFERENCE - timedelta(days=30),
            times=[REFERENCE + timedelta(days=90), MATURITY],
            factors=[
                math.exp(-0.01 * 120 / 365),
                math.exp(-(0.01 * 120 + 0.03 * 275) / 365),
            ],
        )
        context = market(dividend=dividend)
        engine = pricing.AnalyticVanillaEngine()
        call, put = vanilla(), vanilla("put")
        call_price = engine.price(call, context)
        discount = math.exp(-(0.02 * 180 + 0.08 * 185) / 365)
        dividend_rate = (0.01 * 90 + 0.03 * 275) / 365
        self.assertAlmostEqual(
            call_price - engine.price(put, context),
            100 * math.exp(-dividend_rate) - 103 * discount,
            delta=1e-11,
        )
        equivalent = market(
            rate=(0.02 * 180 + 0.08 * 185) / 365, dividend=dividend_rate
        )
        self.assertAlmostEqual(call_price, engine.price(call, equivalent), delta=1e-11)
        terms = {
            "option_type": "call",
            "strike": 103,
            "effective_date": START,
            "expiry_date": END,
        }
        digital = pricing.AnalyticDigitalEngine()
        self.assertAlmostEqual(
            call_price,
            digital.price(AssetOrNothingOption(**terms), context)
            - 103 * digital.price(CashOrNothingOption(**terms, payout=1), context),
            delta=1e-11,
        )
        h = 1e-5
        prices = [
            engine.price(
                call,
                context.replace(
                    model_parameters=context.model_parameters.replace(
                        risk_free_rate=context.model_parameters.risk_free_curve.shifted(
                            shift
                        )
                    )
                ),
            )
            for shift in (h, -h)
        ]
        rho = engine.price_with_greeks(call, context, "rho").rho
        self.assertAlmostEqual(rho, (prices[0] - prices[1]) / (200 * h), delta=1e-8)
        self.assertAlmostEqual(
            pricing.implied_volatility(engine, call, context, call_price),
            0.2,
            delta=1e-7,
        )

    def test_time_greeks_use_the_right_hand_rate_at_curve_knots(self):
        times = [MIDDLE, MIDDLE + timedelta(days=90), MATURITY]
        risk = RateTermStructure(
            reference_time=REFERENCE,
            times=times,
            factors=[
                math.exp(-0.02 * 180 / 365),
                math.exp(-(0.02 * 180 + 0.08 * 90) / 365),
                math.exp(-(0.02 * 180 + 0.08 * 90 - 0.03 * 95) / 365),
            ],
        )
        dividend = RateTermStructure(
            reference_time=REFERENCE,
            times=times,
            factors=[
                math.exp(-0.01 * 180 / 365),
                math.exp(-(0.01 * 180 + 0.04 * 90) / 365),
                math.exp(-(0.01 * 180 + 0.04 * 90 - 0.02 * 95) / 365),
            ],
        )
        context = market(rate=risk, dividend=dividend, valuation=MIDDLE)
        engine = pricing.AnalyticVanillaEngine()
        option = vanilla(strike=100)
        actual = engine.price_with_greeks(option, context, ["theta", "charm", "color"])
        current = engine.price_with_greeks(option, context, ["delta", "gamma"])
        after = engine.price_with_greeks(
            option,
            context.replace(valuation_time=MIDDLE + timedelta(minutes=1)),
            ["delta", "gamma"],
        )
        self.assertAlmostEqual(
            actual.theta, (after.price - current.price) * 1440, delta=2e-6
        )
        self.assertAlmostEqual(
            actual.charm, (after.delta - current.delta) * 1440, delta=1e-7
        )
        self.assertAlmostEqual(
            actual.color, (after.gamma - current.gamma) * 1440, delta=1e-8
        )

    def test_flat_only_engines_reject_even_apparently_flat_nodes(self):
        context = market(
            rate=RateTermStructure(
                reference_time=REFERENCE, times=[MATURITY], factors=[math.exp(-0.03)]
            )
        )
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": START,
            "expiry_date": END,
        }
        asian_terms = terms | {"averaging_start_date": START}
        cases = (
            (pricing.QuadratureVanillaEngine(), EuropeanOption(**terms)),
            (pricing.CoxRossRubinsteinVanillaEngine(), EuropeanOption(**terms)),
            (pricing.MonteCarloVanillaEngine(path_count=64), EuropeanOption(**terms)),
            (pricing.BjerksundStenslandVanillaEngine(), AmericanOption(**terms)),
            (
                pricing.TurnbullWakemanArithmeticAveragePriceEngine(),
                ArithmeticAveragePriceOption(**asian_terms),
            ),
            (
                pricing.MonteCarloAsianEngine(path_count=64),
                GeometricAveragePriceOption(**asian_terms),
            ),
            (
                pricing.AnalyticBarrierEngine(),
                BarrierOption(**terms, barrier_level=130, barrier_type="up_and_out"),
            ),
        )
        for engine, option in cases:
            with self.subTest(engine=type(engine).__name__):
                self.assert_category(
                    ErrorCategory.UNSUPPORTED_OPERATION, engine.price, option, context
                )
                self.assert_category(
                    ErrorCategory.UNSUPPORTED_OPERATION,
                    engine.price_with_greeks,
                    option,
                    context,
                    "delta",
                )

    def test_geometric_asian_uses_exact_weighted_curve_carry(self):
        expiry = REFERENCE + timedelta(days=364)
        half = 182 / 365
        rate = RateTermStructure(
            reference_time=REFERENCE,
            times=[REFERENCE + timedelta(days=182), expiry],
            factors=[math.exp(-0.02 * half), math.exp(-0.10 * half)],
        )
        context = market(rate=rate, dividend=0)
        option = GeometricAveragePriceOption(
            option_type="call",
            strike=100,
            effective_date=START,
            averaging_start_date=START,
            expiry_date=expiry.date(),
        )
        time = 2 * half
        variance = 0.04 * time / 3
        drift = (0.02 * 3 / 8 + 0.08 / 8 - 0.04 / 4) * time
        deviation = math.sqrt(variance)
        d2 = drift / deviation
        d1 = d2 + deviation

        def cdf(z):
            return math.erfc(-z / math.sqrt(2)) / 2

        expected = (
            math.exp(-0.05 * time)
            * 100
            * (math.exp(drift + variance / 2) * cdf(d1) - cdf(d2))
        )
        self.assertAlmostEqual(
            pricing.AnalyticGeometricAveragePriceEngine().price(option, context),
            expected,
            delta=1e-11,
        )

    def test_arithmetic_asian_moments_and_seasoned_affine_payoffs(self):
        expiry = REFERENCE + timedelta(days=364)
        half = 182 / 365
        rate = RateTermStructure(
            reference_time=REFERENCE,
            times=[REFERENCE + timedelta(days=182), expiry],
            factors=[math.exp(-0.02 * half), math.exp(-0.10 * half)],
        )
        context = market(rate=rate, dividend=0)
        terms = {
            "strike": 100,
            "effective_date": START,
            "averaging_start_date": START,
            "expiry_date": expiry.date(),
        }
        call = ArithmeticAveragePriceOption(option_type="call", **terms)
        put = ArithmeticAveragePriceOption(option_type="put", **terms)
        mean = (
            100
            * (
                math.expm1(0.02 * half) / 0.02
                + math.exp(0.02 * half) * math.expm1(0.08 * half) / 0.08
            )
            / (2 * half)
        )
        engine = pricing.MonteCarloAsianEngine(
            path_count=65536,
            step_count=64,
            seed=7,
            sampling="sobol",
            antithetic=True,
        )
        self.assertAlmostEqual(
            engine.price(call, context) - engine.price(put, context),
            math.exp(-0.10 * half) * (mean - 100),
            delta=0.01,
        )
        seasoned_terms = terms | {
            "strike": 25,
            "effective_date": START - timedelta(days=364),
            "averaging_start_date": START - timedelta(days=364),
            "realized_average": 100,
        }
        for direction, expected in (
            ("call", math.exp(-0.10 * half) * (25 + mean / 2)),
            ("put", 0),
        ):
            with self.subTest(direction=direction):
                self.assertAlmostEqual(
                    engine.price(
                        ArithmeticAveragePriceOption(
                            option_type=direction, **seasoned_terms
                        ),
                        context,
                    ),
                    expected,
                    delta=1e-11,
                )
        gpu = pricing.MonteCarloAsianEngine(
            path_count=131072, step_count=64, seed=7, backend="cuda", antithetic=True
        )
        try:
            gpu_price = gpu.price(call, context)
        except KiyosiError as error:
            self.assertEqual(error.category, ErrorCategory.BACKEND_UNAVAILABLE)
        else:
            self.assertAlmostEqual(gpu_price, engine.price(call, context), delta=0.08)

    def test_finite_difference_vanilla_and_barrier_curve_parities(self):
        context = market(dividend=0)
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": START,
            "expiry_date": END,
        }
        expected = pricing.AnalyticVanillaEngine().price(
            EuropeanOption(**terms), context
        )
        settings = {
            "asset_step_count": 600,
            "time_step_count": 1000,
            "asset_upper_boundary": 400,
        }
        american = pricing.FiniteDifferenceVanillaEngine(**settings).price(
            AmericanOption(**terms), context
        )
        self.assertAlmostEqual(american, expected, delta=0.01)
        for option_type, engine, extra, kinds in (
            (
                BarrierOption,
                pricing.FiniteDifferenceBarrierEngine(**settings),
                {"barrier_level": 130},
                ("up_and_in", "up_and_out"),
            ),
            (
                DoubleBarrierOption,
                pricing.FiniteDifferenceDoubleBarrierEngine(**settings),
                {"lower_barrier": 70, "upper_barrier": 130},
                ("knock_in", "knock_out"),
            ),
        ):
            with self.subTest(option=option_type.__name__):
                values = [
                    engine.price(
                        option_type(**terms, **extra, barrier_type=kind), context
                    )
                    for kind in kinds
                ]
                self.assertAlmostEqual(sum(values), expected, delta=0.01)

    def test_binary_barrier_and_touch_curve_parities(self):
        context = market()
        settings = {
            "asset_step_count": 600,
            "time_step_count": 1000,
            "asset_upper_boundary": 400,
        }
        engine = pricing.FiniteDifferenceBinaryBarrierEngine(**settings)
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": START,
            "expiry_date": END,
        }
        for factory, digital_option in (
            (cash_binary_barrier_option, CashOrNothingOption(**terms, payout=1)),
            (asset_binary_barrier_option, AssetOrNothingOption(**terms)),
        ):
            with self.subTest(factory=factory.__name__):
                extra = {"payout": 1} if factory is cash_binary_barrier_option else {}
                values = [
                    engine.price(
                        factory(**terms, barrier_level=130, barrier_type=kind, **extra),
                        context,
                    )
                    for kind in ("up_and_in", "up_and_out")
                ]
                expected = pricing.AnalyticDigitalEngine().price(
                    digital_option, context
                )
                self.assertAlmostEqual(sum(values), expected, delta=0.02)
        touch_terms = {
            "effective_date": START,
            "expiry_date": END,
            "barrier_level": 130,
        }
        for one_factory, no_factory, expected, extra in (
            (
                cash_one_touch_up,
                cash_no_touch_up,
                example_curve().factor(REFERENCE, MATURITY),
                {"payout": 1},
            ),
            (
                asset_one_touch_up,
                asset_no_touch_up,
                100 * math.exp(-0.01),
                {},
            ),
        ):
            with self.subTest(factory=one_factory.__name__):
                one = engine.price(
                    one_factory(**touch_terms, **extra, settlement_timing="at_expiry"),
                    context,
                )
                no = engine.price(no_factory(**touch_terms, **extra), context)
                self.assertAlmostEqual(one + no, expected, delta=0.01)
        double_terms = {
            "effective_date": START,
            "expiry_date": END,
            "lower_barrier": 70,
            "upper_barrier": 130,
            "payout": 1,
        }
        double_engine = pricing.FiniteDifferenceDoubleTouchEngine(**settings)
        one = double_engine.price(
            cash_double_touch(**double_terms, settlement_timing="at_expiry"), context
        )
        no = double_engine.price(cash_double_no_touch(**double_terms), context)
        self.assertAlmostEqual(
            one + no, example_curve().factor(REFERENCE, MATURITY), delta=1e-4
        )

    def test_curve_shape_changes_barrier_value_at_fixed_terminal_factors(self):
        contexts = [
            market(
                rate=RateTermStructure(
                    reference_time=REFERENCE,
                    times=[MIDDLE, MATURITY],
                    factors=[math.exp(-rate * 180 / 365), math.exp(-0.05)],
                ),
                dividend=0,
            )
            for rate in (0.20, -0.10)
        ]
        option = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=START,
            expiry_date=END,
            barrier_level=130,
            barrier_type="up_and_out",
        )
        engine = pricing.FiniteDifferenceBarrierEngine(
            asset_step_count=600, time_step_count=1000, asset_upper_boundary=400
        )
        early_high, late_high = [engine.price(option, context) for context in contexts]
        self.assertGreater(late_high - early_high, 0.05)
        vanilla_engine = pricing.AnalyticVanillaEngine()
        self.assertAlmostEqual(
            vanilla_engine.price(vanilla(strike=100), contexts[0]),
            vanilla_engine.price(vanilla(strike=100), contexts[1]),
            delta=1e-11,
        )

    def test_structured_curve_prices_match_terminal_portfolios(self):
        context = market()
        discount = example_curve().factor(REFERENCE, MATURITY)
        settings = {
            "asset_step_count": 600,
            "time_step_count": 1000,
            "asset_upper_boundary": 400,
        }
        note = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations(
                [END], [100], knock_out_coupon_rates=[0.1]
            ),
        )
        digital = CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=0.1,
            effective_date=START,
            expiry_date=END,
        )
        expected = discount + pricing.AnalyticDigitalEngine().price(digital, context)
        actual = pricing.FiniteDifferenceAutocallableEngine(**settings).price(
            note, context
        )
        self.assertAlmostEqual(actual, expected, delta=0.001)
        early_call = BinarySnowballOption(
            maturity_coupon_rate=0,
            effective_date=START,
            expiry_date=END,
            observations=autocallable_observations(
                [MIDDLE.date()], [1], knock_out_coupon_rates=[0.1]
            ),
        )
        self.assertAlmostEqual(
            pricing.FiniteDifferenceAutocallableEngine(**settings).price(
                early_call, context
            ),
            (1 + 0.1 * 180 / 365) * example_curve().factor(REFERENCE, MIDDLE),
            delta=0.001,
        )
        accumulator = Accumulator(
            strike=100,
            knock_out_level=399,
            daily_quantity=0,
            acceleration_factor=1,
            accumulated_quantity=2,
            effective_date=START,
            expiry_date=END,
        )
        self.assertAlmostEqual(
            pricing.FiniteDifferenceAccumulatorEngine(**settings).price(
                accumulator, context
            ),
            2 * (100 * math.exp(-0.01) - 100 * discount),
            delta=0.001,
        )

    def test_already_paid_touch_needs_no_unused_future_curve_coverage(self):
        curve = RateTermStructure(
            reference_time=REFERENCE, times=[MIDDLE], factors=[0.98]
        )
        context = market(rate=curve, valuation=MIDDLE)
        paid = cash_one_touch_up(
            effective_date=START,
            expiry_date=END,
            barrier_level=130,
            payout=5,
            settlement_timing="at_hit",
            touch_state="touched",
        )
        engine = pricing.FiniteDifferenceBinaryBarrierEngine()
        self.assertEqual(engine.price(paid, context), 0)
        result = engine.price_with_greeks(paid, context, ["theta", "delta"])
        self.assertEqual(result.price, 0)
        self.assertIsNone(result.theta)
        self.assertEqual(result.delta, 0)

    def test_scheduled_at_hit_touch_only_needs_coverage_to_final_observation(self):
        context = market(
            rate=RateTermStructure(
                reference_time=REFERENCE, times=[MIDDLE], factors=[0.98]
            ),
            dividend=RateTermStructure(
                reference_time=REFERENCE, times=[MIDDLE], factors=[0.99]
            ),
        )
        engine = pricing.FiniteDifferenceBinaryBarrierEngine(
            asset_step_count=600, time_step_count=1000, asset_upper_boundary=400
        )
        for factory, direction, barrier, asset in (
            (cash_one_touch_up, "call", 110, False),
            (cash_one_touch_down, "put", 90, False),
            (asset_one_touch_up, "call", 110, True),
            (asset_one_touch_down, "put", 90, True),
        ):
            with self.subTest(factory=factory.__name__):
                extra = {} if asset else {"payout": 1}
                option = factory(
                    effective_date=START,
                    expiry_date=END,
                    barrier_level=barrier,
                    settlement_timing="at_hit",
                    observation_mode="scheduled",
                    observation_dates=[MIDDLE.date()],
                    **extra,
                )
                digital_type = AssetOrNothingOption if asset else CashOrNothingOption
                terminal = digital_type(
                    option_type=direction,
                    strike=barrier,
                    effective_date=START,
                    expiry_date=MIDDLE.date(),
                    **extra,
                )
                self.assertAlmostEqual(
                    engine.price(option, context),
                    pricing.AnalyticDigitalEngine().price(terminal, context),
                    delta=0.02 if asset else 0.0002,
                )
        double = cash_double_touch(
            effective_date=START,
            expiry_date=END,
            lower_barrier=80,
            upper_barrier=120,
            payout=1,
            settlement_timing="at_hit",
            observation_mode="scheduled",
            observation_dates=[MIDDLE.date()],
        )
        expected = sum(
            pricing.AnalyticDigitalEngine().price(
                CashOrNothingOption(
                    option_type=direction,
                    strike=strike,
                    payout=1,
                    effective_date=START,
                    expiry_date=MIDDLE.date(),
                ),
                context,
            )
            for direction, strike in (("call", 120), ("put", 80))
        )
        self.assertAlmostEqual(
            pricing.FiniteDifferenceDoubleTouchEngine(
                asset_step_count=600, time_step_count=1000, asset_upper_boundary=400
            ).price(double, context),
            expected,
            delta=0.0003,
        )


if __name__ == "__main__":
    unittest.main()
