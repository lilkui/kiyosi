"""Exact deterministic BSM prices through supported public pricing engines."""

import math
import unittest
from datetime import datetime, timedelta
from datetime import time as clock_time

from kiyosi import InvalidInputError, pricing
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)

START, END = datetime(2025, 1, 6), datetime(2025, 1, 10)


def cases():
    life = {"effective_time": START, "expiry_time": END}
    vanilla = dict(life, option_type="call", strike=90)
    lambda prefix, families: tuple(prefix + family + "Engine" for family in families)
    yield (
        ins.EuropeanOption(**vanilla),
        (
            "AnalyticEuropeanEngine",
            "CoxRossRubinsteinVanillaEngine",
            "QuadratureVanillaEngine",
            "FiniteDifferenceVanillaEngine",
            "MonteCarloVanillaEngine",
        ),
        10,
    )
    yield (
        ins.AmericanOption(**vanilla),
        (
            "BjerksundStenslandVanillaEngine",
            "CoxRossRubinsteinVanillaEngine",
            "QuadratureVanillaEngine",
            "FiniteDifferenceVanillaEngine",
            "MonteCarloVanillaEngine",
        ),
        10,
    )
    for cls, expected, fields in (
        (ins.CashOrNothingOption, 10, {"payout": 10}),
        (ins.AssetOrNothingOption, 100, {}),
    ):
        yield (
            cls(**vanilla, **fields),
            (
                "AnalyticDigitalEngine",
                "CoxRossRubinsteinDigitalEngine",
                "QuadratureDigitalEngine",
                "FiniteDifferenceDigitalEngine",
                "MonteCarloDigitalEngine",
            ),
            expected,
        )
    for cls, analytic in (
        (ins.GeometricAveragePriceOption, "AnalyticGeometricAveragePriceEngine"),
        (
            ins.ArithmeticAveragePriceOption,
            "TurnbullWakemanArithmeticAveragePriceEngine",
        ),
    ):
        yield (
            cls(**vanilla, averaging_start_time=START),
            (analytic, "MonteCarloAsianEngine"),
            10,
        )
    yield (
        ins.BarrierOption(**vanilla, barrier_level=80, barrier_type="down_and_out"),
        (
            "AnalyticBarrierEngine",
            "CoxRossRubinsteinBarrierEngine",
            "QuadratureBarrierEngine",
            "FiniteDifferenceBarrierEngine",
            "MonteCarloBarrierEngine",
        ),
        10,
    )
    yield (
        ins.DoubleBarrierOption(
            **vanilla, lower_barrier=80, upper_barrier=120, barrier_type="knock_out"
        ),
        (
            "AnalyticDoubleBarrierEngine",
            "FiniteDifferenceDoubleBarrierEngine",
            "MonteCarloDoubleBarrierEngine",
        ),
        10,
    )
    for payout, expected in ((None, 100), (10, 10)):
        yield (
            ins.BinaryBarrierOption(
                payoff_type="cash" if payout is not None else "asset",
                **vanilla,
                barrier_level=80,
                barrier_type="down_and_out",
                payout=payout,
            ),
            ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
            expected,
        )
        yield (
            ins.TouchOption(
                payoff_type="cash" if payout is not None else "asset",
                **life,
                barrier_level=80,
                barrier_type="down_and_out",
                payout=payout,
            ),
            ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
            expected,
        )
    yield (
        ins.DoubleTouchOption(
            **life,
            lower_barrier=80,
            upper_barrier=120,
            barrier_type="knock_out",
            payout=10,
        ),
        ("AnalyticDoubleTouchEngine", "FiniteDifferenceDoubleTouchEngine"),
        10,
    )
    obs = ins.autocallable_observations([END], [120])
    note = dict(life, observations=obs)
    downside = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 0,
    }
    for contract in (
        ins.SnowballOption(
            observation_clock_time=clock_time(),
            **note,
            **downside,
            maturity_coupon_rate=0.1,
        ),
        ins.PhoenixOption(
            observation_clock_time=clock_time(),
            **note,
            **downside,
            coupon_rate=0.1,
        ),
        ins.BinarySnowballOption(**note, maturity_coupon_rate=0.1),
        ins.TernarySnowballOption(
            observation_clock_time=clock_time(),
            **note,
            knock_in_level=80,
            maturity_coupon_rate=0.1,
            minimum_coupon_rate=0,
        ),
    ):
        yield (
            contract,
            (
                "QuadratureAutocallableEngine",
                "FiniteDifferenceAutocallableEngine",
                "MonteCarloAutocallableEngine",
            ),
            1 + 0.1 * 4 / 365,
        )
    yield (
        ins.Accumulator(
            observation_clock_time=clock_time(),
            **life,
            strike=90,
            knock_out_level=120,
            daily_quantity=1,
            acceleration_factor=2,
        ),
        (
            "QuadratureAccumulatorEngine",
            "FiniteDifferenceAccumulatorEngine",
            "MonteCarloAccumulatorEngine",
        ),
        50,
    )
    yield (
        ins.RangeAccrualOption(
            observation_clock_time=clock_time(),
            **life,
            notional=1000,
            annual_coupon_rate=0.1,
            lower_bound=100,
            upper_bound=110,
            calendar=all_days_calendar(),
        ),
        ("AnalyticRangeAccrualEngine",),
        100 * 4 / 365,
    )


def scenarios():
    for option, engines, expected in cases():
        yield option, engines, expected
        if isinstance(
            option,
            (
                ins.EuropeanOption,
                ins.AmericanOption,
                ins.GeometricAveragePriceOption,
                ins.ArithmeticAveragePriceOption,
                ins.CashOrNothingOption,
                ins.AssetOrNothingOption,
                ins.BarrierOption,
                ins.DoubleBarrierOption,
                ins.BinaryBarrierOption,
            ),
        ):
            yield option.replace(option_type="put", strike=110), engines, expected


class DeterministicBsmTests(unittest.TestCase):
    def test_zero_volatility_prices_and_statistics_agree_across_supported_engines(self):
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0
            ),
            spot_price=100,
            valuation_time=START,
        )
        for option, engines, expected in scenarios():
            for name in engines:
                with self.subTest(contract=type(option).__name__, engine=name):
                    engine = getattr(pricing, name)(
                        **(
                            {"path_count": 16, "seed": 42, "thread_count": 1}
                            if name.startswith("MonteCarlo")
                            else {}
                        )
                    )
                    self.assertAlmostEqual(
                        engine.price(option, context), expected, delta=1e-10
                    )
                    result = engine.price_with_greeks(option, context, "all")
                    self.assertAlmostEqual(result.price, expected, delta=1e-10)
                    if isinstance(option, ins.RangeAccrualOption):
                        self.assertEqual(result.status("delta"), "undefined")
                    else:
                        expected_delta = (
                            1
                            if isinstance(option, ins.AssetOrNothingOption)
                            else 5
                            if isinstance(option, ins.Accumulator)
                            else (1 if option.payout is None else 0)
                            if isinstance(
                                option,
                                (
                                    ins.BinaryBarrierOption,
                                    ins.TouchOption,
                                    ins.CashOrNothingOption,
                                ),
                            )
                            else 0
                            if isinstance(
                                option,
                                (
                                    ins.DoubleTouchOption,
                                    ins.SnowballOption,
                                    ins.PhoenixOption,
                                    ins.BinarySnowballOption,
                                    ins.TernarySnowballOption,
                                ),
                            )
                            else -1
                            if option.option_type == "put"
                            else 1
                        )
                        self.assertIsNotNone(result.delta)
                        self.assertAlmostEqual(result.delta, expected_delta, delta=1e-9)
                    for greek in result.requested:
                        value = getattr(result, greek)
                        if value is not None:
                            self.assertTrue(math.isfinite(value))
                    if name.startswith("MonteCarlo") and not isinstance(
                        option, ins.AmericanOption
                    ):
                        estimate = engine.price_with_statistics(option, context)
                        self.assertAlmostEqual(estimate.price, expected, delta=1e-10)
                        self.assertEqual(estimate.standard_error, 0)

    def test_continuous_and_scheduled_hits_use_exact_deterministic_payment_times(self):
        end = START + timedelta(days=365)
        life = {"effective_time": START, "expiry_time": end}
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.1, dividend_yield=0.02, volatility=0
            ),
            spot_price=100,
            valuation_time=START,
        )
        hit = math.log(1.05) / 0.08
        vanilla = 100 * math.exp(-0.02) - 90 * math.exp(-0.1)
        checks = [
            (
                ins.BarrierOption(
                    **life,
                    option_type="call",
                    strike=90,
                    barrier_level=105,
                    barrier_type="up_and_out",
                    rebate=7,
                    rebate_timing="at_hit",
                ),
                (
                    "AnalyticBarrierEngine",
                    "CoxRossRubinsteinBarrierEngine",
                    "QuadratureBarrierEngine",
                    "FiniteDifferenceBarrierEngine",
                    "MonteCarloBarrierEngine",
                ),
                7 * math.exp(-0.1 * hit),
            ),
            (
                ins.BarrierOption(
                    **life,
                    option_type="call",
                    strike=90,
                    barrier_level=105,
                    barrier_type="up_and_in",
                ),
                (
                    "AnalyticBarrierEngine",
                    "FiniteDifferenceBarrierEngine",
                    "MonteCarloBarrierEngine",
                ),
                vanilla,
            ),
            (
                ins.TouchOption(
                    payoff_type="cash",
                    **life,
                    barrier_level=105,
                    barrier_type="up_and_in",
                    payout=7,
                    settlement_timing="at_hit",
                ),
                ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
                7 * math.exp(-0.1 * hit),
            ),
            (
                ins.TouchOption(
                    payoff_type="asset",
                    **life,
                    barrier_level=105,
                    barrier_type="up_and_in",
                    payout=None,
                    settlement_timing="at_hit",
                ),
                ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
                105 * math.exp(-0.1 * hit),
            ),
            (
                ins.DoubleTouchOption(
                    **life,
                    lower_barrier=80,
                    upper_barrier=105,
                    barrier_type="knock_in",
                    payout=7,
                    settlement_timing="at_hit",
                ),
                ("AnalyticDoubleTouchEngine", "FiniteDifferenceDoubleTouchEngine"),
                7 * math.exp(-0.1 * hit),
            ),
            (
                ins.TouchOption(
                    payoff_type="cash",
                    **life,
                    barrier_level=105,
                    barrier_type="up_and_in",
                    payout=7,
                    settlement_timing="at_hit",
                    observation_mode="scheduled",
                    observation_times=[end],
                ),
                ("AnalyticBinaryBarrierEngine", "FiniteDifferenceBinaryBarrierEngine"),
                7 * math.exp(-0.1),
            ),
        ]
        for option, engines, expected in checks:
            for name in engines:
                with self.subTest(contract=type(option).__name__, engine=name):
                    self.assertAlmostEqual(
                        getattr(pricing, name)().price(option, context),
                        expected,
                        delta=1e-10,
                    )

    def test_piecewise_rate_paths_detect_hits_before_reversing(self):
        from kiyosi.market import RateTermStructure

        middle, end = START + timedelta(days=180), START + timedelta(days=365)
        curve = RateTermStructure(
            reference_time=START,
            times=[middle, end],
            factors=[math.exp(-0.2 * 180 / 365), math.exp(0.2 * 5 / 365)],
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=curve, dividend_yield=0, volatility=0
            ),
            spot_price=100,
            valuation_time=START,
        )
        continuous = ins.TouchOption(
            payoff_type="cash",
            effective_time=START,
            expiry_time=end,
            barrier_level=105,
            barrier_type="up_and_in",
            payout=7,
            settlement_timing="at_hit",
        )
        engine = pricing.FiniteDifferenceBinaryBarrierEngine()
        self.assertAlmostEqual(engine.price(continuous, context), 7 / 1.05, delta=1e-10)
        scheduled = continuous.replace(
            observation_mode="scheduled", observation_times=[end]
        )
        self.assertEqual(engine.price(scheduled, context), 0)

    def test_american_exercise_and_seasoned_asian_averages_are_exact(self):
        end = START + timedelta(days=365)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.1, dividend_yield=0.09, volatility=0
            ),
            spot_price=100,
            valuation_time=START,
        )
        american = ins.AmericanOption(
            option_type="call", strike=90.5, effective_time=START, expiry_time=end
        )
        exercise = math.log(0.1 * 90.5 / (0.09 * 100)) / 0.01
        expected = 100 * math.exp(-0.09 * exercise) - 90.5 * math.exp(-0.1 * exercise)
        for name in (
            "BjerksundStenslandVanillaEngine",
            "CoxRossRubinsteinVanillaEngine",
            "QuadratureVanillaEngine",
            "FiniteDifferenceVanillaEngine",
            "MonteCarloVanillaEngine",
        ):
            with self.subTest(engine=name):
                self.assertAlmostEqual(
                    getattr(pricing, name)().price(american, context),
                    expected,
                    delta=1e-10,
                )
        context = context.replace(
            valuation_time=START + timedelta(days=180),
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.08, dividend_yield=0.02, volatility=0
            ),
        )
        remaining = 185 / 365
        arithmetic = 80 * 180 / 365 + 100 * math.expm1(0.06 * remaining) / 0.06
        geometric = math.exp(
            math.log(80) * 180 / 365
            + remaining * (math.log(100) + 0.06 * remaining / 2)
        )
        for cls, analytic, average in (
            (
                ins.ArithmeticAveragePriceOption,
                "TurnbullWakemanArithmeticAveragePriceEngine",
                arithmetic,
            ),
            (
                ins.GeometricAveragePriceOption,
                "AnalyticGeometricAveragePriceEngine",
                geometric,
            ),
        ):
            option = cls(
                option_type="call",
                strike=85,
                effective_time=START,
                averaging_start_time=START,
                expiry_time=end,
                realized_average=80,
            )
            for name in (analytic, "MonteCarloAsianEngine"):
                with self.subTest(engine=name, contract=cls.__name__):
                    self.assertAlmostEqual(
                        getattr(pricing, name)().price(option, context),
                        (average - 85) * math.exp(-0.08 * remaining),
                        delta=1e-10,
                    )

    def test_forward_start_averages_and_sobol_uncertainty_use_the_deterministic_path(
        self,
    ):
        middle, end = START + timedelta(days=180), START + timedelta(days=365)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.08, dividend_yield=0.02, volatility=0
            ),
            spot_price=100,
            valuation_time=START,
        )
        future, lead = 185 / 365, 180 / 365
        arithmetic = (
            100 * math.exp(0.06 * lead) * math.expm1(0.06 * future) / (0.06 * future)
        )
        geometric = 100 * math.exp(0.06 * (lead + future / 2))
        for cls, analytic, average in (
            (
                ins.ArithmeticAveragePriceOption,
                "TurnbullWakemanArithmeticAveragePriceEngine",
                arithmetic,
            ),
            (
                ins.GeometricAveragePriceOption,
                "AnalyticGeometricAveragePriceEngine",
                geometric,
            ),
        ):
            option = cls(
                option_type="call",
                strike=90,
                effective_time=START,
                averaging_start_time=middle,
                expiry_time=end,
            )
            expected = (average - 90) * math.exp(-0.08)
            for name in (analytic, "MonteCarloAsianEngine"):
                with self.subTest(engine=name, contract=cls.__name__):
                    self.assertAlmostEqual(
                        getattr(pricing, name)().price(option, context),
                        expected,
                        delta=1e-10,
                    )
            estimate = pricing.MonteCarloAsianEngine(
                path_count=1, sampling="sobol", antithetic=True
            ).price_with_statistics(option, context)
            self.assertAlmostEqual(estimate.price, expected, delta=1e-10)
            self.assertIsNone(estimate.standard_error)
            self.assertIsNone(estimate.confidence_interval)

    def test_future_structured_events_discount_coupons_and_settle_accumulated_units(
        self,
    ):
        middle, end = START + timedelta(days=180), START + timedelta(days=365)
        calendar = all_days_calendar()
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.1, dividend_yield=0, volatility=0
            ),
            spot_price=100,
            valuation_time=START,
        )
        note = ins.BinarySnowballOption(
            effective_time=START,
            expiry_time=end,
            maturity_coupon_rate=0.1,
            observations=ins.autocallable_observations(
                [middle, end], [105, 105], knock_out_coupon_rates=[0.1, 0.1]
            ),
            calendar=calendar,
        )
        for family in ("Quadrature", "FiniteDifference", "MonteCarlo"):
            with self.subTest(product="snowball", engine=family):
                self.assertAlmostEqual(
                    getattr(pricing, family + "AutocallableEngine")().price(
                        note, context
                    ),
                    (1 + 0.1 * 180 / 365) * math.exp(-0.1 * 180 / 365),
                    delta=1e-10,
                )
        accumulator = ins.Accumulator(
            observation_clock_time=clock_time(),
            effective_time=START,
            expiry_time=end,
            strike=90,
            knock_out_level=105,
            daily_quantity=1,
            acceleration_factor=2,
            knock_out_observation_times=[middle],
            calendar=calendar,
        )
        expected = (
            180 * (100 * math.exp(0.1 * 180 / 365) - 90) * math.exp(-0.1 * 180 / 365)
        )
        for family in ("Quadrature", "FiniteDifference", "MonteCarlo"):
            with self.subTest(product="accumulator", engine=family):
                self.assertAlmostEqual(
                    getattr(pricing, family + "AccumulatorEngine")().price(
                        accumulator, context
                    ),
                    expected,
                    delta=1e-8,
                )

    def test_expiry_only_knock_in_ignores_earlier_recovered_breaches(self):
        end = START + timedelta(days=365)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.6, dividend_yield=0, volatility=0
            ),
            spot_price=70,
            valuation_time=START,
        )
        note = ins.SnowballOption(
            observation_clock_time=None,
            effective_time=START,
            expiry_time=end,
            maturity_coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            knock_in_observation_mode="at_expiry",
            observations=ins.autocallable_observations(
                [START + timedelta(days=30), end], [300, 300]
            ),
            calendar=all_days_calendar(),
        )
        for family in ("Quadrature", "FiniteDifference", "MonteCarlo"):
            with self.subTest(engine=family):
                self.assertAlmostEqual(
                    getattr(pricing, family + "AutocallableEngine")().price(
                        note, context
                    ),
                    1.1 * math.exp(-0.6),
                    delta=1e-10,
                )

    def test_deterministic_boundaries_and_zero_implied_volatility_are_explicit(self):
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0
            ),
            spot_price=100,
            valuation_time=START,
        )
        option = ins.EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        for name in (
            "AnalyticEuropeanEngine",
            "CoxRossRubinsteinVanillaEngine",
            "FiniteDifferenceVanillaEngine",
        ):
            engine = getattr(pricing, name)()
            with self.subTest(engine=name):
                result = engine.price_with_greeks(option, context, "all")
                self.assertEqual(result.status("delta"), "undefined")
                self.assertEqual(result.status("vega"), "undefined")
                self.assertEqual(
                    pricing.implied_volatility(engine, option, context, 0),
                    0,
                )
                digital = ins.CashOrNothingOption(
                    option_type="call",
                    strike=100,
                    payout=10,
                    effective_time=START,
                    expiry_time=END,
                )
                self.assertEqual(
                    pricing.AnalyticDigitalEngine().price(digital, context), 0
                )
        regular = option.replace(strike=90)
        result = pricing.AnalyticEuropeanEngine().price_with_greeks(
            regular, context, "all"
        )
        self.assertAlmostEqual(result.delta, 1, delta=1e-10)
        self.assertAlmostEqual(result.gamma, 0, delta=1e-10)
        self.assertIsNone(result.vega)
        self.assertEqual(result.status("vega"), "undefined")
        for bad in (-0.1, math.nan, math.inf):
            with self.assertRaises(InvalidInputError):
                context.model_parameters.replace(volatility=bad)

    def test_negative_and_nonfinite_volatility_remain_invalid(self):
        for sigma in (-0.1, math.nan, math.inf):
            with self.subTest(sigma=sigma), self.assertRaises(InvalidInputError):
                BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=sigma
                )


if __name__ == "__main__":
    unittest.main()
