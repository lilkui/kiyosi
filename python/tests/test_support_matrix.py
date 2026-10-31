"""CPU product/engine dispatch and terminal contracts through the public API."""

import unittest
from datetime import date

from kiyosi import UnsupportedOperationError, pricing
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)


def terminal_cases():
    dates = {"effective_date": date(2025, 1, 1), "expiry_date": date(2026, 1, 1)}
    vanilla = dict(dates, option_type="call", strike=100)
    barrier = dict(
        vanilla, barrier_type="up_and_out", barrier_level=120, touch_state="untouched"
    )
    cash_digital = (
        "AnalyticDigitalEngine",
        "CoxRossRubinsteinDigitalEngine",
        "FiniteDifferenceDigitalEngine",
        "MonteCarloDigitalEngine",
    )
    binary = ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine")
    notes = (
        "QuadratureAutocallableEngine",
        "FiniteDifferenceAutocallableEngine",
        "MonteCarloAutocallableEngine",
    )
    cases = [
        (
            ins.EuropeanOption(**vanilla),
            (
                "AnalyticEuropeanEngine",
                "CoxRossRubinsteinVanillaEngine",
                "FiniteDifferenceVanillaEngine",
                "MonteCarloVanillaEngine",
            ),
            5,
        ),
        (
            ins.AmericanOption(**vanilla),
            (
                "BjerksundStenslandVanillaEngine",
                "CoxRossRubinsteinVanillaEngine",
                "FiniteDifferenceVanillaEngine",
                "MonteCarloVanillaEngine",
            ),
            5,
        ),
        (ins.CashOrNothingOption(**vanilla, payout=10), cash_digital, 10),
        (ins.AssetOrNothingOption(**vanilla), cash_digital, 105),
        (
            ins.BarrierOption(**barrier),
            (
                "AnalyticBarrierEngine",
                "CoxRossRubinsteinBarrierEngine",
                "QuadratureBarrierEngine",
                "FiniteDifferenceBarrierEngine",
                "MonteCarloBarrierEngine",
            ),
            5,
        ),
        (ins.cash_binary_barrier_option(**barrier, payout=10), binary, 10),
        (ins.asset_binary_barrier_option(**barrier), binary, 105),
        (
            ins.DoubleBarrierOption(
                **vanilla,
                barrier_type="knock_out",
                lower_barrier=80,
                upper_barrier=120,
                touch_state="untouched",
            ),
            (
                "AnalyticDoubleBarrierEngine",
                "FiniteDifferenceDoubleBarrierEngine",
                "MonteCarloDoubleBarrierEngine",
            ),
            5,
        ),
        (
            ins.GeometricAveragePriceOption(
                **vanilla,
                averaging_start_date=dates["effective_date"],
                realized_average=110,
            ),
            ("AnalyticGeometricAveragePriceEngine", "MonteCarloAsianEngine"),
            10,
        ),
        (
            ins.ArithmeticAveragePriceOption(
                **vanilla,
                averaging_start_date=dates["effective_date"],
                realized_average=110,
            ),
            ("TurnbullWakemanArithmeticAveragePriceEngine", "MonteCarloAsianEngine"),
            10,
        ),
        (
            ins.Accumulator(
                **dates,
                strike=100,
                knock_out_level=120,
                daily_quantity=1,
                acceleration_factor=2,
                accumulated_quantity=3,
            ),
            (
                "QuadratureAccumulatorEngine",
                "FiniteDifferenceAccumulatorEngine",
                "MonteCarloAccumulatorEngine",
            ),
            20,
        ),
        (
            ins.RangeAccrualOption(
                **dates,
                notional=1000,
                annual_coupon_rate=0.12,
                lower_bound=95,
                upper_bound=110,
                realized_hit_count=0,
            ),
            ("AnalyticRangeAccrualEngine",),
            120 / 365,
        ),
    ]
    for factory, amount in (
        (ins.cash_one_touch_up, 0),
        (ins.cash_no_touch_up, 10),
        (ins.asset_one_touch_up, 0),
        (ins.asset_no_touch_up, 105),
    ):
        payout = {"payout": 10} if factory.__name__.startswith("cash") else {}
        cases.append(
            (
                factory(**dates, barrier_level=120, touch_state="untouched", **payout),
                binary,
                amount,
            )
        )
    for factory, amount in ((ins.cash_double_touch, 0), (ins.cash_double_no_touch, 10)):
        cases.append(
            (
                factory(
                    **dates,
                    lower_barrier=80,
                    upper_barrier=120,
                    payout=10,
                    touch_state="untouched",
                ),
                ("FiniteDifferenceDoubleTouchEngine",),
                amount,
            )
        )
    common = dict(
        dates,
        observations=ins.autocallable_observations(
            [dates["expiry_date"]], [120], knock_out_coupon_rates=[0.08]
        ),
    )
    loss = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 60,
        "knock_in_observation_mode": "at_expiry",
        "barrier_state": "none",
    }
    cases.extend(
        [
            (
                ins.SnowballOption(**common, **loss, maturity_coupon_rate=0.06),
                notes,
                1.06,
            ),
            (
                ins.BinarySnowballOption(**common, maturity_coupon_rate=0.06),
                notes,
                1.06,
            ),
            (
                ins.TernarySnowballOption(
                    **common,
                    maturity_coupon_rate=0.06,
                    minimum_coupon_rate=-0.02,
                    knock_in_level=80,
                    knock_in_observation_mode="at_expiry",
                    barrier_state="none",
                ),
                notes,
                1.06,
            ),
            (
                ins.PhoenixOption(
                    **dates,
                    **loss,
                    coupon_rate=0.08,
                    observations=ins.autocallable_observations(
                        [dates["expiry_date"]], [120], coupon_barrier_levels=[90]
                    ),
                ),
                notes,
                1.08,
            ),
        ]
    )
    return cases


class SupportMatrixTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cases = terminal_cases()
        cls.engines = {
            name: getattr(pricing, name)()
            for name in pricing.__all__
            if name.endswith("Engine")
        }
        cls.context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=105,
            valuation_time=date(2026, 1, 1),
            calendar=all_days_calendar(),
        )

    def test_every_exported_engine_has_an_accepted_product(self):
        self.assertEqual(
            set(self.engines), {name for _, names, _ in self.cases for name in names}
        )

    def test_supported_terminal_prices_greeks_and_unidentifiable_volatility(self):
        for instrument, supported, expected in self.cases:
            for name in supported:
                engine = self.engines[name]
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    self.assertAlmostEqual(
                        engine.price(instrument, self.context), expected
                    )
                    result = engine.price_with_greeks(
                        instrument, self.context, ["delta"]
                    )
                    self.assertAlmostEqual(result.price, expected)
                    self.assertIsNone(result.delta)
                    with self.assertRaises(UnsupportedOperationError) as error:
                        pricing.implied_volatility(
                            engine, instrument, self.context, expected
                        )
                    self.assertIs(type(error.exception), UnsupportedOperationError)

    def test_other_product_engine_pairs_raise_domain_errors(self):
        for instrument, supported, _ in self.cases:
            for name, engine in self.engines.items():
                if name in supported:
                    continue
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    with self.assertRaises(UnsupportedOperationError):
                        engine.price(instrument, self.context)
                    with self.assertRaises(UnsupportedOperationError):
                        engine.price_with_greeks(instrument, self.context, ["delta"])
                    with self.assertRaises(UnsupportedOperationError):
                        pricing.implied_volatility(engine, instrument, self.context, 1)

    def test_invalid_instrument_representations_keep_native_errors(self):
        for name, engine in self.engines.items():
            for instrument in (
                None,
                True,
                100,
                "option",
                object(),
                self.context,
                engine,
            ):
                with self.subTest(engine=name, instrument=instrument):
                    with self.assertRaises(TypeError):
                        engine.price(instrument, self.context)
                    with self.assertRaises(TypeError):
                        engine.price_with_greeks(instrument, self.context, ["delta"])
                    with self.assertRaises(TypeError):
                        pricing.implied_volatility(engine, instrument, self.context, 1)

    def test_statistics_reject_unsupported_products_as_domain_errors(self):
        products = {
            "MonteCarloVanillaEngine": (ins.EuropeanOption,),
            "MonteCarloAccumulatorEngine": (ins.Accumulator,),
            "MonteCarloAutocallableEngine": (
                ins.SnowballOption,
                ins.BinarySnowballOption,
                ins.TernarySnowballOption,
                ins.PhoenixOption,
            ),
        }
        for name, accepted in products.items():
            engine = self.engines[name]
            for instrument, _, _ in self.cases:
                if isinstance(instrument, accepted):
                    continue
                with (
                    self.subTest(engine=name, instrument=type(instrument).__name__),
                    self.assertRaises(UnsupportedOperationError),
                ):
                    engine.price_with_statistics(instrument, self.context)
            with self.assertRaises(TypeError):
                engine.price_with_statistics(object(), self.context)

    def test_coupon_solver_rejects_unsupported_products_and_engines(self):
        accepted = (
            ins.SnowballOption,
            ins.BinarySnowballOption,
            ins.TernarySnowballOption,
            ins.PhoenixOption,
        )
        note = next(
            product for product, _, _ in self.cases if isinstance(product, accepted)
        )
        for name, engine in self.engines.items():
            if name.endswith("AutocallableEngine"):
                for product, _, _ in self.cases:
                    if isinstance(product, accepted):
                        continue
                    with (
                        self.subTest(engine=name, instrument=type(product).__name__),
                        self.assertRaises(UnsupportedOperationError),
                    ):
                        pricing.implied_coupon(
                            engine,
                            product,
                            self.context,
                            1,
                            quote_convention="preserve_maturity_coupon",
                        )
            else:
                with (
                    self.subTest(engine=name),
                    self.assertRaises(UnsupportedOperationError),
                ):
                    pricing.implied_coupon(
                        engine,
                        note,
                        self.context,
                        1,
                        quote_convention="preserve_maturity_coupon",
                    )
