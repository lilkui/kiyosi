"""Range coupon contracts and independent marginal/payoff references for the Rust candidate."""

import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, time, timedelta

from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    RootNotBracketedError,
    UnsupportedOperationError,
)
from kiyosi.instruments import RangeAccrualOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
    custom_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import AnalyticRangeAccrualEngine, implied_volatility

START = date(2025, 1, 1)


def instant(value):
    return (
        datetime.combine(value, time(), UTC)
        if isinstance(value, date) and not isinstance(value, datetime)
        else value
    )


def contract(**changes):
    values = {
        "notional": 1000,
        "annual_coupon_rate": 0.12,
        "lower_bound": 95,
        "upper_bound": 110,
        "effective_date": START,
        "expiry_date": START + timedelta(days=16),
    }
    values.update(changes)
    return RangeAccrualOption(**values)


def market(
    valuation=START, spot=100, rate=0.04, dividend=0.01, volatility=0.2, calendar=None
):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation,
        calendar=calendar or all_days_calendar(),
    )


def reference(option, context):
    hits = option.realized_hit_count or 0
    total = 0
    model = context.model_parameters
    valuation = context.valuation_time
    current = option.effective_date + timedelta(days=1)
    while current <= option.expiry_date:
        if context.calendar.is_trading_day(current):
            total += 1
            observed = instant(current)
            if observed == valuation:
                hits += option.lower_bound <= context.spot_price <= option.upper_bound
            elif observed > valuation:
                elapsed = (observed - valuation).total_seconds() / (365 * 86400)
                mean = (
                    math.log(context.spot_price)
                    - model.risk_free_curve.log_factor(valuation, observed)
                    + model.dividend_curve.log_factor(valuation, observed)
                    - 0.5 * model.volatility**2 * elapsed
                )
                width = model.volatility * math.sqrt(2 * elapsed)
                upper = math.erfc((mean - math.log(option.upper_bound)) / width) / 2
                lower = (
                    0
                    if option.lower_bound == 0
                    else math.erfc((mean - math.log(option.lower_bound)) / width) / 2
                )
                hits += upper - lower
        current += timedelta(days=1)
    return (
        option.notional
        * option.annual_coupon_rate
        * (option.expiry_date - option.effective_date).days
        / 365
        * hits
        / total
        * model.risk_free_curve.factor(valuation, option.expiry_date)
    )


class RangeAccrualTests(unittest.TestCase):
    def test_unresolvable_curve_knots_fail_before_future_pricing(self):
        knot, expiry = date(2500, 1, 1), date(2500, 1, 2)
        curve = RateTermStructure(
            reference_time=START,
            times=[knot, instant(knot) + timedelta(microseconds=1), expiry],
            factors=[1, 1, 1],
        )
        option = contract(effective_date=date(2499, 12, 31), expiry_date=expiry)
        engine = AnalyticRangeAccrualEngine()
        for rate, dividend in ((curve, 0), (0, curve)):
            context = market(rate=rate, dividend=dividend)
            self.assert_rejection(
                ComputationFailureError, engine.price, option, context
            )
            self.assertEqual(
                engine.price(
                    contract(
                        effective_date=option.effective_date,
                        expiry_date=expiry,
                        annual_coupon_rate=0,
                    ),
                    context,
                ),
                0,
            )

    def test_native_representation_errors_and_unsupported_pairs(self):
        from kiyosi.instruments import EuropeanOption
        from kiyosi.pricing import AnalyticVanillaEngine

        option, context = contract(), market()
        engine = AnalyticRangeAccrualEngine()
        vanilla = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=START,
            expiry_date=option.expiry_date,
        )
        for field in ("effective_date", "expiry_date"):
            for value in ("2025-01-01", instant(START)):
                with (
                    self.subTest(field=field, value=value),
                    self.assertRaises(TypeError),
                ):
                    contract(**{field: value})
        for value in (2**31, -(2**31) - 1):
            with self.assertRaises(OverflowError):
                option.replace(realized_hit_count=value)
        with self.assertRaises(TypeError):
            option.replace(notional=2000)
        with self.assertRaises(TypeError):
            engine.price(vanilla, context)
        with self.assertRaises(TypeError):
            implied_volatility(engine, vanilla, context, 1)
        with self.assertRaises(TypeError):
            AnalyticVanillaEngine().price(option, context)
        with self.assertRaises(TypeError):
            engine.price_with_greeks(option, context, "delta", spot_shift=True)
        with self.assertRaises(TypeError):
            implied_volatility(engine, option, context, True)
        self.assertEqual(option.replace(), option)
        self.assertEqual(option, contract())

    def test_solver_failures_and_no_mutation(self):
        engine, option, context = AnalyticRangeAccrualEngine(), contract(), market()
        self.assert_rejection(
            RootNotBracketedError,
            implied_volatility,
            engine,
            option,
            context,
            -1,
            lower_bound=0.1,
            upper_bound=0.4,
        )
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            contract(annual_coupon_rate=0),
            context,
            0,
        )
        self.assert_rejection(
            UnsupportedOperationError,
            implied_volatility,
            engine,
            option.replace(realized_hit_count=15),
            market(option.expiry_date),
            0,
        )
        self.assert_rejection(
            InvalidInputError,
            implied_volatility,
            engine,
            option,
            market(START + timedelta(days=3)),
            0,
        )
        self.assertEqual(option, contract())
        self.assertEqual(context.model_parameters.volatility, 0.2)

    def test_daily_coupon_matches_exact_marginal_sum(self):
        option = contract(expiry_date=START + timedelta(days=365))
        context = market()
        self.assertAlmostEqual(
            AnalyticRangeAccrualEngine().price(option, context),
            reference(option, context),
            delta=1e-10,
        )
        self.assertAlmostEqual(
            implied_volatility(
                AnalyticRangeAccrualEngine(),
                option,
                context,
                reference(option, context),
                lower_bound=0.1,
                upper_bound=0.4,
            ),
            context.model_parameters.volatility,
            delta=1e-8,
        )

    def assert_rejection(self, expected_error, call, *args, **kwargs):
        with self.assertRaises(expected_error) as raised:
            call(*args, **kwargs)
        self.assertIs(type(raised.exception), expected_error)

    def test_terms_representation_and_owning_replacement(self):
        option = contract()
        self.assertIsNone(option.realized_hit_count)
        self.assertEqual(option.notional, 1000)
        self.assertEqual(option.annual_coupon_rate, 0.12)
        self.assertEqual(option.lower_bound, 95)
        self.assertEqual(option.upper_bound, 110)
        changed = option.replace(realized_hit_count=3)
        self.assertEqual(changed.realized_hit_count, 3)
        self.assertEqual(changed.replace(realized_hit_count=None), option)
        for value in (True, 1.0, 1.5, "1"):
            with self.subTest(value=value):
                with self.assertRaises(TypeError):
                    contract(realized_hit_count=value)
                with self.assertRaises(TypeError):
                    option.replace(realized_hit_count=value)
        with self.assertRaises(OverflowError):
            contract(realized_hit_count=2**100)
        for name in ("notional", "annual_coupon_rate", "lower_bound", "upper_bound"):
            for value in (True, "100"):
                with self.subTest(name=name, value=value), self.assertRaises(TypeError):
                    contract(**{name: value})
            for value in (-1, math.inf, math.nan):
                self.assert_rejection(InvalidInputError, contract, **{name: value})
        for values in (
            {"notional": 0},
            {"lower_bound": 110},
            {"upper_bound": 90},
            {"realized_hit_count": -1},
        ):
            self.assert_rejection(InvalidInputError, contract, **values)
        self.assert_rejection(InvalidInputError, contract, expiry_date=START)
        self.assert_rejection(
            InvalidInputError,
            option.replace,
            realized_hit_count=-1,
        )
        self.assertEqual(option, contract())
        self.assertIn("realized_hit_count", repr(changed))
        with self.assertRaises(AttributeError):
            option.notional = 2

    def test_closed_bounds_and_expiry_coupon(self):
        option = contract(expiry_date=START + timedelta(days=2), realized_hit_count=1)
        engine = AnalyticRangeAccrualEngine()
        for spot, hit in (
            (math.nextafter(95, 0), False),
            (95, True),
            (math.nextafter(95, 100), True),
            (math.nextafter(110, 100), True),
            (110, True),
            (math.nextafter(110, 120), False),
        ):
            with self.subTest(spot=spot):
                result = engine.price_with_greeks(
                    option, market(option.expiry_date, spot), all_greeks=True
                )
                self.assertAlmostEqual(
                    result.price, 1000 * 0.12 / 365 * (1 + hit), delta=1e-12
                )
                for name in (
                    "delta",
                    "gamma",
                    "speed",
                    "vega",
                    "vanna",
                    "zomma",
                    "theta",
                    "charm",
                    "color",
                    "rho",
                ):
                    self.assertIsNone(getattr(result, name))

    def test_history_midnight_intraday_and_pre_effective(self):
        engine = AnalyticRangeAccrualEngine()
        for valuation, hits in (
            (instant(START) - timedelta(days=3), None),
            (instant(START), None),
            (instant(START) + timedelta(hours=12), None),
            (instant(START) + timedelta(days=1), None),
            (instant(START) + timedelta(days=1, hours=12), 1),
            (instant(START) + timedelta(days=8), 5),
        ):
            with self.subTest(valuation=valuation):
                option = contract(realized_hit_count=hits)
                context = market(valuation)
                self.assertAlmostEqual(
                    engine.price(option, context),
                    reference(option, context),
                    delta=1e-10,
                )
        for valuation, count in (
            (START, 1),
            (START + timedelta(days=1), 1),
            (instant(START) + timedelta(days=1, hours=12), 2),
        ):
            self.assert_rejection(
                InvalidInputError,
                engine.price,
                contract(realized_hit_count=count),
                market(valuation),
            )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            contract(),
            market(START + timedelta(days=2)),
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            contract(realized_hit_count=16),
            market(instant(START) + timedelta(days=16, microseconds=1)),
        )

    def test_weekend_calendar_full_term_and_zero_coupon(self):
        option = contract(effective_date=date(2025, 1, 3), expiry_date=date(2025, 1, 6))
        context = market(date(2025, 1, 3), calendar=weekdays_calendar())
        engine = AnalyticRangeAccrualEngine()
        self.assertAlmostEqual(
            engine.price(option, context), reference(option, context), delta=1e-12
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            contract(expiry_date=date(2025, 1, 5)),
            context,
        )
        short_curve = RateTermStructure(
            reference_time=START, times=[START + timedelta(days=1)], factors=[0.999]
        )
        short = market(rate=short_curve)
        self.assert_rejection(InvalidInputError, engine.price, contract(), short)
        self.assertEqual(engine.price(contract(annual_coupon_rate=0), short), 0)
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            contract(),
            market(dividend=short_curve),
        )
        weekend = market(
            datetime(2025, 1, 4, 12, tzinfo=UTC), calendar=weekdays_calendar()
        )
        result = engine.price_with_greeks(option, weekend, ["theta", "charm", "color"])
        before = market(
            datetime(2025, 1, 3, 12, tzinfo=UTC), calendar=weekdays_calendar()
        )
        after = market(
            datetime(2025, 1, 5, 12, tzinfo=UTC), calendar=weekdays_calendar()
        )
        self.assertAlmostEqual(
            result.theta,
            (reference(option, after) - reference(option, before)) / 2,
            delta=1e-12,
        )
        self.assertIsNotNone(result.charm)
        self.assertIsNotNone(result.color)
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            contract(annual_coupon_rate=0),
            market(START + timedelta(days=3)),
        )

    def test_coupon_is_weighted_over_full_contract_and_discounted_only_at_expiry(self):
        option = contract()
        engine = AnalyticRangeAccrualEngine()
        context = market(rate=0.8, dividend=0.8, volatility=1e-9)
        self.assertEqual(
            engine.price(option, context),
            engine.price(
                option,
                context.replace(
                    calendar=custom_calendar(
                        base=all_days_calendar(),
                        trading_days_per_year=252,
                    )
                ),
            ),
        )
        overrides = context.replace(
            calendar=custom_calendar(
                base=weekdays_calendar(),
                closed_dates=[START + timedelta(days=1)],
                open_dates=[START + timedelta(days=3)],
            )
        )
        self.assertAlmostEqual(
            engine.price(option, overrides), reference(option, overrides), delta=1e-12
        )
        self.assertAlmostEqual(
            engine.price(option, context),
            120 * 16 / 365 * math.exp(-0.8 * 16 / 365),
            delta=1e-12,
        )
        live = option.replace(realized_hit_count=3)
        current = market(
            START + timedelta(days=7), rate=0.8, dividend=0.8, volatility=1e-9
        )
        self.assertAlmostEqual(
            engine.price(live, current),
            120 * 13 / 365 * math.exp(-0.8 * 9 / 365),
            delta=1e-12,
        )
        self.assertEqual(
            engine.price(
                option, market(spot=80, rate=0.8, dividend=0.8, volatility=1e-9)
            ),
            0,
        )

    def test_flat_nonflat_and_negative_rates_against_independent_probabilities(self):
        option = contract()
        rate = RateTermStructure(
            reference_time=START,
            times=[START + timedelta(days=5), option.expiry_date],
            factors=[1.001, 0.997],
        )
        dividend = RateTermStructure(
            reference_time=START - timedelta(days=1),
            times=[START + timedelta(days=7), option.expiry_date],
            factors=[0.998, 1.002],
        )
        engine = AnalyticRangeAccrualEngine()
        for context in (
            market(),
            market(rate=-0.03),
            market(rate=rate, dividend=dividend),
        ):
            self.assertAlmostEqual(
                engine.price(option, context), reference(option, context), delta=1e-10
            )
        lower_zero = contract(lower_bound=0)
        self.assertAlmostEqual(
            engine.price(lower_zero, market()),
            reference(lower_zero, market()),
            delta=1e-10,
        )

    def test_extreme_coupon_tail_scaling_and_overflow(self):
        mean = math.log(100) + 0.01 / 365
        width = 0.2 / math.sqrt(365)
        option = contract(
            notional=1e300,
            annual_coupon_rate=1e100,
            lower_bound=math.exp(mean + 40 * width),
            upper_bound=math.exp(mean + 41 * width),
            expiry_date=START + timedelta(days=1),
        )
        expected = math.exp(
            math.log(1e300)
            + math.log(1e100)
            - math.log(365)
            - 0.04 / 365
            - 804.608442013753788
        )
        engine = AnalyticRangeAccrualEngine()
        self.assertAlmostEqual(
            engine.price(option, market()) / expected, 1, delta=2e-11
        )
        overflow = contract(notional=1e300, annual_coupon_rate=1e100)
        self.assert_rejection(ComputationFailureError, engine.price, overflow, market())

    def test_adjacent_price_bounds_at_tiny_volatility(self):
        below = math.nextafter(100.0, 0.0)
        above = math.nextafter(100.0, math.inf)
        engine = AnalyticRangeAccrualEngine()
        for lower, upper, volatility, expected, days in (
            (100, above, 1e-16, 0.5, 1),
            (below, 100, 1e-16, 0.5, 1),
            (below, above, 1e-16, 1.0, 1),
            (100, above, 1e-16, 0.5, 2),
            (100, above, math.ulp(0.0), 0.5, 1),
            (below, 100, math.ulp(0.0), 0.5, 1),
            (below, above, math.ulp(0.0), 1.0, 1),
            (below, 110, 1e-14, 0.6069959804126283, 1),
            (90, above, 1e-14, 0.6069959804126285, 1),
        ):
            with self.subTest(lower=lower, upper=upper, days=days):
                option = contract(
                    notional=365 / days,
                    annual_coupon_rate=1,
                    lower_bound=lower,
                    upper_bound=upper,
                    expiry_date=START + timedelta(days=days),
                )
                context = market(rate=0, dividend=0, volatility=volatility)
                self.assertAlmostEqual(
                    engine.price(option, context), expected, delta=2e-14
                )

    def test_exact_prices_and_greeks_support_concurrent_reads(self):
        option, context = contract(), market()
        engine = AnalyticRangeAccrualEngine()
        expected = reference(option, context)
        with ThreadPoolExecutor(2) as pool:
            for value in pool.map(lambda _: engine.price(option, context), range(4)):
                self.assertAlmostEqual(value, expected, delta=1e-10)
        result = engine.price_with_greeks(
            option, context, ["delta", "gamma"], spot_shift=0.25
        )
        up = engine.price(option, market(spot=100.25))
        down = engine.price(option, market(spot=99.75))
        self.assertEqual(result.price, engine.price(option, context))
        self.assertAlmostEqual(result.delta, (up - down) / 0.5, delta=1e-12)
        self.assertAlmostEqual(
            result.gamma, (up - 2 * result.price + down) / 0.25**2, delta=1e-12
        )

    def test_smoothed_greeks_against_independent_reference(self):
        valuation = START - timedelta(days=3)
        curve = RateTermStructure(
            reference_time=START - timedelta(days=5),
            times=[START + timedelta(days=3), START + timedelta(days=8)],
            factors=[1.001, 0.997],
        )
        h, v, r = 0.1, 0.0001, 0.0001
        for days, rate, dividend in ((1, 0.04, 0.01), (8, curve, -0.01)):
            option = contract(expiry_date=START + timedelta(days=days))
            context = market(valuation, spot=99, rate=rate, dividend=dividend)
            engine = AnalyticRangeAccrualEngine()
            result = engine.price_with_greeks(
                option, context, all_greeks=True, spot_shift=0.1
            )

            def price(
                spot=99,
                vol=0.2,
                rate=0.0,
                when=valuation,
                context=context,
                option=option,
            ):
                model = context.model_parameters.replace(
                    risk_free_rate=context.model_parameters.risk_free_curve.shifted(
                        rate
                    ),
                    volatility=vol,
                )
                return reference(
                    option,
                    PricingContext(
                        model_parameters=model,
                        spot_price=spot,
                        valuation_time=when,
                        calendar=context.calendar,
                    ),
                )

            def delta(**args):
                return (price(spot=99 + h, **args) - price(spot=99 - h, **args)) / (
                    2 * h
                )

            def gamma(**args):
                return (
                    price(spot=99 + h, **args)
                    - 2 * price(**args)
                    + price(spot=99 - h, **args)
                ) / h**2

            expected = {
                "delta": delta(),
                "gamma": gamma(),
                "speed": (
                    price(spot=99 + 2 * h)
                    - 2 * price(spot=99 + h)
                    + 2 * price(spot=99 - h)
                    - price(spot=99 - 2 * h)
                )
                / (2 * h**3),
                "vega": (price(vol=0.2 + v) - price(vol=0.2 - v)) / (200 * v),
                "vanna": (delta(vol=0.2 + v) - delta(vol=0.2 - v)) / (200 * v),
                "zomma": (gamma(vol=0.2 + v) - gamma(vol=0.2 - v)) / (200 * v),
                "rho": (price(rate=r) - price(rate=-r)) / (200 * r),
                "theta": (
                    price(when=valuation + timedelta(days=1))
                    - price(when=valuation - timedelta(days=1))
                )
                / 2,
                "charm": (
                    delta(when=valuation + timedelta(days=1))
                    - delta(when=valuation - timedelta(days=1))
                )
                / 2,
                "color": (
                    gamma(when=valuation + timedelta(days=1))
                    - gamma(when=valuation - timedelta(days=1))
                )
                / 2,
            }
            for name, value in expected.items():
                with self.subTest(greek=name):
                    self.assertIsNotNone(getattr(result, name))
                    self.assertAlmostEqual(getattr(result, name), value, delta=2e-8)

    def test_curve_knot_uses_right_hand_time_greek_before_effective(self):
        valuation = START - timedelta(days=3)
        option = contract(expiry_date=START + timedelta(days=1))
        rate = RateTermStructure(
            reference_time=START - timedelta(days=5),
            times=[valuation, option.expiry_date],
            factors=[0.999, 1.002],
        )
        context = market(valuation, rate=rate)
        after = market(valuation + timedelta(days=1), rate=rate)
        result = AnalyticRangeAccrualEngine().price_with_greeks(
            option, context, "theta"
        )
        self.assertAlmostEqual(
            result.theta,
            reference(option, after) - reference(option, context),
            delta=1e-12,
        )

    def test_long_contract_and_current_spot_stencil(self):
        engine = AnalyticRangeAccrualEngine()
        option = contract(expiry_date=START + timedelta(days=21204))
        self.assertAlmostEqual(
            engine.price(option, market()), reference(option, market()), delta=1e-8
        )
        seasoned = option.replace(realized_hit_count=20000)
        context = market(START + timedelta(days=20001))
        self.assertAlmostEqual(
            engine.price(seasoned, context), reference(seasoned, context), delta=1e-8
        )
        current = contract(realized_hit_count=0)
        for spot, available in ((95.005, False), (95.1, True)):
            result = engine.price_with_greeks(
                current, market(START + timedelta(days=1), spot=spot), "delta"
            )
            self.assertEqual(result.delta is not None, available)

    def test_event_availability_and_shift_validation(self):
        engine = AnalyticRangeAccrualEngine()
        option = contract(realized_hit_count=0)
        result = engine.price_with_greeks(
            option, market(START + timedelta(days=1), spot=95), all_greeks=True
        )
        for name in (
            "delta",
            "gamma",
            "speed",
            "vanna",
            "zomma",
            "theta",
            "charm",
            "color",
        ):
            self.assertIsNone(getattr(result, name))
        self.assertIsNotNone(result.vega)
        self.assertIsNotNone(result.rho)
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            market(),
            [],
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            market(),
            "delta",
            volatility_shift=0,
        )
        for field in (
            "path_count",
            "seed",
            "backend",
            "thread_count",
            "sampling",
            "antithetic",
        ):
            with self.subTest(field=field), self.assertRaises(TypeError):
                AnalyticRangeAccrualEngine(**{field: 1})


if __name__ == "__main__":
    unittest.main()
