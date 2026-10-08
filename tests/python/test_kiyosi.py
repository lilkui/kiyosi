import csv
import math
import numbers
import os
import subprocess
import sys
import textwrap
import unittest
from datetime import UTC, date, datetime, timedelta, timezone
from pathlib import Path
from typing import get_args

import kiyosi

# Jobs set KIYOSI_TEST_CUDA=disabled only after building with KIYOSI_ENABLE_CUDA=OFF,
# or enabled after building with CUDA support on a host with a usable GPU.
# Unset runs the portable tests, including CUDA validation and expiry settlement.
from kiyosi import market, pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    ArithmeticAveragePriceOption,
    AssetOrNothingOption,
    BarrierOption,
    BarrierTouchState,
    BinarySnowballOption,
    CashOrNothingOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    PhoenixOption,
    TouchOption,
    asset_no_touch_down,
    asset_one_touch_down,
    asset_one_touch_up,
    both_down_snowball,
    cash_binary_barrier_option,
    cash_one_touch_up,
    dual_coupon_snowball,
    standard_snowball,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    fixed_interval_schedule,
    monthly_schedule,
)
from kiyosi.pricing import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    AnalyticVanillaEngine,
    calculate_numerical_greeks,
    implied_coupon,
    implied_volatility,
)


def parity_fields(value):
    if value == "-":
        return {}
    return dict(item.split("=", 1) for item in value.split(";"))


def parity_cases():
    path = Path(__file__).parents[1] / "fixtures" / "api_parity.tsv"
    with path.open(newline="", encoding="utf-8") as stream:
        return [
            {
                **row,
                "inputs": parity_fields(row["inputs"]),
                "expected": parity_fields(row["expected"]),
            }
            for row in csv.DictReader(stream, delimiter="\t")
        ]


def utc_timestamp(value):
    return datetime.fromisoformat(value)


class KiyosiPythonTests(unittest.TestCase):
    def test_digital_finite_difference_payoff_bounds(self):
        start = date(2025, 1, 6)
        context = PricingContext(model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ), spot_price=100, valuation_time=start)
        engine = pricing.FiniteDifferenceDigitalEngine(asset_step_count=10000, time_step_count=1)
        stable = pricing.FiniteDifferenceDigitalEngine(
            asset_step_count=10000, time_step_count=1, scheme="implicit_euler"
        )
        for option_type in ("call", "put"):
            terms = dict(option_type=option_type, strike=100.5, effective_date=start,
                         expiry_date=start + timedelta(days=365))
            for option in (CashOrNothingOption(**terms, payout=100), AssetOrNothingOption(**terms)):
                with self.subTest(option=type(option).__name__, option_type=option_type):
                    for value in (lambda: engine.price(option, context),
                                  lambda: engine.price_with_greeks(option, context, greeks="delta")):
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            value()
                        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)
                    self.assertGreater(stable.price(option, context), 40)
                    self.assertLess(stable.price(option, context), 60)
        volatile_context = PricingContext(model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=3
        ), spot_price=100, valuation_time=start)
        for option_type in ("call", "put"):
            option = CashOrNothingOption(option_type=option_type, strike=110, payout=100,
                                        effective_date=start, expiry_date=start + timedelta(days=1825))
            with self.subTest(default_grid=option_type):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    pricing.FiniteDifferenceDigitalEngine().price(option, volatile_context)
                self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)
        option = CashOrNothingOption(option_type="call", strike=20, payout=100,
                                    effective_date=start, expiry_date=start + timedelta(days=1))
        price = pricing.FiniteDifferenceDigitalEngine().price(option, context)
        self.assertLessEqual(price, 100 * math.exp(-0.05 / 365))
        self.assertAlmostEqual(price, 99.98630230808251)

    def test_monte_carlo_seeds_accept_integral_index_protocol(self):
        @numbers.Integral.register
        class IndexInteger:
            def __init__(self, value):
                self.value = value

            def __index__(self):
                return self.value

        for engine_type in (
            pricing.MonteCarloVanillaEngine, pricing.MonteCarloAccumulatorEngine,
            pricing.MonteCarloSnowballEngine, pricing.MonteCarloBinarySnowballEngine,
            pricing.MonteCarloTernarySnowballEngine, pricing.MonteCarloPhoenixEngine,
        ):
            for value in (0, 7, 2**64 - 1):
                with self.subTest(engine=engine_type.__name__, seed=value):
                    self.assertEqual(engine_type(seed=IndexInteger(value)).seed, value)
            for value in (-1, 2**64):
                with self.subTest(engine=engine_type.__name__, seed=value):
                    with self.assertRaises(OverflowError):
                        engine_type(seed=IndexInteger(value))
            for value in (True, 1.5, IndexInteger(1.5)):
                with self.subTest(engine=engine_type.__name__, seed=value):
                    with self.assertRaises(TypeError):
                        engine_type(seed=value)
        engine = pricing.MonteCarloVanillaEngine(path_count=IndexInteger(32), step_count=2, seed=IndexInteger(7))
        expected = pricing.MonteCarloVanillaEngine(path_count=32, step_count=2, seed=7)
        self.assertEqual(engine.price(self.option, self.context), expected.price(self.option, self.context))

        class BrokenInteger(IndexInteger):
            def __index__(self):
                raise RuntimeError("index failed")

        with self.assertRaisesRegex(RuntimeError, "index failed"):
            pricing.MonteCarloVanillaEngine(seed=BrokenInteger(7))

    def test_implied_volatility_rejects_fixed_cashflows(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.3
        )
        for engine in (pricing.MonteCarloBinarySnowballEngine(path_count=64, seed=73),
                       pricing.FiniteDifferenceBinarySnowballEngine()):
            for valuation in (start, start + timedelta(days=1)):
                context = PricingContext(model_parameters=parameters, spot_price=100, valuation_time=valuation)
                note = BinarySnowballOption(
                    knock_out_coupon_rates=[0.3, 0.1], maturity_coupon_rate=0.1,
                    knock_out_levels=[120, 120], observation_dates=[start, end],
                    barrier_state="none", effective_date=start, expiry_date=end,
                )
                quote = 1.1 * math.exp(-0.04 * (end - valuation).days / 365)
                with self.subTest(engine=type(engine).__name__, valuation=valuation):
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        implied_volatility(engine, note, context, quote)
                    self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
            note = BinarySnowballOption(
                knock_out_coupon_rates=[0.2], maturity_coupon_rate=0.1,
                knock_out_levels=[120], observation_dates=[end], effective_date=start, expiry_date=end,
            )
            context = PricingContext(model_parameters=parameters, spot_price=100, valuation_time=start)
            low = PricingContext(model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.05
            ), spot_price=100, valuation_time=start)
            self.assertEqual(implied_volatility(engine, note, context, engine.price(note, low),
                                                lower_bound=0.05, upper_bound=0.4), 0.05)
            early_note = BinarySnowballOption(
                knock_out_coupon_rates=[0, 0], maturity_coupon_rate=0,
                knock_out_levels=[120, 120], observation_dates=[date(2025, 7, 1), end],
                effective_date=start, expiry_date=end,
            )
            for rate in (0, 0.04):
                low_context = PricingContext(model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=0.01, volatility=0.05
                ), spot_price=100, valuation_time=start)
                quote = engine.price(early_note, low_context)
                with self.subTest(engine=type(engine).__name__, rate=rate):
                    if rate == 0:
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            implied_volatility(engine, early_note, low_context, quote,
                                               lower_bound=0.05, upper_bound=0.4)
                        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
                    else:
                        self.assertEqual(implied_volatility(engine, early_note, low_context, quote,
                                                            lower_bound=0.05, upper_bound=0.4), 0.05)
        accumulator = Accumulator(
            strike=100, knock_out_level=120, daily_quantity=0, acceleration_factor=1,
            accumulated_quantity=0, effective_date=start, expiry_date=end,
        )
        for engine in (pricing.MonteCarloAccumulatorEngine(path_count=64, seed=73),
                       pricing.FiniteDifferenceAccumulatorEngine()):
            with self.subTest(engine=type(engine).__name__):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    implied_volatility(engine, accumulator, context, 0)
                self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)

    def test_barrier_finite_difference_preserves_volatility_tails(self):
        start = date(2025, 1, 1)
        end = start + timedelta(days=1825)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0, volatility=0.8
            ),
            spot_price=100, valuation_time=start, calendar=market.all_days_calendar(),
        )
        for mode in ("continuous", "scheduled"):
            option = BarrierOption(
                option_type="call", strike=100, effective_date=start, expiry_date=end,
                barrier_level=0.01, barrier_type="down_and_out", observation_mode=mode,
                observation_dates=[end] if mode == "scheduled" else [],
            )
            expected = AnalyticBarrierEngine().price(option, context)
            for assets, times in ((200, 200), (800, 1000)):
                with self.subTest(mode=mode, assets=assets, times=times):
                    engine = pricing.FiniteDifferenceBarrierEngine(
                        asset_step_count=assets, time_step_count=times
                    )
                    self.assertAlmostEqual(engine.price(option, context), expected, delta=0.02)

    def test_analytic_low_volatility_and_default_implied_volatility(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        terms = dict(option_type="call", strike=100, effective_date=start, expiry_date=end)
        barrier = dict(**terms, barrier_level=120, barrier_type="up_and_out")
        vanilla = 100 * (math.exp(-0.02) - math.exp(-0.05))
        context = PricingContext(model_parameters=BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2), spot_price=100, valuation_time=start)
        low = PricingContext(model_parameters=BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.0001), spot_price=100, valuation_time=start)
        for engine, option, expected in (
            (AnalyticBarrierEngine(), BarrierOption(**barrier), vanilla),
            (AnalyticBinaryBarrierEngine(), cash_binary_barrier_option(**barrier, payout=1), math.exp(-0.05)),
            (pricing.BjerksundStenslandVanillaEngine(), AmericanOption(**terms), vanilla),
        ):
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(engine.price(option, low), expected, delta=1e-9)
                quote = engine.price(option, context)
                self.assertAlmostEqual(implied_volatility(engine, option, context, quote), 0.2, delta=1e-6)

    def test_numerical_time_greeks_center_clipped_stencils(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(option_type="call", strike=100, effective_date=start, expiry_date=end)
        engine = AnalyticVanillaEngine()
        parameters = BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2)
        for time in (datetime(2025, 1, 1, 12, tzinfo=UTC), datetime(2025, 12, 31, 12, tzinfo=UTC)):
            with self.subTest(time=time):
                context = PricingContext(model_parameters=parameters, spot_price=100, valuation_time=time)
                before = engine.price_with_greeks(option, PricingContext(model_parameters=parameters, spot_price=100, valuation_time=time - timedelta(hours=12)), greeks=["delta", "gamma"])
                after = engine.price_with_greeks(option, PricingContext(model_parameters=parameters, spot_price=100, valuation_time=time + timedelta(hours=12)), greeks=["delta", "gamma"])
                result = calculate_numerical_greeks(engine, option, context)
                self.assertAlmostEqual(result.theta, after.price - before.price, delta=1e-9)
                # Analytic delta/gamma disappear exactly at expiry, so compare them only at the start boundary.
                if time.year == 2025 and time.month == 1:
                    self.assertAlmostEqual(result.charm, after.delta - before.delta, delta=1e-6)
                    self.assertAlmostEqual(result.color, after.gamma - before.gamma, delta=1e-6)

    def test_vanilla_monte_carlo_validates_generic_settings_at_expiry(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2)
        for option_class in (EuropeanOption, AmericanOption):
            option = option_class(option_type="call", strike=100, effective_date=start, expiry_date=end)
            for paths, steps in ((0, 2), (-1, 2), (10_000_001, 2), (1, 0), (1, 1), (1, 10_001)):
                for time in (start, end):
                    with self.subTest(option=option_class.__name__, paths=paths, steps=steps, time=time):
                        engine = pricing.MonteCarloVanillaEngine(path_count=paths, step_count=steps)
                        context = PricingContext(model_parameters=parameters, spot_price=110, valuation_time=time)
                        for operation in (
                            lambda: engine.price(option, context),
                            lambda: engine.price_with_greeks(option, context, greeks=["delta", "gamma"]),
                        ):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                operation()
                            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
            for backend in ("cpu", "cuda"):
                engine = pricing.MonteCarloVanillaEngine(path_count=1, step_count=2, backend=backend)
                expired = PricingContext(model_parameters=parameters, spot_price=110, valuation_time=end)
                self.assertEqual(engine.price(option, expired), 10)

    def test_digital_tiny_volatility_greeks_preserve_finite_limits(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = AnalyticDigitalEngine()
        sigma = 1e-200
        density = 1 / math.sqrt(2 * math.pi)
        for option_type, sign in (("call", 1), ("put", -1)):
            terms = dict(option_type=option_type, strike=100, effective_date=start, expiry_date=end)
            cash = CashOrNothingOption(**terms, payout=1)
            asset = AssetOrNothingOption(**terms)
            for rate in (-0.03, 0.03):
                context = PricingContext(model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=0, volatility=sigma), spot_price=100, valuation_time=start)
                exercised = sign * rate > 0
                for option, price, delta in (
                    (cash, math.exp(-rate) if exercised else 0, 0),
                    (asset, 100 if exercised else 0, 1 if exercised else 0),
                ):
                    with self.subTest(option_type=option_type, rate=rate, option=type(option).__name__):
                        result = engine.price_with_greeks(option, context, greeks=["delta", "gamma"])
                        self.assertAlmostEqual(result.price, price, delta=1e-12)
                        self.assertEqual(result.delta, delta)
                        self.assertEqual(result.gamma, 0)
            atm = PricingContext(model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=sigma), spot_price=100, valuation_time=start)
            for option, price, delta, gamma in (
                (cash, 0.5, sign * density / 100 / sigma, -sign * 0.5 * density / 100 / 100 / sigma),
                (asset, 50, sign * density / sigma, sign * 0.5 * density / 100 / sigma),
            ):
                result = engine.price_with_greeks(option, atm, greeks=["delta", "gamma"])
                self.assertEqual(result.price, price)
                self.assertTrue(math.isclose(result.delta, delta, rel_tol=1e-12))
                self.assertTrue(math.isclose(result.gamma, gamma, rel_tol=1e-12))
            tail = PricingContext(model_parameters=BlackScholesMertonParameters(
                risk_free_rate=40 * sigma, dividend_yield=0, volatility=sigma), spot_price=100, valuation_time=start)
            result = engine.price_with_greeks(cash, tail, greeks=["delta", "gamma"])
            self.assertTrue(math.isclose(result.delta, sign * 1.4632702508383032e-150, rel_tol=1e-11))
            self.assertTrue(math.isclose(result.gamma, -sign * 5.853081003353213e49, rel_tol=1e-11))
            subnormal = PricingContext(model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=1e-310), spot_price=100, valuation_time=start)
            self.assertEqual(engine.price(asset, subnormal), 50)
            result = engine.price_with_greeks(asset, subnormal, greeks=["gamma"])
            self.assertTrue(math.isclose(result.gamma, sign * 1.9947114020071634e307, rel_tol=1e-11))
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine.price_with_greeks(asset, subnormal, greeks=["delta"])
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)

    def test_fd_prices_respect_payoff_bounds_just_before_expiry(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2,
            ), spot_price=99.25, valuation_time=datetime(2026, 1, 5, 23, 59, 59, tzinfo=UTC),
        )
        for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
            engine = pricing.FiniteDifferenceVanillaEngine(scheme=scheme)
            for option_type in ("call", "put"):
                with self.subTest(scheme=scheme, option_type=option_type):
                    terms = dict(option_type=option_type, strike=100, effective_date=start, expiry_date=end)
                    european = engine.price(EuropeanOption(**terms), context)
                    american = engine.price(AmericanOption(**terms), context)
                    self.assertGreaterEqual(european, 0)
                    self.assertGreaterEqual(american, 0.75 if option_type == "put" else 0)
                    if option_type == "call":
                        self.assertAlmostEqual(european, 0, delta=1e-10)
                    else:
                        self.assertEqual(american, 0.75)

    def test_fd_prices_reject_underflowed_asset_spacing(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        tiny = math.ulp(0.0)
        option = EuropeanOption(option_type="call", strike=tiny, effective_date=start, expiry_date=end)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2,
            ), spot_price=tiny, valuation_time=start,
        )
        for upper in (None, 4 * tiny):
            with self.subTest(upper=upper):
                engine = pricing.FiniteDifferenceVanillaEngine(asset_upper_boundary=upper)
                with self.assertRaises(kiyosi.KiyosiError) as caught:
                    engine.price(option, context)
                self.assertEqual(caught.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_seeded_autocallable_paths_stay_coupled_across_knock_out_changes(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = BinarySnowballOption(
            knock_out_coupon_rates=[0, 0], maturity_coupon_rate=0.2,
            knock_out_levels=[110, 110], observation_dates=[date(2025, 7, 1), end],
            effective_date=start, expiry_date=end,
        )
        parameters = BlackScholesMertonParameters(risk_free_rate=0, dividend_yield=0, volatility=0.2)
        engine = pricing.MonteCarloBinarySnowballEngine(path_count=2000, seed=1)
        previous = 1.2
        for bump in range(31):
            spot = 100 + 0.01 * bump
            with self.subTest(spot=spot):
                context = PricingContext(model_parameters=parameters, spot_price=spot, valuation_time=start)
                actual = engine.price(note, context)
                self.assertLessEqual(actual, previous)
                previous = actual

    def test_fd_automatic_domain_includes_distant_barriers(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = BarrierOption(
            option_type="call", strike=100, barrier_level=500,
            barrier_type="up_and_out", effective_date=start, expiry_date=end,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0.02, volatility=0.2,
            ), spot_price=100, valuation_time=start,
        )
        finite = pricing.FiniteDifferenceBarrierEngine()
        self.assertAlmostEqual(finite.price(option, context), AnalyticBarrierEngine().price(option, context), delta=0.05)
        self.assertEqual(finite.price_with_greeks(option, context, ["gamma"]).price, finite.price(option, context))
        clipped = pricing.FiniteDifferenceBarrierEngine(asset_upper_boundary=400)
        with self.assertRaises(kiyosi.KiyosiError) as caught:
            clipped.price(option, context)
        self.assertEqual(caught.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_fd_boundaries_preserve_long_expiry_volatility_tails(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0.02, volatility=0.6,
            ), spot_price=100, valuation_time=start,
        )
        for option_type in ("call", "put"):
            terms = dict(option_type=option_type, strike=100, effective_date=start, expiry_date=end)
            option = EuropeanOption(**terms)
            expected = AnalyticVanillaEngine().price(option, context)
            for upper in (None, 125.0):
                with self.subTest(option_type=option_type, upper=upper):
                    finite = pricing.FiniteDifferenceVanillaEngine(
                        asset_step_count=800, time_step_count=1000, asset_upper_boundary=upper,
                    )
                    self.assertAlmostEqual(finite.price(option, context), expected, delta=0.01)
            for digital in (CashOrNothingOption(payout=10, **terms), AssetOrNothingOption(**terms)):
                with self.subTest(option_type=option_type, digital=type(digital).__name__):
                    finite = pricing.FiniteDifferenceDigitalEngine(asset_step_count=800, time_step_count=1000)
                    self.assertAlmostEqual(
                        finite.price(digital, context), AnalyticDigitalEngine().price(digital, context), delta=0.01,
                    )

    def test_fd_numerical_greeks_hold_automatic_grid_fixed(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end,
        )
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0.02, volatility=0.2,
        )
        finite = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=800, time_step_count=1000,
        )
        for spot in (95.0, 95.1, 100.0, 100.1, 110.0):
            with self.subTest(spot=spot):
                context = PricingContext(
                    model_parameters=parameters, spot_price=spot, valuation_time=start,
                )
                expected = AnalyticVanillaEngine().price_with_greeks(
                    option, context, ["gamma", "speed"],
                )
                actual = calculate_numerical_greeks(finite, option, context)
                self.assertAlmostEqual(actual.gamma, expected.gamma, delta=2e-5 + 0.01 * abs(expected.gamma))
                self.assertAlmostEqual(actual.speed, expected.speed, delta=2e-5 + 0.05 * abs(expected.speed))
                self.assertEqual(actual.price, finite.price(option, context))

    def test_fd_greek_grids_preserve_extreme_expiry_settlements(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0.02, volatility=0.2,
            ), spot_price=1e308, valuation_time=end,
        )
        terms = dict(option_type="call", strike=100, effective_date=start, expiry_date=end)
        for engine, option in (
            (pricing.FiniteDifferenceVanillaEngine(), EuropeanOption(**terms)),
            (pricing.FiniteDifferenceBarrierEngine(), BarrierOption(barrier_level=80, barrier_type="down_and_out", touch_state="untouched", **terms)),
        ):
            with self.subTest(engine=type(engine).__name__):
                joint = engine.price_with_greeks(option, context, ["gamma"])
                self.assertEqual(joint.price, engine.price(option, context))
                self.assertIsNone(joint.gamma)

    def test_fd_spot_greeks_at_and_between_grid_nodes(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        option = BarrierOption(
            option_type="call", strike=100, barrier_level=80,
            barrier_type="down_and_out", effective_date=start, expiry_date=end,
        )
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.03, dividend_yield=0.02, volatility=0.2,
        )
        finite = pricing.FiniteDifferenceBarrierEngine(
            asset_step_count=800, time_step_count=1000,
        )
        for spot in (95.0, 95.1):
            with self.subTest(spot=spot):
                context = PricingContext(
                    model_parameters=parameters, spot_price=spot, valuation_time=start,
                )
                expected = AnalyticBarrierEngine().price_with_greeks(
                    option, context, ["gamma", "speed"],
                )
                actual = finite.price_with_greeks(option, context, ["gamma", "speed"])
                self.assertAlmostEqual(actual.gamma, expected.gamma, delta=2e-5 + 0.01 * abs(expected.gamma))
                self.assertAlmostEqual(actual.speed, expected.speed, delta=2e-5 + 0.05 * abs(expected.speed))

    def test_asset_one_touch_current_hits_settle_at_spot(self):
        start, event, end = date(2025, 1, 1), date(2025, 1, 2), date(2026, 1, 1)
        for factory, spot, barrier in (
            (asset_one_touch_up, 120, 110), (asset_one_touch_down, 80, 90)
        ):
            for valuation in (event, end):
                context = PricingContext(
                    model_parameters=self.parameters, spot_price=spot, valuation_time=valuation
                )
                for mode in ("scheduled", "continuous"):
                    with self.subTest(spot=spot, valuation=valuation, mode=mode):
                        terms = dict(
                            effective_date=valuation if mode == "continuous" else start,
                            expiry_date=end, barrier_level=barrier, observation_mode=mode,
                            observation_dates=[valuation] if mode == "scheduled" else [],
                        )
                        engine = AnalyticBinaryBarrierEngine()
                        self.assertEqual(engine.price(factory(settlement_timing="at_hit", **terms), context), spot)
                        self.assertAlmostEqual(
                            engine.price(factory(settlement_timing="at_expiry", **terms), context),
                            spot * math.exp(-0.02 * (end - valuation).days / 365), delta=1e-12,
                        )
            already_paid = factory(
                effective_date=start, expiry_date=end, barrier_level=barrier,
                settlement_timing="at_hit", observation_mode="scheduled",
                observation_dates=[start, event], touch_state="touched",
            )
            context = PricingContext(
                model_parameters=self.parameters, spot_price=spot, valuation_time=event
            )
            self.assertEqual(AnalyticBinaryBarrierEngine().price(already_paid, context), 0)

    def test_fixed_interval_schedule_bounds_extreme_intervals(self):
        # Isolate the overflow regression so a broken loop cannot hang the test runner.
        result = subprocess.run(
            [sys.executable, "-c", textwrap.dedent("""
                from datetime import date
                from kiyosi.market import fixed_interval_schedule
                for interval in (2**31 - 1, 2**31 - 100):
                    schedule = fixed_interval_schedule(
                        start=date(2025, 1, 1), end=date(2026, 1, 1),
                        interval_days=interval)
                    assert list(schedule) == []
            """)],
            capture_output=True, text=True, timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        self.assertEqual(
            list(fixed_interval_schedule(start=start, end=end, interval_days=365,
                                         calendar=market.all_days_calendar())),
            [end],
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            fixed_interval_schedule(start=start, end=end, interval_days=0)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_SCHEDULE)

    def test_fd_low_volatility_prices_remain_bounded(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction, spot, rate, dividend in (
            ("put", 95, 0.1, 0), ("call", 105, 0, 0.1)
        ):
            option = EuropeanOption(
                option_type=direction, strike=100, effective_date=start, expiry_date=end
            )
            for volatility in (0.001, 0.01):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
                    ),
                    spot_price=spot, valuation_time=start,
                )
                analytic = AnalyticVanillaEngine().price(option, context)
                for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
                    with self.subTest(direction=direction, volatility=volatility, scheme=scheme):
                        coarse = pricing.FiniteDifferenceVanillaEngine(
                            asset_step_count=200, time_step_count=2000, scheme=scheme
                        ).price(option, context)
                        fine = pricing.FiniteDifferenceVanillaEngine(
                            asset_step_count=800, time_step_count=2000, scheme=scheme
                        ).price(option, context)
                        self.assertGreaterEqual(coarse, 0)
                        self.assertGreaterEqual(fine, 0)
                        self.assertLessEqual(
                            coarse, (spot if direction == "call" else 100) * math.exp(-0.1)
                        )
                        self.assertLess(abs(fine - analytic), abs(coarse - analytic))

    def test_fd_current_events_use_actual_spot(self):
        start, event, end = date(2025, 1, 1), date(2025, 1, 2), date(2026, 1, 1)
        note = BinarySnowballOption(
            knock_out_coupon_rates=[0.1, 0.1], maturity_coupon_rate=0.2,
            knock_out_levels=[120, 130], observation_dates=[event, end],
            effective_date=start, expiry_date=end,
        )
        accumulator = Accumulator(
            strike=100, knock_out_level=120, daily_quantity=1,
            acceleration_factor=2, accumulated_quantity=3,
            effective_date=start, expiry_date=end,
        )
        for spot in (120, 120.1):
            context = PricingContext(
                model_parameters=self.parameters, spot_price=spot, valuation_time=event,
            )
            for upper in (499, 501):
                with self.subTest(spot=spot, upper=upper):
                    self.assertAlmostEqual(
                        pricing.FiniteDifferenceBinarySnowballEngine(asset_upper_boundary=upper).price(note, context),
                        1 + 0.1 / 365, delta=1e-12,
                    )
                    self.assertAlmostEqual(
                        pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=upper).price(accumulator, context),
                        3 * (spot - 100), delta=1e-12,
                    )

    def setUp(self):
        self.parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        self.context = PricingContext(
            model_parameters=self.parameters,
            spot_price=100.0,
            valuation_time=date(2025, 1, 1),
        )
        self.option = EuropeanOption(
            option_type="call",
            strike=100.0,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )

    def test_american_fd_boundaries_preserve_continuation(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=800, time_step_count=800, asset_upper_boundary=400
        )
        for direction, spot, rate in (("call", 390, 0.05), ("put", 0.1, -0.05)):
            with self.subTest(direction=direction):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=rate, dividend_yield=0, volatility=0.2
                    ),
                    spot_price=spot,
                    valuation_time=start,
                )
                terms = dict(
                    option_type=direction, strike=100,
                    effective_date=start, expiry_date=end,
                )
                self.assertAlmostEqual(
                    engine.price(AmericanOption(**terms), context),
                    AnalyticVanillaEngine().price(EuropeanOption(**terms), context),
                    delta=0.001,
                )

    def test_scheduled_barriers_settle_at_and_after_final_fixing(self):
        start, fixing, end = date(2025, 1, 1), date(2025, 1, 2), date(2026, 1, 1)
        for valuation in (fixing, date(2025, 1, 3)):
            context = PricingContext(
                model_parameters=self.parameters, spot_price=100, valuation_time=valuation
            )
            for kind in ("up_and_in", "up_and_out"):
                for rebate in (0, 10):
                    option = BarrierOption(
                        option_type="call", strike=100, effective_date=start, expiry_date=end,
                        barrier_level=120, barrier_type=kind, rebate=rebate,
                        observation_mode="scheduled", observation_dates=[fixing], touch_state="untouched",
                    )
                    expected = (
                        rebate * math.exp(-0.05 * (end - valuation).days / 365)
                        if kind == "up_and_in" else AnalyticVanillaEngine().price(self.option, context)
                    )
                    for engine in (AnalyticBarrierEngine(), pricing.FiniteDifferenceBarrierEngine()):
                        with self.subTest(valuation=valuation, kind=kind, rebate=rebate, engine=engine):
                            self.assertEqual(engine.price(option, context), expected)
        touch = cash_one_touch_up(
            effective_date=start, expiry_date=end, barrier_level=120, payout=10,
            settlement_timing="at_expiry", observation_mode="scheduled",
            observation_dates=[fixing], touch_state="untouched",
        )
        for spot in (100, 120):
            context = PricingContext(
                model_parameters=self.parameters, spot_price=spot, valuation_time=fixing
            )
            expected = 0 if spot < 120 else 10 * math.exp(-0.05 * 364 / 365)
            self.assertEqual(AnalyticBinaryBarrierEngine().price(touch, context), expected)

    def test_implied_phoenix_coupon_requires_payable_coupon(self):
        start, fixing, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        for valuation in (fixing, end):
            barrier = 90 if valuation == end else 130
            note = PhoenixOption(
                coupon_rate=0.1, initial_spot=100, knock_in_level=70,
                knock_out_levels=[120, 120], coupon_barrier_levels=[barrier, 90],
                upper_strike=100, lower_strike=0, observation_dates=[fixing, end],
                knock_in_observation_mode="every_trading_day", barrier_state="none",
                effective_date=start, expiry_date=end,
            )
            for spot in (80, 90, 120):
                context = PricingContext(
                    model_parameters=self.parameters, spot_price=spot, valuation_time=valuation
                )
                for engine in (pricing.MonteCarloPhoenixEngine(path_count=64, seed=1),
                               pricing.FiniteDifferencePhoenixEngine(asset_step_count=40, time_step_count=40, asset_upper_boundary=400)):
                    with self.subTest(valuation=valuation, spot=spot, engine=engine):
                        quote = engine.price(note, context)
                        if (valuation == end and spot < barrier) or (valuation == fixing and spot >= 120):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                implied_coupon(engine, note, context, quote)
                            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
                        else:
                            self.assertAlmostEqual(implied_coupon(engine, note, context, quote), 0.1, delta=1e-7)

    def test_implied_snowball_coupon_respects_terminal_state(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        note = standard_snowball(
            coupon_rate=0.1, initial_spot=100, knock_in_level=70, knock_out_level=120,
            observation_dates=[end], effective_date=start, expiry_date=end, barrier_state="none",
        )
        for spot in (60, 80, 120):
            context = PricingContext(
                model_parameters=self.parameters, spot_price=spot, valuation_time=end
            )
            for convention in ("shift_maturity_coupon", "preserve_maturity_coupon"):
                for engine in (pricing.MonteCarloSnowballEngine(path_count=64, seed=1),
                               pricing.FiniteDifferenceSnowballEngine(asset_step_count=40, time_step_count=40)):
                    with self.subTest(spot=spot, convention=convention, engine=engine):
                        quote = engine.price(note, context)
                        if spot < 70 or (spot < 120 and convention == "preserve_maturity_coupon"):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                implied_coupon(engine, note, context, quote, quote_convention=convention)
                            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
                        else:
                            self.assertAlmostEqual(
                                implied_coupon(engine, note, context, quote, quote_convention=convention),
                                0.1, delta=1e-7,
                            )

    def test_zero_barrier_rebates_ignore_payment_timing(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for rate, dividend in ((-0.02, 0), (-0.01, -0.02)):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=dividend, volatility=0.2
                ),
                spot_price=100,
                valuation_time=start,
            )
            for direction in ("call", "put"):
                for kind, barrier in (("up_and_out", 120), ("down_and_out", 80)):
                    with self.subTest(rate=rate, dividend=dividend, direction=direction, kind=kind):
                        terms = dict(
                            option_type=direction,
                            strike=100,
                            effective_date=start,
                            expiry_date=end,
                            barrier_level=barrier,
                            barrier_type=kind,
                            rebate=0,
                        )
                        engine = AnalyticBarrierEngine()
                        self.assertEqual(
                            engine.price(BarrierOption(**terms, rebate_timing="at_hit"), context),
                            engine.price(BarrierOption(**terms, rebate_timing="at_expiry"), context),
                        )

    def test_european_mc_uses_terminal_distribution(self):
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction,
                strike=100,
                effective_date=date(2025, 1, 1),
                expiry_date=date(2026, 1, 1),
            )
            baseline = pricing.MonteCarloVanillaEngine(
                path_count=20000, step_count=2, seed=42
            ).price(option, self.context)
            self.assertAlmostEqual(
                baseline, AnalyticVanillaEngine().price(option, self.context), delta=0.5
            )
            self.assertNotEqual(
                pricing.MonteCarloVanillaEngine(path_count=20000, step_count=2, seed=43).price(option, self.context),
                baseline,
            )
            for steps in (2, 7, 50, 10000):
                with self.subTest(direction=direction, steps=steps):
                    engine = pricing.MonteCarloVanillaEngine(
                        path_count=19999, step_count=steps, seed=42
                    )
                    self.assertEqual(engine.price(option, self.context), baseline)
                    self.assertEqual(engine.step_count, steps)
            for backend in ("cpu", "cuda"):
                for steps in (1, 10001):
                    with self.subTest(direction=direction, steps=steps, backend=backend):
                        engine = pricing.MonteCarloVanillaEngine(
                            path_count=10, step_count=steps, seed=42, backend=backend
                        )
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            engine.price(option, self.context)
                        self.assertEqual(
                            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                        )

    def test_coupon_choice_conversion_is_process_safe(self):
        code = textwrap.dedent(f"""
            import sys
            from datetime import date
            sys.path.insert(0, {str(Path(sys.modules['kiyosi._native'].__file__).parent)!r})
            import _native as k
            start, end = date(2025, 1, 1), date(2026, 1, 1)
            context = k.PricingContext(
                model_parameters=k.BlackScholesMertonParameters(
                    risk_free_rate=.05, dividend_yield=.02, volatility=.2),
                spot_price=100, valuation_time=start)
            note = k.BinarySnowballOption(
                knock_out_coupon_rates=[.1], maturity_coupon_rate=.1,
                knock_out_levels=[120], observation_dates=[end],
                effective_date=start, expiry_date=end)
            engine = k.MonteCarloBinarySnowballEngine(path_count=2, seed=1)
            for choice in ('unknown', chr(0xD800)):
                try:
                    k.implied_coupon(engine, note, context, 1, quote_convention=choice)
                except ValueError:
                    pass
                else:
                    raise AssertionError('invalid choice accepted')
            print('conversion errors returned safely')
            """)
        result = subprocess.run(
            [sys.executable, "-X", "faulthandler", "-c", code],
            capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("conversion errors returned safely", result.stdout)

    def test_bjerksund_respects_european_and_immediate_exercise_bounds(self):
        start, end = date(2025, 1, 6), date(2026, 1, 6)
        for option_type in ("call", "put"):
            for spot, strike, rate, dividend, volatility in (
                (200, 100, 0.02, 0.02, 0.6), (100, 100, 0, 0.02, 0.01),
                (100, 100, 0, 0.1, 0.05), (200, 100, 0, 0.1, 0.01),
            ):
                if option_type == "put":
                    spot, strike, rate, dividend = strike, spot, dividend, rate
                with self.subTest(option_type=option_type, spot=spot, volatility=volatility):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility,
                        ), spot_price=spot, valuation_time=start,
                    )
                    terms = dict(option_type=option_type, strike=strike, effective_date=start, expiry_date=end)
                    european = AnalyticVanillaEngine().price(EuropeanOption(**terms), context)
                    american = pricing.BjerksundStenslandVanillaEngine().price(AmericanOption(**terms), context)
                    intrinsic = max(spot - strike if option_type == "call" else strike - spot, 0)
                    self.assertGreaterEqual(american, european - 1e-12)
                    self.assertGreaterEqual(american, intrinsic)
                    self.assertAlmostEqual(american, max(european, intrinsic), delta=1e-10)

    def test_bjerksund_negative_transformed_rates(self):
        engine = pricing.BjerksundStenslandVanillaEngine()
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction, rate, dividend, spot, intrinsic in (
            ("call", -.01, 0, 200, 100), ("put", 0, -.01, 50, 50)
        ):
            with self.subTest(direction=direction):
                option = AmericanOption(option_type=direction, strike=100,
                                        effective_date=start, expiry_date=end)
                parameters = BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=dividend, volatility=.2)
                context = PricingContext(model_parameters=parameters, spot_price=spot,
                                         valuation_time=start)
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(option, context)
                self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
                expired = PricingContext(model_parameters=parameters, spot_price=spot,
                                         valuation_time=end)
                self.assertEqual(engine.price(option, expired), intrinsic)

    def test_binomial_rejects_underflowed_trees(self):
        engine = pricing.CoxRossRubinsteinVanillaEngine(1000)
        for volatility in (23, 25):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=.05, dividend_yield=.02, volatility=volatility),
                spot_price=100, valuation_time=date(2025, 1, 1))
            for option_class in (EuropeanOption, AmericanOption):
                for direction in ("call", "put"):
                    with self.subTest(volatility=volatility, option=option_class, direction=direction):
                        option = option_class(option_type=direction, strike=100,
                                              effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            engine.price(option, context)
                        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)

    def test_quadrature_high_variance_tails(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        for volatility in (3, 5):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=.05, dividend_yield=.02, volatility=volatility),
                spot_price=100, valuation_time=start)
            for direction in ("call", "put"):
                for option_class, numerical, analytic in (
                    (EuropeanOption, pricing.QuadratureVanillaEngine(), AnalyticVanillaEngine()),
                    (AssetOrNothingOption, pricing.QuadratureDigitalEngine(), AnalyticDigitalEngine()),
                    (CashOrNothingOption, pricing.QuadratureDigitalEngine(), AnalyticDigitalEngine()),
                ):
                    with self.subTest(volatility=volatility, direction=direction, option=option_class):
                        terms = dict(option_type=direction, strike=100, effective_date=start, expiry_date=end)
                        if option_class is CashOrNothingOption:
                            terms["payout"] = 10
                        option = option_class(**terms)
                        self.assertAlmostEqual(numerical.price(option, context),
                                               analytic.price(option, context), delta=1e-8)

    def test_trading_monte_carlo_rejects_invalid_paths(self):
        start, end = date(2025, 1, 1), date(2025, 1, 2)
        accumulator = Accumulator(strike=100, knock_out_level=120, daily_quantity=1,
                                  acceleration_factor=2, effective_date=start, expiry_date=end)
        note = BinarySnowballOption(
            knock_out_coupon_rates=[.1], maturity_coupon_rate=.1,
            knock_out_levels=[120], observation_dates=[end], effective_date=start, expiry_date=end)
        for rate, volatility in ((.05, 1e308), (.05, 1000), (1e308, .2)):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=.02, volatility=volatility),
                spot_price=100, valuation_time=start)
            for engine, option in (
                (pricing.MonteCarloAccumulatorEngine(path_count=2, seed=1), accumulator),
                (pricing.MonteCarloBinarySnowballEngine(path_count=2, seed=1), note),
            ):
                with self.subTest(rate=rate, volatility=volatility, engine=engine):
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        engine.price(option, context)
                    self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)

    def test_analytic_charm_retains_dividend_carry(self):
        for dividend in (-.02, .02):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=.05, dividend_yield=dividend, volatility=.001),
                spot_price=100, valuation_time=date(2025, 1, 1))
            for direction, sign in (("call", 1), ("put", -1)):
                for in_the_money in (False, True):
                    with self.subTest(dividend=dividend, direction=direction, itm=in_the_money):
                        strike = 50 if (direction == "call") == in_the_money else 200
                        option = EuropeanOption(option_type=direction, strike=strike,
                                                effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
                        engine = AnalyticVanillaEngine()
                        expected = sign * dividend * math.exp(-dividend) / 365 if in_the_money else 0
                        self.assertAlmostEqual(engine.price_with_greeks(option, context, "charm").charm,
                                               expected, delta=1e-15)
                        self.assertAlmostEqual(calculate_numerical_greeks(engine, option, context).charm,
                                               expected, delta=2e-9)

    def test_string_choice_boundary_and_literal_aliases(self):
        self.assertEqual(get_args(kiyosi.Greek)[:2], ("delta", "gamma"))
        self.assertNotIn("price", get_args(kiyosi.Greek))
        self.assertFalse(hasattr(kiyosi, "RiskMeasure"))
        self.assertEqual(get_args(pricing.MonteCarloBackend), ("cpu", "cuda"))
        self.assertEqual(
            get_args(market.BusinessDayConvention), ("following", "preceding")
        )
        with self.assertRaises(ValueError):
            EuropeanOption(
                option_type="CALL",
                strike=100,
                effective_date=date(2025, 1, 1),
                expiry_date=date(2026, 1, 1),
            )
        with self.assertRaises(TypeError):
            EuropeanOption(
                option_type=1,
                strike=100,
                effective_date=date(2025, 1, 1),
                expiry_date=date(2026, 1, 1),
            )
        with self.assertRaises(ValueError):
            pricing.FiniteDifferenceVanillaEngine(scheme="Crank-Nicolson")
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceVanillaEngine(scheme=1)

    def test_explicit_pricing_result_surface(self):
        self.assertFalse(hasattr(kiyosi, "price"))
        self.assertFalse(hasattr(pricing, "price"))
        self.assertFalse(hasattr(kiyosi, "GreeksLevel"))
        self.assertFalse(hasattr(pricing, "GreeksLevel"))
        result = AnalyticVanillaEngine().price_with_greeks(
            self.option, self.context, all_greeks=True
        )
        self.assertIs(type(result.price), float)
        self.assertEqual(result.require("delta"), result.delta)
        with self.assertRaises(ValueError):
            result.require("price")

    def test_selected_greeks_preserve_native_values_and_scalar_price(self):
        engine = AnalyticVanillaEngine()
        context = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 6, 1),
        )
        value = engine.price(self.option, context)
        basic = engine.price_with_greeks(self.option, context, ["delta", "gamma"])
        full = engine.price_with_greeks(self.option, context, all_greeks=True)
        numerical = calculate_numerical_greeks(engine, self.option, context)
        self.assertIs(type(value), float)
        self.assertEqual(basic.price, value)
        self.assertEqual(full.price, value)
        self.assertEqual(basic.delta, full.delta)
        self.assertEqual(basic.gamma, full.gamma)
        selected = engine.price_with_greeks(self.option, context, "vega")
        self.assertEqual(selected.price, value)
        self.assertEqual(selected.vega, full.vega)
        self.assertIsNone(selected.delta)
        self.assertIsNone(selected.gamma)
        pair = engine.price_with_greeks(self.option, context, ["rho", "vega", "rho"])
        self.assertEqual(pair.rho, full.rho)
        self.assertEqual(pair.vega, full.vega)
        self.assertIsNone(pair.speed)
        generated = engine.price_with_greeks(
            self.option, context, (greek for greek in ("delta",))
        )
        self.assertEqual(generated.delta, full.delta)
        self.assertIsNone(generated.gamma)
        time = (self.option.expiry_date - date(2025, 6, 1)).days / 365
        d1 = (0.05 - 0.02 + 0.2**2 / 2) * time / (0.2 * math.sqrt(time))
        self.assertAlmostEqual(
            basic.delta, math.exp(-0.02 * time) * (1 + math.erf(d1 / math.sqrt(2))) / 2
        )
        self.assertAlmostEqual(
            basic.gamma,
            math.exp(-0.02 * time - d1**2 / 2)
            / (100 * 0.2 * math.sqrt(2 * math.pi * time)),
        )
        for name in (
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
            with self.subTest(measure=name):
                self.assertAlmostEqual(
                    getattr(full, name), getattr(numerical, name), delta=1e-5
                )
        for name in (
            "speed",
            "theta",
            "charm",
            "color",
            "vega",
            "vanna",
            "zomma",
            "rho",
        ):
            with self.subTest(measure=name):
                self.assertIsNone(getattr(basic, name))

    def test_greek_iterators_initialize_once_and_preserve_errors(self):
        engine = AnalyticVanillaEngine()
        for phase in ("initialization", "advance"):
            for error_type in (TypeError, RuntimeError, MemoryError):
                failure = error_type("Greek iteration failed")

                class FailingGreeks:
                    iter_calls = 0
                    first = True

                    def __iter__(self):
                        self.iter_calls += 1
                        if phase == "initialization":
                            raise failure
                        return self

                    def __next__(self):
                        if self.first:
                            self.first = False
                            return "delta"
                        raise failure

                values = FailingGreeks()
                with self.subTest(phase=phase, error=error_type.__name__):
                    with self.assertRaises(error_type) as caught:
                        engine.price_with_greeks(self.option, self.context, values)
                    self.assertEqual(values.iter_calls, 1)
                    if phase == "initialization" and error_type is TypeError:
                        self.assertEqual(
                            str(caught.exception),
                            "greeks must be a Greek name or an iterable of Greek names",
                        )
                    else:
                        self.assertIs(caught.exception, failure)

    def test_joint_pricing_requires_explicit_greeks_and_valid_shift_settings(self):
        engine = AnalyticVanillaEngine()
        with self.assertRaises(TypeError):
            engine.price_with_greeks(self.option, self.context)
        for level in (True, 0, None):
            with self.subTest(level=level), self.assertRaises(TypeError):
                engine.price_with_greeks(self.option, self.context, level)
        with self.assertRaises(ValueError):
            engine.price_with_greeks(self.option, self.context, "basic")
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(self.option, self.context, [])
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        with self.assertRaises(ValueError):
            engine.price_with_greeks(self.option, self.context, ["price"])
        with self.assertRaises(TypeError):
            engine.price_with_greeks(self.option, self.context, ["delta", 1])
        with self.assertRaises(TypeError):
            engine.price_with_greeks(
                self.option, self.context, "delta", all_greeks=True
            )
        with self.assertRaises(TypeError):
            engine.price_with_greeks(self.option, self.context, all_greeks=1)
        with self.assertRaises(TypeError):
            engine.price_with_greeks(
                self.option, self.context, ["delta", "gamma"], spot_shift=True
            )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(
                self.option, self.context, all_greeks=True, spot_shift=0
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )

    def test_joint_pricing_at_expiry_has_no_greeks(self):
        context = PricingContext(
            model_parameters=self.parameters,
            spot_price=110,
            valuation_time=self.option.expiry_date,
        )
        for engine in (
            AnalyticVanillaEngine(),
            pricing.FiniteDifferenceVanillaEngine(),
            pricing.MonteCarloVanillaEngine(path_count=64, step_count=3, seed=73),
        ):
            self.assertEqual(engine.price(self.option, context), 10)
            for kwargs in ({"greeks": "delta"}, {"all_greeks": True}):
                with self.subTest(engine=type(engine).__name__, request=kwargs):
                    result = engine.price_with_greeks(self.option, context, **kwargs)
                    self.assertEqual(result.price, 10)
                    for name in (
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
                        self.assertIsNone(getattr(result, name))
            numerical = calculate_numerical_greeks(engine, self.option, context)
            self.assertEqual(numerical.price, 10)
            self.assertIsNone(numerical.delta)
            self.assertIsNone(numerical.vega)

    def test_joint_monte_carlo_reuses_explicit_seed_without_changing_settings(self):
        engine = pricing.MonteCarloVanillaEngine(path_count=512, step_count=5, seed=73)
        context = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 6, 1),
        )
        first = engine.price_with_greeks(self.option, context, all_greeks=True)
        second = engine.price_with_greeks(self.option, context, all_greeks=True)
        basic = engine.price_with_greeks(self.option, context, ["delta", "gamma"])
        self.assertEqual(first.price, engine.price(self.option, context))
        self.assertEqual(first.delta, basic.delta)
        self.assertEqual(first.gamma, basic.gamma)
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
            with self.subTest(measure=name):
                self.assertTrue(math.isfinite(getattr(first, name)))
                self.assertEqual(getattr(first, name), getattr(second, name))
        self.assertEqual(engine.seed, 73)

    def test_domain_values_have_value_equality_and_readable_representations(self):
        same_parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        same_option = EuropeanOption(
            option_type="call",
            strike=100.0,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        self.assertEqual(self.parameters, same_parameters)
        self.assertEqual(self.option, same_option)
        self.assertNotEqual(
            self.option,
            EuropeanOption(
                option_type="put",
                strike=100.0,
                effective_date=date(2025, 1, 1),
                expiry_date=date(2026, 1, 1),
            ),
        )
        with self.assertRaises(TypeError):
            hash(self.parameters)

        schedule = fixed_interval_schedule(
            start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10
        )
        same_schedule = fixed_interval_schedule(
            start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10
        )
        self.assertEqual(schedule, same_schedule)

        note_terms = {
            "coupon_rate": 0.1,
            "initial_spot": 100,
            "knock_in_level": 80,
            "knock_out_level": 105,
            "observation_dates": [date(2026, 1, 1)],
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        note = standard_snowball(**note_terms)
        self.assertEqual(note, standard_snowball(**note_terms))

        result = AnalyticVanillaEngine().price_with_greeks(
            self.option, self.context, all_greeks=True
        )
        cases = (
            (self.parameters, "BlackScholesMertonParameters(", "volatility=0.2"),
            (self.option, "EuropeanOption(", "strike=100.0"),
            (schedule, "ObservationSchedule(", "dates=["),
            (
                market.weekdays_calendar(),
                "TradingCalendar(",
                "trading_days_per_year=252",
            ),
            (self.context, "PricingContext(", "spot_price=100.0"),
            (note, "SnowballOption(", "knock_out_coupon_rates=[0.1]"),
            (
                pricing.FiniteDifferenceVanillaEngine(),
                "FiniteDifferenceVanillaEngine(",
                "asset_step_count=",
            ),
            (result, "PricingResult(", "price="),
        )
        for value, prefix, field in cases:
            with self.subTest(type=type(value).__name__):
                representation = repr(value)
                self.assertTrue(representation.startswith(prefix))
                self.assertIn(field, representation)

    def test_snowball_implied_coupon_uses_explicit_quote_convention(self):
        terms = {
            "initial_spot": 100,
            "knock_in_level": 70,
            "observation_dates": [date(2025, 7, 1), date(2026, 1, 1)],
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        standard = standard_snowball(coupon_rate=0.10, knock_out_level=105, **terms)
        both_down = both_down_snowball(
            initial_coupon_rate=0.10,
            coupon_rate_decrement=0.01,
            initial_knock_out_level=110,
            knock_out_level_decrement=5,
            **terms,
        )
        dual = dual_coupon_snowball(
            knock_out_coupon_rate=0.10,
            maturity_coupon_rate=0.03,
            knock_out_level=105,
            **terms,
        )
        target_standard = standard_snowball(
            coupon_rate=0.12, knock_out_level=105, **terms
        )
        target_both_down = both_down_snowball(
            initial_coupon_rate=0.12,
            coupon_rate_decrement=0.01,
            initial_knock_out_level=110,
            knock_out_level_decrement=5,
            **terms,
        )
        target_dual = dual_coupon_snowball(
            knock_out_coupon_rate=0.12,
            maturity_coupon_rate=0.03,
            knock_out_level=105,
            **terms,
        )

        linked = "shift_maturity_coupon"
        fixed = "preserve_maturity_coupon"
        engine = pricing.FiniteDifferenceSnowballEngine(
            asset_step_count=40, time_step_count=40
        )
        observed_price = engine.price(target_standard, self.context)
        with self.assertRaises(TypeError):
            implied_coupon(engine, standard, self.context, observed_price)
        self.assertFalse(hasattr(standard, "with_quoted_coupon_rate"))

        cases = (
            (standard, target_standard, linked),
            (both_down, target_both_down, linked),
            (dual, target_dual, fixed),
        )
        for instrument, target, convention in cases:
            with self.subTest(instrument=instrument):
                implied = implied_coupon(
                    engine,
                    instrument,
                    self.context,
                    engine.price(target, self.context),
                    quote_convention=convention,
                    price_tolerance=1e-6,
                    parameter_tolerance=1e-6,
                )
                self.assertAlmostEqual(implied, 0.12, places=5)

    def test_negative_binary_snowball_coupon_can_be_implied(self):
        effective = date(2025, 1, 1)
        expiry = date(2026, 1, 1)
        option = BinarySnowballOption(
            knock_out_coupon_rates=[-0.1],
            maturity_coupon_rate=-0.1,
            knock_out_levels=[1],
            observation_dates=[expiry],
            effective_date=effective,
            expiry_date=expiry,
        )
        context = PricingContext(
            model_parameters=self.parameters, spot_price=100, valuation_time=effective
        )
        engine = pricing.MonteCarloBinarySnowballEngine(path_count=32, seed=73)
        price = engine.price(option, context)
        implied = implied_coupon(
            engine,
            option,
            context,
            price,
            quote_convention="preserve_maturity_coupon",
            lower_bound=-0.2,
            upper_bound=0.2,
        )
        self.assertAlmostEqual(implied, -0.1, delta=1e-7)

    def test_implied_parameters_reject_known_unidentifiable_states(self):
        expiry = date(2026, 1, 1)
        expired = PricingContext(
            model_parameters=self.parameters,
            spot_price=110,
            valuation_time=expiry,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            implied_volatility(AnalyticVanillaEngine(), self.option, expired, 10)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION
        )

        note = BinarySnowballOption(
            knock_out_coupon_rates=[0.1, 0.1],
            maturity_coupon_rate=0.05,
            knock_out_levels=[120, 120],
            observation_dates=[date(2025, 7, 1), expiry],
            barrier_state="knocked_out",
            effective_date=date(2025, 1, 1),
            expiry_date=expiry,
        )
        engine = pricing.MonteCarloBinarySnowballEngine(path_count=32, seed=1)
        for solve in (
            lambda: implied_volatility(engine, note, expired, 0),
            lambda: implied_coupon(
                engine,
                note,
                expired,
                0,
                quote_convention="shift_maturity_coupon",
            ),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                solve()
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION
            )

    def test_binary_snowball_rejects_knock_in_history(self):
        expiry = date(2026, 1, 1)
        terms = {
            "knock_out_coupon_rates": [0.05],
            "maturity_coupon_rate": 0.05,
            "knock_out_levels": [110],
            "observation_dates": [expiry],
            "effective_date": date(2025, 1, 1),
            "expiry_date": expiry,
        }
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BinarySnowballOption(**terms, barrier_state="knocked_in")
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        self.assertEqual(
            BinarySnowballOption(**terms, barrier_state="knocked_out").barrier_state,
            "knocked_out",
        )

    def test_sequence_conversion_preserves_iterator_initialization_errors(self):
        expiry = date(2026, 1, 1)
        terms = {
            "knock_out_coupon_rates": [0.05],
            "maturity_coupon_rate": 0.05,
            "knock_out_levels": [110],
            "observation_dates": [expiry],
            "effective_date": date(2025, 1, 1),
            "expiry_date": expiry,
        }
        for field in ("observation_dates", "knock_out_levels"):
            for error_type in (RuntimeError, MemoryError):

                class FailingIterable:
                    def __iter__(self, error_type=error_type):
                        raise error_type("iterator initialization failed")

                with self.subTest(field=field, error=error_type.__name__):
                    with self.assertRaises(error_type) as error:
                        BinarySnowballOption(**{**terms, field: FailingIterable()})
                    self.assertEqual(
                        str(error.exception), "iterator initialization failed"
                    )
            with (
                self.subTest(field=field, kind="non-iterable"),
                self.assertRaises(TypeError),
            ):
                BinarySnowballOption(**{**terms, field: 42})
            with self.subTest(field=field, kind="generator"):
                note = BinarySnowballOption(
                    **{**terms, field: (value for value in terms[field])}
                )
                self.assertEqual(getattr(note, field), terms[field])

    def test_structured_history_is_explicit_after_observation(self):
        terms = {
            "knock_out_coupon_rates": [0.05, 0.05],
            "maturity_coupon_rate": 0.05,
            "knock_out_levels": [110, 110],
            "observation_dates": [date(2025, 1, 2), date(2025, 1, 6)],
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2025, 1, 6),
        }
        engine = pricing.MonteCarloBinarySnowballEngine(path_count=32, seed=1)
        later = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 1, 3),
        )
        missing = BinarySnowballOption(**terms)
        self.assertIsNone(missing.barrier_state)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(missing, later)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        self.assertGreater(
            engine.price(BinarySnowballOption(**terms, barrier_state="none"), later), 0
        )
        self.assertFalse(hasattr(missing, "initial_spot"))
        self.assertFalse(hasattr(missing, "upper_strike"))
        self.assertFalse(hasattr(missing, "lower_strike"))

    def test_public_api_has_targeted_docstrings(self):
        self.assertFalse(hasattr(market, "sse_calendar_data_first_year"))
        self.assertFalse(hasattr(market, "sse_calendar_data_last_year"))
        self.assertFalse(hasattr(market, "sse_calendar_data_version"))
        self.assertEqual(
            market.sse_calendar().is_trading_day(date(2100, 2, 8)),
            market.weekdays_calendar().is_trading_day(date(2100, 2, 8)),
        )
        self.assertIn("validated", BlackScholesMertonParameters.__doc__.lower())
        self.assertIn("weekdays", market.weekdays_calendar.__doc__.lower())
        self.assertIn(
            "reversed", market.TradingCalendar.trading_days_between.__doc__.lower()
        )
        self.assertIn("price", AnalyticVanillaEngine.price.__doc__.lower())
        self.assertIn("all_greeks: bool = False", AnalyticVanillaEngine.price_with_greeks.__doc__)
        self.assertIn("greek", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("percentage point", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("calendar day", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("never a zero sentinel", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("absolute", calculate_numerical_greeks.__doc__.lower())
        self.assertIn("boundary", calculate_numerical_greeks.__doc__.lower())
        self.assertIn("solve", implied_volatility.__doc__.lower())
        self.assertIn("solve", implied_coupon.__doc__.lower())
        self.assertIn("start is excluded", fixed_interval_schedule.__doc__.lower())
        self.assertIn("duplicate", fixed_interval_schedule.__doc__.lower())
        self.assertIn("not guaranteed", monthly_schedule.__doc__.lower())
        self.assertIn("clamped", monthly_schedule.__doc__.lower())

    def test_schedule_builders_expose_bounded_following_behavior(self):
        self.assertEqual(
            fixed_interval_schedule(
                start=date(2025, 1, 3), end=date(2025, 1, 7), interval_days=1
            ).dates,
            [date(2025, 1, 6), date(2025, 1, 7)],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 1), end=date(2025, 3, 1), lock_up_months=1
            ).dates,
            [date(2025, 2, 3)],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 31), end=date(2025, 4, 30), lock_up_months=1
            ).dates,
            [date(2025, 2, 28), date(2025, 3, 31), date(2025, 4, 30)],
        )

    def test_monthly_schedule_does_not_wrap_extreme_lock_up(self):
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 1),
                end=date(2025, 12, 31),
                lock_up_months=786433,
            ).dates,
            [],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(9999, 11, 1),
                end=date(9999, 12, 31),
                lock_up_months=2,
            ).dates,
            [],
        )

    def test_weekdays_calendar_is_the_explicit_default(self):
        self.assertFalse(hasattr(market, "exchange_calendar"))
        calendar = market.weekdays_calendar()
        self.assertEqual(calendar.trading_days_per_year, 252)
        self.assertFalse(calendar.is_trading_day(date(2025, 1, 4)))
        self.assertFalse(self.context.calendar.is_trading_day(date(2025, 1, 4)))
        self.assertEqual(
            calendar.trading_days_between(date(2025, 1, 4), date(2025, 1, 6)), 0
        )
        for operation in (
            calendar.trading_days_between,
            calendar.trading_year_fraction,
        ):
            with (
                self.subTest(operation=operation.__name__),
                self.assertRaises(kiyosi.KiyosiError) as error,
            ):
                operation(date(2025, 1, 6), date(2025, 1, 4))
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE
            )

    def test_nominal_weekend_expiry_requires_explicit_adjustment(self):
        calendar = market.weekdays_calendar()
        nominal = date(2025, 1, 5)
        self.assertEqual(calendar.adjust(nominal, "following"), date(2025, 1, 6))
        self.assertEqual(calendar.adjust(nominal, "preceding"), date(2025, 1, 3))
        context = PricingContext(
            model_parameters=self.parameters,
            spot_price=110,
            valuation_time=date(2025, 1, 3),
            calendar=calendar,
        )
        terms = {
            "strike": 100,
            "knock_out_level": 1000,
            "daily_quantity": 0,
            "acceleration_factor": 1,
            "accumulated_quantity": 1,
            "effective_date": date(2025, 1, 3),
        }
        nominal_option = Accumulator(**terms, expiry_date=nominal)
        adjusted_option = Accumulator(
            **terms, expiry_date=calendar.adjust(nominal, "following")
        )
        for engine in (
            pricing.MonteCarloAccumulatorEngine(path_count=32, seed=7),
            pricing.FiniteDifferenceAccumulatorEngine(),
        ):
            with self.subTest(engine=type(engine).__name__):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(nominal_option, context)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_DATE
                )
                self.assertTrue(math.isfinite(engine.price(adjusted_option, context)))
        note_terms = {
            "knock_out_coupon_rates": [0],
            "maturity_coupon_rate": 0.01,
            "knock_out_levels": [200],
            "observation_dates": [date(2025, 1, 3)],
            "effective_date": date(2025, 1, 3),
        }
        nominal_note = BinarySnowballOption(**note_terms, expiry_date=nominal)
        adjusted_note = BinarySnowballOption(
            **note_terms, expiry_date=calendar.adjust(nominal, "following")
        )
        note_context = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 1, 3),
            calendar=calendar,
        )
        for engine in (
            pricing.MonteCarloBinarySnowballEngine(path_count=32, seed=7),
            pricing.FiniteDifferenceBinarySnowballEngine(),
        ):
            with self.subTest(engine=type(engine).__name__):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(nominal_note, note_context)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_DATE
                )
                self.assertTrue(
                    math.isfinite(engine.price(adjusted_note, note_context))
                )

    def test_native_domain_errors_expose_categories(self):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.0
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_VOLATILITY
        )

    def test_accumulator_errors_identify_rejected_terms(self):
        terms = {
            "strike": 100.0,
            "knock_out_level": 110.0,
            "daily_quantity": 1.0,
            "acceleration_factor": 2.0,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        cases = (
            (
                "strike",
                0.0,
                kiyosi.ErrorCategory.INVALID_STRIKE,
                "strike must be finite and positive",
            ),
            (
                "knock_out_level",
                0.0,
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                "knock-out level must be finite and positive",
            ),
            (
                "daily_quantity",
                -1.0,
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                "daily quantity must be finite and non-negative",
            ),
            (
                "acceleration_factor",
                -1.0,
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                "acceleration factor must be finite and non-negative",
            ),
            (
                "accumulated_quantity",
                -1.0,
                kiyosi.ErrorCategory.INVALID_PARAMETER,
                "accumulated quantity must be finite and non-negative",
            ),
        )
        for field, value, category, message in cases:
            with (
                self.subTest(field=field),
                self.assertRaises(kiyosi.KiyosiError) as error,
            ):
                Accumulator(**{**terms, field: value})
            self.assertEqual(error.exception.category, category)
            self.assertEqual(str(error.exception), message)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            Accumulator(
                **{
                    **terms,
                    "effective_date": terms["expiry_date"],
                    "expiry_date": terms["effective_date"],
                }
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE
        )
        self.assertEqual(
            str(error.exception), "expiry date must not precede the effective date"
        )

    def test_contract_date_errors_have_shared_categories(self):
        earlier = date(2025, 1, 1)
        later = date(2026, 1, 1)
        factories = (
            (
                "European",
                lambda start, end: EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_date=start,
                    expiry_date=end,
                ),
            ),
            (
                "Asian",
                lambda start, end: ArithmeticAveragePriceOption(
                    option_type="call",
                    strike=100,
                    averaging_start_date=start,
                    effective_date=start,
                    expiry_date=end,
                ),
            ),
            (
                "Barrier",
                lambda start, end: BarrierOption(
                    option_type="call",
                    strike=100,
                    barrier_level=120,
                    barrier_type="up_and_out",
                    effective_date=start,
                    expiry_date=end,
                ),
            ),
            (
                "Accumulator",
                lambda start, end: Accumulator(
                    strike=100,
                    knock_out_level=110,
                    daily_quantity=1,
                    acceleration_factor=2,
                    effective_date=start,
                    expiry_date=end,
                ),
            ),
            (
                "Snowball",
                lambda start, end: BinarySnowballOption(
                    knock_out_coupon_rates=[0.1],
                    maturity_coupon_rate=0.05,
                    knock_out_levels=[110],
                    observation_dates=[end],
                    effective_date=start,
                    expiry_date=end,
                ),
            ),
        )
        for name, factory in factories:
            with (
                self.subTest(product=name),
                self.assertRaises(kiyosi.KiyosiError) as error,
            ):
                factory(later, earlier)
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE
            )

        for schedule in (
            lambda: fixed_interval_schedule(start=later, end=earlier, interval_days=1),
            lambda: monthly_schedule(start=later, end=earlier, lock_up_months=1),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                schedule()
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE
            )

        with self.assertRaises(kiyosi.KiyosiError) as error:
            ArithmeticAveragePriceOption(
                option_type="call",
                strike=100,
                averaging_start_date=earlier - timedelta(days=1),
                effective_date=earlier,
                expiry_date=later,
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_SCHEDULE
        )

    def test_accumulator_knock_out_settles_existing_quantity(self):
        effective_date = date(2025, 1, 1)
        option = Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
            effective_date=effective_date,
            expiry_date=date(2025, 1, 6),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.05, volatility=0.2
            ),
            spot_price=110,
            valuation_time=effective_date,
            calendar=market.all_days_calendar(),
        )
        engines = (
            pricing.FiniteDifferenceAccumulatorEngine(
                asset_step_count=400, asset_upper_boundary=400
            ),
            pricing.MonteCarloAccumulatorEngine(path_count=64, seed=73),
        )
        for engine in engines:
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(engine.price(option, context), 30.0, delta=1e-6)

    def test_phoenix_terminal_coupon_is_paid_once(self):
        effective_date = date(2025, 1, 6)
        expiry_date = effective_date + timedelta(days=91)
        option = PhoenixOption(
            coupon_rate=0.0025,
            initial_spot=100,
            knock_in_level=80,
            knock_out_levels=[120],
            coupon_barrier_levels=[90],
            upper_strike=100,
            lower_strike=60,
            observation_dates=[expiry_date],
            knock_in_observation_mode="every_trading_day",
            effective_date=effective_date,
            expiry_date=expiry_date,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=1e-8
            ),
            spot_price=100,
            valuation_time=effective_date,
            calendar=market.all_days_calendar(),
        )
        engines = (
            pricing.FiniteDifferencePhoenixEngine(
                asset_step_count=400,
                time_step_count=1600,
                asset_upper_boundary=400,
            ),
            pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
        )
        for engine in engines:
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(
                    engine.price(option, context), 1 + 0.0025 * 91 / 365, delta=1e-6
                )

    def test_phoenix_annual_coupon_is_scale_and_frequency_invariant(self):
        start, middle, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        for scale in (100, 1000):
            for dates in ([end], [middle, end]):
                option = PhoenixOption(
                    coupon_rate=0.08,
                    initial_spot=scale,
                    knock_in_level=0.5 * scale,
                    knock_out_levels=[2 * scale] * len(dates),
                    coupon_barrier_levels=[0.9 * scale] * len(dates),
                    upper_strike=scale,
                    lower_strike=0,
                    observation_dates=dates,
                    knock_in_observation_mode="at_expiry",
                    effective_date=start,
                    expiry_date=end,
                )
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0,
                        dividend_yield=0,
                        volatility=1e-8,
                    ),
                    spot_price=scale,
                    valuation_time=start,
                    calendar=market.all_days_calendar(),
                )
                for engine in (
                    pricing.FiniteDifferencePhoenixEngine(
                        asset_step_count=400,
                        time_step_count=400,
                        asset_upper_boundary=4 * scale,
                    ),
                    pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
                ):
                    with self.subTest(
                        scale=scale, dates=dates, engine=type(engine).__name__
                    ):
                        self.assertAlmostEqual(
                            engine.price(option, context), 1.08, delta=1e-6
                        )

    def test_numeric_and_date_boundaries_are_checked(self):
        with self.assertRaises(TypeError):
            EuropeanOption(
                option_type="call",
                strike="100",
                effective_date=date(2025, 1, 1),
                expiry_date=date(2026, 1, 1),
            )
        with self.assertRaises(TypeError):
            BlackScholesMertonParameters(
                risk_free_rate=True, dividend_yield=0.02, volatility=0.2
            )
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceVanillaEngine(asset_step_count=1.5)
        with self.assertRaises(TypeError):
            pricing.MonteCarloVanillaEngine(seed=True)
        with self.assertRaises(OverflowError):
            pricing.FiniteDifferenceVanillaEngine(asset_step_count=2**40)
        with self.assertRaises(OverflowError):
            BlackScholesMertonParameters(
                risk_free_rate=10**400, dividend_yield=0.02, volatility=0.2
            )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BlackScholesMertonParameters(
                risk_free_rate=float("inf"), dividend_yield=0.02, volatility=0.2
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_RISK_FREE_RATE
        )
        for seed in (-1, 2**64):
            with self.subTest(seed=seed), self.assertRaises(OverflowError):
                pricing.MonteCarloVanillaEngine(seed=seed)
        maximum_seed = 2**64 - 1
        self.assertEqual(
            pricing.MonteCarloVanillaEngine(seed=maximum_seed).seed, maximum_seed
        )
        with self.assertRaises(TypeError):
            PricingContext(
                model_parameters=self.parameters,
                spot_price=100,
                valuation_time=datetime.fromisoformat("2025-01-01T00:00:00"),
            )
        aware = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=datetime(2025, 1, 1, 8, tzinfo=UTC),
        )
        self.assertIsNotNone(AnalyticVanillaEngine().price(self.option, aware))

    def test_monte_carlo_backend_defaults_and_round_trips(self):
        default = pricing.MonteCarloVanillaEngine()
        cuda = pricing.MonteCarloVanillaEngine(backend="cuda")

        self.assertEqual(default.backend, "cpu")
        self.assertEqual(cuda.backend, "cuda")
        self.assertEqual(
            repr(cuda),
            "MonteCarloVanillaEngine(path_count=100000, step_count=50, "
            "seed=None, backend='cuda')",
        )
        self.assertNotEqual(
            kiyosi.ErrorCategory.BACKEND_UNAVAILABLE,
            kiyosi.ErrorCategory.BACKEND_FAILURE,
        )
        self.assertNotEqual(
            kiyosi.ErrorCategory.BACKEND_FAILURE,
            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
        )
        for value in (True, 1):
            with self.subTest(value=value), self.assertRaises(TypeError):
                pricing.MonteCarloVanillaEngine(backend=value)
        with self.assertRaises(ValueError):
            pricing.MonteCarloVanillaEngine(backend="CUDA")

        for engine_type in (
            pricing.MonteCarloAccumulatorEngine,
            pricing.MonteCarloPhoenixEngine,
            pricing.MonteCarloSnowballEngine,
            pricing.MonteCarloBinarySnowballEngine,
            pricing.MonteCarloTernarySnowballEngine,
        ):
            with self.subTest(engine=engine_type.__name__):
                default = engine_type()
                cuda = engine_type(backend="cuda")
                self.assertEqual(default.backend, "cpu")
                self.assertEqual(cuda.backend, "cuda")
                self.assertEqual(
                    repr(cuda),
                    f"{engine_type.__name__}(path_count=20000, seed=1, backend='cuda')",
                )
                with self.assertRaises(TypeError):
                    engine_type(backend=True)

    def test_trading_day_monte_carlo_accepts_optional_seed(self):
        for engine_type in (
            pricing.MonteCarloAccumulatorEngine,
            pricing.MonteCarloSnowballEngine,
            pricing.MonteCarloBinarySnowballEngine,
            pricing.MonteCarloTernarySnowballEngine,
            pricing.MonteCarloPhoenixEngine,
        ):
            with self.subTest(engine=engine_type.__name__):
                self.assertEqual(engine_type().seed, 1)
                self.assertIsNone(engine_type(seed=None).seed)
                self.assertEqual(engine_type(seed=73).seed, 73)
                self.assertEqual(engine_type(seed=2**64 - 1).seed, 2**64 - 1)
                with self.assertRaises(TypeError):
                    engine_type(seed=True)
                for invalid in (-1, 2**64):
                    with self.assertRaises(OverflowError):
                        engine_type(seed=invalid)

    @unittest.skipUnless(
        os.environ.get("KIYOSI_TEST_CUDA") == "disabled",
        "requires a job built with KIYOSI_ENABLE_CUDA=OFF",
    )
    def test_cuda_backend_unavailable_is_deferred_and_categorized(self):
        engine = pricing.MonteCarloVanillaEngine(
            path_count=20,
            step_count=2,
            seed=42,
            backend="cuda",
        )
        self.assertEqual(engine.backend, "cuda")

        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(self.option, self.context)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.BACKEND_UNAVAILABLE
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(self.option, self.context, all_greeks=True)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.BACKEND_UNAVAILABLE
        )

    @unittest.skipUnless(
        os.environ.get("KIYOSI_TEST_CUDA") == "disabled",
        "requires a job built with KIYOSI_ENABLE_CUDA=OFF",
    )
    def test_american_cuda_backend_unavailable_is_deferred_and_categorized(self):
        option = AmericanOption(
            option_type="put",
            strike=100.0,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=20,
            step_count=3,
            seed=42,
            backend="cuda",
        )

        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(option, self.context)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.BACKEND_UNAVAILABLE
        )

    @unittest.skipUnless(
        os.environ.get("KIYOSI_TEST_CUDA") == "enabled",
        "requires a CUDA-enabled build and a usable GPU",
    )
    def test_cuda_prices_pre_expiry_known_cashflow(self):
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.0, volatility=1e-8
            ),
            spot_price=100.0,
            valuation_time=date(2025, 1, 1),
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=4096, step_count=3, seed=42, backend="cuda"
        )
        # With negligible volatility and no dividends, neither call exercises early.
        # One year's deterministic growth gives S - K exp(-rT).
        expected = 100.0 * (1.0 - math.exp(-0.05))
        for option_type in (EuropeanOption, AmericanOption):
            with self.subTest(option=option_type.__name__):
                option = option_type(
                    option_type="call", strike=100.0,
                    effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
                )
                self.assertAlmostEqual(engine.price(option, context), expected, delta=1e-6)

    def test_american_cuda_validates_before_backend_and_prices_expiry(self):
        option = AmericanOption(
            option_type="put",
            strike=100.0,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        for path_count, step_count in ((0, 50), (20, 2)):
            with self.subTest(path_count=path_count, step_count=step_count):
                engine = pricing.MonteCarloVanillaEngine(
                    path_count=path_count,
                    step_count=step_count,
                    seed=42,
                    backend="cuda",
                )
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(option, self.context)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                )

        expiry_date = date(2026, 1, 1)
        expiry_option = AmericanOption(
            option_type="put",
            strike=100.0,
            effective_date=expiry_date,
            expiry_date=expiry_date,
        )
        expiry_context = PricingContext(
            model_parameters=self.parameters,
            spot_price=90.0,
            valuation_time=expiry_date,
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=20,
            step_count=3,
            seed=42,
            backend="cuda",
        )
        self.assertEqual(engine.price(expiry_option, expiry_context), 10.0)

    def test_temporal_accessors_preserve_date_and_timestamp_semantics(self):
        averaging_start_date = date(2025, 2, 1)
        average = GeometricAveragePriceOption(
            option_type="call",
            strike=100,
            averaging_start_date=averaging_start_date,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        self.assertIs(type(average.averaging_start_date), date)
        self.assertEqual(average.averaging_start_date, averaging_start_date)

        expiry_date = date(2026, 1, 1)
        note = standard_snowball(
            coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            knock_out_level=105,
            observation_dates=[expiry_date],
            effective_date=date(2025, 1, 1),
            expiry_date=expiry_date,
        )
        self.assertEqual(note.observation_dates, [expiry_date])
        self.assertIs(type(note.effective_date), date)
        self.assertIs(type(note.expiry_date), date)

        local_time = datetime(
            2025, 1, 1, 8, 9, 10, 123456, tzinfo=timezone(timedelta(hours=8))
        )
        context = PricingContext(
            model_parameters=self.parameters, spot_price=100, valuation_time=local_time
        )
        self.assertEqual(
            context.valuation_time,
            datetime(2025, 1, 1, 0, 9, 10, 123456, tzinfo=UTC),
        )
        midnight = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 1, 1),
        )
        self.assertEqual(midnight.valuation_time, datetime(2025, 1, 1, tzinfo=UTC))
        for value in (date(1600, 1, 1), date(2500, 1, 1), date(9999, 1, 1)):
            with self.subTest(value=value):
                context = PricingContext(
                    model_parameters=self.parameters,
                    spot_price=100,
                    valuation_time=value,
                )
                self.assertEqual(
                    context.valuation_time,
                    datetime.combine(value, datetime.min.time(), UTC),
                )
        latest = datetime(9999, 12, 31, 23, 59, 59, 999999, tzinfo=UTC)
        context = PricingContext(
            model_parameters=self.parameters, spot_price=100, valuation_time=latest
        )
        self.assertEqual(context.valuation_time, latest)

    def test_date_subclasses_use_stored_calendar_components(self):
        class ShadowedDate(date):
            year = property(lambda self: 67562)
            month = property(lambda self: 257)
            day = property(lambda self: 257)

        start, end = ShadowedDate(2025, 1, 6), ShadowedDate(2026, 1, 6)
        option = EuropeanOption(option_type="call", strike=100, effective_date=start, expiry_date=end)
        self.assertEqual(option.effective_date, date(2025, 1, 6))
        self.assertEqual(option.expiry_date, date(2026, 1, 6))
        note = standard_snowball(
            coupon_rate=0.1, initial_spot=100, knock_in_level=80, knock_out_level=105,
            observation_dates=[end], effective_date=start, expiry_date=end,
        )
        self.assertEqual(note.observation_dates, [date(2026, 1, 6)])
        context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=start)
        self.assertEqual(context.valuation_time, datetime(2025, 1, 6, tzinfo=UTC))

    def test_datetime_subclasses_use_stored_components_and_builtin_utc_conversion(self):
        class ShadowedTimestamp(datetime):
            year = property(lambda self: 67562)
            month = property(lambda self: 257)
            day = property(lambda self: 257)
            hour = property(lambda self: 25)
            minute = property(lambda self: 61)
            second = property(lambda self: 61)
            microsecond = property(lambda self: 1000001)

            def utcoffset(self):
                return None

            def astimezone(self, tz=None):
                raise AssertionError("overridden astimezone must not be used")

        local = ShadowedTimestamp(2025, 1, 6, 8, 9, 10, 123456, tzinfo=timezone(timedelta(hours=8)))
        context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=local)
        self.assertEqual(context.valuation_time, datetime(2025, 1, 6, 0, 9, 10, 123456, tzinfo=UTC))
        self.assertEqual(context.valuation_date, date(2025, 1, 6))

        class PretendAware(datetime):
            def utcoffset(self):
                return timedelta(0)

        with self.assertRaises(TypeError):
            PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=PretendAware(2025, 1, 6))

    def test_observation_dates_are_independent_copies(self):
        effective = date(2025, 1, 1)
        expiry = date(2025, 1, 31)
        dates = [date(2025, 1, 13), date(2025, 1, 21), expiry]
        schedule = fixed_interval_schedule(start=effective, end=expiry, interval_days=10)
        terms = dict(
            effective_date=effective,
            expiry_date=expiry,
            barrier_level=120,
            observation_mode="scheduled",
            observation_dates=dates,
        )
        barrier = BarrierOption(
            **terms, option_type="call", strike=100, barrier_type="up_and_out"
        )
        binary = cash_binary_barrier_option(
            **terms, option_type="call", strike=100, barrier_type="up_and_out", payout=10
        )
        touch = cash_one_touch_up(**terms, payout=10)
        note = standard_snowball(
            coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            knock_out_level=105,
            observation_dates=dates,
            effective_date=effective,
            expiry_date=expiry,
        )
        for owner, attribute in (
            (schedule, "dates"),
            (barrier, "observation_dates"),
            (binary, "observation_dates"),
            (touch, "observation_dates"),
            (note, "observation_dates"),
        ):
            with self.subTest(owner=type(owner).__name__):
                returned = getattr(owner, attribute)
                self.assertEqual(returned, dates)
                self.assertTrue(all(type(value) is date for value in returned))
                returned.clear()
                self.assertEqual(getattr(owner, attribute), dates)
        iterator = iter(schedule)
        del schedule
        self.assertEqual(list(iterator), dates)
        self.assertEqual(
            list(fixed_interval_schedule(start=effective, end=effective, interval_days=10)),
            [],
        )

    def test_geometric_asian_uses_elapsed_average_and_forward_start(self):
        engine = pricing.AnalyticGeometricAveragePriceEngine()
        effective = date(2025, 1, 1)
        expiry = date(2026, 1, 1)
        context = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 7, 1),
        )

        for start, realized, expected in (
            (effective, 80, 0.005141652127146822),
            (effective, 120, 9.472410353379193),
            (date(2025, 10, 1), 0, 5.064920706779442),
        ):
            with self.subTest(start=start, realized=realized):
                option = GeometricAveragePriceOption(
                    option_type="call",
                    strike=100,
                    averaging_start_date=start,
                    realized_average=realized,
                    effective_date=effective,
                    expiry_date=expiry,
                )
                self.assertAlmostEqual(
                    engine.price(option, context), expected, delta=1e-10
                )

        missing = GeometricAveragePriceOption(
            option_type="call",
            strike=100,
            averaging_start_date=effective,
            effective_date=effective,
            expiry_date=expiry,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(missing, context)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )

        starting = GeometricAveragePriceOption(
            option_type="call",
            strike=100,
            averaging_start_date=date(2025, 7, 1),
            effective_date=effective,
            expiry_date=expiry,
        )
        greeks = engine.price_with_greeks(starting, context, all_greeks=True)
        self.assertIsNotNone(greeks.delta)
        self.assertIsNone(greeks.theta)

        ending = GeometricAveragePriceOption(
            option_type="call",
            strike=100,
            averaging_start_date=effective,
            realized_average=120,
            effective_date=effective,
            expiry_date=expiry,
        )
        near_expiry = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 12, 31),
        )
        at_expiry = PricingContext(
            model_parameters=self.parameters, spot_price=100, valuation_time=expiry
        )
        self.assertAlmostEqual(
            engine.price(ending, near_expiry), 19.937346821828626, delta=1e-10
        )
        self.assertEqual(engine.price(ending, at_expiry), 20)

    def test_single_arithmetic_fixing_has_european_time_value(self):
        effective = date(2025, 1, 1)
        expiry = date(2026, 1, 1)
        asian_engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        vanilla_engine = AnalyticVanillaEngine()
        for option_type in ("call", "put"):
            asian = ArithmeticAveragePriceOption(
                option_type=option_type,
                strike=100,
                averaging_start_date=expiry,
                effective_date=effective,
                expiry_date=expiry,
            )
            vanilla = EuropeanOption(
                option_type=option_type,
                strike=100,
                effective_date=effective,
                expiry_date=expiry,
            )
            for valuation in (effective, date(2025, 12, 31)):
                context = PricingContext(
                    model_parameters=self.parameters,
                    spot_price=100,
                    valuation_time=valuation,
                )
                self.assertEqual(
                    asian_engine.price(asian, context),
                    vanilla_engine.price(vanilla, context),
                )
                if option_type == "call" and valuation == effective:
                    self.assertAlmostEqual(
                        asian_engine.price(asian, context),
                        9.227005508154036,
                        delta=1e-12,
                    )

        fixed = ArithmeticAveragePriceOption(
            option_type="call",
            strike=100,
            averaging_start_date=expiry,
            realized_average=110,
            effective_date=effective,
            expiry_date=expiry,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            asian_engine.price(fixed, self.context)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        at_expiry = PricingContext(
            model_parameters=self.parameters, spot_price=110, valuation_time=expiry
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            asian_engine.price(fixed, at_expiry)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )

    def test_arithmetic_asian_requires_elapsed_average(self):
        effective = date(2025, 1, 1)
        start = date(2025, 2, 1)
        expiry = date(2026, 1, 1)
        missing = ArithmeticAveragePriceOption(
            option_type="call",
            strike=100,
            averaging_start_date=start,
            effective_date=effective,
            expiry_date=expiry,
        )
        known = ArithmeticAveragePriceOption(
            option_type="call",
            strike=100,
            averaging_start_date=start,
            realized_average=110,
            effective_date=effective,
            expiry_date=expiry,
        )
        engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        for valuation in (effective, start):
            context = PricingContext(
                model_parameters=self.parameters,
                spot_price=100,
                valuation_time=valuation,
            )
            self.assertIsInstance(engine.price(missing, context), float)
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine.price(known, context)
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )

        during = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 6, 1),
        )
        at_expiry = PricingContext(
            model_parameters=self.parameters, spot_price=100, valuation_time=expiry
        )
        self.assertIsInstance(engine.price(known, during), float)
        for context in (during, at_expiry):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine.price(missing, context)
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(missing, during, ["delta", "gamma"])
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        self.assertEqual(engine.price(known, at_expiry), 10)

    def test_digital_barrier_schedule_and_analytics(self):
        digital = CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=10,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        self.assertGreater(AnalyticDigitalEngine().price(digital, self.context), 0)
        barrier = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
            barrier_level=80,
            barrier_type="down_and_out",
            rebate=1,
            rebate_timing="at_expiry",
        )
        self.assertGreater(AnalyticBarrierEngine().price(barrier, self.context), 0)
        self.assertEqual(barrier.rebate_timing, "at_expiry")
        self.assertEqual(barrier.observation_mode, "continuous")
        self.assertIn("observation_dates=[]", repr(barrier))
        self.assertFalse(hasattr(barrier, "rebate_payment"))
        self.assertFalse(hasattr(barrier, "observation"))
        self.assertEqual(
            len(
                fixed_interval_schedule(
                    start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10
                )
            ),
            5,
        )
        engine = AnalyticVanillaEngine()
        self.assertIsNotNone(
            calculate_numerical_greeks(engine, self.option, self.context).vega
        )
        observed_price = engine.price(self.option, self.context)
        self.assertAlmostEqual(
            implied_volatility(engine, self.option, self.context, observed_price),
            self.parameters.volatility,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            implied_volatility(
                engine,
                self.option,
                self.context,
                observed_price,
                lower_bound=0.5,
                upper_bound=0.1,
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )

    def test_calculate_numerical_greeks_rejects_invalid_shift_settings(self):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            calculate_numerical_greeks(
                AnalyticVanillaEngine(), self.option, self.context, spot_shift=0.0
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )

    def test_engine_settings_are_validated_when_pricing(self):
        configured_engines = (
            pricing.CoxRossRubinsteinVanillaEngine,
            pricing.FiniteDifferenceVanillaEngine,
            pricing.MonteCarloVanillaEngine,
            pricing.MonteCarloSnowballEngine,
        )
        for engine_type in configured_engines:
            with self.subTest(engine=engine_type.__name__):
                self.assertIn("validated when price() is called", engine_type.__doc__)
                self.assertIn(
                    "validated when price() is called", engine_type.__init__.__doc__
                )

        engine = pricing.FiniteDifferenceVanillaEngine(asset_step_count=0)
        self.assertEqual(engine.asset_step_count, 0)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(self.option, self.context)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(self.option, self.context, ["delta", "gamma"])
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )

    def test_calculate_numerical_greeks_retains_valid_boundary_results(self):
        engine = AnalyticVanillaEngine()

        low_volatility = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                volatility=0.00005,
            ),
            spot_price=100.0,
            valuation_time=date(2025, 1, 1),
        )
        result = calculate_numerical_greeks(engine, self.option, low_volatility)
        self.assertAlmostEqual(result.price, engine.price(self.option, low_volatility))
        self.assertIsNotNone(result.delta)
        self.assertIsNotNone(result.rho)
        self.assertIsNone(result.vega)
        self.assertIsNone(result.vanna)
        self.assertIsNone(result.zomma)

        low_spot = PricingContext(
            model_parameters=self.parameters,
            spot_price=0.005,
            valuation_time=date(2025, 1, 1),
        )
        result = calculate_numerical_greeks(engine, self.option, low_spot)
        self.assertAlmostEqual(result.price, engine.price(self.option, low_spot))
        self.assertIsNotNone(result.vega)
        self.assertIsNotNone(result.theta)
        self.assertIsNotNone(result.rho)
        for measure in ("delta", "gamma", "speed", "charm", "color", "vanna", "zomma"):
            self.assertIsNone(getattr(result, measure))

    def test_touch_factories_require_only_payoff_relevant_terms(self):
        terms = {"effective_date": date(2025, 1, 1), "expiry_date": date(2026, 1, 1)}
        cash = cash_one_touch_up(
            **terms,
            barrier_level=130,
            payout=10,
            settlement_timing="at_hit",
            observation_mode="scheduled",
            observation_dates=[date(2025, 6, 2), terms["expiry_date"]],
        )
        asset = asset_no_touch_down(**terms, barrier_level=70)
        self.assertIsInstance(cash, TouchOption)
        self.assertTrue(cash.is_one_touch)
        self.assertTrue(cash.is_up)
        self.assertEqual(cash.payout, 10)
        self.assertEqual(cash.payoff_type, "cash")
        self.assertEqual(cash.settlement_timing, "at_hit")
        self.assertEqual(
            cash.observation_dates, [date(2025, 6, 2), terms["expiry_date"]]
        )
        self.assertFalse(asset.is_one_touch)
        self.assertFalse(asset.is_up)
        self.assertIsNone(asset.payout)
        self.assertEqual(asset.payoff_type, "asset")
        with self.assertRaises(TypeError):
            cash_one_touch_up(**terms, barrier_level=130, payout=10, strike=100)
        with self.assertRaises(TypeError):
            asset_no_touch_down(**terms, barrier_level=70, payout=10)

        binary = cash_binary_barrier_option(
            **terms,
            option_type="call",
            strike=100,
            barrier_level=80,
            barrier_type="down_and_out",
            payout=10,
        )
        self.assertEqual(binary.option_type, "call")
        self.assertEqual(binary.strike, 100)
        self.assertEqual(binary.payoff_type, "cash")
        engine = AnalyticBinaryBarrierEngine()
        self.assertGreater(engine.price(cash, self.context), 0)
        self.assertGreater(engine.price(binary, self.context), 0)

    def test_scheduled_barrier_has_no_risk_after_last_fixing(self):
        start, expiry = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=self.parameters,
            spot_price=100,
            valuation_time=date(2025, 1, 3),
        )
        barrier = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=expiry,
            barrier_level=120,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_dates=[date(2025, 1, 2)],
            touch_state="untouched",
        )
        vanilla = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=expiry,
        )
        self.assertAlmostEqual(
            AnalyticBarrierEngine().price(barrier, context),
            AnalyticVanillaEngine().price(vanilla, context),
        )

    def test_scheduled_fixing_does_not_recur_at_noon(self):
        start, expiry = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=self.parameters,
            spot_price=125,
            valuation_time=datetime(2025, 1, 2, 12, tzinfo=UTC),
        )
        barrier = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=expiry,
            barrier_level=120,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_dates=[date(2025, 1, 2)],
            touch_state="untouched",
        )
        vanilla = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=expiry,
        )
        expected = AnalyticVanillaEngine().price(vanilla, context)
        self.assertAlmostEqual(
            AnalyticBarrierEngine().price(barrier, context), expected
        )
        self.assertAlmostEqual(
            pricing.FiniteDifferenceBarrierEngine().price(barrier, context),
            expected,
            delta=0.1,
        )

    def test_snowball_skips_weekend_knock_in_at_valuation(self):
        note = standard_snowball(
            coupon_rate=0.12,
            initial_spot=100,
            knock_in_level=80,
            knock_out_level=120,
            observation_dates=[date(2025, 1, 6)],
            barrier_state="none",
            effective_date=date(2025, 1, 1),
            expiry_date=date(2025, 1, 6),
        )
        saturday = datetime(2025, 1, 4, tzinfo=UTC)

        def context(at):
            return PricingContext(
                model_parameters=self.parameters,
                spot_price=79,
                valuation_time=at,
            )

        engine = pricing.MonteCarloSnowballEngine(path_count=20000, seed=1)
        self.assertAlmostEqual(
            engine.price(note, context(saturday)),
            engine.price(note, context(saturday + timedelta(microseconds=1))),
            delta=1e-7,
        )

    def test_time_greeks_omit_stencils_without_barrier_history(self):
        barrier = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
            barrier_level=120,
            barrier_type="up_and_out",
        )
        result = AnalyticBarrierEngine().price_with_greeks(
            barrier,
            self.context,
            all_greeks=True,
        )
        self.assertIsNotNone(result.price)
        self.assertIsNotNone(result.delta)
        for name in ("theta", "charm", "color"):
            self.assertIsNone(getattr(result, name))

    def test_barrier_history_is_required_and_changes_remaining_value(self):
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
            "barrier_level": 120,
            "barrier_type": "up_and_out",
        }
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=date(2025, 7, 1),
        )
        engine = AnalyticBarrierEngine()
        self.assertEqual(get_args(BarrierTouchState), ("untouched", "touched"))
        self.assertIsNone(BarrierOption(**terms).touch_state)
        with self.assertRaises(ValueError):
            BarrierOption(**terms, touch_state="invalid")
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(BarrierOption(**terms, touch_state=None), context)
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
        )
        self.assertGreater(
            engine.price(BarrierOption(**terms, touch_state="untouched"), context), 0
        )
        self.assertEqual(
            engine.price(BarrierOption(**terms, touch_state="touched"), context), 0
        )
        knocked_in = BarrierOption(
            **(terms | {"barrier_type": "up_and_in"}), touch_state="touched"
        )
        self.assertAlmostEqual(
            engine.price(knocked_in, context),
            AnalyticVanillaEngine().price(
                EuropeanOption(
                    option_type="call",
                    strike=100,
                    effective_date=terms["effective_date"],
                    expiry_date=terms["expiry_date"],
                ),
                context,
            ),
        )
        touch_terms = {
            "effective_date": terms["effective_date"],
            "expiry_date": terms["expiry_date"],
            "barrier_level": 120,
            "payout": 10,
            "touch_state": "touched",
        }
        binary_engine = AnalyticBinaryBarrierEngine()
        self.assertEqual(
            binary_engine.price(
                cash_one_touch_up(**touch_terms, settlement_timing="at_hit"), context
            ),
            0,
        )
        self.assertAlmostEqual(
            binary_engine.price(
                cash_one_touch_up(**touch_terms, settlement_timing="at_expiry"), context
            ),
            10 * math.exp(-0.04 * 184 / 365),
        )

    def test_implied_solver_tolerances_have_separate_units(self):
        option = EuropeanOption(option_type="call", strike=100,
                                effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        context = PricingContext(model_parameters=self.parameters, spot_price=100,
                                 valuation_time=date(2025, 7, 1))
        engine = AnalyticVanillaEngine()
        quote = engine.price(option, context)
        result = implied_volatility(engine, option, context, quote,
                                    price_tolerance=1e-12, parameter_tolerance=1e-10)
        self.assertAlmostEqual(result, 0.2, delta=1e-10)
        for name in ("price_tolerance", "parameter_tolerance"):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                implied_volatility(engine, option, context, quote, **{name: 0})
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_unrepresentable_numerical_shifts(self):
        option = EuropeanOption(option_type="call", strike=100,
                                effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        context = PricingContext(model_parameters=self.parameters, spot_price=100,
                                 valuation_time=date(2025, 7, 1))
        normal = calculate_numerical_greeks(AnalyticVanillaEngine(), option, context)
        self.assertGreater(normal.vega, 0)
        self.assertGreater(normal.rho, 0)
        tiny = calculate_numerical_greeks(AnalyticVanillaEngine(), option, context,
                                        volatility_shift=1e-20, rate_shift=1e-20)
        for name in ("vega", "vanna", "zomma", "rho"):
            self.assertIsNone(getattr(tiny, name))
        self.assertIsNotNone(tiny.delta)

    def test_public_api_matches_shared_language_parity_cases(self):
        cases = parity_cases()
        self.assertTrue(cases)
        for case in cases:
            with self.subTest(case=case["case_id"]):
                values = case["inputs"]
                expected = case["expected"]
                kind = case["kind"]
                if kind == "construction":
                    option = EuropeanOption(
                        option_type=values["type"],
                        strike=float(values["strike"]),
                        effective_date=date.fromisoformat(values["effective_date"]),
                        expiry_date=date.fromisoformat(values["expiry_date"]),
                    )
                    self.assertEqual(option.option_type, expected["type"])
                    self.assertEqual(option.strike, float(expected["strike"]))
                    self.assertEqual(
                        option.effective_date,
                        date.fromisoformat(expected["effective_date"]),
                    )
                    self.assertEqual(
                        option.expiry_date, date.fromisoformat(expected["expiry_date"])
                    )
                elif kind == "defaults":
                    engine = pricing.FiniteDifferenceVanillaEngine()
                    self.assertEqual(
                        engine.asset_step_count, int(expected["asset_step_count"])
                    )
                    self.assertEqual(
                        engine.time_step_count, int(expected["time_step_count"])
                    )
                    self.assertEqual(engine.scheme, expected["scheme"])
                    self.assertEqual(expected["asset_upper_boundary"], "none")
                    self.assertIsNone(engine.asset_upper_boundary)
                elif kind in ("pricing", "pricing_greeks"):
                    parameters = BlackScholesMertonParameters(
                        risk_free_rate=float(values["risk_free_rate"]),
                        dividend_yield=float(values["dividend_yield"]),
                        volatility=float(values["volatility"]),
                    )
                    context = PricingContext(
                        model_parameters=parameters,
                        spot_price=float(values["spot_price"]),
                        valuation_time=date.fromisoformat(values["valuation_date"]),
                    )
                    option = EuropeanOption(
                        option_type=values["type"],
                        strike=float(values["strike"]),
                        effective_date=date.fromisoformat(values["effective_date"]),
                        expiry_date=date.fromisoformat(values["expiry_date"]),
                    )
                    result = AnalyticVanillaEngine().price(option, context)
                    self.assertAlmostEqual(
                        result, float(expected["price"]), delta=float(case["tolerance"])
                    )
                    if kind == "pricing_greeks":
                        kwargs = (
                            {"all_greeks": True}
                            if values.get("all_greeks") == "true"
                            else {"greeks": values["greeks"].split(",")}
                        )
                        result = AnalyticVanillaEngine().price_with_greeks(
                            option, context, **kwargs
                        )
                        for name, value in expected.items():
                            with self.subTest(measure=name):
                                if value == "none":
                                    self.assertIsNone(getattr(result, name))
                                else:
                                    self.assertAlmostEqual(
                                        getattr(result, name),
                                        float(value),
                                        delta=float(case["tolerance"]),
                                    )
                elif kind == "initial_snowball_history":
                    terms = dict(
                        coupon_rate=0.1, initial_spot=100, knock_in_level=80,
                        knock_out_level=110,
                        observation_dates=[date.fromisoformat(values["expiry_date"])],
                        effective_date=date.fromisoformat(values["effective_date"]),
                        expiry_date=date.fromisoformat(values["expiry_date"]),
                    )
                    absent = standard_snowball(**terms)
                    explicit_none = standard_snowball(**terms, barrier_state="none")
                    context = PricingContext(
                        model_parameters=self.parameters, spot_price=80,
                        valuation_time=date.fromisoformat(values["valuation_date"]),
                    )
                    for engine in (
                        pricing.MonteCarloSnowballEngine(path_count=64, seed=1),
                        pricing.FiniteDifferenceSnowballEngine(),
                    ):
                        b = engine.price_with_greeks(explicit_none, context, all_greeks=True)
                        if "category" in expected:
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                engine.price_with_greeks(absent, context, all_greeks=True)
                            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
                            continue
                        a = engine.price_with_greeks(absent, context, all_greeks=True)
                        for name in ("price", "delta", "gamma", "speed", "theta", "charm",
                                     "color", "vega", "vanna", "zomma", "rho"):
                            self.assertEqual(getattr(a, name), getattr(b, name))
                        self.assertEqual(a.delta is not None, expected["delta"] == "available")
                elif kind in ("unidentifiable_touch", "unidentifiable_coupon"):
                    start = date.fromisoformat(values["effective_date"])
                    end = date.fromisoformat(values["expiry_date"])
                    context = PricingContext(
                        model_parameters=self.parameters, spot_price=100,
                        valuation_time=date.fromisoformat(values["valuation_date"]),
                    )
                    if kind == "unidentifiable_touch":
                        option = cash_one_touch_up(
                            effective_date=start, expiry_date=end, barrier_level=90, payout=10,
                            touch_state=None if values["history"] == "absent" else "touched",
                        )
                        engine = AnalyticBinaryBarrierEngine()
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            implied_volatility(engine, option, context, engine.price(option, context))
                        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
                    else:
                        option = BinarySnowballOption(
                            knock_out_coupon_rates=[0.1], maturity_coupon_rate=0.1,
                            knock_out_levels=[110],
                            observation_dates=[date.fromisoformat(values["observation_date"])],
                            barrier_state="none", effective_date=start, expiry_date=end,
                        )
                        for engine in (pricing.MonteCarloBinarySnowballEngine(path_count=2, seed=1),
                                       pricing.FiniteDifferenceBinarySnowballEngine()):
                            price = engine.price(option, context)
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                implied_coupon(engine, option, context, price,
                                               quote_convention="preserve_maturity_coupon")
                            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
                            self.assertAlmostEqual(implied_coupon(
                                engine, option, context, price,
                                quote_convention="shift_maturity_coupon"), 0.1, delta=1e-6)
                elif kind == "domain_error":
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        BlackScholesMertonParameters(
                            risk_free_rate=float(values["risk_free_rate"]),
                            dividend_yield=float(values["dividend_yield"]),
                            volatility=float(values["volatility"]),
                        )
                    self.assertEqual(
                        error.exception.category,
                        getattr(kiyosi.ErrorCategory, expected["category"].upper()),
                    )
                elif kind == "date_round_trip":
                    option = GeometricAveragePriceOption(
                        option_type=values["type"],
                        strike=float(values["strike"]),
                        averaging_start_date=date.fromisoformat(
                            values["averaging_start_date"]
                        ),
                        effective_date=date.fromisoformat(values["effective_date"]),
                        expiry_date=date.fromisoformat(values["expiry_date"]),
                    )
                    self.assertEqual(
                        option.averaging_start_date,
                        date.fromisoformat(expected["averaging_start_date"]),
                    )
                    self.assertEqual(
                        option.effective_date,
                        date.fromisoformat(expected["effective_date"]),
                    )
                    self.assertEqual(
                        option.expiry_date, date.fromisoformat(expected["expiry_date"])
                    )
                elif kind == "timestamp_round_trip":
                    parameters = BlackScholesMertonParameters(
                        risk_free_rate=float(values["risk_free_rate"]),
                        dividend_yield=float(values["dividend_yield"]),
                        volatility=float(values["volatility"]),
                    )
                    context = PricingContext(
                        model_parameters=parameters,
                        spot_price=float(values["spot_price"]),
                        valuation_time=utc_timestamp(values["valuation_time"]),
                    )
                    self.assertEqual(
                        context.valuation_time,
                        utc_timestamp(expected["valuation_time"]),
                    )
                    self.assertEqual(
                        context.valuation_date,
                        date.fromisoformat(expected["valuation_date"]),
                    )
                elif kind == "numeric_boundary":
                    engine = pricing.MonteCarloVanillaEngine(seed=int(values["seed"]))
                    self.assertEqual(engine.seed, int(expected["seed"]))
                else:
                    self.fail(f"unknown parity case kind: {kind}")


if __name__ == "__main__":
    unittest.main()
