import unittest
from datetime import date

from kiyosi import ErrorCategory, KiyosiError
from kiyosi import instruments as ins
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticBarrierEngine


class HistoryReplaceTests(unittest.TestCase):
    def cases(self):
        start, first, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        life = {"effective_date": start, "expiry_date": end}
        knock_in = {"knock_in_level": 75, "knock_in_observation_mode": "at_expiry"}
        participation = {"initial_spot": 105, "upper_strike": 100, "lower_strike": 55}
        for factory, extra in (
            (ins.SnowballOption, knock_in | participation),
            (ins.BinarySnowballOption, {}),
            (ins.TernarySnowballOption, knock_in | {"minimum_coupon_rate": 0.015625}),
            (ins.PhoenixOption, knock_in | participation),
        ):
            phoenix = factory is ins.PhoenixOption
            records = [
                ins.AutocallableObservation(
                    when,
                    level,
                    **(
                        {"coupon_barrier_level": coupon}
                        if phoenix
                        else {"coupon_rate": coupon}
                    ),
                )
                for when, level, coupon in (
                    (first, 115, 85 if phoenix else 0.125),
                    (end, 110, 80 if phoenix else 0.25),
                )
            ]
            terms = life | extra | {"principal_ratio": 1.5, "observations": records}
            terms |= (
                {"coupon_rate": 0.125} if phoenix else {"maturity_coupon_rate": 0.0625}
            )
            yield factory, terms, "barrier_state", "none", "knocked_out"
        barrier = life | {
            "barrier_level": 120,
            "observation_mode": "scheduled",
            "observation_dates": [first, end],
        }
        strike = {"option_type": "put", "strike": 103, "barrier_type": "up_and_out"}
        yield (
            ins.BarrierOption,
            barrier | strike | {"rebate": 7, "rebate_timing": "at_hit"},
            "touch_state",
            "untouched",
            "touched",
        )
        yield (
            ins.cash_binary_barrier_option,
            barrier | strike | {"payout": 17},
            "touch_state",
            "untouched",
            "touched",
        )
        yield (
            ins.asset_binary_barrier_option,
            barrier | strike,
            "touch_state",
            "untouched",
            "touched",
        )
        yield (
            ins.cash_one_touch_up,
            barrier | {"payout": 19, "settlement_timing": "at_expiry"},
            "touch_state",
            "untouched",
            "touched",
        )
        yield (
            ins.asset_no_touch_down,
            barrier | {"barrier_level": 75},
            "touch_state",
            "untouched",
            "touched",
        )
        for factory in (
            ins.GeometricAveragePriceOption,
            ins.ArithmeticAveragePriceOption,
        ):
            yield (
                factory,
                life
                | {"option_type": "put", "strike": 103, "averaging_start_date": first},
                "realized_average",
                99,
                102.5,
            )
        yield (
            ins.Accumulator,
            life
            | {
                "strike": 103,
                "knock_out_level": 120,
                "daily_quantity": 3,
                "acceleration_factor": 2.5,
            },
            "accumulated_quantity",
            11,
            200,
        )

    def test_replace_preserves_all_terms_and_original_for_every_product(self):
        for factory, terms, field, initial, replacement in self.cases():
            with self.subTest(product=factory.__name__):
                original = factory(**terms, **{field: initial})
                changed = original.replace(**{field: replacement})
                self.assertIs(type(changed), type(original))
                self.assertIsNot(changed, original)
                self.assertEqual(getattr(changed, field), replacement)
                self.assertEqual(changed, factory(**terms, **{field: replacement}))
                self.assertEqual(original, factory(**terms, **{field: initial}))
                corrected = changed.replace(**{field: initial})
                self.assertEqual(corrected, original)
                self.assertEqual(getattr(changed, field), replacement)
                reset = changed.replace(
                    **{field: None if isinstance(initial, str) else 0}
                )
                self.assertEqual(
                    reset,
                    factory(
                        **terms, **{field: None if isinstance(initial, str) else 0}
                    ),
                )
                self.assertEqual(getattr(changed, field), replacement)
                self.assertEqual(reset.replace(**{field: replacement}), changed)
                if field == "barrier_state" and factory is not ins.BinarySnowballOption:
                    knocked_in = original.replace(barrier_state="knocked_in")
                    self.assertEqual(
                        knocked_in, factory(**terms, barrier_state="knocked_in")
                    )
                    self.assertEqual(knocked_in.replace(barrier_state="none"), original)

    def test_replace_validates_domain_without_mutating_original(self):
        for factory, terms, field, initial, _ in self.cases():
            if isinstance(initial, str):
                if factory is not ins.BinarySnowballOption:
                    continue
                invalids = ("knocked_in",)
            else:
                invalids = (-1, float("nan"), float("inf"), -float("inf"))
            original = factory(**terms, **{field: initial})
            for invalid in invalids:
                with self.subTest(product=factory.__name__, value=invalid):
                    with self.assertRaises(KiyosiError) as caught:
                        original.replace(**{field: invalid})
                    self.assertEqual(
                        caught.exception.category, ErrorCategory.INVALID_PARAMETER
                    )
                    self.assertEqual(original, factory(**terms, **{field: initial}))

    def test_replace_requires_keyword_and_valid_representation_for_every_product(self):
        for factory, terms, field, initial, replacement in self.cases():
            original = factory(**terms, **{field: initial})
            with self.subTest(product=factory.__name__):
                with self.assertRaises(TypeError):
                    original.replace()
                with self.assertRaises(TypeError):
                    original.replace(replacement)
                with self.assertRaises(TypeError):
                    original.replace(**{field: replacement}, strike=100)
                invalids = (
                    (True, 1, object())
                    if isinstance(initial, str)
                    else (True, None, "102.5", object())
                )
                for invalid in invalids:
                    with self.assertRaises(TypeError):
                        original.replace(**{field: invalid})
                if isinstance(initial, str):
                    with self.assertRaises(ValueError):
                        original.replace(**{field: "invalid"})
                else:
                    with self.assertRaises(OverflowError):
                        original.replace(**{field: 10**1000})
                self.assertEqual(original, factory(**terms, **{field: initial}))

    def test_replace_defers_valuation_history_checks_until_pricing(self):
        option = ins.BarrierOption(
            option_type="call",
            strike=100,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
            barrier_level=120,
            barrier_type="up_and_out",
            touch_state="untouched",
        )
        unknown = option.replace(touch_state=None)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=date(2025, 7, 1),
        )
        engine = AnalyticBarrierEngine()
        with self.assertRaises(KiyosiError) as caught:
            engine.price(unknown, context)
        self.assertEqual(caught.exception.category, ErrorCategory.INVALID_PARAMETER)
        self.assertGreater(
            engine.price(unknown.replace(touch_state="untouched"), context), 0
        )
        self.assertEqual(
            engine.price(option.replace(touch_state="touched"), context), 0
        )
        self.assertEqual(option.touch_state, "untouched")
        self.assertIsNone(unknown.touch_state)


if __name__ == "__main__":
    unittest.main()
