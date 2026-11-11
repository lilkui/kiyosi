"""Term-structure validation and pricing through the Python API."""

import math
import numbers
import pickle
from datetime import UTC, datetime, timedelta, timezone

import pytest
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
from support.assertions import assert_rejection

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


def test_public_curve_parts_reconstruct_extreme_shifts_without_loss(subtests):
    base = example_curve()
    for shift in (0.0, 1000.0, -1000.0):
        with subtests.test(shift=shift):
            curve = base.shifted(shift)
            restored = RateTermStructure(
                reference_time=curve.reference_time,
                times=curve.nodes,
                factors=curve.base_factors,
            ).shifted(curve.parallel_shift)
            assert restored == curve
            assert curve.base_factors == base.factors
            assert curve.parallel_shift == shift
            assert restored.log_factor(REFERENCE, MATURITY) == curve.log_factor(
                REFERENCE, MATURITY
            )
            copied = list(curve.base_factors)
            copied.clear()
            assert curve.base_factors == base.factors
    assert base.shifted(1000).factors[-1] == 0.0
    for curve in (
        RateTermStructure.flat(0.05),
        RateTermStructure.bounded_flat(
            0.05, reference_time=REFERENCE, max_time=MATURITY
        ),
    ):
        assert curve.base_factors == (())
        assert curve.parallel_shift == 0.0


def test_rounded_flat_nodes_explain_exact_constant_curve_requirement():
    curve = RateTermStructure(
        reference_time=START,
        times=[MIDDLE, END],
        factors=[math.exp(-0.05 * 180 / 365), math.exp(-0.05)],
    )
    assert curve.exact_flat_rate is None
    with pytest.raises(UnsupportedOperationError, match="bounded_flat"):
        pricing.CoxRossRubinsteinVanillaEngine(step_count=64).price(
            vanilla(), market(rate=curve)
        )
    explicit = RateTermStructure.bounded_flat(0.05, reference_time=START, max_time=END)
    assert explicit.exact_flat_rate == 0.05
    assert math.isfinite(
        pricing.CoxRossRubinsteinVanillaEngine(step_count=64).price(
            vanilla(), market(rate=explicit)
        )
    )


def test_bounded_flat_curves_preserve_exact_rates_coverage_and_engine_support():
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    curve = RateTermStructure.bounded_flat(0.05, reference_time=start, max_time=end)
    assert curve.exact_flat_rate == 0.05
    assert curve.reference_time == start
    assert curve.max_time == end
    assert curve.forward_rate(start + timedelta(days=180)) == 0.05
    assert curve.factor(start, end) == math.exp(-0.05)
    for outside in (start - timedelta(days=1), end + timedelta(days=1)):
        with pytest.raises(InvalidInputError):
            curve.forward_rate(outside)
    shifted = curve.shifted(0.01)
    assert shifted.exact_flat_rate == 0.05 + 0.01
    assert shifted.max_time == end
    assert curve.exact_flat_rate == 0.05
    for value in (curve, shifted):
        restored = pickle.loads(pickle.dumps(value))
        assert restored == value
        assert restored.max_time == end
        assert "bounded_flat" in repr(value)
    for rate, reference, maximum in (
        (0.05, end, start),
        (float("nan"), start, end),
    ):
        with pytest.raises(InvalidInputError):
            RateTermStructure.bounded_flat(
                rate, reference_time=reference, max_time=maximum
            )
    point = RateTermStructure.bounded_flat(0.05, reference_time=start, max_time=start)
    assert point.factor(start, start) == 1
    option = EuropeanOption(
        option_type="call", strike=100, effective_time=start, expiry_time=end
    )
    engine = pricing.CoxRossRubinsteinVanillaEngine()
    prices = [
        engine.price(
            option,
            PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=0.02, volatility=0.2
                ),
                spot_price=100,
                valuation_time=start,
            ),
        )
        for rate in (curve, 0.05)
    ]
    assert prices[0] == prices[1]


def test_flat_rate_queries_and_curve_roundtrips_preserve_coverage(subtests):
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
        with subtests.test(model=type(model).__name__):
            for field in ("flat_risk_free_rate", "flat_dividend_yield"):
                assert getattr(model, field) == 0.03
            for field in ("risk_free_curve", "dividend_curve"):
                copied = getattr(model, field)
                assert isinstance(copied, RateTermStructure)
                assert copied == curve
                assert copied is not curve
            replay = model.replace(
                risk_free_rate=model.risk_free_curve,
                dividend_yield=model.dividend_curve,
            )
            assert replay == model
            with pytest.raises(InvalidInputError):
                PricingContext(
                    model_parameters=replay,
                    spot_price=100,
                    valuation_time=REFERENCE - timedelta(days=1),
                )
            undated = model.replace(risk_free_rate=0.03, dividend_yield=0.03)
            assert undated.flat_risk_free_rate == model.flat_risk_free_rate
            assert undated.flat_dividend_yield == model.flat_dividend_yield
            nodes = model.replace(
                risk_free_rate=example_curve(), dividend_yield=example_curve()
            )
            for field in ("flat_risk_free_rate", "flat_dividend_yield"):
                with pytest.raises(UnsupportedOperationError):
                    getattr(nodes, field)


def test_log_interpolation_interval_factors_and_right_hand_forwards():
    curve = example_curve()
    assert curve.reference_time == REFERENCE
    assert curve.max_time == MATURITY
    assert curve.nodes == ((MIDDLE, MATURITY))
    assert curve.factors[0] == math.exp(-0.02 * 180 / 365)
    assert curve.factor(
        REFERENCE, REFERENCE + timedelta(days=90, hours=12)
    ) == pytest.approx(math.exp(-0.02 * 90.5 / 365), rel=0, abs=1e-14)
    assert curve.log_factor(
        REFERENCE + timedelta(days=90), MIDDLE + timedelta(days=30)
    ) == pytest.approx(-(0.02 * 90 + 0.08 * 30) / 365, rel=0, abs=1e-14)
    assert curve.forward_rate(MIDDLE - timedelta(microseconds=1)) == pytest.approx(
        0.02, rel=0, abs=1e-13
    )
    assert curve.forward_rate(MIDDLE) == pytest.approx(0.08, rel=0, abs=1e-13)
    assert curve.forward_rate(MATURITY) == pytest.approx(0.08, rel=0, abs=1e-13)
    assert curve.factor(REFERENCE, REFERENCE) == 1
    assert curve.factor(MATURITY, MATURITY) == 1
    assert curve.factor(REFERENCE, MATURITY) == pytest.approx(
        curve.factor(REFERENCE, MIDDLE) * curve.factor(MIDDLE, MATURITY),
        rel=0,
        abs=1e-14,
    )


def test_node_validation_rejects_bad_factors_order_and_counts(subtests):
    for factor in (0, -0.5, math.nan, math.inf):
        with subtests.test(factor=factor):
            assert_rejection(
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
        with subtests.test(times=times):
            assert_rejection(
                InvalidInputError,
                RateTermStructure,
                reference_time=REFERENCE,
                times=times,
                factors=[0.95] * len(times),
            )
    assert_rejection(
        InvalidInputError,
        RateTermStructure,
        reference_time=REFERENCE,
        times=[MIDDLE, MATURITY],
        factors=[0.95],
    )


def test_negative_node_rates_are_valid_without_monotonic_factors():
    curve = RateTermStructure(
        reference_time=REFERENCE, times=[MIDDLE, MATURITY], factors=[1.02, 0.99]
    )
    assert curve.factor(REFERENCE, MIDDLE) == 1.02
    assert curve.forward_rate(REFERENCE) < 0
    assert curve.forward_rate(MIDDLE) > 0
    assert curve.factor(MIDDLE, MATURITY) == pytest.approx(
        0.99 / 1.02, rel=0, abs=1e-14
    )


def test_microsecond_intervals_remain_distinct_far_from_reference():
    origin = datetime(1, 1, 1)
    second = datetime(9998, 1, 1)
    first = second - timedelta(microseconds=1)
    curve = RateTermStructure(
        reference_time=origin,
        times=[first, second],
        factors=[math.exp(-100), math.exp(-101)],
    )
    assert curve.log_factor(first, second) == pytest.approx((-1), rel=0, abs=1e-14)
    assert curve.factor(first, second) == pytest.approx(math.exp(-1), rel=0, abs=1e-14)
    long_years = (first - origin).total_seconds() / (365 * 86400)
    assert curve.forward_rate(first - timedelta(microseconds=1)) == pytest.approx(
        100 / long_years, rel=0, abs=1e-14
    )
    rate = 365 * 86400 * 1e6
    assert curve.forward_rate(first) / rate == pytest.approx(1, rel=0, abs=1e-13)
    context = market(rate=curve, dividend=0, valuation=first)
    equivalent = market(rate=rate, dividend=0, valuation=first)
    option = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=first,
        expiry_time=second,
    )
    engine = pricing.AnalyticEuropeanEngine()
    assert engine.price(option, context) == pytest.approx(
        100 * (1 - math.exp(-1)), rel=0, abs=1e-11
    )
    assert engine.price(option, context) == pytest.approx(
        engine.price(option, equivalent), rel=0, abs=1e-11
    )
    distant_context = market(rate=curve, dividend=0, valuation=origin, volatility=1e-8)
    distant_option = EuropeanOption(
        option_type="call",
        strike=math.exp(100),
        effective_time=origin,
        expiry_time=second,
    )
    assert_rejection(
        ComputationFailureError, engine.price, distant_option, distant_context
    )


def test_queries_reject_extrapolation_and_reversed_intervals(subtests):
    curve = example_curve()
    for start, end in (
        (REFERENCE - timedelta(microseconds=1), MIDDLE),
        (REFERENCE, MATURITY + timedelta(microseconds=1)),
        (MIDDLE, REFERENCE),
    ):
        with subtests.test(start=start, end=end):
            assert_rejection(InvalidInputError, curve.factor, start, end)
            assert_rejection(InvalidInputError, curve.log_factor, start, end)
    for time in (
        REFERENCE - timedelta(microseconds=1),
        MATURITY + timedelta(microseconds=1),
    ):
        with subtests.test(time=time):
            assert_rejection(InvalidInputError, curve.forward_rate, time)


def test_flat_curves_and_parallel_shifts_preserve_coverage(subtests):
    flat = RateTermStructure.flat(-0.03)
    dated = RateTermStructure.flat(-0.03, reference_time=REFERENCE)
    assert flat.reference_time is None
    assert flat.max_time is None
    assert dated.reference_time == REFERENCE
    assert dated.max_time is None
    before = REFERENCE - timedelta(days=365)
    assert flat.factor(before, REFERENCE) == pytest.approx(
        math.exp(0.03), rel=0, abs=1e-14
    )
    assert_rejection(InvalidInputError, dated.factor, before, REFERENCE)
    curve = example_curve()
    shifted = curve.shifted(0.01)
    assert shifted.reference_time == curve.reference_time
    assert shifted.max_time == curve.max_time
    assert shifted.nodes == curve.nodes
    assert curve.shifted(0) == curve
    assert shifted.factor(MIDDLE, MATURITY) == pytest.approx(
        curve.factor(MIDDLE, MATURITY) * math.exp(-0.01 * 185 / 365),
        rel=0,
        abs=1e-14,
    )
    assert flat.shifted(0.01).forward_rate(REFERENCE) == pytest.approx(
        (-0.02), rel=0, abs=1e-15
    )
    assert_rejection(InvalidInputError, curve.shifted, math.inf)
    for rate in (math.nan, math.inf):
        with subtests.test(rate=rate):
            assert_rejection(InvalidInputError, RateTermStructure.flat, rate)


def test_python_representation_errors_precede_core_validation(subtests):
    for value in (True, "0.03", None):
        with subtests.test(value=value), pytest.raises(TypeError):
            RateTermStructure.flat(value)
    with pytest.raises(OverflowError):
        RateTermStructure.flat(1 << 10000)
    for factor in (True, "0.95", None):
        with subtests.test(factor=factor), pytest.raises(TypeError):
            RateTermStructure(
                reference_time=REFERENCE, times=[MIDDLE], factors=[factor]
            )
    for time in (True, "2025-01-01", datetime(2025, 1, 1, tzinfo=UTC)):
        with subtests.test(time=time), pytest.raises(TypeError):
            RateTermStructure(reference_time=time, times=[MIDDLE], factors=[0.95])
    with pytest.raises(TypeError):
        example_curve().shifted(True)


def test_timezone_free_timestamps_and_owned_nodes():
    times = [MIDDLE, MATURITY]
    factors = [0.98, 0.94]
    curve = RateTermStructure(reference_time=REFERENCE, times=times, factors=factors)
    times.clear()
    factors[0] = 9
    assert curve.reference_time == REFERENCE
    assert curve.nodes == ((MIDDLE, MATURITY))
    assert curve.factors == ((0.98, 0.94))
    copied_nodes = list(curve.nodes)
    copied_factors = list(curve.factors)
    copied_nodes.clear()
    copied_factors[0] = 7
    assert curve.nodes == ((MIDDLE, MATURITY))
    assert curve.factors == ((0.98, 0.94))
    with pytest.raises(AttributeError):
        curve.reference_time = MATURITY
    date_curve = RateTermStructure(reference_time=START, times=[END], factors=[0.94])
    assert date_curve.reference_time == REFERENCE
    assert date_curve.nodes == ((MATURITY,))


def test_context_coverage_and_replace_preserve_independent_curves(subtests):
    risk_free = example_curve()
    dividend = RateTermStructure(
        reference_time=REFERENCE - timedelta(days=30),
        times=[REFERENCE + timedelta(days=90), MATURITY + timedelta(days=30)],
        factors=[0.99, 0.96],
    )
    context = market(rate=risk_free, dividend=dividend)
    model = context.model_parameters
    assert model.risk_free_curve == risk_free
    assert model.dividend_curve == dividend
    with pytest.raises(UnsupportedOperationError):
        _ = model.flat_risk_free_rate
    with pytest.raises(UnsupportedOperationError):
        _ = model.flat_dividend_yield
    rolled = context.replace(valuation_time=MIDDLE)
    assert rolled.valuation_time == MIDDLE
    assert rolled.model_parameters == model
    for time in (
        REFERENCE - timedelta(microseconds=1),
        MATURITY + timedelta(microseconds=1),
    ):
        with subtests.test(time=time):
            assert_rejection(
                InvalidInputError,
                context.replace,
                valuation_time=time,
            )
    assert context.valuation_time == REFERENCE
    assert context.model_parameters == model
    replaced = model.replace(volatility=0.25)
    assert replaced.risk_free_curve == risk_free
    assert replaced.dividend_curve == dividend
    assert replaced.volatility == 0.25
    assert_rejection(InvalidInputError, model.replace, volatility=-0.1)
    assert model.volatility == 0.2


def test_curve_prices_match_quantlib_and_discounted_parity():
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
            assert price == pytest.approx(reference.NPV(), rel=0, abs=1e-11)
            prices.append(price)
            equivalent = market(
                rate=(0.02 * 180 + 0.08 * 185) / 365,
                dividend=(0.01 * 180 + 0.03 * 185) / 365,
            )
            assert price == pytest.approx(
                engine.price(vanilla(direction), equivalent), rel=0, abs=1e-11
            )
        assert prices[0] - prices[1] == pytest.approx(
            100 * dividend_factors[-1] - 103 * risk_factors[-1], rel=0, abs=1e-11
        )
        scalar = market(rate=0.03)
        explicit = market(rate=RateTermStructure.flat(0.03, reference_time=REFERENCE))
        assert engine.price(vanilla(), scalar) == engine.price(vanilla(), explicit)
    finally:
        del saved


def test_replace_signatures_validation_timing_and_failure_state(subtests):
    model = market().model_parameters
    context = market()
    for obj, fields in (
        (model, ("risk_free_rate", "dividend_yield", "volatility")),
        (
            context,
            (
                "model_parameters",
                "spot_price",
                "valuation_time",
            ),
        ),
    ):
        replacement = obj.replace()
        assert replacement == obj
        assert replacement is not obj
        for field in fields:
            for value in (None, True, "invalid"):
                with (
                    subtests.test(field=field, value=value),
                    pytest.raises(TypeError),
                ):
                    obj.replace(**{field: value})
        with pytest.raises(TypeError):
            obj.replace(1)
        with pytest.raises(TypeError):
            obj.replace(unknown=1)
    for field, value, expected_error in (
        ("risk_free_rate", math.nan, InvalidInputError),
        ("dividend_yield", math.inf, InvalidInputError),
        ("volatility", -0.1, InvalidInputError),
    ):
        assert_rejection(expected_error, model.replace, **{field: value})
    for spot in (0, math.nan, math.inf):
        assert_rejection(InvalidInputError, context.replace, spot_price=spot)
    with pytest.raises(OverflowError):
        model.replace(volatility=10**1000)
    with pytest.raises(TypeError):
        context.replace(valuation_time=datetime(2025, 1, 6, tzinfo=UTC))
    changed = context.replace(
        spot_price=105,
        valuation_time=datetime(2025, 1, 6, 9),
    )
    assert changed.spot_price == 105
    assert changed.valuation_time == datetime(2025, 1, 6, 9)
    assert context.valuation_time == REFERENCE
    assert context.spot_price == 100
    assert context.model_parameters == model
    zero = model.replace(risk_free_rate=0, dividend_yield=0)
    assert ((zero.flat_risk_free_rate, zero.flat_dividend_yield)) == ((0, 0))
    short = RateTermStructure(reference_time=REFERENCE, times=[MIDDLE], factors=[0.98])
    shorter = context.replace(model_parameters=model.replace(risk_free_rate=short))
    assert_rejection(
        InvalidInputError,
        pricing.AnalyticEuropeanEngine().price,
        vanilla(),
        shorter,
    )
    for time in (
        REFERENCE - timedelta(microseconds=1),
        MATURITY + timedelta(microseconds=1),
    ):
        assert_rejection(InvalidInputError, market, valuation=time)
    for risk, dividend in ((0, short), (short, 0)):
        candidate = market(rate=risk, dividend=dividend)
        assert_rejection(
            InvalidInputError,
            candidate.replace,
            valuation_time=MATURITY,
        )


def test_protocol_conversion_errors_and_datetime_boundaries(subtests):
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

    with pytest.raises(ConversionFailure):
        RateTermStructure.flat(BadFloat())
    with pytest.raises(ConversionFailure):
        RateTermStructure(
            reference_time=REFERENCE, times=broken_iterable(), factors=[0.95]
        )
    with pytest.raises(OverflowError):
        RateTermStructure(
            reference_time=REFERENCE, times=[MIDDLE], factors=[1 << 10000]
        )
    curve = RateTermStructure(
        reference_time=START,
        times=(time for time in [MIDDLE, MATURITY]),
        factors=(float(value) for value in (Fraction(49, 50), Fraction(47, 50))),
    )
    assert curve.factors == ((0.98, 0.94))
    for time in (
        datetime.min.replace(tzinfo=timezone(timedelta(hours=1))),
        datetime.max.replace(tzinfo=timezone(-timedelta(hours=1))),
    ):
        with subtests.test(time=time), pytest.raises(TypeError):
            RateTermStructure.flat(0.03, reference_time=time)
        with subtests.test(time=time), pytest.raises(TypeError):
            curve.forward_rate(time)
    for operation in (curve.factor, curve.log_factor):
        with pytest.raises(TypeError):
            operation(True, MATURITY)
    for times, factors in (
        (None, [0.95]),
        ([MIDDLE], None),
        ([True], [0.95]),
        ([MIDDLE], [False]),
    ):
        with (
            subtests.test(times=times, factors=factors),
            pytest.raises(TypeError),
        ):
            RateTermStructure(reference_time=REFERENCE, times=times, factors=factors)
    for obj, field, value in (
        (curve, "factors", [1]),
        (market().model_parameters, "risk_free_curve", curve),
        (market(), "spot_price", 101),
    ):
        with pytest.raises(AttributeError):
            setattr(obj, field, value)


def test_bulk_curve_factors_follow_scalar_overflow_and_underflow_rules():
    import pickle

    curve = RateTermStructure(
        reference_time=REFERENCE, times=[MATURITY], factors=[0.95]
    )
    overflowing = curve.shifted(-1000)
    assert_rejection(ComputationFailureError, lambda: overflowing.factors)
    assert_rejection(ComputationFailureError, overflowing.factor, REFERENCE, MATURITY)
    underflowing = curve.shifted(1000)
    assert underflowing.factors == ((0.0,))
    assert underflowing.factor(REFERENCE, MATURITY) == 0.0
    assert curve.factors == ((0.95,))
    for extreme in (overflowing, underflowing):
        assert pickle.loads(pickle.dumps(extreme)) == extreme
    assert_rejection(
        InvalidInputError,
        RateTermStructure,
        reference_time=REFERENCE,
        times=[MATURITY],
        factors=underflowing.factors,
    )


def test_curve_numerical_limits_ownership_and_concurrency():
    from concurrent.futures import ThreadPoolExecutor

    assert RateTermStructure.flat(1000).factor(REFERENCE, MATURITY) == 0
    assert_rejection(
        ComputationFailureError,
        RateTermStructure.flat(-1000).factor,
        REFERENCE,
        MATURITY,
    )
    assert_rejection(
        ComputationFailureError,
        RateTermStructure.flat(1e308).log_factor,
        REFERENCE,
        MATURITY + timedelta(days=365),
    )
    assert_rejection(
        InvalidInputError,
        RateTermStructure.flat(1e308).shifted,
        1e308,
    )
    curve = example_curve()
    shifted = curve.shifted(0.01)
    assert shifted.factors[1] == pytest.approx(
        curve.factors[1] * math.exp(-0.01), rel=0, abs=1e-14
    )
    assert_rejection(InvalidInputError, curve.shifted(1e308).shifted, 1e308)
    context = market()
    model = context.model_parameters
    owned_curve = model.risk_free_curve
    replaced = context.replace(spot_price=105)
    del context, model
    assert owned_curve == example_curve()
    assert replaced.model_parameters.risk_free_curve == example_curve()
    engine = pricing.AnalyticEuropeanEngine()
    option = vanilla()
    expected = engine.price(option, replaced)
    with ThreadPoolExecutor(max_workers=4) as pool:
        prices = list(pool.map(lambda _: engine.price(option, replaced), range(8)))
    assert prices == [expected] * 8
