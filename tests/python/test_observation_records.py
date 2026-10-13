import unittest
from datetime import date

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    BinarySnowballOption,
    AutocallableObservation,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)


class ObservationRecordTests(unittest.TestCase):
    def test_record_constructors_preserve_associations_and_core_validation(self):
        start, first, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2,
            ), spot_price=100, valuation_time=start, calendar=all_days_calendar(),
        )
        common = dict(effective_date=start, expiry_date=end, barrier_state="none")
        participation = dict(initial_spot=100, upper_strike=100, lower_strike=60)
        knock_in = dict(knock_in_level=75, knock_in_observation_mode="at_expiry")
        for factory, engine, extra in (
            (BinarySnowballOption, pricing.FiniteDifferenceBinarySnowballEngine(), {}),
            (TernarySnowballOption, pricing.FiniteDifferenceTernarySnowballEngine(),
             knock_in | {"minimum_coupon_rate": 0.01}),
            (SnowballOption, pricing.FiniteDifferenceSnowballEngine(), knock_in | participation),
            (PhoenixOption, pricing.FiniteDifferencePhoenixEngine(), knock_in | participation),
        ):
            phoenix = factory is PhoenixOption
            records = [
                AutocallableObservation(first, 115, **(
                    {"coupon_barrier_level": 85} if phoenix else {"coupon_rate": 0.08})),
                AutocallableObservation(end, 110, **(
                    {"coupon_barrier_level": 80} if phoenix else {"coupon_rate": 0.12})),
            ]
            terms = common | extra | ({"coupon_rate": 0.08} if phoenix else {"maturity_coupon_rate": 0.05})
            sequence = {"observation_dates": [first, end], "knock_out_levels": [115, 110]}
            sequence |= ({"coupon_barrier_levels": [85, 80]} if phoenix else {"knock_out_coupon_rates": [0.08, 0.12]})
            with self.subTest(product=factory.__name__):
                expected = factory(**terms, **sequence)
                note = factory(**terms, observations=iter(records))
                self.assertEqual(note, expected)
                self.assertEqual(engine.price(note, context), engine.price(expected, context))
                records.reverse()
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    factory(**terms, observations=records)
                self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_SCHEDULE)
                records.sort(key=lambda record: record.date)
                self.assertEqual(factory(**terms, observations=records), note)
                note.observations.reverse()
                self.assertEqual(note.observations, records)
                with self.assertRaises(AttributeError):
                    records[0].knock_out_level = 1
                with self.assertRaises(TypeError):
                    factory(**terms, **sequence, observations=records)
                for bad in ([True], [object()], [records[0], 1]):
                    with self.assertRaises(TypeError):
                        factory(**terms, observations=bad)
                for bad in ([], [AutocallableObservation(first, -1)],
                            [AutocallableObservation(first, 115, **(
                                {"coupon_rate": 0.1} if phoenix else {"coupon_barrier_level": 1}))]):
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        factory(**terms, observations=bad)
                    self.assertEqual(error.exception.category,
                                     kiyosi.ErrorCategory.INVALID_SCHEDULE if not bad else kiyosi.ErrorCategory.INVALID_PARAMETER)
        for field in ("knock_out_level", "coupon_rate", "coupon_barrier_level"):
            values = dict(knock_out_level=110, coupon_rate=0, coupon_barrier_level=0)
            values[field] = True
            with self.assertRaises(TypeError):
                AutocallableObservation(first, **values)

    def test_coupon_solves_preserve_original_observation_records(self):
        start, first, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        common = {
            "knock_out_levels": [115, 110],
            "observation_dates": [first, end],
            "barrier_state": "none",
            "principal_ratio": 1.5,
            "effective_date": start,
            "expiry_date": end,
        }
        knock_in = {"knock_in_level": 75, "knock_in_observation_mode": "at_expiry"}
        participation = {"initial_spot": 105, "upper_strike": 100, "lower_strike": 55}
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04,
                dividend_yield=0.01,
                volatility=0.2,
            ),
            spot_price=105,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        for factory, engine_type, extra in (
            (
                SnowballOption,
                pricing.FiniteDifferenceSnowballEngine,
                knock_in | participation,
            ),
            (BinarySnowballOption, pricing.FiniteDifferenceBinarySnowballEngine, {}),
            (
                TernarySnowballOption,
                pricing.FiniteDifferenceTernarySnowballEngine,
                knock_in | {"minimum_coupon_rate": 0.015625},
            ),
            (
                PhoenixOption,
                pricing.FiniteDifferencePhoenixEngine,
                knock_in | participation | {"coupon_barrier_levels": [85, 80]},
            ),
        ):
            engine = engine_type(asset_step_count=80, time_step_count=40)
            terms = common | extra
            phoenix = factory is PhoenixOption
            original_coupons = (
                {"coupon_rate": 0.125}
                if phoenix
                else {
                    "knock_out_coupon_rates": [0.125, 0.25],
                    "maturity_coupon_rate": 0.0625,
                }
            )
            note = factory(**terms, **original_coupons)
            original_price = engine.price(note, context)
            for convention in (
                (None,)
                if phoenix
                else (
                    "shift_maturity_coupon",
                    "preserve_maturity_coupon",
                )
            ):
                target_coupons = (
                    {"coupon_rate": 0.375}
                    if phoenix
                    else {
                        "knock_out_coupon_rates": [0.375, 0.5],
                        "maturity_coupon_rate": 0.3125
                        if convention == "shift_maturity_coupon"
                        else 0.0625,
                    }
                )
                target = factory(**terms, **target_coupons)
                kwargs = {} if phoenix else {"quote_convention": convention}
                with self.subTest(product=factory.__name__, convention=convention):
                    solved = pricing.implied_coupon(
                        engine,
                        note,
                        context,
                        engine.price(target, context),
                        lower_bound=0.125,
                        upper_bound=0.5,
                        **kwargs,
                    )
                    self.assertAlmostEqual(solved, 0.375, delta=1e-7)
                    self.assertEqual(note.observation_dates, [first, end])
                    self.assertEqual(note.knock_out_levels, [115, 110])
                    if phoenix:
                        self.assertEqual(note.coupon_rate, 0.125)
                        self.assertEqual(note.coupon_barrier_levels, [85, 80])
                    else:
                        self.assertEqual(note.knock_out_coupon_rates, [0.125, 0.25])
                        self.assertEqual(note.maturity_coupon_rate, 0.0625)
                    self.assertEqual(engine.price(note, context), original_price)

    def test_observation_records_preserve_associations_and_own_python_sequences(self):
        start, first, end = date(2025, 1, 1), date(2025, 1, 11), date(2025, 1, 31)
        dates, levels, coupons, barriers = (
            [first, end],
            [110, 105],
            [0.365, 0.73],
            [108, 102],
        )
        common = {
            "knock_out_levels": levels,
            "observation_dates": dates,
            "barrier_state": "none",
            "effective_date": start,
            "expiry_date": end,
        }
        snowball = BinarySnowballOption(
            **common,
            knock_out_coupon_rates=coupons,
            maturity_coupon_rate=0.05,
        )
        phoenix = PhoenixOption(
            **common,
            coupon_rate=0.365,
            initial_spot=100,
            knock_in_level=70,
            upper_strike=100,
            lower_strike=60,
            coupon_barrier_levels=barriers,
            knock_in_observation_mode="every_trading_day",
        )
        dates.reverse()
        levels.reverse()
        coupons.reverse()
        barriers.reverse()
        for note in (snowball, phoenix):
            self.assertEqual(note.observation_dates, [first, end])
            self.assertEqual(note.knock_out_levels, [110, 105])
            note.observation_dates.reverse()
            note.knock_out_levels.reverse()
            self.assertEqual(note.observation_dates, [first, end])
            self.assertEqual(note.knock_out_levels, [110, 105])
        self.assertEqual(snowball.knock_out_coupon_rates, [0.365, 0.73])
        self.assertEqual(phoenix.coupon_barrier_levels, [108, 102])
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04,
                dividend_yield=0.01,
                volatility=0.2,
            ),
            spot_price=106,
            valuation_time=end,
            calendar=all_days_calendar(),
        )
        for engine in (
            pricing.FiniteDifferenceBinarySnowballEngine(),
            pricing.MonteCarloBinarySnowballEngine(path_count=8, seed=7),
        ):
            self.assertAlmostEqual(engine.price(snowball, context), 1.06, delta=1e-14)
        for engine in (
            pricing.FiniteDifferencePhoenixEngine(),
            pricing.MonteCarloPhoenixEngine(path_count=8, seed=7),
        ):
            self.assertAlmostEqual(engine.price(phoenix, context), 1.02, delta=1e-14)
        for principal, category in (
            (-1, kiyosi.ErrorCategory.INVALID_PARAMETER),
            (1, kiyosi.ErrorCategory.INVALID_SCHEDULE),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                BinarySnowballOption(
                    **{**common, "observation_dates": [first, end]},
                    knock_out_coupon_rates=[0.365],
                    maturity_coupon_rate=0.05,
                    principal_ratio=principal,
                )
            self.assertEqual(error.exception.category, category)
