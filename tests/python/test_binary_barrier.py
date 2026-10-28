"""Binary/touch public contracts and the existing independent QuantLib scenarios."""

import importlib
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
import QuantLib as ql
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
)
from kiyosi.pricing import AnalyticBinaryBarrierEngine, implied_volatility

GREEKS = (
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


class BinaryBarrierTests(unittest.TestCase):
    def terms(self, **changes):
        return {
            "option_type": "call",
            "strike": 100,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
            "barrier_level": 120,
            "barrier_type": "up_and_out",
        } | changes

    def touch(self, factory=ins.cash_one_touch_up, **changes):
        terms = self.terms()
        for key in ("option_type", "strike", "barrier_type"):
            terms.pop(key)
        if factory.__name__.startswith("cash"):
            terms["payout"] = 10
        return factory(**(terms | changes))

    def context(
        self,
        spot=100,
        time=date(2025, 1, 1),
        *,
        rate=0.04,
        dividend=0.01,
        sigma=0.2,
        **changes,
    ):
        return PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=rate, dividend_yield=dividend, volatility=sigma
            ),
            spot_price=spot,
            valuation_time=time,
            **changes,
        )

    def assert_category(self, category, action):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            action()
        self.assertEqual(error.exception.category.name, category)

    def test_strict_history_current_and_paid_cash_and_asset_settlement(self):
        engine = AnalyticBinaryBarrierEngine()
        for factory in (ins.cash_one_touch_up, ins.asset_one_touch_up):
            asset = factory is ins.asset_one_touch_up
            for timing in ("at_hit", "at_expiry"):
                option = self.touch(factory, settlement_timing=timing)
                self.assertIsNone(option.touch_state)
                self.assertEqual(option.observation_mode, "continuous")
                self.assertFalse(hasattr(option, "strike"))
                self.assertFalse(hasattr(option, "option_type"))
                amount = 130 if asset else 10
                expected = (
                    amount
                    if timing == "at_hit"
                    else amount * math.exp(-(0.01 if asset else 0.04))
                )
                self.assertAlmostEqual(
                    engine.price(option, self.context(130)), expected, delta=1e-12
                )
                self.assert_category(
                    "INVALID_PARAMETER",
                    lambda option=option: engine.price(
                        option, self.context(130, date(2025, 1, 2))
                    ),
                )
                known = option.replace(touch_state="touched")
                self.assert_category(
                    "INVALID_OPTION",
                    lambda known=known: engine.price(known, self.context(130)),
                )
                context = self.context(time=date(2025, 7, 1))
                expected = (
                    0
                    if timing == "at_hit"
                    else (100 if asset else 10)
                    * math.exp(-(0.01 if asset else 0.04) * 184 / 365)
                )
                self.assertAlmostEqual(
                    engine.price(known, context), expected, delta=1e-12
                )
                self.assert_category(
                    "UNSUPPORTED_OPERATION",
                    lambda context=context, expected=expected, known=known: (
                        implied_volatility(engine, known, context, expected)
                    ),
                )
                self.assertEqual(known.replace(touch_state=None), option)
                self.assertEqual(option.replace(), option)
                with self.assertRaises(ValueError):
                    option.replace(touch_state="unknown")
                with self.assertRaises(TypeError):
                    option.replace(touch_state=True)
                with self.assertRaises(TypeError):
                    option.replace(payout=7)
                self.assertIsNone(option.touch_state)
        for kind in ("up_and_in", "up_and_out"):
            option = ins.cash_binary_barrier_option(
                **self.terms(barrier_type=kind), payout=10
            )
            self.assert_category(
                "INVALID_PARAMETER",
                lambda option=option: engine.price(
                    option, self.context(130, date(2025, 7, 1))
                ),
            )
            known = option.replace(touch_state="touched")
            self.assertEqual(
                engine.price(known, self.context(100, date(2025, 7, 1))) == 0,
                kind == "up_and_out",
            )
            self.assertEqual(known.replace(touch_state=None), option)

    def test_representation_domain_validation_and_failure_state(self):
        for factory in (
            ins.cash_binary_barrier_option,
            ins.asset_binary_barrier_option,
        ):
            extra = {"payout": 10} if factory is ins.cash_binary_barrier_option else {}
            option = factory(**self.terms(), **extra)
            for field in ("strike", "barrier_level", *extra):
                for value in (True, "100", None, 1 + 0j):
                    with (
                        self.subTest(field=field, value=value),
                        self.assertRaises(TypeError),
                    ):
                        factory(**(self.terms() | extra | {field: value}))
            for field in (
                "option_type",
                "barrier_type",
                "observation_mode",
                "touch_state",
            ):
                with self.subTest(field=field), self.assertRaises(TypeError):
                    factory(**(self.terms() | extra | {field: True}))
                with self.subTest(field=field), self.assertRaises(ValueError):
                    factory(**(self.terms() | extra | {field: "unknown"}))
            for fields, category in (
                ({"strike": 0, "barrier_level": 0}, "INVALID_STRIKE"),
                ({"barrier_level": 0}, "INVALID_PARAMETER"),
                ({"expiry_date": date(2024, 1, 1)}, "INVALID_TIME_RANGE"),
                ({"observation_mode": "scheduled"}, "INVALID_SCHEDULE"),
                ({"observation_dates": [date(2025, 1, 2)]}, "INVALID_SCHEDULE"),
                (
                    {
                        "observation_mode": "scheduled",
                        "observation_dates": [date(2026, 1, 2)],
                    },
                    "INVALID_SCHEDULE",
                ),
                (
                    {
                        "observation_mode": "scheduled",
                        "observation_dates": [date(2025, 1, 2)] * 2,
                    },
                    "INVALID_SCHEDULE",
                ),
            ):
                self.assert_category(
                    category,
                    lambda fields=fields, factory=factory, extra=extra: factory(
                        **(self.terms() | extra | fields)
                    ),
                )
            for fields in ({"strike": 7}, {"touch_state": True}):
                with self.assertRaises(TypeError):
                    option.replace(**fields)
            self.assertEqual(option, factory(**self.terms(), **extra))
            with self.assertRaises(AttributeError):
                option.strike = 0
        for payout in (0, -1, math.inf, math.nan):
            self.assert_category(
                "INVALID_PARAMETER", lambda payout=payout: self.touch(payout=payout)
            )
            self.assert_category(
                "INVALID_PARAMETER",
                lambda payout=payout: ins.cash_binary_barrier_option(
                    **self.terms(strike=0), payout=payout
                ),
            )
        for field in (
            "barrier_level",
            "payout",
            "settlement_timing",
            "observation_mode",
            "touch_state",
        ):
            with self.subTest(field=field), self.assertRaises(TypeError):
                self.touch(**{field: True})
        for field in ("settlement_timing", "observation_mode", "touch_state"):
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.touch(**{field: "unknown"})
        with self.assertRaises(TypeError):
            self.touch(ins.cash_no_touch_up, settlement_timing="at_hit")
        with self.assertRaises(TypeError):
            self.touch(effective_date=datetime(2025, 1, 1, tzinfo=UTC))
        with self.assertRaises(TypeError):
            AnalyticBinaryBarrierEngine().price(object(), self.context())

    def test_scheduled_boundaries_history_and_owned_dates(self):
        engine = AnalyticBinaryBarrierEngine()
        fixing = date(2025, 1, 2)
        for factory in (ins.cash_one_touch_up, ins.cash_no_touch_up):
            one = factory is ins.cash_one_touch_up
            option = self.touch(
                factory,
                observation_mode="scheduled",
                observation_dates=(d for d in [fixing]),
            )
            self.assert_category(
                "UNSUPPORTED_OPERATION",
                lambda option=option: engine.price(option, self.context()),
            )
            self.assert_category(
                "INVALID_OPTION",
                lambda option=option: engine.price(
                    option.replace(touch_state="touched"), self.context(120, fixing)
                ),
            )
            expected = 10 * math.exp(-0.04 * 364 / 365) if one else 0
            self.assertAlmostEqual(
                engine.price(option, self.context(120, fixing)), expected, delta=1e-12
            )
            noon = datetime(2025, 1, 2, 12, tzinfo=UTC)
            self.assert_category(
                "INVALID_PARAMETER",
                lambda option=option, noon=noon: engine.price(
                    option, self.context(130, noon)
                ),
            )
            known = option.replace(touch_state="untouched")
            expected = 0 if one else 10 * math.exp(-0.04 * 363 / 365)
            self.assertAlmostEqual(
                engine.price(known, self.context(130, date(2025, 1, 3))),
                expected,
                delta=1e-12,
            )
            dates = known.observation_dates
            dates.clear()
            self.assertEqual(known.observation_schedule.dates, [fixing])
            self.assertEqual(known.observation_dates, [fixing])
            self.assertAlmostEqual(known.mean_observation_year_fraction, 1 / 365)
        weekend = self.touch(
            observation_mode="scheduled",
            observation_dates=[date(2025, 1, 4)],
            touch_state="touched",
        )
        self.assert_category(
            "INVALID_DATE",
            lambda: engine.price(weekend, self.context(time=date(2025, 1, 5))),
        )

    def test_sensitivities_solve_expiry_curves_and_concurrent_reads(self):
        engine = AnalyticBinaryBarrierEngine()
        options = [
            ins.cash_binary_barrier_option(
                **self.terms(touch_state="untouched"), payout=10
            ),
            self.touch(touch_state="untouched"),
        ]
        for option in options:
            context = self.context(time=date(2025, 7, 1))
            result = engine.price_with_greeks(option, context, all_greeks=True)
            self.assertTrue(all(getattr(result, g) is not None for g in GREEKS))
            sigma = implied_volatility(
                engine,
                option,
                context,
                result.price,
                lower_bound=0.15,
                upper_bound=0.25,
            )
            self.assertAlmostEqual(sigma, 0.2, delta=1e-7)
            self.assert_category(
                "INVALID_PARAMETER",
                lambda context=context, option=option: engine.price_with_greeks(
                    option, context, []
                ),
            )
            self.assert_category(
                "INVALID_PARAMETER",
                lambda context=context, option=option: engine.price_with_greeks(
                    option, context, "delta", spot_shift=0
                ),
            )
            boundary = engine.price_with_greeks(
                option, self.context(120), all_greeks=True
            )
            self.assertTrue(all(getattr(boundary, g) is None for g in GREEKS))
            near = engine.price_with_greeks(
                option, self.context(119.995), all_greeks=True
            )
            self.assertIsNone(near.delta)
            self.assertIsNone(near.speed)
            self.assertIsNotNone(near.vega)
            expiry = self.context(time=date(2026, 1, 1))
            terminal = engine.price_with_greeks(option, expiry, all_greeks=True)
            self.assertTrue(all(getattr(terminal, g) is None for g in GREEKS))
            self.assert_category(
                "UNSUPPORTED_OPERATION",
                lambda option=option, expiry=expiry, terminal=terminal: (
                    implied_volatility(engine, option, expiry, terminal.price)
                ),
            )
            self.assert_category(
                "INVALID_TIME_RANGE",
                lambda option=option: engine.price(
                    option,
                    self.context(time=datetime(2026, 1, 1, 0, 0, 0, 1, tzinfo=UTC)),
                ),
            )
            curve = RateTermStructure(
                reference_time=date(2025, 1, 1),
                times=[date(2026, 1, 1)],
                factors=[0.95],
            )
            curves = self.context(rate=curve)
            self.assert_category(
                "UNSUPPORTED_OPERATION",
                lambda curves=curves, option=option: engine.price(option, curves),
            )
            self.assert_category(
                "UNSUPPORTED_OPERATION",
                lambda curves=curves, option=option: engine.price_with_greeks(
                    option, curves, []
                ),
            )
            with ThreadPoolExecutor(4) as pool:
                prices = list(
                    pool.map(
                        lambda _, context=context, option=option: engine.price(
                            option, context
                        ),
                        range(16),
                    )
                )
            self.assertTrue(all(p == result.price for p in prices))
        dense = self.touch(
            observation_mode="scheduled",
            observation_dates=[
                date(2025, 1, 1) + timedelta(days=i) for i in range(1, 366)
            ],
            touch_state="untouched",
        )
        result = engine.price_with_greeks(
            dense,
            self.context(
                time=datetime(2025, 1, 1, 12, tzinfo=UTC), calendar=all_days_calendar()
            ),
            all_greeks=True,
        )
        self.assertTrue(
            all(getattr(result, g) is None for g in ("theta", "charm", "color"))
        )
        self.assertIsNotNone(result.delta)

    def test_existing_quantlib_price_and_greek_scenarios(self):

        reference = importlib.import_module("quantlib_reference.binary")
        saved = ql.SavedSettings()
        self.addCleanup(lambda: saved.__exit__(None, None, None))
        engine = AnalyticBinaryBarrierEngine()
        for identifier, inputs in reference.scenarios():
            with self.subTest(identifier=identifier):
                kind = inputs["BarrierType"]
                asset = inputs["asset_settlement"]
                terms = {
                    "effective_date": date.fromisoformat(inputs["effective_date"]),
                    "expiry_date": date.fromisoformat(inputs["expiry_date"]),
                    "barrier_level": inputs["barrier"],
                    "touch_state": "untouched",
                }
                if not asset:
                    terms["payout"] = inputs["payout"]
                if inputs["option"] == "none":
                    name = f"{'asset' if asset else 'cash'}_{'one_touch' if kind.endswith('_in') else 'no_touch'}_{'up' if kind.startswith('up') else 'down'}"
                    if kind.endswith("_in"):
                        terms["settlement_timing"] = inputs["settlement"]
                    option = getattr(ins, name)(**terms)
                else:
                    factory = (
                        ins.asset_binary_barrier_option
                        if asset
                        else ins.cash_binary_barrier_option
                    )
                    option = factory(
                        **terms,
                        option_type=inputs["option"],
                        strike=inputs["strike"],
                        barrier_type=kind,
                    )
                context = self.context(
                    inputs["spot"],
                    date.fromisoformat(inputs["valuation"]),
                    rate=inputs["rate"],
                    dividend=inputs["dividend"],
                    sigma=inputs["volatility"],
                )
                expected = reference.measure(inputs, "price")
                self.assertAlmostEqual(
                    engine.price(option, context),
                    expected,
                    delta=reference.BUDGET["price"],
                )
                if not reference.exclusions(inputs):
                    result = engine.price_with_greeks(
                        option, context, all_greeks=True, spot_shift=0.02
                    )
                    expected_greeks, metadata = reference.g.reference(
                        inputs,
                        reference.STABILITY,
                        measure_fn=reference.measure,
                        unavailable=reference.exclusions(inputs),
                    )
                    for greek in GREEKS:
                        value = expected_greeks[greek]
                        self.assertAlmostEqual(
                            getattr(result, greek),
                            value,
                            delta=reference.BUDGET[greek]
                            + metadata[f"uncertainty_{greek}"],
                            msg=f"{identifier}: {greek}",
                        )

    def test_subnormal_ratio_retains_half_probability_at_expiry(self):
        spot, barrier = 1e20, 1e-300
        drift = math.log(barrier) - math.log(spot)
        option = self.touch(ins.cash_one_touch_down, barrier_level=barrier, payout=1)
        context = self.context(spot, rate=0, dividend=-drift, sigma=1e-200)
        self.assertEqual(AnalyticBinaryBarrierEngine().price(option, context), 0.5)
