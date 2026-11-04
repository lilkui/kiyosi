"""Immutable value copying and transport through spawned Python processes."""

import copy
import inspect
import multiprocessing
import pickle
import unittest
from concurrent.futures import ProcessPoolExecutor
from datetime import datetime

from kiyosi import InvalidInputError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p

START, END = datetime(2025, 1, 2), datetime(2025, 1, 10)


def price_in_worker(engine, option, context):
    return engine.price_with_greeks(option, context, "delta")


def values():
    life = {"effective_time": START, "expiry_time": END}
    vanilla = {"option_type": "call", "strike": 100, **life}
    curve = m.RateTermStructure(
        reference_time=START, times=[END], factors=[0.99]
    ).shifted(0.01)
    surface = m.LocalVolatilitySurface(
        times=[START, END], spots=[80, 120], values=[[0.2, 0.3], [0.3, 0.4]]
    )
    model = m.BlackScholesMertonParameters(
        risk_free_rate=curve, dividend_yield=0.02, volatility=0.2
    )
    calendar = m.custom_calendar(
        base=m.custom_calendar(base=m.sse_calendar(), closed_dates=[START.date()]),
        open_dates=[datetime(2025, 1, 4).date()],
        trading_days_per_year=260,
    )
    schedule = m.monthly_schedule(
        start_time=START, end_time=END, first_observation_month=1
    )
    common = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 0,
        "observations": [i.AutocallableObservation(END, 110)],
        **life,
    }
    yield from (
        curve,
        m.RateTermStructure.flat(0.02),
        m.RateTermStructure.flat(-0.01, reference_time=START),
        surface,
        model,
        m.LocalVolatilityParameters(
            risk_free_rate=curve, dividend_yield=0, surface=surface
        ),
        m.HestonParameters(
            risk_free_rate=curve,
            dividend_yield=0,
            initial_variance=0.04,
            long_run_variance=0.04,
            mean_reversion_speed=1,
            vol_of_vol=0.3,
            correlation=-0.5,
        ),
        calendar,
        m.weekdays_calendar(),
        m.all_days_calendar(),
        schedule,
        m.fixed_interval_schedule(start_time=START, end_time=END, interval_days=2),
        m.PricingContext(
            model_parameters=model,
            spot_price=100,
            valuation_time=START,
            calendar=calendar,
        ),
        i.AutocallableObservation(END, 110),
        i.EuropeanOption(**vanilla),
        i.AmericanOption(**vanilla),
        i.CashOrNothingOption(payout=10, **vanilla),
        i.AssetOrNothingOption(**vanilla),
        i.GeometricAveragePriceOption(averaging_start_time=START, **vanilla),
        i.ArithmeticAveragePriceOption(averaging_start_time=START, **vanilla),
        i.BarrierOption(
            barrier_level=120,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_times=[END],
            touch_state="untouched",
            **vanilla,
        ),
        i.DoubleBarrierOption(
            lower_barrier=80, upper_barrier=120, barrier_type="knock_out", **vanilla
        ),
        i.RangeAccrualOption(
            notional=100,
            annual_coupon_rate=0.1,
            lower_bound=80,
            upper_bound=120,
            realized_hit_count=0,
            **life,
        ),
        i.Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            **life,
        ),
        i.Accumulator(
            strike=100,
            knock_out_level=110,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
            knock_out_observation_times=[],
            **life,
        ),
        i.SnowballOption(maturity_coupon_rate=0.1, **common),
        i.PhoenixOption(coupon_rate=0.1, **common),
        i.PhoenixOption(coupon_per_period=0.01, **common),
        i.BinarySnowballOption(
            maturity_coupon_rate=0.1, observations=common["observations"], **life
        ),
        i.TernarySnowballOption(
            maturity_coupon_rate=0.1,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            observations=common["observations"],
            **life,
        ),
    )
    for factory, cash in (
        (i.cash_binary_barrier_option, True),
        (i.asset_binary_barrier_option, False),
    ):
        yield factory(
            barrier_level=120,
            barrier_type="up_and_out",
            **vanilla,
            **({"payout": 10} if cash else {}),
        )
    for factory, cash in ((i.cash_touch_option, True), (i.asset_touch_option, False)):
        for kind in ("up_and_in", "down_and_out"):
            yield factory(
                barrier_level=120,
                barrier_type=kind,
                **life,
                **({"payout": 10} if cash else {}),
            )
    for factory in (i.cash_double_touch, i.cash_double_no_touch):
        yield factory(lower_barrier=80, upper_barrier=120, payout=10, **life)


class SerializationTests(unittest.TestCase):
    def test_values_round_trip_and_immutable_copies_keep_identity(self):
        for value in values():
            with self.subTest(value=type(value).__name__):
                self.assertIs(copy.copy(value), value)
                self.assertIs(copy.deepcopy(value), value)
                restored = pickle.loads(pickle.dumps(value))
                self.assertEqual(restored, value)
                self.assertIsNot(restored, value)
        for name in p.__all__:
            if name.endswith("Engine"):
                engine = getattr(p, name)()
                restored = pickle.loads(pickle.dumps(engine))
                self.assertIs(type(restored), type(engine))
                for field in inspect.signature(type(engine)).parameters:
                    self.assertEqual(getattr(restored, field), getattr(engine, field))

    def test_nondefault_engine_settings_survive_serialization(self):
        cases = (
            (
                p.MonteCarloVanillaEngine,
                {
                    "path_count": 128,
                    "step_count": 4,
                    "seed": 77,
                    "thread_count": 1,
                    "sampling": "sobol",
                    "antithetic": True,
                },
            ),
            (
                p.FiniteDifferenceVanillaEngine,
                {
                    "asset_step_count": 120,
                    "time_step_count": 30,
                    "scheme": "implicit_euler",
                    "asset_upper_boundary": 500,
                },
            ),
        )
        for factory, fields in cases:
            engine = factory(**fields)
            restored = pickle.loads(pickle.dumps(engine))
            for name, expected in fields.items():
                with self.subTest(engine=factory.__name__, field=name):
                    self.assertEqual(getattr(restored, name), expected)

    def test_spawn_transports_contracts_contexts_engines_and_results(self):
        option = i.EuropeanOption(
            option_type="call",
            strike=100,
            effective_time=START,
            expiry_time=END,
        )
        context = m.PricingContext(
            model_parameters=m.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=START,
        )
        engine = p.AnalyticEuropeanEngine()
        expected = price_in_worker(engine, option, context)
        with ProcessPoolExecutor(
            max_workers=1, mp_context=multiprocessing.get_context("spawn")
        ) as pool:
            actual = pool.submit(price_in_worker, engine, option, context).result(
                timeout=30
            )
        self.assertEqual(actual.price, expected.price)
        self.assertEqual(actual.delta, expected.delta)
        self.assertIsNone(actual.gamma)
        estimate = p.MonteCarloVanillaEngine(
            path_count=64, seed=42
        ).price_with_statistics(option, context)
        restored = pickle.loads(pickle.dumps(estimate))
        self.assertEqual(
            (restored.price, restored.standard_error, restored.confidence_interval),
            (estimate.price, estimate.standard_error, estimate.confidence_interval),
        )

    def test_reconstruction_retains_domain_validation(self):
        from kiyosi._serialization import _restore

        with self.assertRaises(InvalidInputError):
            _restore(
                i.EuropeanOption,
                {
                    "option_type": "call",
                    "strike": -1,
                    "effective_time": START,
                    "expiry_time": END,
                },
            )
