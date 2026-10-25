import math
import os
import statistics
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, time, timedelta

import kiyosi
from kiyosi.instruments import RangeAccrualOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import MonteCarloRangeAccrualEngine

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
    def assert_category(self, category, call, *args, **kwargs):
        with self.assertRaises(kiyosi.KiyosiError) as raised:
            call(*args, **kwargs)
        self.assertEqual(raised.exception.category, category)

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
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_PARAMETER, contract, **{name: value}
                )
        for values in (
            {"notional": 0},
            {"lower_bound": 110},
            {"upper_bound": 90},
            {"realized_hit_count": -1},
        ):
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER, contract, **values
            )
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_TIME_RANGE, contract, expiry_date=START
        )
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_PARAMETER,
            option.replace,
            realized_hit_count=-1,
        )
        self.assertEqual(option, contract())
        self.assertIn("realized_hit_count", repr(changed))
        with self.assertRaises(AttributeError):
            option.notional = 2

    def test_closed_bounds_and_expiry_coupon(self):
        option = contract(expiry_date=START + timedelta(days=2), realized_hit_count=1)
        engine = MonteCarloRangeAccrualEngine()
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
        engine = MonteCarloRangeAccrualEngine(
            path_count=8192, seed=12, sampling="sobol"
        )
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
                    delta=0.012,
                )
        for valuation, count in (
            (START, 1),
            (START + timedelta(days=1), 1),
            (instant(START) + timedelta(days=1, hours=12), 2),
        ):
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                engine.price,
                contract(realized_hit_count=count),
                market(valuation),
            )
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_PARAMETER,
            engine.price,
            contract(),
            market(START + timedelta(days=2)),
        )
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_TIME_RANGE,
            engine.price,
            contract(realized_hit_count=16),
            market(instant(START) + timedelta(days=16, microseconds=1)),
        )

    def test_weekend_calendar_full_term_and_zero_coupon(self):
        option = contract(effective_date=date(2025, 1, 3), expiry_date=date(2025, 1, 6))
        context = market(date(2025, 1, 3), calendar=weekdays_calendar())
        engine = MonteCarloRangeAccrualEngine(path_count=1)
        self.assertAlmostEqual(
            engine.price(option, context), reference(option, context), delta=1e-12
        )
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_DATE,
            engine.price,
            contract(expiry_date=date(2025, 1, 5)),
            context,
        )
        short_curve = RateTermStructure(
            reference_time=START, times=[START + timedelta(days=1)], factors=[0.999]
        )
        short = market(rate=short_curve)
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_TIME_RANGE, engine.price, contract(), short
        )
        self.assertEqual(engine.price(contract(annual_coupon_rate=0), short), 0)
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_TIME_RANGE,
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
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_PARAMETER,
            engine.price,
            contract(annual_coupon_rate=0),
            market(START + timedelta(days=3)),
        )

    def test_coupon_is_weighted_over_full_contract_and_discounted_only_at_expiry(self):
        option = contract()
        engine = MonteCarloRangeAccrualEngine(path_count=1, seed=42)
        context = market(rate=0.8, dividend=0.8, volatility=1e-9)
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
        for context in (
            market(),
            market(rate=-0.03),
            market(rate=rate, dividend=dividend),
        ):
            prices = [
                MonteCarloRangeAccrualEngine(path_count=8192, seed=seed).price(
                    option, context
                )
                for seed in range(12)
            ]
            self.assertLess(
                abs(statistics.mean(prices) - reference(option, context)),
                6 * statistics.stdev(prices) / math.sqrt(len(prices)) + 1e-10,
            )
            for seed in (7, 23):
                quasi = MonteCarloRangeAccrualEngine(
                    path_count=16384, seed=seed, sampling="sobol", antithetic=True
                )
                self.assertAlmostEqual(
                    quasi.price(option, context),
                    reference(option, context),
                    delta=0.006,
                )
        lower_zero = contract(lower_bound=0)
        self.assertAlmostEqual(
            MonteCarloRangeAccrualEngine(path_count=16384, sampling="sobol").price(
                lower_zero, market()
            ),
            reference(lower_zero, market()),
            delta=0.01,
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
        for backend in ["cpu", "cuda"] if os.getenv("KIYOSI_TEST_CUDA") else ["cpu"]:
            with self.subTest(backend=backend):
                engine = MonteCarloRangeAccrualEngine(
                    path_count=1, seed=42, backend=backend
                )
                self.assertAlmostEqual(
                    engine.price(option, market()) / expected, 1, delta=2e-11
                )
                overflow = contract(notional=1e300, annual_coupon_rate=1e100)
                self.assert_category(
                    kiyosi.ErrorCategory.INVALID_RESULT,
                    engine.price,
                    overflow,
                    market(),
                )

    def test_seeded_threads_concurrent_reads_and_common_greek_samples(self):
        option, context = contract(), market()
        for sampling in ("pseudo_random", "sobol"):
            for antithetic in (False, True):
                engine = MonteCarloRangeAccrualEngine(
                    path_count=4096,
                    seed=42,
                    sampling=sampling,
                    antithetic=antithetic,
                    thread_count=1,
                )
                expected = engine.price(option, context)
                for threads in (0, 4):
                    parallel = MonteCarloRangeAccrualEngine(
                        path_count=4096,
                        seed=42,
                        sampling=sampling,
                        antithetic=antithetic,
                        thread_count=threads,
                    )
                    self.assertEqual(parallel.price(option, context), expected)
                with ThreadPoolExecutor(2) as pool:
                    self.assertEqual(
                        list(
                            pool.map(
                                lambda _, engine=engine: engine.price(option, context),
                                range(2),
                            )
                        ),
                        [expected, expected],
                    )
        engine = MonteCarloRangeAccrualEngine(path_count=4096, seed=42)
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
        option = contract(expiry_date=START + timedelta(days=1))
        valuation = START - timedelta(days=3)
        context = market(valuation, spot=99)
        engine = MonteCarloRangeAccrualEngine(path_count=1, seed=42)
        result = engine.price_with_greeks(
            option, context, all_greeks=True, spot_shift=0.1
        )

        def price(spot=99, vol=0.2, rate=0.04, when=valuation):
            return reference(option, market(when, spot=spot, rate=rate, volatility=vol))

        h, v, r = 0.1, 0.0001, 0.0001
        price()

        def delta(**args):
            return (price(spot=99 + h, **args) - price(spot=99 - h, **args)) / (2 * h)

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
            "rho": (price(rate=0.04 + r) - price(rate=0.04 - r)) / (200 * r),
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
        result = MonteCarloRangeAccrualEngine(path_count=1).price_with_greeks(
            option, context, "theta"
        )
        self.assertAlmostEqual(
            result.theta,
            reference(option, after) - reference(option, context),
            delta=1e-12,
        )

    def test_active_sobol_dimension_and_current_spot_stencil(self):
        engine = MonteCarloRangeAccrualEngine(path_count=1, sampling="sobol")
        option = contract(expiry_date=START + timedelta(days=21204))
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_PARAMETER, engine.price, option, market()
        )
        self.assertGreater(
            engine.price(
                option.replace(realized_hit_count=20000),
                market(START + timedelta(days=20001)),
            ),
            0,
        )
        current = contract(realized_hit_count=0)
        for spot, available in ((95.005, False), (95.1, True)):
            result = engine.price_with_greeks(
                current, market(START + timedelta(days=1), spot=spot), "delta"
            )
            self.assertEqual(result.delta is not None, available)

    def test_event_availability_and_configuration_validation(self):
        engine = MonteCarloRangeAccrualEngine(path_count=32)
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
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_PARAMETER,
            engine.price_with_greeks,
            option,
            market(),
            [],
        )
        self.assert_category(
            kiyosi.ErrorCategory.INVALID_PARAMETER,
            engine.price_with_greeks,
            option,
            market(),
            "delta",
            volatility_shift=0,
        )
        for values in (
            {"path_count": 0},
            {"path_count": 10_000_001},
            {"path_count": 3, "sampling": "sobol"},
        ):
            self.assert_category(
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                MonteCarloRangeAccrualEngine,
                **values,
            )
        self.assert_category(
            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
            MonteCarloRangeAccrualEngine,
            path_count=4096,
            sampling="sobol",
            backend="cuda",
        )
        for values in (
            {"path_count": True},
            {"path_count": 3.5},
            {"seed": True},
            {"thread_count": 1.5},
        ):
            with self.assertRaises(TypeError):
                MonteCarloRangeAccrualEngine(**values)

    @unittest.skipUnless(os.getenv("KIYOSI_TEST_CUDA"), "CUDA runtime tests disabled")
    def test_cuda_prices_curves_greeks_and_current_settlement(self):
        option = contract()
        rate = RateTermStructure(
            reference_time=START,
            times=[START + timedelta(days=5), option.expiry_date],
            factors=[1.001, 0.997],
        )
        engine = MonteCarloRangeAccrualEngine(
            path_count=131072, seed=42, backend="cuda", antithetic=True
        )
        for context in (market(), market(rate=rate)):
            self.assertAlmostEqual(
                engine.price(option, context), reference(option, context), delta=0.015
            )
        single = contract(expiry_date=START + timedelta(days=1))
        context = market(START - timedelta(days=3), spot=99)
        gpu = engine.price_with_greeks(single, context, all_greeks=True, spot_shift=0.1)
        cpu = MonteCarloRangeAccrualEngine(path_count=1, seed=42).price_with_greeks(
            single, context, all_greeks=True, spot_shift=0.1
        )
        for name in (
            "price",
            "delta",
            "gamma",
            "speed",
            "theta",
            "charm",
            "color",
            "vega",
            "vanna",
            "zomma",
            "rho",
        ):
            self.assertAlmostEqual(getattr(gpu, name), getattr(cpu, name), delta=2e-8)
        settled = contract(expiry_date=START + timedelta(days=2), realized_hit_count=1)
        self.assertAlmostEqual(
            engine.price(settled, market(settled.expiry_date)),
            2 * 1000 * 0.12 / 365,
            delta=1e-12,
        )


if __name__ == "__main__":
    unittest.main()
