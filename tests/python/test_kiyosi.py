import csv
import unittest
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import kiyosi
import kiyosi.market as market
import kiyosi.pricing as pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    BarrierOption,
    BarrierType,
    CashOrNothingOption,
    EuropeanOption,
    GeometricAveragePriceOption,
    KnockInObservationMode,
    ObservationMode,
    OptionType,
    PayoffType,
    PhoenixOption,
    RebateTiming,
    SettlementTiming,
    TouchOption,
    asset_no_touch_down,
    both_down_snowball,
    cash_binary_barrier_option,
    cash_one_touch_up,
    dual_coupon_snowball,
    standard_snowball,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext, fixed_interval_schedule, monthly_schedule
from kiyosi.pricing import AnalyticBarrierEngine, AnalyticBinaryBarrierEngine, AnalyticDigitalEngine, AnalyticVanillaEngine, FiniteDifferenceScheme, NumericalAnalyticsEngine, implied_coupon, implied_volatility


def parity_fields(value):
    if value == "-":
        return {}
    return dict(item.split("=", 1) for item in value.split(";"))


def parity_cases():
    path = Path(__file__).parents[1] / "fixtures" / "api_parity.tsv"
    with path.open(newline="", encoding="utf-8") as stream:
        return [
            {**row, "inputs": parity_fields(row["inputs"]), "expected": parity_fields(row["expected"])}
            for row in csv.DictReader(stream, delimiter="\t")
        ]


def utc_timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


class KiyosiPythonTests(unittest.TestCase):
    def setUp(self):
        self.parameters = BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2)
        self.context = PricingContext(model_parameters=self.parameters, spot_price=100.0, valuation_time=date(2025, 1, 1))
        self.option = EuropeanOption(option_type=OptionType.CALL, strike=100.0, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))

    def test_explicit_pricing_result_surface(self):
        self.assertFalse(hasattr(kiyosi, "price"))
        self.assertFalse(hasattr(pricing, "price"))
        result = AnalyticVanillaEngine().price(self.option, self.context)
        self.assertEqual(len(result), 11)
        self.assertEqual(result["price"], result.price)
        self.assertIn("speed", result)

    def test_domain_values_have_value_equality_and_readable_representations(self):
        same_parameters = BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2)
        same_option = EuropeanOption(option_type=OptionType.CALL, strike=100.0, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        self.assertEqual(self.parameters, same_parameters)
        self.assertEqual(self.option, same_option)
        self.assertNotEqual(self.option, EuropeanOption(option_type=OptionType.PUT, strike=100.0, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1)))
        with self.assertRaises(TypeError):
            hash(self.parameters)

        schedule = fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)
        same_schedule = fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)
        self.assertEqual(schedule, same_schedule)

        note_terms = dict(coupon_rate=0.1, initial_spot=100, knock_in_level=80, knock_out_level=105, observation_dates=[date(2026, 1, 1)], effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        note = standard_snowball(**note_terms)
        self.assertEqual(note, standard_snowball(**note_terms))

        result = AnalyticVanillaEngine().price(self.option, self.context)
        cases = (
            (self.parameters, "BlackScholesMertonParameters(", "volatility=0.2"),
            (self.option, "EuropeanOption(", "strike=100.0"),
            (schedule, "ObservationSchedule(", "dates=["),
            (market.weekdays_calendar(), "TradingCalendar(", "trading_days_per_year=252"),
            (self.context, "PricingContext(", "spot_price=100.0"),
            (note, "SnowballOption(", "knock_out_coupon_rates=[0.1]"),
            (pricing.FiniteDifferenceVanillaEngine(), "FiniteDifferenceVanillaEngine(", "asset_step_count="),
            (result, "PricingResult(", "price="),
        )
        for value, prefix, field in cases:
            with self.subTest(type=type(value).__name__):
                representation = repr(value)
                self.assertTrue(representation.startswith(prefix))
                self.assertIn(field, representation)

    def test_snowball_implied_coupon_uses_explicit_quote_convention(self):
        terms = dict(
            initial_spot=100,
            knock_in_level=70,
            observation_dates=[date(2025, 7, 1), date(2026, 1, 1)],
            effective_date=date(2025, 1, 1),
            expiry_date=date(2026, 1, 1),
        )
        standard = standard_snowball(coupon_rate=0.10, knock_out_level=105, **terms)
        both_down = both_down_snowball(
            initial_coupon_rate=0.10,
            coupon_rate_decrement=0.01,
            initial_knock_out_level=110,
            knock_out_level_decrement=5,
            **terms,
        )
        dual = dual_coupon_snowball(
            knock_out_coupon_rate=0.10,
            maturity_coupon_rate=0.03,
            knock_out_level=105,
            **terms,
        )
        target_standard = standard_snowball(
            coupon_rate=0.12, knock_out_level=105, **terms
        )
        target_both_down = both_down_snowball(
            initial_coupon_rate=0.12,
            coupon_rate_decrement=0.01,
            initial_knock_out_level=110,
            knock_out_level_decrement=5,
            **terms,
        )
        target_dual = dual_coupon_snowball(
            knock_out_coupon_rate=0.12,
            maturity_coupon_rate=0.03,
            knock_out_level=105,
            **terms,
        )

        linked = pricing.CouponQuoteConvention.SHIFT_MATURITY_COUPON
        fixed = pricing.CouponQuoteConvention.PRESERVE_MATURITY_COUPON
        engine = pricing.FiniteDifferenceSnowballEngine(asset_step_count=40, time_step_count=40)
        analytics = NumericalAnalyticsEngine(engine)
        observed_price = engine.price(target_standard, self.context).price
        with self.assertRaises(TypeError):
            analytics.implied_coupon(standard, self.context, observed_price)
        self.assertFalse(hasattr(standard, "with_quoted_coupon_rate"))

        cases = (
            (standard, target_standard, linked),
            (both_down, target_both_down, linked),
            (dual, target_dual, fixed),
        )
        for instrument, target, convention in cases:
            with self.subTest(instrument=instrument):
                implied = analytics.implied_coupon(
                    instrument,
                    self.context,
                    engine.price(target, self.context).price,
                    quote_convention=convention,
                    tolerance=1e-6,
                )
                standalone = implied_coupon(
                    engine,
                    instrument,
                    self.context,
                    engine.price(target, self.context).price,
                    quote_convention=convention,
                    tolerance=1e-6,
                )
                self.assertAlmostEqual(implied, 0.12, places=5)
                self.assertEqual(standalone, implied)

    def test_public_api_has_targeted_docstrings(self):
        self.assertIn("validated", BlackScholesMertonParameters.__doc__.lower())
        self.assertIn("weekdays", market.weekdays_calendar.__doc__.lower())
        self.assertIn("reversed", market.TradingCalendar.trading_days_between.__doc__.lower())
        self.assertIn("price", AnalyticVanillaEngine.price.__doc__.lower())
        self.assertIn("risk-measure", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("percentage point", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("calendar day", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("never a zero sentinel", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("absolute", NumericalAnalyticsEngine.__doc__.lower())
        self.assertIn("boundary", NumericalAnalyticsEngine.__doc__.lower())
        self.assertIn("solve", NumericalAnalyticsEngine.implied_volatility.__doc__.lower())
        self.assertIn("solve", implied_volatility.__doc__.lower())
        self.assertIn("solve", implied_coupon.__doc__.lower())
        self.assertIn("start is excluded", fixed_interval_schedule.__doc__.lower())
        self.assertIn("duplicate", fixed_interval_schedule.__doc__.lower())
        self.assertIn("not guaranteed", monthly_schedule.__doc__.lower())
        self.assertIn("clamped", monthly_schedule.__doc__.lower())

    def test_schedule_builders_expose_bounded_following_behavior(self):
        self.assertEqual(
            fixed_interval_schedule(
                start=date(2025, 1, 3), end=date(2025, 1, 7), interval_days=1
            ).dates,
            [date(2025, 1, 6), date(2025, 1, 7)],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 1), end=date(2025, 3, 1), lock_up_months=1
            ).dates,
            [date(2025, 2, 3)],
        )
        self.assertEqual(
            monthly_schedule(
                start=date(2025, 1, 31), end=date(2025, 4, 30), lock_up_months=1
            ).dates,
            [date(2025, 2, 28), date(2025, 3, 31), date(2025, 4, 30)],
        )

    def test_weekdays_calendar_is_the_explicit_default(self):
        self.assertFalse(hasattr(market, "exchange_calendar"))
        calendar = market.weekdays_calendar()
        self.assertEqual(calendar.trading_days_per_year, 252)
        self.assertFalse(calendar.is_trading_day(date(2025, 1, 4)))
        self.assertFalse(self.context.calendar.is_trading_day(date(2025, 1, 4)))
        self.assertEqual(calendar.trading_days_between(date(2025, 1, 4), date(2025, 1, 6)), 0)
        for operation in (calendar.trading_days_between, calendar.trading_year_fraction):
            with self.subTest(operation=operation.__name__), self.assertRaises(kiyosi.KiyosiError) as error:
                operation(date(2025, 1, 6), date(2025, 1, 4))
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE)

    def test_native_domain_errors_expose_categories(self):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.0)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_VOLATILITY)

    def test_accumulator_errors_identify_rejected_terms(self):
        terms = {
            "strike": 100.0,
            "knock_out_level": 110.0,
            "daily_quantity": 1.0,
            "acceleration_factor": 2.0,
            "effective_date": date(2025, 1, 1),
            "expiry_date": date(2026, 1, 1),
        }
        cases = (
            ("strike", 0.0, kiyosi.ErrorCategory.INVALID_STRIKE, "strike must be finite and positive"),
            ("knock_out_level", 0.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "knock-out level must be finite and positive"),
            ("daily_quantity", -1.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "daily quantity must be finite and non-negative"),
            ("acceleration_factor", -1.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "acceleration factor must be finite and non-negative"),
            ("accumulated_quantity", -1.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "accumulated quantity must be finite and non-negative"),
        )
        for field, value, category, message in cases:
            with self.subTest(field=field), self.assertRaises(kiyosi.KiyosiError) as error:
                Accumulator(**{**terms, field: value})
            self.assertEqual(error.exception.category, category)
            self.assertEqual(str(error.exception), message)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            Accumulator(**{**terms, "effective_date": terms["expiry_date"], "expiry_date": terms["effective_date"]})
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE)
        self.assertEqual(str(error.exception), "expiry date must not precede the effective date")

    def test_accumulator_knock_out_settles_existing_quantity(self):
        effective_date = date(2025, 1, 1)
        option = Accumulator(
            strike=100, knock_out_level=110, daily_quantity=1, acceleration_factor=2,
            accumulated_quantity=3, effective_date=effective_date, expiry_date=date(2025, 1, 6),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(risk_free_rate=0.05, dividend_yield=0.05, volatility=0.2),
            spot_price=110, valuation_time=effective_date, calendar=market.all_days_calendar(),
        )
        engines = (
            pricing.FiniteDifferenceAccumulatorEngine(asset_step_count=400, asset_upper_boundary=400),
            pricing.MonteCarloAccumulatorEngine(path_count=64, seed=73),
        )
        for engine in engines:
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(engine.price(option, context).price, 30.0, delta=1e-6)

    def test_phoenix_terminal_coupon_is_paid_once(self):
        effective_date = date(2025, 1, 6)
        expiry_date = effective_date + timedelta(days=91)
        option = PhoenixOption(
            coupon_rate=0.0025, initial_spot=100, knock_in_level=80,
            knock_out_levels=[120], coupon_barrier_levels=[90], upper_strike=100,
            lower_strike=60, observation_dates=[expiry_date],
            knock_in_observation_mode=KnockInObservationMode.EVERY_TRADING_DAY, effective_date=effective_date, expiry_date=expiry_date,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(risk_free_rate=0, dividend_yield=0, volatility=1e-8),
            spot_price=100, valuation_time=effective_date, calendar=market.all_days_calendar(),
        )
        engines = (
            pricing.FiniteDifferencePhoenixEngine(
                asset_step_count=400, time_step_count=1600, asset_upper_boundary=400,
            ),
            pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
        )
        for engine in engines:
            with self.subTest(engine=type(engine).__name__):
                self.assertAlmostEqual(engine.price(option, context).price, 1.25, delta=1e-6)

    def test_numeric_and_date_boundaries_are_checked(self):
        with self.assertRaises(TypeError):
            EuropeanOption(option_type=OptionType.CALL, strike="100", effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        with self.assertRaises(TypeError):
            BlackScholesMertonParameters(risk_free_rate=True, dividend_yield=0.02, volatility=0.2)
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceVanillaEngine(asset_step_count=1.5)
        with self.assertRaises(TypeError):
            pricing.MonteCarloVanillaEngine(seed=True)
        with self.assertRaises(OverflowError):
            pricing.FiniteDifferenceVanillaEngine(asset_step_count=2**40)
        with self.assertRaises(OverflowError):
            BlackScholesMertonParameters(risk_free_rate=10**400, dividend_yield=0.02, volatility=0.2)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BlackScholesMertonParameters(risk_free_rate=float("inf"), dividend_yield=0.02, volatility=0.2)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RISK_FREE_RATE)
        for seed in (-1, 2**64):
            with self.subTest(seed=seed), self.assertRaises(OverflowError):
                pricing.MonteCarloVanillaEngine(seed=seed)
        maximum_seed = 2**64 - 1
        self.assertEqual(pricing.MonteCarloVanillaEngine(seed=maximum_seed).seed, maximum_seed)
        with self.assertRaises(TypeError):
            PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=datetime(2025, 1, 1))
        aware = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=datetime(2025, 1, 1, 8, tzinfo=timezone.utc))
        self.assertIsNotNone(AnalyticVanillaEngine().price(self.option, aware).price)

    def test_monte_carlo_backend_defaults_and_round_trips(self):
        default = pricing.MonteCarloVanillaEngine()
        cuda = pricing.MonteCarloVanillaEngine(backend=pricing.MonteCarloBackend.CUDA)

        self.assertEqual(default.backend, pricing.MonteCarloBackend.CPU)
        self.assertEqual(cuda.backend, pricing.MonteCarloBackend.CUDA)
        self.assertEqual(
            repr(cuda),
            "MonteCarloVanillaEngine(path_count=100000, step_count=50, "
            "seed=None, backend=MonteCarloBackend.CUDA)",
        )
        self.assertNotEqual(kiyosi.ErrorCategory.BACKEND_UNAVAILABLE,
                            kiyosi.ErrorCategory.BACKEND_FAILURE)
        self.assertNotEqual(kiyosi.ErrorCategory.BACKEND_FAILURE,
                            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION)
        for value in ("cuda", True):
            with self.subTest(value=value), self.assertRaises(TypeError):
                pricing.MonteCarloVanillaEngine(backend=value)

        for engine_type in (
            pricing.MonteCarloAccumulatorEngine,
            pricing.MonteCarloPhoenixEngine,
            pricing.MonteCarloSnowballEngine,
            pricing.MonteCarloBinarySnowballEngine,
            pricing.MonteCarloTernarySnowballEngine,
        ):
            with self.subTest(engine=engine_type.__name__):
                default = engine_type()
                cuda = engine_type(backend=pricing.MonteCarloBackend.CUDA)
                self.assertEqual(default.backend, pricing.MonteCarloBackend.CPU)
                self.assertEqual(cuda.backend, pricing.MonteCarloBackend.CUDA)
                self.assertEqual(
                    repr(cuda),
                    f"{engine_type.__name__}(path_count=20000, seed=1, "
                    "backend=MonteCarloBackend.CUDA)",
                )
                with self.assertRaises(TypeError):
                    engine_type(backend="cuda")

    def test_cuda_backend_unavailable_is_deferred_and_categorized(self):
        engine = pricing.MonteCarloVanillaEngine(
            path_count=20, step_count=2, seed=42,
            backend=pricing.MonteCarloBackend.CUDA,
        )
        self.assertEqual(engine.backend, pricing.MonteCarloBackend.CUDA)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(self.option, self.context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.BACKEND_UNAVAILABLE)

    def test_american_cuda_backend_unavailable_is_deferred_and_categorized(self):
        option = AmericanOption(
            option_type=OptionType.PUT, strike=100.0,
            effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=20, step_count=3, seed=42,
            backend=pricing.MonteCarloBackend.CUDA,
        )

        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(option, self.context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.BACKEND_UNAVAILABLE)

    def test_american_cuda_validates_before_backend_and_prices_expiry(self):
        option = AmericanOption(
            option_type=OptionType.PUT, strike=100.0,
            effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
        )
        for path_count, step_count in ((0, 50), (20, 2)):
            with self.subTest(path_count=path_count, step_count=step_count):
                engine = pricing.MonteCarloVanillaEngine(
                    path_count=path_count, step_count=step_count, seed=42,
                    backend=pricing.MonteCarloBackend.CUDA,
                )
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(option, self.context)
                self.assertEqual(error.exception.category,
                                 kiyosi.ErrorCategory.INVALID_PARAMETER)

        expiry_date = date(2026, 1, 1)
        expiry_option = AmericanOption(
            option_type=OptionType.PUT, strike=100.0, effective_date=expiry_date, expiry_date=expiry_date,
        )
        expiry_context = PricingContext(
            model_parameters=self.parameters, spot_price=90.0, valuation_time=expiry_date,
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=20, step_count=3, seed=42,
            backend=pricing.MonteCarloBackend.CUDA,
        )
        self.assertEqual(engine.price(expiry_option, expiry_context).price, 10.0)

    def test_temporal_accessors_preserve_date_and_timestamp_semantics(self):
        averaging_start_date = date(2025, 2, 1)
        average = GeometricAveragePriceOption(option_type=OptionType.CALL, strike=100, averaging_start_date=averaging_start_date, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        self.assertIs(type(average.averaging_start_date), date)
        self.assertEqual(average.averaging_start_date, averaging_start_date)

        expiry_date = date(2026, 1, 1)
        note = standard_snowball(coupon_rate=0.1, initial_spot=100, knock_in_level=80, knock_out_level=105, observation_dates=[expiry_date], effective_date=date(2025, 1, 1), expiry_date=expiry_date)
        self.assertEqual(note.observation_dates, [expiry_date])
        self.assertIs(type(note.effective_date), date)
        self.assertIs(type(note.expiry_date), date)

        local_time = datetime(2025, 1, 1, 8, 9, 10, 123456, tzinfo=timezone(timedelta(hours=8)))
        context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=local_time)
        self.assertEqual(context.valuation_time, datetime(2025, 1, 1, 0, 9, 10, 123456, tzinfo=timezone.utc))
        midnight = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=date(2025, 1, 1))
        self.assertEqual(midnight.valuation_time, datetime(2025, 1, 1, tzinfo=timezone.utc))

    def test_digital_barrier_schedule_and_analytics(self):
        digital = CashOrNothingOption(option_type=OptionType.CALL, strike=100, payout=10, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        self.assertGreater(AnalyticDigitalEngine().price(digital, self.context).price, 0)
        barrier = BarrierOption(option_type=OptionType.CALL, strike=100, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1), barrier_level=80, barrier_type=BarrierType.DOWN_AND_OUT, rebate=1, rebate_timing=RebateTiming.AT_EXPIRY)
        self.assertGreater(AnalyticBarrierEngine().price(barrier, self.context).price, 0)
        self.assertEqual(barrier.rebate_timing, RebateTiming.AT_EXPIRY)
        self.assertEqual(barrier.observation_mode, ObservationMode.CONTINUOUS)
        self.assertIn("observation_dates=[]", repr(barrier))
        self.assertFalse(hasattr(barrier, "rebate_payment"))
        self.assertFalse(hasattr(barrier, "observation"))
        self.assertEqual(len(fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)), 5)
        engine = AnalyticVanillaEngine()
        analytics = NumericalAnalyticsEngine(engine)
        self.assertIs(analytics.engine, engine)
        with self.assertRaises(AttributeError):
            analytics.engine = AnalyticVanillaEngine()
        self.assertIsNotNone(analytics.price(self.option, self.context).vega)
        observed_price = engine.price(self.option, self.context).price
        standalone = implied_volatility(engine, self.option, self.context, observed_price)
        self.assertAlmostEqual(
            analytics.implied_volatility(self.option, self.context, observed_price),
            self.parameters.volatility,
        )
        self.assertEqual(
            standalone,
            analytics.implied_volatility(self.option, self.context, observed_price),
        )

        categories = []
        for solve in (
            lambda: implied_volatility(engine, self.option, self.context, observed_price, lower_bound=0.5, upper_bound=0.1),
            lambda: analytics.implied_volatility(self.option, self.context, observed_price, lower_bound=0.5, upper_bound=0.1),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                solve()
            categories.append(error.exception.category)
        self.assertEqual(categories[0], categories[1])

    def test_calculate_numerical_risk_measures_forwards_explicit_shift_settings(self):
        analytics = NumericalAnalyticsEngine(AnalyticVanillaEngine(), spot_shift=0.0)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            analytics.price(self.option, self.context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
        self.assertTrue(hasattr(pricing, "calculate_numerical_risk_measures"))

    def test_engine_settings_are_validated_when_pricing(self):
        configured_engines = (
            pricing.CoxRossRubinsteinVanillaEngine,
            pricing.FiniteDifferenceVanillaEngine,
            pricing.MonteCarloVanillaEngine,
            pricing.MonteCarloSnowballEngine,
        )
        for engine_type in configured_engines:
            with self.subTest(engine=engine_type.__name__):
                self.assertIn("validated when price() is called", engine_type.__doc__)
                self.assertIn("validated when price() is called", engine_type.__init__.__doc__)

        engine = pricing.FiniteDifferenceVanillaEngine(asset_step_count=0)
        self.assertEqual(engine.asset_step_count, 0)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(self.option, self.context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_calculate_numerical_risk_measures_retains_valid_boundary_results(self):
        engine = AnalyticVanillaEngine()
        analytics = NumericalAnalyticsEngine(engine)

        low_volatility = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                volatility=0.00005,
            ),
            spot_price=100.0,
            valuation_time=date(2025, 1, 1),
        )
        result = analytics.price(self.option, low_volatility)
        self.assertAlmostEqual(result.price, engine.price(self.option, low_volatility).price)
        self.assertIsNotNone(result.delta)
        self.assertIsNotNone(result.rho)
        self.assertIsNone(result.vega)
        self.assertIsNone(result.vanna)
        self.assertIsNone(result.zomma)

        low_spot = PricingContext(
            model_parameters=self.parameters,
            spot_price=0.005,
            valuation_time=date(2025, 1, 1),
        )
        result = analytics.price(self.option, low_spot)
        self.assertAlmostEqual(result.price, engine.price(self.option, low_spot).price)
        self.assertIsNotNone(result.vega)
        self.assertIsNotNone(result.theta)
        self.assertIsNotNone(result.rho)
        for measure in ("delta", "gamma", "speed", "charm", "color", "vanna", "zomma"):
            self.assertIsNone(getattr(result, measure))

    def test_touch_factories_require_only_payoff_relevant_terms(self):
        terms = dict(effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        cash = cash_one_touch_up(
            **terms, barrier_level=130, payout=10,
            settlement_timing=SettlementTiming.AT_HIT,
            observation_mode=ObservationMode.SCHEDULED,
            observation_dates=[date(2025, 6, 2), terms["expiry_date"]])
        asset = asset_no_touch_down(**terms, barrier_level=70)
        self.assertIsInstance(cash, TouchOption)
        self.assertTrue(cash.is_one_touch)
        self.assertTrue(cash.is_up)
        self.assertEqual(cash.payout, 10)
        self.assertEqual(cash.payoff_type, PayoffType.CASH)
        self.assertEqual(cash.settlement_timing, SettlementTiming.AT_HIT)
        self.assertEqual(cash.observation_dates, [date(2025, 6, 2), terms["expiry_date"]])
        self.assertFalse(asset.is_one_touch)
        self.assertFalse(asset.is_up)
        self.assertIsNone(asset.payout)
        self.assertEqual(asset.payoff_type, PayoffType.ASSET)
        with self.assertRaises(TypeError):
            cash_one_touch_up(**terms, barrier_level=130, payout=10, strike=100)
        with self.assertRaises(TypeError):
            asset_no_touch_down(**terms, barrier_level=70, payout=10)

        binary = cash_binary_barrier_option(
            **terms, option_type=OptionType.CALL, strike=100, barrier_level=80,
            barrier_type=BarrierType.DOWN_AND_OUT, payout=10)
        self.assertEqual(binary.option_type, OptionType.CALL)
        self.assertEqual(binary.strike, 100)
        self.assertEqual(binary.payoff_type, PayoffType.CASH)
        engine = AnalyticBinaryBarrierEngine()
        self.assertGreater(engine.price(cash, self.context).price, 0)
        self.assertGreater(engine.price(binary, self.context).price, 0)

    def test_public_api_matches_shared_language_parity_cases(self):
        cases = parity_cases()
        self.assertEqual(len(cases), 7)
        for case in cases:
            with self.subTest(case=case["case_id"]):
                values = case["inputs"]
                expected = case["expected"]
                kind = case["kind"]
                if kind == "construction":
                    option = EuropeanOption(option_type=getattr(OptionType, values["type"].upper()), strike=float(values["strike"]), effective_date=date.fromisoformat(values["effective_date"]), expiry_date=date.fromisoformat(values["expiry_date"]))
                    self.assertEqual(option.option_type, getattr(OptionType, expected["type"].upper()))
                    self.assertEqual(option.strike, float(expected["strike"]))
                    self.assertEqual(option.effective_date, date.fromisoformat(expected["effective_date"]))
                    self.assertEqual(option.expiry_date, date.fromisoformat(expected["expiry_date"]))
                elif kind == "defaults":
                    engine = pricing.FiniteDifferenceVanillaEngine()
                    self.assertEqual(engine.asset_step_count, int(expected["asset_step_count"]))
                    self.assertEqual(engine.time_step_count, int(expected["time_step_count"]))
                    self.assertEqual(engine.scheme, getattr(FiniteDifferenceScheme, expected["scheme"].upper()))
                    self.assertEqual(expected["asset_upper_boundary"], "none")
                    self.assertIsNone(engine.asset_upper_boundary)
                elif kind == "pricing":
                    parameters = BlackScholesMertonParameters(risk_free_rate=float(values["risk_free_rate"]), dividend_yield=float(values["dividend_yield"]), volatility=float(values["volatility"]))
                    context = PricingContext(model_parameters=parameters, spot_price=float(values["spot_price"]), valuation_time=date.fromisoformat(values["valuation_date"]))
                    option = EuropeanOption(option_type=getattr(OptionType, values["type"].upper()), strike=float(values["strike"]), effective_date=date.fromisoformat(values["effective_date"]), expiry_date=date.fromisoformat(values["expiry_date"]))
                    result = AnalyticVanillaEngine().price(option, context)
                    self.assertAlmostEqual(result.price, float(expected["price"]), delta=float(case["tolerance"]))
                elif kind == "domain_error":
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        BlackScholesMertonParameters(risk_free_rate=float(values["risk_free_rate"]), dividend_yield=float(values["dividend_yield"]), volatility=float(values["volatility"]))
                    self.assertEqual(error.exception.category, getattr(kiyosi.ErrorCategory, expected["category"].upper()))
                elif kind == "date_round_trip":
                    option = GeometricAveragePriceOption(option_type=getattr(OptionType, values["type"].upper()), strike=float(values["strike"]), averaging_start_date=date.fromisoformat(values["averaging_start_date"]), effective_date=date.fromisoformat(values["effective_date"]), expiry_date=date.fromisoformat(values["expiry_date"]))
                    self.assertEqual(option.averaging_start_date, date.fromisoformat(expected["averaging_start_date"]))
                    self.assertEqual(option.effective_date, date.fromisoformat(expected["effective_date"]))
                    self.assertEqual(option.expiry_date, date.fromisoformat(expected["expiry_date"]))
                elif kind == "timestamp_round_trip":
                    parameters = BlackScholesMertonParameters(risk_free_rate=float(values["risk_free_rate"]), dividend_yield=float(values["dividend_yield"]), volatility=float(values["volatility"]))
                    context = PricingContext(model_parameters=parameters, spot_price=float(values["spot_price"]), valuation_time=utc_timestamp(values["valuation_time"]))
                    self.assertEqual(context.valuation_time, utc_timestamp(expected["valuation_time"]))
                    self.assertEqual(context.valuation_date, date.fromisoformat(expected["valuation_date"]))
                elif kind == "numeric_boundary":
                    engine = pricing.MonteCarloVanillaEngine(seed=int(values["seed"]))
                    self.assertEqual(engine.seed, int(expected["seed"]))
                else:
                    self.fail(f"unknown parity case kind: {kind}")


if __name__ == "__main__":
    unittest.main()
