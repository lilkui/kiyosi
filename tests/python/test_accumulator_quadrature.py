import math
import sys
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, time, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import Accumulator, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    weekdays_calendar,
)

START = date(2025, 1, 1)


def contract(end=date(2025, 2, 3), knock_out=110, acceleration=2, daily=1, quantity=3):
    return Accumulator(
        strike=100,
        knock_out_level=knock_out,
        daily_quantity=daily,
        acceleration_factor=acceleration,
        accumulated_quantity=quantity,
        effective_date=START,
        expiry_date=end,
    )


def market(
    spot=100.25, valuation=None, volatility=0.2, rate=0.04, dividend=0.01, daily=False
):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation or datetime(2025, 1, 1, 12, tzinfo=UTC),
        calendar=all_days_calendar() if daily else weekdays_calendar(),
    )


def terminal_reference(option, context):
    horizon = (
        datetime.combine(option.expiry_date, time(), UTC) - context.valuation_time
    ).total_seconds() / (365 * 86400)
    parameters = context.model_parameters
    deviation = parameters.volatility * math.sqrt(horizon)
    stock = context.spot_price * math.exp(-parameters.dividend_yield * horizon)
    cash = option.strike * math.exp(-parameters.risk_free_rate * horizon)

    def lower_payoff(level):
        d2 = (
            math.log(context.spot_price / level)
            + (
                parameters.risk_free_rate
                - parameters.dividend_yield
                - parameters.volatility**2 / 2
            )
            * horizon
        ) / deviation
        return (
            stock * math.erfc((d2 + deviation) / math.sqrt(2))
            - cash * math.erfc(d2 / math.sqrt(2))
        ) / 2

    purchase = (
        lower_payoff(option.knock_out_level)
        + (option.acceleration_factor - 1) * lower_payoff(option.strike)
        if option.knock_out_level > option.strike
        else option.acceleration_factor * lower_payoff(option.knock_out_level)
    )
    return (
        option.accumulated_quantity * (stock - cash) + option.daily_quantity * purchase
    )


def exposure(option, context):
    current = context.valuation_time.date()
    observations = 0
    while current <= option.expiry_date:
        if datetime.combine(
            current, time(), UTC
        ) >= context.valuation_time and context.calendar.is_trading_day(current):
            observations += 1
        current += timedelta(days=1)
    return option.strike * (
        option.accumulated_quantity
        + observations * option.daily_quantity * max(1, option.acceleration_factor)
    )


class AccumulatorQuadratureTests(unittest.TestCase):
    def test_fixed_holdings_without_future_purchases(self):
        context = market(spot=90)
        engine = pricing.QuadratureAccumulatorEngine()
        reference = pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=1600, time_step_count=1200
        )
        for inactive in (False, True):
            option = contract(
                knock_out=95 if inactive else 110,
                acceleration=0 if inactive else 2,
                daily=1 if inactive else 0,
                quantity=3,
            )
            with self.subTest(inactive=inactive):
                price = engine.price(option, context)
                self.assertAlmostEqual(
                    price, reference.price(option, context), delta=5e-4
                )
                self.assertAlmostEqual(
                    engine.price(option.replace(accumulated_quantity=6), context),
                    2 * price,
                    delta=1e-11,
                )
                result = engine.price_with_greeks(
                    option, context, ["delta", "gamma"], spot_shift=0.01
                )
                high = engine.price(option, context.replace(spot_price=90.01))
                low = engine.price(option, context.replace(spot_price=89.99))
                self.assertAlmostEqual(result.delta, (high - low) / 0.02, delta=1e-9)
                self.assertAlmostEqual(
                    result.gamma,
                    ((high - price) - (price - low)) / 0.01 / 0.01,
                    delta=1e-9,
                )

    def test_configuration_protocols_and_ownership(self):
        engine = pricing.QuadratureAccumulatorEngine()
        self.assertEqual(engine.asset_step_count, 512)
        self.assertEqual(engine.integration_standard_deviations, 8)
        self.assertIn("asset_step_count=512", repr(engine))
        with self.assertRaises(AttributeError):
            engine.asset_step_count = 1024
        for value in (True, 4.5, "512"):
            with self.subTest(value=value), self.assertRaises(TypeError):
                pricing.QuadratureAccumulatorEngine(asset_step_count=value)
        with self.assertRaises(OverflowError):
            pricing.QuadratureAccumulatorEngine(asset_step_count=2**40)
        with self.assertRaises(TypeError):
            pricing.QuadratureAccumulatorEngine(integration_standard_deviations=True)
        for kwargs in (
            {"asset_step_count": 3},
            {"integration_standard_deviations": 0},
            {"integration_standard_deviations": math.inf},
            {"integration_standard_deviations": math.nan},
        ):
            with (
                self.subTest(kwargs=kwargs),
                self.assertRaises(kiyosi.KiyosiError) as error,
            ):
                pricing.QuadratureAccumulatorEngine(**kwargs)
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )
        vanilla = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=START,
            expiry_date=date(2025, 2, 3),
        )
        with self.assertRaises(TypeError):
            engine.price(vanilla, market())

    def test_single_observation_matches_independent_cash_asset_decomposition(self):
        valuation = datetime(2025, 1, 3, 12, tzinfo=UTC)
        default = pricing.QuadratureAccumulatorEngine()
        minimal = pricing.QuadratureAccumulatorEngine(
            asset_step_count=4, integration_standard_deviations=0.1
        )
        for knock_out in (90, 100, 110):
            for acceleration in (0, 1, 2):
                for spot in (80, 100, 120):
                    with self.subTest(
                        knock_out=knock_out, acceleration=acceleration, spot=spot
                    ):
                        option = contract(date(2025, 1, 6), knock_out, acceleration)
                        context = market(spot, valuation, 0.6, -0.02, 0.03)
                        actual = default.price(option, context)
                        self.assertAlmostEqual(
                            actual, terminal_reference(option, context), delta=1e-9
                        )
                        self.assertEqual(actual, minimal.price(option, context))
        option = contract(date(2025, 1, 6))
        context = market(100, datetime(2025, 1, 5, 23, 59, 59, tzinfo=UTC))
        self.assertAlmostEqual(
            default.price(option, context),
            terminal_reference(option, context),
            delta=1e-9,
        )

    def test_settlement_boundaries_validation_and_failure_reuse(self):
        end = date(2025, 1, 6)
        option = contract(end)
        engine = pricing.QuadratureAccumulatorEngine()
        for spot, expected in ((90, -50), (100, 0), (109, 36), (110, 30), (111, 33)):
            with self.subTest(spot=spot):
                context = market(spot, end)
                self.assertEqual(engine.price(option, context), expected)
                result = engine.price_with_greeks(option, context, all_greeks=True)
                self.assertEqual(result.price, expected)
                for greek in (
                    "delta",
                    "gamma",
                    "speed",
                    "vega",
                    "rho",
                    "theta",
                    "vanna",
                    "zomma",
                    "charm",
                    "color",
                ):
                    self.assertIsNone(getattr(result, greek))
        self.assertEqual(engine.price(option, market(110, START)), 30)
        for instrument, context, category in (
            (
                option,
                market(100, end + timedelta(days=1)),
                kiyosi.ErrorCategory.INVALID_TIME_RANGE,
            ),
            (
                contract(date(2025, 1, 5)),
                market(100, date(2025, 1, 3)),
                kiyosi.ErrorCategory.INVALID_DATE,
            ),
            (
                contract(end, quantity=sys.float_info.max),
                market(111, end),
                kiyosi.ErrorCategory.INVALID_RESULT,
            ),
        ):
            with (
                self.subTest(category=category),
                self.assertRaises(kiyosi.KiyosiError) as error,
            ):
                engine.price(instrument, context)
            self.assertEqual(error.exception.category, category)
        self.assertEqual(engine.price(option, market(110, end)), 30)

    def test_representative_contracts_converge_in_space_and_domain(self):
        month, quarter, year = date(2025, 2, 3), date(2025, 4, 1), date(2026, 1, 1)
        scenarios = (
            (month, 80, 0.05, 105, 2, 0.04, False),
            (month, 100, 0.2, 110, 2, 0.04, False),
            (month, 120, 0.6, 150, 1, -0.02, True),
            (month, 100, 0.05, 150, 0, 0.04, False),
            (quarter, 80, 0.2, 110, 2, 0.04, True),
            (quarter, 100, 0.05, 105, 2, -0.02, False),
            (quarter, 120, 0.2, 150, 1, 0.04, False),
            (quarter, 100, 0.6, 110, 2, 0.04, True),
            (year, 80, 0.05, 105, 2, 0.04, False),
            (year, 100, 0.2, 110, 2, 0.04, False),
            (year, 120, 0.6, 150, 2, -0.02, True),
            (year, 100, 0.05, 150, 1, 0.04, False),
            (year, 80, 0.6, 110, 0, 0.04, False),
            (quarter, 120, 0.05, 105, 2, 0.04, False),
            (month, 80, 0.6, 150, 2, -0.02, True),
            (quarter, 100, 0.2, 105, 1, 0.04, False),
        )
        normal = pricing.QuadratureAccumulatorEngine()
        refined = pricing.QuadratureAccumulatorEngine(asset_step_count=1024)
        expanded = pricing.QuadratureAccumulatorEngine(
            asset_step_count=1024, integration_standard_deviations=10
        )
        for end, spot, sigma, knock_out, acceleration, rate, daily in scenarios:
            with self.subTest(
                end=end, spot=spot, sigma=sigma, knock_out=knock_out, daily=daily
            ):
                option = contract(end, knock_out, acceleration)
                context = market(spot, volatility=sigma, rate=rate, daily=daily)
                base, fine, wide = (
                    engine.price(option, context)
                    for engine in (normal, refined, expanded)
                )
                scale = exposure(option, context)
                self.assertLessEqual(abs(base - fine) / scale, 1e-6)
                self.assertLessEqual(abs(fine - wide) / scale, 1e-6)

    def test_quantity_affinity_exact_limits_and_shared_instance_threads(self):
        engine = pricing.QuadratureAccumulatorEngine()
        context = market(90)
        values = [
            engine.price(contract(quantity=quantity), context) for quantity in (0, 3, 6)
        ]
        self.assertAlmostEqual(
            values[2] - values[1], values[1] - values[0], delta=1e-10
        )
        self.assertEqual(engine.price(contract(daily=0, quantity=0), context), 0)
        self.assertEqual(
            engine.price(contract(daily=0), market(90, rate=0, dividend=0)), -30
        )
        self.assertAlmostEqual(
            engine.price(
                contract(date(2025, 1, 2)),
                market(90, volatility=1e-8, rate=0, dividend=0),
            ),
            -50,
            delta=1e-9,
        )
        self.assertAlmostEqual(
            engine.price(contract(), market(90, volatility=1e-8, rate=0, dividend=0)),
            -490,
            delta=1e-9,
        )
        self.assertAlmostEqual(
            engine.price(contract(), market(120, volatility=1e-8, rate=0, dividend=0)),
            60,
            delta=1e-9,
        )
        option = contract()
        with ThreadPoolExecutor(max_workers=2) as pool:
            values = list(pool.map(lambda _: engine.price(option, context), range(2)))
        self.assertEqual(values, [engine.price(option, context)] * 2)

    def test_current_observation_adjacent_thresholds_and_extreme_result(self):
        end = date(2025, 1, 6)
        option = contract(end)
        engine = pricing.QuadratureAccumulatorEngine()
        for spot, quantity in ((90, 5), (100, 4), (109, 4)):
            context = market(spot, date(2025, 1, 3))
            self.assertAlmostEqual(
                engine.price(option, context),
                terminal_reference(contract(end, quantity=quantity), context),
                delta=1e-9,
            )
        for spot, quantity in (
            (math.nextafter(100, 0), 5),
            (math.nextafter(100, 110), 4),
            (math.nextafter(110, 0), 4),
            (math.nextafter(110, 120), 3),
        ):
            self.assertEqual(
                engine.price(option, market(spot, end)), quantity * (spot - 100)
            )
        self.assertEqual(engine.price(contract(end, 90), market(90, end)), -30)
        self.assertEqual(engine.price(contract(end, 90), market(89, end)), -55)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(option, market(volatility=1e308))
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RESULT)

    def test_price_agrees_with_monte_carlo_batch_uncertainty(self):
        option = contract(date(2025, 4, 1))
        context = market()
        batches = [
            pricing.MonteCarloAccumulatorEngine(
                path_count=8192, seed=100 + batch
            ).price(option, context)
            for batch in range(16)
        ]
        mean = sum(batches) / len(batches)
        standard_error = math.sqrt(
            sum((value - mean) ** 2 for value in batches)
            / (len(batches) * (len(batches) - 1))
        )
        self.assertGreater(standard_error, 0)
        actual = pricing.QuadratureAccumulatorEngine().price(option, context)
        self.assertLessEqual(abs(actual - mean), 6 * standard_error)

    def test_greek_availability_and_normalized_convergence(self):
        option = contract()
        engine = pricing.QuadratureAccumulatorEngine()
        boundary = engine.price_with_greeks(option, market(100, START), all_greeks=True)
        for greek in ("delta", "gamma", "speed", "theta"):
            self.assertIsNone(getattr(boundary, greek))
        small = engine.price_with_greeks(
            option, market(90, volatility=1e-4), greeks=["vega", "vanna", "zomma"]
        )
        for greek in ("vega", "vanna", "zomma"):
            self.assertIsNone(getattr(small, greek))
        valuation = datetime(2025, 1, 4, 12, tzinfo=UTC)
        cases = (
            (contract(), market(valuation=valuation)),
            (contract(date(2026, 1, 1), 105), market(80, valuation, 0.05)),
            (contract(date(2025, 4, 1), 150, 1), market(120, valuation, 0.6, -0.02)),
        )
        refined = pricing.QuadratureAccumulatorEngine(asset_step_count=1024)
        for option, context in cases:
            with self.subTest(
                expiry=option.expiry_date,
                spot=context.spot_price,
                volatility=context.model_parameters.volatility,
            ):
                base = engine.price_with_greeks(option, context, all_greeks=True)
                fine = refined.price_with_greeks(option, context, all_greeks=True)
                smaller = refined.price_with_greeks(
                    option,
                    context,
                    all_greeks=True,
                    spot_shift=0.005,
                    volatility_shift=0.00005,
                    rate_shift=0.00005,
                )
                for greek, order, absolute, relative in (
                    ("delta", 1, 1e-6, 0.005),
                    ("gamma", 2, 1e-5, 0.01),
                    ("vega", 0, 1e-8, 0.005),
                    ("rho", 0, 1e-8, 0.005),
                    ("theta", 0, 1e-8, 0.02),
                    ("speed", 3, 1e-4, 0.05),
                    ("vanna", 1, 1e-7, 0.05),
                    ("charm", 1, 1e-7, 0.05),
                    ("zomma", 2, 1e-6, 0.05),
                    ("color", 2, 1e-6, 0.05),
                ):
                    with self.subTest(greek=greek):
                        scale = option.strike**order / exposure(option, context)
                        reference = getattr(fine, greek)
                        self.assertIsNotNone(reference)
                        reference *= scale
                        limit = absolute + relative * abs(reference)
                        for result in (base, smaller):
                            value = getattr(result, greek)
                            self.assertIsNotNone(value)
                            self.assertLessEqual(abs(value * scale - reference), limit)

    def test_reused_spot_and_volatility_curves_match_scalar_prices(self):
        engine = pricing.QuadratureAccumulatorEngine(asset_step_count=128)
        option = contract(date(2025, 1, 20))
        spot, h = 100.25, 0.25
        volatilities = (0.25, 0.3)
        valuation = datetime(2025, 1, 4, 12, tzinfo=UTC)

        def values(vol):
            return [
                engine.price(option, market(spot + offset * h, valuation, vol, -0.02))
                for offset in (0, 1, -1, 2, -2)
            ]

        expected = []
        for volatility in volatilities:
            v = (volatility + 0.002) - volatility
            center, up, down, up2, down2 = values(volatility)
            high, high_up, high_down, _, _ = values(volatility + v)
            low, low_up, low_down, _, _ = values(volatility - v)
            expected.append(
                (
                    center,
                    {
                        "delta": (up - down) / (2 * h),
                        "gamma": (up - 2 * center + down) / h**2,
                        "speed": (up2 - 2 * up + 2 * down - down2) / (2 * h**3),
                        "vanna": (high_up - high_down - low_up + low_down)
                        / (400 * h * v),
                        "zomma": (
                            (high_up - 2 * high + high_down)
                            - (low_up - 2 * low + low_down)
                        )
                        / (200 * h**2 * v),
                    },
                )
            )

        def evaluate(volatility):
            return engine.price_with_greeks(
                option,
                market(spot, valuation, volatility, -0.02),
                ["delta", "gamma", "speed", "vanna", "zomma"],
                spot_shift=h,
                volatility_shift=0.002,
            )

        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(evaluate, volatilities))
        for result, (center, greeks) in zip(results, expected, strict=True):
            self.assertEqual(result.price, center)
            for greek, reference in greeks.items():
                with self.subTest(greek=greek):
                    self.assertAlmostEqual(
                        getattr(result, greek), reference, delta=1e-10
                    )

    def test_implied_volatility_uses_shared_solver(self):
        option = contract(date(2025, 1, 6))
        context = market(100, datetime(2025, 1, 3, 12, tzinfo=UTC), volatility=0.4)
        engine = pricing.QuadratureAccumulatorEngine()
        quote = engine.price(option, context)
        volatility = pricing.implied_volatility(
            engine, option, context, quote, lower_bound=0.2, upper_bound=0.6
        )
        self.assertAlmostEqual(volatility, 0.4, delta=1e-6)


if __name__ == "__main__":
    unittest.main()
