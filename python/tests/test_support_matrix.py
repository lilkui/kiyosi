"""CPU product/engine dispatch and terminal contracts through the public API."""

import unittest
from datetime import datetime, timedelta
from datetime import time as clock_time

from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    HestonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
    all_days_calendar,
)
from support.history import dated as dated_history
from support.history import history_fields

HESTON_ENGINES = (
    "AnalyticHestonEuropeanEngine",
    "FiniteDifferenceHestonVanillaEngine",
)


def terminal_cases():
    dates = {
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
    }
    vanilla = dict(dates, option_type="call", strike=100)
    barrier = dict(
        vanilla, barrier_type="up_and_out", barrier_level=120, touch_state="untouched"
    )
    cash_digital = (
        "AnalyticDigitalEngine",
        "QuadratureDigitalEngine",
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
                "AnalyticHestonEuropeanEngine",
                "FiniteDifferenceHestonVanillaEngine",
                "CoxRossRubinsteinVanillaEngine",
                "QuadratureVanillaEngine",
                "FiniteDifferenceVanillaEngine",
                "MonteCarloVanillaEngine",
            ),
            5,
        ),
        (
            ins.AmericanOption(**vanilla),
            (
                "BjerksundStenslandVanillaEngine",
                "QuadratureVanillaEngine",
                "CoxRossRubinsteinVanillaEngine",
                "FiniteDifferenceVanillaEngine",
                "FiniteDifferenceHestonVanillaEngine",
                "MonteCarloVanillaEngine",
            ),
            5,
        ),
        (ins.CashOrNothingOption(**vanilla, payout=10), cash_digital, 10),
        (ins.AssetOrNothingOption(**vanilla), cash_digital, 105),
        (
            ins.BarrierOption(**history_fields(**barrier)),
            (
                "AnalyticBarrierEngine",
                "CoxRossRubinsteinBarrierEngine",
                "QuadratureBarrierEngine",
                "FiniteDifferenceBarrierEngine",
                "MonteCarloBarrierEngine",
            ),
            5,
        ),
        (
            ins.cash_binary_barrier_option(**history_fields(**barrier, payout=10)),
            binary,
            10,
        ),
        (ins.asset_binary_barrier_option(**history_fields(**barrier)), binary, 105),
        (
            ins.DoubleBarrierOption(
                **history_fields(
                    **vanilla,
                    barrier_type="knock_out",
                    lower_barrier=80,
                    upper_barrier=120,
                    touch_state="untouched",
                )
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
                **history_fields(
                    **vanilla,
                    averaging_start_time=dates["effective_time"],
                    realized_average=110,
                )
            ),
            ("AnalyticGeometricAveragePriceEngine", "MonteCarloAsianEngine"),
            10,
        ),
        (
            ins.ArithmeticAveragePriceOption(
                **history_fields(
                    **vanilla,
                    averaging_start_time=dates["effective_time"],
                    realized_average=110,
                )
            ),
            ("TurnbullWakemanArithmeticAveragePriceEngine", "MonteCarloAsianEngine"),
            10,
        ),
        (
            ins.Accumulator(
                knock_out_settled=False,
                observation_clock_time=clock_time(),
                effective_time=dates["effective_time"] - timedelta(days=1),
                expiry_time=dates["expiry_time"],
                strike=100,
                knock_out_level=120,
                daily_quantity=1,
                acceleration_factor=2,
                accumulated_quantity=3,
                history_time=dates["effective_time"],
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
                **history_fields(
                    observation_clock_time=clock_time(),
                    **dates,
                    calendar=all_days_calendar(),
                    notional=1000,
                    annual_coupon_rate=0.12,
                    lower_bound=95,
                    upper_bound=110,
                    realized_hit_count=0,
                )
            ),
            ("AnalyticRangeAccrualEngine",),
            120 / 365,
        ),
    ]
    for factory, kind, amount in (
        (ins.cash_touch_option, "up_and_in", 0),
        (ins.cash_no_touch_option, "up_and_out", 10),
        (ins.asset_touch_option, "up_and_in", 0),
        (ins.asset_no_touch_option, "up_and_out", 105),
    ):
        payout = (
            {"payout": 10}
            if factory in (ins.cash_touch_option, ins.cash_no_touch_option)
            else {}
        )
        cases.append(
            (
                factory(
                    **history_fields(
                        **dates,
                        barrier_level=120,
                        barrier_type=kind,
                        touch_state="untouched",
                        **payout,
                    )
                ),
                binary,
                amount,
            )
        )
    for factory, amount in ((ins.cash_double_touch, 0), (ins.cash_double_no_touch, 10)):
        cases.append(
            (
                factory(
                    **history_fields(
                        **dates,
                        lower_barrier=80,
                        upper_barrier=120,
                        payout=10,
                        touch_state="untouched",
                    )
                ),
                ("AnalyticDoubleTouchEngine", "FiniteDifferenceDoubleTouchEngine"),
                amount,
            )
        )
    common = dict(
        dates,
        observations=ins.autocallable_observations(
            [dates["expiry_time"]], [120], knock_out_coupon_rates=[0.08]
        ),
    )
    loss = {
        "initial_spot": 100,
        "knock_in_level": 80,
        "upper_strike": 100,
        "lower_strike": 60,
        "knock_in_observation_mode": "at_expiry",
        "barrier_state": "no_event",
    }
    cases.extend(
        [
            (
                ins.SnowballOption(
                    **history_fields(
                        observation_clock_time=None,
                        **common,
                        **loss,
                        maturity_coupon_rate=0.06,
                    )
                ),
                notes,
                1.06,
            ),
            (
                ins.BinarySnowballOption(
                    **history_fields(**common, maturity_coupon_rate=0.06)
                ),
                notes,
                1.06,
            ),
            (
                ins.TernarySnowballOption(
                    **history_fields(
                        observation_clock_time=None,
                        **common,
                        maturity_coupon_rate=0.06,
                        minimum_coupon_rate=-0.02,
                        knock_in_level=80,
                        knock_in_observation_mode="at_expiry",
                        barrier_state="no_event",
                    )
                ),
                notes,
                1.06,
            ),
            (
                ins.PhoenixOption(
                    **history_fields(
                        observation_clock_time=None,
                        **dates,
                        **loss,
                        coupon_per_period=0.08,
                        observations=ins.autocallable_observations(
                            [dates["expiry_time"]], [120], coupon_barrier_levels=[90]
                        ),
                    )
                ),
                notes,
                1.08,
            ),
        ]
    )
    return cases


def terminal_delta(instrument):
    if isinstance(
        instrument,
        (
            ins.EuropeanOption,
            ins.AmericanOption,
            ins.AssetOrNothingOption,
            ins.BarrierOption,
            ins.DoubleBarrierOption,
        ),
    ):
        return 1
    if isinstance(instrument, ins.BinaryBarrierOption):
        return int(instrument.payoff_type == "asset")
    if isinstance(instrument, ins.TouchOption):
        return int(instrument.payoff_type == "asset" and not instrument.is_one_touch)
    if isinstance(instrument, ins.Accumulator):
        return 4
    return 0


class SupportMatrixTests(unittest.TestCase):
    def test_monte_carlo_terminal_statistics_and_seeds_match_settlement(self):
        for instrument, names, expected in self.cases:
            for name in names:
                if not name.startswith("MonteCarlo"):
                    continue
                context = self.context_for(name)
                option = dated_history(instrument, context)
                for sampling in ("pseudo_random", "sobol"):
                    engine = self.engines[name].replace(sampling=sampling)
                    with self.subTest(
                        instrument=type(option).__name__, engine=name, sampling=sampling
                    ):
                        price, seed = engine.price_with_seed(option, context)
                        self.assertAlmostEqual(price, expected, delta=1e-12)
                        self.assertIsNone(seed)
                        if isinstance(option, ins.AmericanOption):
                            continue
                        estimate = engine.price_with_statistics(option, context)
                        self.assertAlmostEqual(estimate.price, expected, delta=1e-12)
                        self.assertIsNone(estimate.seed)
                        self.assertEqual(
                            estimate.standard_error,
                            0 if sampling == "pseudo_random" else None,
                        )
                        self.assertEqual(
                            estimate.uncertainty_status,
                            "available"
                            if sampling == "pseudo_random"
                            else "unsupported_sampling",
                        )

    def test_empty_greek_requests_return_price_only_for_every_engine(self):
        for instrument, names, expected in self.cases:
            for name in names:
                context = self.context_for(name)
                option = dated_history(instrument, context)
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    result = self.engines[name].price_with_greeks(option, context, [])
                    self.assertAlmostEqual(result.price, expected, delta=1e-12)
                    self.assertEqual(result.requested, ())
                    self.assertEqual(result.status("delta"), "not_requested")
                    self.assertIsNone(result.delta)
                    self.assertIsNone(result.seed)

    def test_empty_requests_price_before_expiry_and_validate_controls(self):
        for instrument, names, _ in self.cases[:4]:
            for name in names:
                context = self.context_for(name).replace(
                    valuation_time=instrument.effective_time
                )
                engine = self.engines[name]
                if name.startswith("MonteCarlo"):
                    engine = (
                        engine.replace(path_count=32, seed=42, step_count=4)
                        if name == "MonteCarloVanillaEngine"
                        else engine.replace(path_count=32, seed=42)
                    )
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    result = engine.price_with_greeks(instrument, context, [])
                    self.assertEqual(result.price, engine.price(instrument, context))
                    self.assertEqual(result.requested, ())
                    self.assertEqual(result.status("vega"), "not_requested")
                    with self.assertRaises(InvalidInputError):
                        engine.price_with_greeks(instrument, context, [], spot_shift=0)
                    if name not in HESTON_ENGINES:
                        scalar = pricing.numerical_greeks(
                            engine, instrument, context, []
                        )
                        self.assertEqual(scalar.price, result.price)
                        self.assertEqual(scalar.requested, ())

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
            valuation_time=datetime(2026, 1, 1),
        )

    def test_every_exported_engine_has_an_accepted_product(self):
        self.assertEqual(
            set(self.engines), {name for _, names, _ in self.cases for name in names}
        )

    def test_heston_model_is_rejected_by_other_engines_even_at_expiry(self):
        context = self.context_for("AnalyticHestonEuropeanEngine")
        for instrument, supported, _ in self.cases:
            for name in supported:
                if name in HESTON_ENGINES:
                    continue
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    with self.assertRaises(UnsupportedOperationError):
                        self.engines[name].price(
                            dated_history(instrument, context), context
                        )
                    with self.assertRaises(UnsupportedOperationError):
                        self.engines[name].price_with_greeks(
                            dated_history(instrument, context), context, "delta"
                        )
                    with self.assertRaises(UnsupportedOperationError):
                        pricing.implied_volatility(
                            self.engines[name],
                            dated_history(instrument, context),
                            context,
                            1,
                            lower_bound=0.0,
                            upper_bound=4.0,
                        )

    def context_for(self, name):
        if name in HESTON_ENGINES:
            return self.context.replace(
                model_parameters=HestonParameters(
                    risk_free_rate=0.04,
                    dividend_yield=0.01,
                    initial_variance=0.04,
                    long_run_variance=0.04,
                    mean_reversion_speed=1.5,
                    vol_of_vol=0.3,
                    correlation=-0.7,
                )
            )
        return self.context

    def test_scalar_operations_preserve_expired_contract_errors(self):
        for instrument, supported, _ in self.cases:
            for name in supported:
                expired = self.context_for(name).replace(
                    valuation_time=datetime(2026, 1, 2)
                )
                engine = self.engines[name]
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    with self.assertRaises(InvalidInputError):
                        engine.price(dated_history(instrument, expired), expired)
                    with self.assertRaises(
                        UnsupportedOperationError
                        if name in HESTON_ENGINES
                        else InvalidInputError
                    ):
                        pricing.implied_volatility(
                            engine,
                            dated_history(instrument, expired),
                            expired,
                            1,
                            lower_bound=0.0,
                            upper_bound=4.0,
                        )
        self.assertEqual(self.context.valuation_time, datetime(2026, 1, 1))

    def test_solver_rejects_unregistered_engine_representations(self):
        instrument = self.cases[0][0]
        for engine in (
            None,
            True,
            100,
            "engine",
            object(),
            self.context,
            instrument,
            pricing.AnalyticEuropeanEngine,
        ):
            with self.subTest(engine=engine), self.assertRaises(TypeError):
                pricing.implied_volatility(
                    engine,
                    dated_history(instrument, self.context),
                    self.context,
                    1,
                    lower_bound=0.0,
                    upper_bound=4.0,
                )

    def test_supported_terminal_prices_greeks_and_unidentifiable_volatility(self):
        for instrument, supported, expected in self.cases:
            for name in supported:
                engine = self.engines[name]
                if name.startswith("MonteCarlo"):
                    engine = engine.with_resolved_seed()
                context = self.context_for(name)
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    self.assertAlmostEqual(
                        engine.price(dated_history(instrument, context), context),
                        expected,
                    )
                    request = [] if name in HESTON_ENGINES else ["delta"]
                    if name in HESTON_ENGINES:
                        with self.assertRaises(UnsupportedOperationError):
                            engine.price_with_greeks(
                                dated_history(instrument, context), context, ["delta"]
                            )
                    result = engine.price_with_greeks(
                        dated_history(instrument, context), context, request
                    )
                    self.assertAlmostEqual(result.price, expected)
                    if name in HESTON_ENGINES:
                        self.assertIsNone(result.delta)
                    else:
                        self.assertAlmostEqual(
                            result.delta, terminal_delta(instrument), delta=1e-10
                        )
                    self.assertEqual(result.requested, tuple(request))
                    self.assertEqual(
                        result.status("delta"),
                        "not_requested" if name in HESTON_ENGINES else "available",
                    )
                    self.assertEqual(result.status("gamma"), "not_requested")
                    with self.assertRaises(UnsupportedOperationError) as error:
                        pricing.implied_volatility(
                            engine,
                            dated_history(instrument, self.context),
                            self.context,
                            expected,
                            lower_bound=0.0,
                            upper_bound=4.0,
                        )
                    self.assertIs(type(error.exception), UnsupportedOperationError)

    def test_local_volatility_status_preserves_requests_across_fd_products(self):
        local = self.context.replace(
            model_parameters=LocalVolatilityParameters(
                risk_free_rate=0.04,
                dividend_yield=0.01,
                surface=LocalVolatilitySurface(
                    times=[datetime(2025, 1, 1), datetime(2026, 1, 1)],
                    spots=[80, 120],
                    values=[[0.2, 0.2], [0.2, 0.2]],
                ),
            )
        )
        for instrument, supported, _ in self.cases:
            for name in supported:
                if not name.startswith("FiniteDifference") or name in HESTON_ENGINES:
                    continue
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    result = self.engines[name].price_with_greeks(
                        dated_history(instrument, local), local, ["delta", "vega"]
                    )
                    self.assertEqual(result.requested, ("delta", "vega"))
                    self.assertEqual(result.supported, ("delta", "gamma"))
                    self.assertEqual(result.status("delta"), "available")
                    self.assertAlmostEqual(
                        result.delta, terminal_delta(instrument), delta=1e-10
                    )
                    self.assertEqual(result.status("vega"), "unsupported")
                    self.assertEqual(result.status("gamma"), "not_requested")

    def test_other_product_engine_pairs_raise_domain_errors(self):
        for instrument, supported, _ in self.cases:
            for name, engine in self.engines.items():
                if name in supported:
                    continue
                with self.subTest(instrument=type(instrument).__name__, engine=name):
                    with self.assertRaises(UnsupportedOperationError):
                        engine.price(
                            dated_history(instrument, self.context), self.context
                        )
                    with self.assertRaises(UnsupportedOperationError):
                        engine.price_with_greeks(
                            dated_history(instrument, self.context),
                            self.context,
                            ["delta"],
                        )
                    with self.assertRaises(UnsupportedOperationError):
                        pricing.implied_volatility(
                            engine,
                            dated_history(instrument, self.context),
                            self.context,
                            1,
                            lower_bound=0.0,
                            upper_bound=4.0,
                        )

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
                        engine.price(
                            dated_history(instrument, self.context), self.context
                        )
                    with self.assertRaises(TypeError):
                        engine.price_with_greeks(
                            dated_history(instrument, self.context),
                            self.context,
                            ["delta"],
                        )
                    with self.assertRaises(TypeError):
                        pricing.implied_volatility(
                            engine,
                            dated_history(instrument, self.context),
                            self.context,
                            1,
                            lower_bound=0.0,
                            upper_bound=4.0,
                        )

    def test_autocallable_instrument_conversion_precedes_greek_name_validation(self):
        for name, engine in self.engines.items():
            if not name.endswith("AutocallableEngine"):
                continue
            for instrument, expected_error in (
                (self.cases[0][0], UnsupportedOperationError),
                (object(), TypeError),
            ):
                with self.subTest(engine=name, instrument=type(instrument).__name__):
                    with self.assertRaises(expected_error) as error:
                        engine.price_with_greeks(
                            dated_history(instrument, self.context),
                            self.context,
                            ["invalid"],
                        )
                    self.assertIs(type(error.exception), expected_error)

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
                    engine.price_with_statistics(
                        dated_history(instrument, self.context), self.context
                    )
            with self.assertRaises(TypeError):
                engine.price_with_statistics(
                    dated_history(object(), self.context), self.context
                )

    def test_coupon_solver_rejects_unsupported_products_and_engines(self):
        for solver, accepted, keywords in (
            (
                pricing.implied_snowball_coupon,
                (
                    ins.SnowballOption,
                    ins.BinarySnowballOption,
                    ins.TernarySnowballOption,
                ),
                {"quote_convention": "preserve_maturity_coupon"},
            ),
            (pricing.implied_phoenix_coupon, (ins.PhoenixOption,), {}),
        ):
            note = next(
                product for product, _, _ in self.cases if isinstance(product, accepted)
            )
            for name, engine in self.engines.items():
                if name.endswith("AutocallableEngine"):
                    for product, _, _ in self.cases:
                        if isinstance(product, accepted):
                            continue
                        with (
                            self.subTest(
                                solver=solver.__name__,
                                engine=name,
                                instrument=type(product).__name__,
                            ),
                            self.assertRaises(UnsupportedOperationError),
                        ):
                            solver(engine, product, self.context, 1, **keywords)
                else:
                    with (
                        self.subTest(solver=solver.__name__, engine=name),
                        self.assertRaises(UnsupportedOperationError),
                    ):
                        solver(engine, note, self.context, 1, **keywords)
