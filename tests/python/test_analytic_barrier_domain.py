import math
import unittest
from datetime import UTC, date, datetime, timedelta

from price_differences import price_differences

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    BarrierOption,
    asset_binary_barrier_option,
    asset_no_touch_down,
    asset_no_touch_up,
    asset_one_touch_down,
    asset_one_touch_up,
    cash_binary_barrier_option,
    cash_no_touch_down,
    cash_no_touch_up,
    cash_one_touch_down,
    cash_one_touch_up,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)


class AnalyticBarrierDomainTests(unittest.TestCase):
    def test_equivalent_hit_rebates_and_touch_payments_share_discount_validation(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for up in (False, True):
            level = 120 if up else 80
            barrier = BarrierOption(
                option_type="call" if up else "put",
                strike=level,
                effective_date=start,
                expiry_date=end,
                barrier_level=level,
                barrier_type="up_and_out" if up else "down_and_out",
                rebate=7,
                rebate_timing="at_hit",
            )
            touch = (cash_one_touch_up if up else cash_one_touch_down)(
                effective_date=start,
                expiry_date=end,
                barrier_level=level,
                payout=7,
                settlement_timing="at_hit",
            )
            for dividend in (-0.04, -0.08, -0.08000000000000002, -0.1):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=-0.02, dividend_yield=dividend, volatility=0.2
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                with self.subTest(up=up, dividend=dividend):
                    contracts = (
                        (pricing.AnalyticBarrierEngine(), barrier),
                        (pricing.AnalyticBinaryBarrierEngine(), touch),
                    )
                    if dividend == -0.1:
                        prices = [
                            engine.price(option, context)
                            for engine, option in contracts
                        ]
                        self.assertAlmostEqual(*prices, delta=1e-12)
                        self.assertGreater(prices[1], 0)
                    else:
                        for engine, option in contracts:
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                engine.price(option, context)
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.INVALID_RESULT,
                            )

    def test_dense_scheduled_greeks_preserve_independent_stencils(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        common = {
            "effective_date": start,
            "expiry_date": end,
            "barrier_level": 80,
            "observation_mode": "scheduled",
            "touch_state": "untouched",
            "observation_dates": [start + timedelta(days=i) for i in range(1, 366)],
        }
        terms = dict(
            common, option_type="call", strike=100, barrier_type="down_and_out"
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
            calendar=all_days_calendar(),
        )
        contracts = [
            (pricing.AnalyticBarrierEngine(), BarrierOption(**terms)),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                cash_binary_barrier_option(**terms, payout=10),
            ),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                asset_binary_barrier_option(**terms),
            ),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                cash_no_touch_down(**common, payout=10),
            ),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                asset_one_touch_down(**common, settlement_timing="at_expiry"),
            ),
        ]
        greeks = (
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
        )
        for engine, option in contracts:
            with self.subTest(option=type(option).__name__):
                time_greeks = ("theta", "charm", "color")
                expected = price_differences(
                    engine,
                    option,
                    context,
                    greeks=tuple(greek for greek in greeks if greek not in time_greeks),
                )
                actual = engine.price_with_greeks(option, context, greeks)
                self.assertEqual(actual.price, engine.price(option, context))
                for greek in greeks:
                    if greek in time_greeks:
                        self.assertIsNone(getattr(actual, greek))
                    else:
                        self.assertEqual(getattr(actual, greek), getattr(expected, greek))
                solved = pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    actual.price,
                    lower_bound=0.15,
                    upper_bound=0.25,
                )
                self.assertAlmostEqual(solved, 0.2, delta=1e-7)
                with self.assertRaises(kiyosi.KiyosiError) as invalid:
                    pricing.implied_volatility(
                        engine,
                        option,
                        context,
                        actual.price,
                        lower_bound=0,
                    )
                self.assertEqual(
                    invalid.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                )

    def test_binary_barriers_survive_volatility_variance_underflow(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticBinaryBarrierEngine()
        for sigma in (1e-160, 1e-200):
            for rate in (0.0, 0.05):
                parameters = BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=rate, volatility=sigma
                )
                for spot in (90, 100, 110):
                    context = PricingContext(
                        model_parameters=parameters,
                        spot_price=spot,
                        valuation_time=start,
                    )
                    for direction in ("call", "put"):
                        probability = (
                            0.5
                            if spot == 100
                            else float(
                                spot > 100 if direction == "call" else spot < 100
                            )
                        )
                        for up in (False, True):
                            for knock_in in (False, True):
                                terms = {
                                    "option_type": direction,
                                    "strike": 100,
                                    "effective_date": start,
                                    "expiry_date": end,
                                    "barrier_level": 120 if up else 80,
                                    "barrier_type": ("up" if up else "down")
                                    + ("_and_in" if knock_in else "_and_out"),
                                }
                                for asset in (False, True):
                                    with self.subTest(
                                        sigma=sigma,
                                        rate=rate,
                                        spot=spot,
                                        direction=direction,
                                        up=up,
                                        knock_in=knock_in,
                                        asset=asset,
                                    ):
                                        option = (
                                            asset_binary_barrier_option(**terms)
                                            if asset
                                            else cash_binary_barrier_option(
                                                **terms, payout=10
                                            )
                                        )
                                        expected = (
                                            0
                                            if knock_in
                                            else (
                                                (spot if asset else 10)
                                                * math.exp(-rate)
                                                * probability
                                            )
                                        )
                                        price = engine.price(option, context)
                                        self.assertAlmostEqual(
                                            price, expected, delta=1e-11
                                        )
                                        self.assertEqual(
                                            engine.price_with_greeks(
                                                option, context, "delta"
                                            ).price,
                                            price,
                                        )

    def test_scheduled_analytic_barriers_reject_spots_outside_shifted_domain(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        for up in (True, False):
            context = PricingContext(
                model_parameters=parameters,
                spot_price=150 if up else 50,
                valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
                calendar=all_days_calendar(),
            )
            common = {
                "effective_date": start,
                "expiry_date": end,
                "barrier_level": 120 if up else 80,
                "observation_mode": "scheduled",
                "observation_dates": [end],
            }
            for knock_in in (True, False):
                terms = dict(
                    common,
                    option_type="call",
                    strike=100,
                    barrier_type=("up" if up else "down")
                    + ("_and_in" if knock_in else "_and_out"),
                )
                contracts = [
                    (pricing.AnalyticBarrierEngine(), BarrierOption(**terms)),
                    (
                        pricing.AnalyticBinaryBarrierEngine(),
                        cash_binary_barrier_option(**terms, payout=10),
                    ),
                    (
                        pricing.AnalyticBinaryBarrierEngine(),
                        asset_binary_barrier_option(**terms),
                    ),
                ]
                if knock_in:
                    cash = cash_one_touch_up if up else cash_one_touch_down
                    asset = asset_one_touch_up if up else asset_one_touch_down
                    for timing in ("at_hit", "at_expiry"):
                        contracts.extend(
                            [
                                (
                                    pricing.AnalyticBinaryBarrierEngine(),
                                    cash(**common, payout=10, settlement_timing=timing),
                                ),
                                (
                                    pricing.AnalyticBinaryBarrierEngine(),
                                    asset(**common, settlement_timing=timing),
                                ),
                            ]
                        )
                else:
                    cash = cash_no_touch_up if up else cash_no_touch_down
                    asset = asset_no_touch_up if up else asset_no_touch_down
                    contracts.extend(
                        [
                            (
                                pricing.AnalyticBinaryBarrierEngine(),
                                cash(**common, payout=10),
                            ),
                            (pricing.AnalyticBinaryBarrierEngine(), asset(**common)),
                        ]
                    )
                for engine, option in contracts:
                    with self.subTest(up=up, knock_in=knock_in, option=repr(option)):
                        for evaluate, arguments in (
                            (engine.price, (option, context)),
                            (engine.price_with_greeks, (option, context, "delta")),
                        ):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                evaluate(*arguments)
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                            )
