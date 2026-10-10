import unittest
from datetime import UTC, date, datetime

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
