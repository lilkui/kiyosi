"""Sampling uncertainty follows the same contracts across Monte Carlo products."""

import math
import pickle
import unittest
from datetime import datetime, time, timedelta

from kiyosi import ComputationFailureError, InvalidInputError, UnsupportedOperationError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from support.history import dated as dated_history
from support.history import history_fields

START, END = datetime(2025, 1, 2), datetime(2026, 1, 2)
LIFE = {"effective_time": START, "expiry_time": END}


def contracts():
    vanilla = {"option_type": "call", "strike": 100, **LIFE}
    return (
        (
            "cash",
            p.MonteCarloDigitalEngine,
            i.CashOrNothingOption(payout=10, **vanilla),
        ),
        ("asset", p.MonteCarloDigitalEngine, i.AssetOrNothingOption(**vanilla)),
        (
            "geometric",
            p.MonteCarloAsianEngine,
            i.GeometricAveragePriceOption(
                **history_fields(averaging_start_time=START, **vanilla)
            ),
        ),
        (
            "arithmetic",
            p.MonteCarloAsianEngine,
            i.ArithmeticAveragePriceOption(
                **history_fields(averaging_start_time=START, **vanilla)
            ),
        ),
        (
            "barrier",
            p.MonteCarloBarrierEngine,
            i.BarrierOption(
                **history_fields(
                    barrier_type="up_and_out", barrier_level=120, **vanilla
                )
            ),
        ),
        (
            "double",
            p.MonteCarloDoubleBarrierEngine,
            i.DoubleBarrierOption(
                **history_fields(
                    barrier_type="knock_out",
                    lower_barrier=80,
                    upper_barrier=120,
                    **vanilla,
                )
            ),
        ),
    )


def context():
    return m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        ),
        spot_price=100,
        valuation_time=START,
    )


def engine(factory, **changes):
    settings = {"path_count": 4096, "seed": 42, "max_active_workers": 1, **changes}
    if factory is p.MonteCarloAsianEngine:
        settings["step_count"] = 8
    return factory(**settings)


def seed_cases():
    return (
        *contracts(),
        (
            "vanilla",
            p.MonteCarloVanillaEngine,
            i.EuropeanOption(option_type="call", strike=100, **LIFE),
        ),
        (
            "snowball",
            p.MonteCarloAutocallableEngine,
            i.standard_snowball(
                **history_fields(
                    coupon_rate=0.1,
                    initial_spot=100,
                    knock_in_level=80,
                    knock_out_level=110,
                    observation_times=[END],
                    observation_clock_time=time(),
                    **LIFE,
                )
            ),
        ),
        (
            "accumulator",
            p.MonteCarloAccumulatorEngine,
            i.Accumulator(
                **history_fields(
                    strike=100,
                    knock_out_level=120,
                    daily_quantity=1,
                    acceleration_factor=2,
                    observation_clock_time=time(),
                    **LIFE,
                )
            ),
        ),
    )


class MonteCarloStatisticsTests(unittest.TestCase):
    def test_scalar_seeds_replay_all_products_without_mutating_engines(self):
        snapshot = context()
        cases = (
            *seed_cases(),
            (
                "american",
                p.MonteCarloVanillaEngine,
                i.AmericanOption(option_type="put", strike=100, **LIFE),
            ),
        )
        deterministic = snapshot.replace(
            model_parameters=snapshot.model_parameters.replace(volatility=0)
        )
        for name, factory, option in cases:
            for sampling in ("pseudo_random", "sobol"):
                for configured in (None, 2**64 - 1):
                    with self.subTest(product=name, sampling=sampling, seed=configured):
                        sampler = engine(
                            factory, path_count=32, seed=configured, sampling=sampling
                        )
                        value, seed = sampler.price_with_seed(option, snapshot)
                        self.assertIsNotNone(seed)
                        if configured is not None:
                            self.assertEqual(seed, configured)
                        self.assertEqual(sampler.seed, configured)
                        replay = sampler.replace(seed=seed, max_active_workers=4)
                        self.assertEqual(replay.price(option, snapshot), value)
                        self.assertEqual(
                            replay.price_with_seed(option, snapshot), (value, seed)
                        )
                        self.assertEqual(
                            sampler.price_with_seed(option, deterministic),
                            (sampler.price(option, deterministic), None),
                        )
                        with self.assertRaises(InvalidInputError):
                            sampler.price_with_seed(
                                option,
                                snapshot.replace(
                                    valuation_time=END + timedelta(days=1)
                                ),
                            )

    def test_greek_seeds_replay_native_numerical_and_pickled_results(self):
        snapshot = context()
        cases = (
            *seed_cases(),
            (
                "american",
                p.MonteCarloVanillaEngine,
                i.AmericanOption(option_type="put", strike=100, **LIFE),
            ),
        )
        for name, factory, option in cases:
            option = dated_history(option, snapshot)
            sampler = engine(factory, path_count=32, seed=None)
            for numerical in (False, True):
                with self.subTest(product=name, numerical=numerical):
                    calculate = (
                        (
                            lambda e, option=option: p.numerical_greeks(
                                e, option, snapshot, "delta"
                            )
                        )
                        if numerical
                        else (
                            lambda e, option=option: e.price_with_greeks(
                                option, snapshot, "delta"
                            )
                        )
                    )
                    result = calculate(sampler)
                    self.assertIsNotNone(result.seed)
                    self.assertIsNone(sampler.seed)
                    self.assertEqual(
                        calculate(sampler.replace(seed=result.seed)), result
                    )
                    self.assertEqual(pickle.loads(pickle.dumps(result)), result)
                    self.assertEqual(
                        sampler.replace(seed=result.seed).price(option, snapshot),
                        result.price,
                    )
            deterministic = snapshot.replace(
                model_parameters=snapshot.model_parameters.replace(volatility=0)
            )
            self.assertIsNone(
                sampler.price_with_greeks(option, deterministic, "delta").seed
            )

    def test_reported_seeds_replay_unseeded_and_configured_estimates(self):
        cases = seed_cases()
        snapshot = context()
        deterministic = snapshot.replace(
            model_parameters=snapshot.model_parameters.replace(volatility=0)
        )
        for name, factory, option in cases:
            for sampling in ("pseudo_random", "sobol"):
                for configured in (None, 2**64 - 1):
                    with self.subTest(product=name, sampling=sampling, seed=configured):
                        sampler = engine(
                            factory,
                            path_count=256,
                            seed=configured,
                            sampling=sampling,
                            antithetic=True,
                        )
                        estimate = sampler.price_with_statistics(
                            dated_history(option, snapshot), snapshot
                        )
                        self.assertIsNotNone(estimate.seed)
                        self.assertGreaterEqual(estimate.seed, 0)
                        self.assertLessEqual(estimate.seed, 2**64 - 1)
                        if configured is not None:
                            self.assertEqual(estimate.seed, configured)
                        self.assertEqual(sampler.seed, configured)
                        replay = engine(
                            factory,
                            path_count=256,
                            seed=estimate.seed,
                            sampling=sampling,
                            antithetic=True,
                            max_active_workers=4,
                        )
                        self.assertEqual(
                            replay.price_with_statistics(
                                dated_history(option, snapshot), snapshot
                            ),
                            estimate,
                        )
                        self.assertEqual(
                            replay.price(dated_history(option, snapshot), snapshot),
                            estimate.price,
                        )
                        restored = pickle.loads(pickle.dumps(estimate))
                        self.assertEqual(restored, estimate)
                        self.assertEqual(restored.seed, estimate.seed)
                        self.assertIsNone(
                            sampler.price_with_statistics(
                                dated_history(option, deterministic), deterministic
                            ).seed
                        )

    def test_statistics_preserve_prices_and_uncertainty_across_products_and_threads(
        self,
    ):
        snapshot = context()
        for name, factory, option in contracts():
            for antithetic in (False, True):
                with self.subTest(product=name, antithetic=antithetic):
                    serial = engine(factory, antithetic=antithetic)
                    estimate = serial.price_with_statistics(
                        dated_history(option, snapshot), snapshot
                    )
                    self.assertEqual(
                        estimate.price,
                        serial.price(dated_history(option, snapshot), snapshot),
                    )
                    self.assertGreater(estimate.standard_error, 0)
                    self.assertEqual(estimate.uncertainty_status, "available")
                    lower, upper = estimate.confidence_interval
                    self.assertLess(lower, estimate.price)
                    self.assertLess(estimate.price, upper)
                    parallel = engine(
                        factory, max_active_workers=4, antithetic=antithetic
                    )
                    for other in (
                        parallel.price_with_statistics(
                            dated_history(option, snapshot), snapshot
                        ),
                        pickle.loads(pickle.dumps(estimate)),
                    ):
                        self.assertEqual(other.price, estimate.price)
                        self.assertEqual(other.standard_error, estimate.standard_error)
                        self.assertEqual(
                            other.confidence_interval, estimate.confidence_interval
                        )

    def test_digital_uncertainty_matches_bernoulli_and_pair_formulas(self):
        option = contracts()[0][2]
        amount = 10 * math.exp(-0.05)
        for antithetic in (False, True):
            with self.subTest(antithetic=antithetic):
                estimate = engine(
                    p.MonteCarloDigitalEngine, antithetic=antithetic
                ).price_with_statistics(dated_history(option, context()), context())
                probability = estimate.price / amount
                scale, count = amount, 4096
                if antithetic:
                    probability = 2 * probability - 1
                    scale, count = amount / 2, 2048
                error = scale * math.sqrt(probability * (1 - probability) / (count - 1))
                self.assertAlmostEqual(estimate.standard_error, error, delta=1e-13)
                lower, upper = estimate.confidence_interval
                self.assertAlmostEqual(
                    lower, estimate.price - 1.959963984540054 * error, delta=1e-12
                )
                self.assertAlmostEqual(
                    upper, estimate.price + 1.959963984540054 * error, delta=1e-12
                )

    def test_reported_uncertainty_agrees_with_analytic_prices(self):
        references = {
            "cash": p.AnalyticDigitalEngine(),
            "asset": p.AnalyticDigitalEngine(),
            "geometric": p.AnalyticGeometricAveragePriceEngine(),
            "barrier": p.AnalyticBarrierEngine(),
            "double": p.AnalyticDoubleBarrierEngine(),
        }
        for name, factory, option in contracts():
            if name not in references:
                continue
            with self.subTest(product=name):
                snapshot = context()
                estimate = engine(factory).price_with_statistics(
                    dated_history(option, snapshot), snapshot
                )
                expected = references[name].price(
                    dated_history(option, snapshot), snapshot
                )
                self.assertLess(
                    abs(estimate.price - expected), 6 * estimate.standard_error
                )

    def test_uncertainty_omission_follows_sampling_and_independent_counts(self):
        cases = (
            ("single", {"path_count": 1}, "insufficient_observations"),
            (
                "single_pair",
                {"path_count": 2, "antithetic": True},
                "insufficient_observations",
            ),
            (
                "unpaired",
                {"path_count": 3, "antithetic": True},
                "incomplete_antithetic_pair",
            ),
            ("sobol", {"path_count": 16, "sampling": "sobol"}, "unsupported_sampling"),
        )
        for case, settings, status in cases:
            for name, factory, option in contracts():
                with self.subTest(case=case, product=name):
                    sampler = engine(factory, **settings)
                    estimate = sampler.price_with_statistics(
                        dated_history(option, context()), context()
                    )
                    self.assertEqual(
                        estimate.price,
                        sampler.price(dated_history(option, context()), context()),
                    )
                    self.assertIsNone(estimate.standard_error)
                    self.assertIsNone(estimate.confidence_interval)
                    self.assertEqual(estimate.uncertainty_status, status)
                    self.assertEqual(pickle.loads(pickle.dumps(estimate)), estimate)

    def test_terminal_statistics_report_known_values(self):
        snapshot = context().replace(spot_price=110, valuation_time=END)
        for name, factory, original in contracts():
            if name in ("barrier", "double"):
                option = original.replace_history(
                    touch_state="untouched",
                    history_time=original.history_time or original.effective_time,
                )
            elif name in ("geometric", "arithmetic"):
                option = original.replace_history(
                    realized_average=110,
                    history_time=original.history_time or original.effective_time,
                )
            else:
                option = original
            expected_price = 110 if name == "asset" else 10
            for sampling in ("pseudo_random", "sobol"):
                with self.subTest(product=name, sampling=sampling):
                    estimate = engine(
                        factory, path_count=1, sampling=sampling
                    ).price_with_statistics(dated_history(option, snapshot), snapshot)
                    self.assertEqual(estimate.price, expected_price)
                    if sampling == "sobol":
                        self.assertIsNone(estimate.standard_error)
                        self.assertIsNone(estimate.confidence_interval)
                    else:
                        self.assertEqual(estimate.standard_error, 0)
                        self.assertEqual(
                            estimate.confidence_interval,
                            (expected_price, expected_price),
                        )

    def test_analytic_rebate_shifts_do_not_add_sampling_variance(self):
        for name, factory, original in contracts()[-2:]:
            zero = original.replace(rebate_timing="at_hit")
            rebate = zero.replace(rebate=7)
            sampler = engine(factory)
            snapshot = context()
            reference = (
                p.AnalyticBarrierEngine()
                if name == "barrier"
                else p.AnalyticDoubleBarrierEngine()
            )
            with self.subTest(product=name):
                base = sampler.price_with_statistics(
                    dated_history(zero, snapshot), snapshot
                )
                actual = sampler.price_with_statistics(
                    dated_history(rebate, snapshot), snapshot
                )
                shift = reference.price(
                    dated_history(rebate, snapshot), snapshot
                ) - reference.price(dated_history(zero, snapshot), snapshot)
                self.assertEqual(actual.standard_error, base.standard_error)
                self.assertAlmostEqual(actual.price - base.price, shift, delta=1e-12)

    def test_statistics_preserve_scheduled_barrier_prices(self):
        for name, factory, original in contracts()[-2:]:
            option = original.replace(
                touch_state=original.touch_state,
                observation_times=[START + timedelta(days=182), END],
                history_time=(original.history_time or original.effective_time)
                if original.touch_state is not None
                else None,
            )
            with self.subTest(product=name):
                sampler = engine(factory)
                estimate = sampler.price_with_statistics(
                    dated_history(option, context()), context()
                )
                self.assertEqual(
                    estimate.price,
                    sampler.price(dated_history(option, context()), context()),
                )
                self.assertGreater(estimate.standard_error, 0)

    def test_resolved_barriers_report_zero_error_before_expiry(self):
        snapshot = context().replace(spot_price=120)
        vanilla = i.EuropeanOption(option_type="call", strike=100, **LIFE)
        for name, factory, original in contracts()[-2:]:
            for knock_in in (False, True):
                kind = (
                    ("up_and_in" if knock_in else "up_and_out")
                    if name == "barrier"
                    else ("knock_in" if knock_in else "knock_out")
                )
                option = original.replace(barrier_type=kind, rebate=7)
                expected = (
                    p.AnalyticEuropeanEngine().price(
                        dated_history(vanilla, snapshot), snapshot
                    )
                    if knock_in
                    else 7 * math.exp(-0.05)
                )
                with self.subTest(product=name, knock_in=knock_in):
                    estimate = engine(factory).price_with_statistics(
                        dated_history(option, snapshot), snapshot
                    )
                    self.assertAlmostEqual(estimate.price, expected, delta=1e-12)
                    self.assertEqual(estimate.standard_error, 0)
                    self.assertEqual(
                        estimate.confidence_interval, (estimate.price, estimate.price)
                    )

    def test_uncertainty_failure_does_not_prevent_scalar_pricing(self):
        option = contracts()[0][2].replace(payout=1.6e308)
        sampler = engine(p.MonteCarloDigitalEngine, path_count=2, seed=0)
        price = sampler.price(dated_history(option, context()), context())
        self.assertAlmostEqual(price / (8e307 * math.exp(-0.05)), 1, delta=1e-13)
        with self.assertRaises(ComputationFailureError):
            sampler.price_with_statistics(dated_history(option, context()), context())
        self.assertEqual(
            sampler.price(dated_history(option, context()), context()), price
        )
        self.assertEqual(sampler.price_with_seed(option, context()), (price, 0))

    def test_arithmetic_statistics_support_curves_and_known_history(self):
        option = contracts()[3][2]
        curve = m.RateTermStructure(reference_time=START, times=[END], factors=[0.95])
        snapshot = context().replace(
            model_parameters=m.BlackScholesMertonParameters(
                risk_free_rate=curve, dividend_yield=0.02, volatility=0.2
            )
        )
        sampler = engine(p.MonteCarloAsianEngine)
        estimate = sampler.price_with_statistics(
            dated_history(option, snapshot), snapshot
        )
        self.assertEqual(
            estimate.price, sampler.price(dated_history(option, snapshot), snapshot)
        )
        self.assertGreater(estimate.standard_error, 0)
        snapshot = context().replace(
            valuation_time=START + timedelta(days=182, hours=12)
        )
        for direction in ("call", "put"):
            with self.subTest(direction=direction):
                seasoned = option.replace(
                    option_type=direction,
                    realized_average=1000,
                    history_time=option.history_time or option.effective_time,
                )
                estimate = sampler.price_with_statistics(
                    dated_history(seasoned, snapshot), snapshot
                )
                expected = (
                    (400 + 50 * math.expm1(0.015) / 0.015) * math.exp(-0.025)
                    if direction == "call"
                    else 0
                )
                self.assertAlmostEqual(estimate.price, expected, delta=1e-12)
                self.assertEqual(estimate.standard_error, 0)

    def test_statistics_preserve_life_history_and_model_rejections(self):
        snapshot = context()
        heston = m.HestonParameters(
            risk_free_rate=0.05,
            dividend_yield=0.02,
            initial_variance=0.04,
            long_run_variance=0.04,
            mean_reversion_speed=1,
            vol_of_vol=0.2,
            correlation=-0.5,
        )
        for name, factory, option in contracts():
            sampler = engine(factory)
            with self.subTest(product=name):
                with self.assertRaises(InvalidInputError):
                    sampler.price_with_statistics(
                        dated_history(
                            option,
                            snapshot.replace(valuation_time=END + timedelta(days=1)),
                        ),
                        snapshot.replace(valuation_time=END + timedelta(days=1)),
                    )
                with self.assertRaises(UnsupportedOperationError):
                    sampler.price_with_statistics(
                        dated_history(
                            option, snapshot.replace(model_parameters=heston)
                        ),
                        snapshot.replace(model_parameters=heston),
                    )
                if name not in ("cash", "asset"):
                    with self.assertRaises(InvalidInputError):
                        sampler.price_with_statistics(
                            dated_history(
                                option,
                                snapshot.replace(
                                    valuation_time=START + timedelta(days=1)
                                ),
                            ),
                            snapshot.replace(valuation_time=START + timedelta(days=1)),
                        )
        american = i.AmericanOption(option_type="put", strike=100, **LIFE)
        with self.assertRaises(UnsupportedOperationError):
            engine(p.MonteCarloVanillaEngine).price_with_statistics(
                dated_history(american, snapshot), snapshot
            )
