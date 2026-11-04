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
    def test_results_compare_by_value_including_metadata_and_uncertainty(self):
        option = i.EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        context = m.PricingContext(
            model_parameters=m.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=110,
            valuation_time=END,
        )
        engine = p.AnalyticEuropeanEngine()
        result = engine.price_with_greeks(option, context, "delta")
        restored = pickle.loads(pickle.dumps(result))
        self.assertIsNot(result, restored)
        self.assertEqual(result, restored)
        self.assertNotEqual(
            result,
            engine.price_with_greeks(option.replace(strike=99), context, "delta"),
        )
        self.assertNotEqual(
            result,
            engine.price_with_greeks(
                option.replace(option_type="put", strike=120), context, "delta"
            ),
        )
        theta = engine.price_with_greeks(option, context, "theta")
        self.assertNotEqual(theta, engine.price_with_greeks(option, context, "color"))
        digital = i.CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=10,
            effective_time=START,
            expiry_time=END,
        )
        unsupported = p.CoxRossRubinsteinDigitalEngine().price_with_greeks(
            digital, context, "theta"
        )
        self.assertEqual(theta.price, unsupported.price)
        self.assertIsNone(theta.theta)
        self.assertIsNone(unsupported.theta)
        self.assertNotEqual(theta, unsupported)
        estimate = p.MonteCarloVanillaEngine(
            path_count=32, seed=42
        ).price_with_statistics(option, context)
        other = p.MonteCarloVanillaEngine(
            path_count=32, seed=42, sampling="sobol"
        ).price_with_statistics(option, context)
        self.assertEqual(estimate, pickle.loads(pickle.dumps(estimate)))
        self.assertEqual(estimate.price, other.price)
        self.assertNotEqual(estimate, other)
        self.assertNotEqual(estimate, result)
        self.assertNotEqual(result, object())

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

    def test_repr_exposes_terms_settings_and_results(self):
        for value in values():
            with self.subTest(value=type(value).__name__):
                text = repr(value)
                self.assertIn(type(value).__name__, text)
                self.assertNotIn("object at 0x", text)
                self.assertEqual(repr(pickle.loads(pickle.dumps(value))), text)
        vanilla = i.EuropeanOption(
            option_type="call", strike=100, effective_time=START, expiry_time=END
        )
        self.assertIn("strike=100.0", repr(vanilla))
        self.assertIn("option_type='call'", repr(vanilla))
        for name in p.__all__:
            if name.endswith("Engine"):
                engine = getattr(p, name)()
                with self.subTest(engine=name):
                    text = repr(engine)
                    self.assertIn(name, text)
                    self.assertNotIn("object at 0x", text)
                    self.assertEqual(repr(pickle.loads(pickle.dumps(engine))), text)
        self.assertIn(
            "asset_step_count=123",
            repr(p.FiniteDifferenceVanillaEngine(asset_step_count=123)),
        )
        context = m.PricingContext(
            model_parameters=m.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=START,
        )
        self.assertIn("spot_price=100.0", repr(context))
        result = p.AnalyticEuropeanEngine().price_with_greeks(vanilla, context, "delta")
        self.assertIn("price=", repr(result))
        self.assertIn("'delta':", repr(result))
        self.assertEqual(repr(pickle.loads(pickle.dumps(result))), repr(result))
        estimate = p.MonteCarloVanillaEngine(
            path_count=16, seed=42, thread_count=1
        ).price_with_statistics(vanilla, context)
        self.assertIn("standard_error=", repr(estimate))
        self.assertEqual(repr(pickle.loads(pickle.dumps(estimate))), repr(estimate))

    def test_repr_bounds_large_surfaces_and_handles_extreme_curve_shifts(self):
        from datetime import timedelta

        surface = m.LocalVolatilitySurface(
            times=[START + timedelta(days=n) for n in range(20)],
            spots=list(range(1, 21)),
            values=[[0.2] * 20 for _ in range(20)],
        )
        self.assertLess(len(repr(surface)), 2000)
        self.assertIn("...", repr(surface))
        curve = m.RateTermStructure(
            reference_time=START, times=[END], factors=[0.99]
        ).shifted(-1e6)
        self.assertIn(".shifted(-1000000.0)", repr(curve))
        self.assertEqual(repr(pickle.loads(pickle.dumps(curve))), repr(curve))

    def test_repr_memory_is_bounded_for_surfaces_and_nested_contexts(self):
        import tracemalloc
        from datetime import timedelta

        surface = m.LocalVolatilitySurface(
            times=[START + timedelta(days=n) for n in range(500)],
            spots=list(range(1, 501)),
            values=[[0.2] * 500 for _ in range(500)],
        )
        model = m.LocalVolatilityParameters(
            risk_free_rate=0.05, dividend_yield=0.02, surface=surface
        )
        context = m.PricingContext(
            model_parameters=model, spot_price=100, valuation_time=START
        )
        for value in (surface, model, context):
            with self.subTest(value=type(value).__name__):
                tracemalloc.start()
                try:
                    text = repr(value)
                    _, peak = tracemalloc.get_traced_memory()
                finally:
                    tracemalloc.stop()
                self.assertIn("...", text)
                self.assertLess(peak, 256 * 1024)

    def test_repr_bounds_large_contract_schedules(self):
        from datetime import timedelta

        times = [START + timedelta(days=n) for n in range(1000)]
        observations = [i.AutocallableObservation(time, 110) for time in times]
        for original in values():
            if isinstance(original, i.Accumulator):
                option = original.replace(
                    expiry_time=times[-1], knock_out_observation_times=times
                )
            elif isinstance(
                original,
                (
                    i.SnowballOption,
                    i.BinarySnowballOption,
                    i.TernarySnowballOption,
                    i.PhoenixOption,
                ),
            ):
                option = original.replace(
                    expiry_time=times[-1], observations=observations
                )
            else:
                continue
            with self.subTest(contract=type(option).__name__):
                text = repr(option)
                self.assertIn(type(option).__name__, text)
                self.assertIn("...", text)
                self.assertLess(len(text), 2000)
                self.assertEqual(pickle.loads(pickle.dumps(option)), option)

    def test_calendar_repr_identifies_base_and_overrides(self):
        from datetime import timedelta

        base = m.weekdays_calendar()
        calendar = m.custom_calendar(
            base=base,
            closed_dates=[START.date()],
            open_dates=[datetime(2025, 1, 4).date()],
        )
        self.assertIn("kind='weekdays'", repr(base))
        self.assertIn("kind='custom'", repr(calendar))
        self.assertIn("base=TradingCalendar", repr(calendar))
        self.assertIn("closed_dates=[datetime.date(2025, 1, 2)]", repr(calendar))
        self.assertIn("open_dates=[datetime.date(2025, 1, 4)]", repr(calendar))
        self.assertNotEqual(repr(calendar), repr(base))
        self.assertFalse(calendar.is_trading_day(START.date()))
        self.assertEqual(repr(pickle.loads(pickle.dumps(calendar))), repr(calendar))
        large = m.custom_calendar(
            base=base,
            closed_dates=[START.date() + timedelta(days=n) for n in range(100)],
        )
        self.assertLess(len(repr(large)), 1000)
        self.assertIn("...", repr(large))

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
