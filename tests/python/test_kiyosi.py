import csv
import math
import unittest
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import kiyosi
import kiyosi.market as market
import kiyosi.pricing as pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    ArithmeticAveragePriceOption,
    AutocallableBarrierState,
    BarrierOption,
    BarrierType,
    BarrierTouchState,
    BinarySnowballOption,
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
from kiyosi.pricing import AnalyticBarrierEngine, AnalyticBinaryBarrierEngine, AnalyticDigitalEngine, AnalyticVanillaEngine, FiniteDifferenceScheme, GreeksLevel, calculate_numerical_risk_measures, implied_coupon, implied_volatility


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
        result = AnalyticVanillaEngine().price_with_greeks(self.option, self.context, GreeksLevel.FULL)
        self.assertEqual(len(result), 11)
        self.assertEqual(result["price"], result.price)
        self.assertIn("speed", result)
        self.assertNotIn("unknown", result)
        self.assertNotIn(1, result)
        self.assertNotIn(None, result)
        self.assertNotIn([], result)

    def test_pricing_levels_preserve_native_greeks_and_scalar_price(self):
        engine = AnalyticVanillaEngine()
        context = PricingContext(
            model_parameters=self.parameters, spot_price=100,
            valuation_time=date(2025, 6, 1),
        )
        value = engine.price(self.option, context)
        basic = engine.price_with_greeks(self.option, context, GreeksLevel.BASIC)
        full = engine.price_with_greeks(self.option, context, GreeksLevel.FULL)
        numerical = calculate_numerical_risk_measures(engine, self.option, context)
        self.assertIs(type(value), float)
        self.assertEqual(basic.price, value)
        self.assertEqual(full.price, value)
        self.assertEqual(basic.delta, full.delta)
        self.assertEqual(basic.gamma, full.gamma)
        time = (self.option.expiry_date - date(2025, 6, 1)).days / 365
        d1 = (0.05 - 0.02 + 0.2**2 / 2) * time / (0.2 * math.sqrt(time))
        self.assertAlmostEqual(basic.delta, math.exp(-0.02 * time) * (1 + math.erf(d1 / math.sqrt(2))) / 2)
        self.assertAlmostEqual(basic.gamma, math.exp(-0.02 * time - d1**2 / 2) / (100 * 0.2 * math.sqrt(2 * math.pi * time)))
        for name in ("delta", "gamma", "speed", "theta", "charm", "color", "vega", "vanna", "zomma", "rho"):
            with self.subTest(measure=name):
                self.assertAlmostEqual(getattr(full, name), getattr(numerical, name), delta=1e-5)
        for name in ("speed", "theta", "charm", "color", "vega", "vanna", "zomma", "rho"):
            with self.subTest(measure=name):
                self.assertIsNone(getattr(basic, name))

    def test_joint_pricing_requires_explicit_enum_and_valid_shift_settings(self):
        engine = AnalyticVanillaEngine()
        with self.assertRaises(TypeError):
            engine.price_with_greeks(self.option, self.context)
        for level in ("basic", True, 0, None):
            with self.subTest(level=level), self.assertRaises(TypeError):
                engine.price_with_greeks(self.option, self.context, level)
        with self.assertRaises(TypeError):
            engine.price_with_greeks(self.option, self.context, GreeksLevel.BASIC, spot_shift=True)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(self.option, self.context, GreeksLevel.FULL, spot_shift=0)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_joint_pricing_at_expiry_has_no_greeks(self):
        context = PricingContext(
            model_parameters=self.parameters, spot_price=110,
            valuation_time=self.option.expiry_date,
        )
        for engine in (AnalyticVanillaEngine(), pricing.FiniteDifferenceVanillaEngine(),
                       pricing.MonteCarloVanillaEngine(path_count=64, step_count=3, seed=73)):
            self.assertEqual(engine.price(self.option, context), 10)
            for level in (GreeksLevel.BASIC, GreeksLevel.FULL):
                with self.subTest(engine=type(engine).__name__, level=level):
                    result = engine.price_with_greeks(self.option, context, level)
                    self.assertEqual(result.price, 10)
                    for name in ("delta", "gamma", "speed", "theta", "charm", "color", "vega", "vanna", "zomma", "rho"):
                        self.assertIsNone(getattr(result, name))
            numerical = calculate_numerical_risk_measures(engine, self.option, context)
            self.assertEqual(numerical.price, 10)
            self.assertIsNone(numerical.delta)
            self.assertIsNone(numerical.vega)

    def test_joint_monte_carlo_reuses_explicit_seed_without_changing_settings(self):
        engine = pricing.MonteCarloVanillaEngine(path_count=512, step_count=5, seed=73)
        context = PricingContext(
            model_parameters=self.parameters, spot_price=100,
            valuation_time=date(2025, 6, 1),
        )
        first = engine.price_with_greeks(self.option, context, GreeksLevel.FULL)
        second = engine.price_with_greeks(self.option, context, GreeksLevel.FULL)
        basic = engine.price_with_greeks(self.option, context, GreeksLevel.BASIC)
        self.assertEqual(first.price, engine.price(self.option, context))
        self.assertEqual(first.delta, basic.delta)
        self.assertEqual(first.gamma, basic.gamma)
        for name in ("price", "delta", "gamma", "speed", "theta", "charm", "color", "vega", "vanna", "zomma", "rho"):
            with self.subTest(measure=name):
                self.assertTrue(math.isfinite(getattr(first, name)))
                self.assertEqual(getattr(first, name), getattr(second, name))
        self.assertEqual(engine.seed, 73)

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

        result = AnalyticVanillaEngine().price_with_greeks(self.option, self.context, GreeksLevel.FULL)
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
        observed_price = engine.price(target_standard, self.context)
        with self.assertRaises(TypeError):
            implied_coupon(engine, standard, self.context, observed_price)
        self.assertFalse(hasattr(standard, "with_quoted_coupon_rate"))

        cases = (
            (standard, target_standard, linked),
            (both_down, target_both_down, linked),
            (dual, target_dual, fixed),
        )
        for instrument, target, convention in cases:
            with self.subTest(instrument=instrument):
                implied = implied_coupon(
                    engine,
                    instrument,
                    self.context,
                    engine.price(target, self.context),
                    quote_convention=convention,
                    tolerance=1e-6,
                )
                self.assertAlmostEqual(implied, 0.12, places=5)

    def test_negative_binary_snowball_coupon_can_be_implied(self):
        effective = date(2025, 1, 1)
        expiry = date(2026, 1, 1)
        option = BinarySnowballOption(
            knock_out_coupon_rates=[-0.1], maturity_coupon_rate=-0.1,
            initial_spot=100, knock_out_levels=[1], upper_strike=100, lower_strike=60,
            observation_dates=[expiry], effective_date=effective, expiry_date=expiry,
        )
        context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=effective)
        engine = pricing.MonteCarloBinarySnowballEngine(path_count=32, seed=73)
        price = engine.price(option, context)
        implied = implied_coupon(
            engine, option, context, price,
            quote_convention=pricing.CouponQuoteConvention.PRESERVE_MATURITY_COUPON,
            lower_bound=-0.2, upper_bound=0.2,
        )
        self.assertAlmostEqual(implied, -0.1, delta=1e-7)

    def test_binary_snowball_rejects_knock_in_history(self):
        expiry = date(2026, 1, 1)
        terms = dict(
            knock_out_coupon_rates=[0.05], maturity_coupon_rate=0.05,
            initial_spot=100, knock_out_levels=[110], upper_strike=100,
            lower_strike=60, observation_dates=[expiry],
            effective_date=date(2025, 1, 1), expiry_date=expiry,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BinarySnowballOption(**terms, barrier_state=AutocallableBarrierState.KNOCKED_IN)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
        self.assertEqual(BinarySnowballOption(**terms, barrier_state=AutocallableBarrierState.KNOCKED_OUT).barrier_state,
                         AutocallableBarrierState.KNOCKED_OUT)

    def test_public_api_has_targeted_docstrings(self):
        self.assertIn("validated", BlackScholesMertonParameters.__doc__.lower())
        self.assertIn("weekdays", market.weekdays_calendar.__doc__.lower())
        self.assertIn("reversed", market.TradingCalendar.trading_days_between.__doc__.lower())
        self.assertIn("price", AnalyticVanillaEngine.price.__doc__.lower())
        self.assertIn("risk-measure", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("percentage point", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("calendar day", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("never a zero sentinel", kiyosi.PricingResult.__doc__.lower())
        self.assertIn("absolute", calculate_numerical_risk_measures.__doc__.lower())
        self.assertIn("boundary", calculate_numerical_risk_measures.__doc__.lower())
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

    def test_nominal_weekend_expiry_requires_explicit_adjustment(self):
        calendar = market.weekdays_calendar()
        nominal = date(2025, 1, 5)
        self.assertEqual(calendar.adjust(nominal, market.BusinessDayConvention.FOLLOWING), date(2025, 1, 6))
        self.assertEqual(calendar.adjust(nominal, market.BusinessDayConvention.PRECEDING), date(2025, 1, 3))
        context = PricingContext(model_parameters=self.parameters, spot_price=110,
                                 valuation_time=date(2025, 1, 3), calendar=calendar)
        terms = dict(strike=100, knock_out_level=1000, daily_quantity=0,
                     acceleration_factor=1, accumulated_quantity=1,
                     effective_date=date(2025, 1, 3))
        nominal_option = Accumulator(**terms, expiry_date=nominal)
        adjusted_option = Accumulator(**terms, expiry_date=calendar.adjust(
            nominal, market.BusinessDayConvention.FOLLOWING))
        for engine in (pricing.MonteCarloAccumulatorEngine(path_count=32, seed=7),
                       pricing.FiniteDifferenceAccumulatorEngine()):
            with self.subTest(engine=type(engine).__name__):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(nominal_option, context)
                self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_DATE)
                self.assertTrue(math.isfinite(engine.price(adjusted_option, context)))
        note_terms = dict(knock_out_coupon_rates=[0], maturity_coupon_rate=0.01,
                          initial_spot=100, knock_out_levels=[200], upper_strike=100,
                          lower_strike=60, observation_dates=[date(2025, 1, 3)],
                          effective_date=date(2025, 1, 3))
        nominal_note = BinarySnowballOption(**note_terms, expiry_date=nominal)
        adjusted_note = BinarySnowballOption(**note_terms, expiry_date=calendar.adjust(
            nominal, market.BusinessDayConvention.FOLLOWING))
        note_context = PricingContext(model_parameters=self.parameters, spot_price=100,
                                      valuation_time=date(2025, 1, 3), calendar=calendar)
        for engine in (pricing.MonteCarloBinarySnowballEngine(path_count=32, seed=7),
                       pricing.FiniteDifferenceBinarySnowballEngine()):
            with self.subTest(engine=type(engine).__name__):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(nominal_note, note_context)
                self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_DATE)
                self.assertTrue(math.isfinite(engine.price(adjusted_note, note_context)))

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

    def test_contract_date_errors_have_shared_categories(self):
        earlier = date(2025, 1, 1)
        later = date(2026, 1, 1)
        factories = (
            ("European", lambda start, end: EuropeanOption(
                option_type=OptionType.CALL, strike=100, effective_date=start, expiry_date=end)),
            ("Asian", lambda start, end: ArithmeticAveragePriceOption(
                option_type=OptionType.CALL, strike=100, averaging_start_date=start,
                effective_date=start, expiry_date=end)),
            ("Barrier", lambda start, end: BarrierOption(
                option_type=OptionType.CALL, strike=100, barrier_level=120,
                barrier_type=BarrierType.UP_AND_OUT, effective_date=start, expiry_date=end)),
            ("Accumulator", lambda start, end: Accumulator(
                strike=100, knock_out_level=110, daily_quantity=1, acceleration_factor=2,
                effective_date=start, expiry_date=end)),
            ("Snowball", lambda start, end: BinarySnowballOption(
                knock_out_coupon_rates=[0.1], maturity_coupon_rate=0.05, initial_spot=100,
                knock_out_levels=[110], upper_strike=100, lower_strike=60,
                observation_dates=[end], effective_date=start, expiry_date=end)),
        )
        for name, factory in factories:
            with self.subTest(product=name), self.assertRaises(kiyosi.KiyosiError) as error:
                factory(later, earlier)
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE)

        for schedule in (
            lambda: fixed_interval_schedule(start=later, end=earlier, interval_days=1),
            lambda: monthly_schedule(start=later, end=earlier, lock_up_months=1),
        ):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                schedule()
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_TIME_RANGE)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            ArithmeticAveragePriceOption(
                option_type=OptionType.CALL, strike=100,
                averaging_start_date=earlier - timedelta(days=1),
                effective_date=earlier, expiry_date=later,
            )
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_SCHEDULE)

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
                self.assertAlmostEqual(engine.price(option, context), 30.0, delta=1e-6)

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
                self.assertAlmostEqual(engine.price(option, context), 1 + 0.0025 * 91 / 365, delta=1e-6)

    def test_phoenix_annual_coupon_is_scale_and_frequency_invariant(self):
        start, middle, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        for scale in (100, 1000):
            for dates in ([end], [middle, end]):
                option = PhoenixOption(
                    coupon_rate=0.08, initial_spot=scale, knock_in_level=0.5 * scale,
                    knock_out_levels=[2 * scale] * len(dates),
                    coupon_barrier_levels=[0.9 * scale] * len(dates),
                    upper_strike=scale, lower_strike=0, observation_dates=dates,
                    knock_in_observation_mode=KnockInObservationMode.AT_EXPIRY,
                    effective_date=start, expiry_date=end,
                )
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0, dividend_yield=0, volatility=1e-8,
                    ),
                    spot_price=scale, valuation_time=start,
                    calendar=market.all_days_calendar(),
                )
                for engine in (
                    pricing.FiniteDifferencePhoenixEngine(
                        asset_step_count=400, time_step_count=400, asset_upper_boundary=4 * scale,
                    ),
                    pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
                ):
                    with self.subTest(scale=scale, dates=dates, engine=type(engine).__name__):
                        self.assertAlmostEqual(engine.price(option, context), 1.08, delta=1e-6)

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
        self.assertIsNotNone(AnalyticVanillaEngine().price(self.option, aware))

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
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(self.option, self.context, GreeksLevel.FULL)
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
        self.assertEqual(engine.price(expiry_option, expiry_context), 10.0)

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
        for value in (date(1600, 1, 1), date(2500, 1, 1), date(9999, 1, 1)):
            with self.subTest(value=value):
                context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=value)
                self.assertEqual(context.valuation_time, datetime.combine(value, datetime.min.time(), timezone.utc))
        latest = datetime(9999, 12, 31, 23, 59, 59, 999999, tzinfo=timezone.utc)
        context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=latest)
        self.assertEqual(context.valuation_time, latest)

    def test_geometric_asian_uses_elapsed_average_and_forward_start(self):
        engine = pricing.AnalyticGeometricAveragePriceEngine()
        effective = date(2025, 1, 1)
        expiry = date(2026, 1, 1)
        context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=date(2025, 7, 1))

        for start, realized, expected in (
            (effective, 80, 0.005141652127146822),
            (effective, 120, 9.472410353379193),
            (date(2025, 10, 1), 0, 5.064920706779442),
        ):
            with self.subTest(start=start, realized=realized):
                option = GeometricAveragePriceOption(
                    option_type=OptionType.CALL, strike=100, averaging_start_date=start,
                    realized_average=realized, effective_date=effective, expiry_date=expiry,
                )
                self.assertAlmostEqual(engine.price(option, context), expected, delta=1e-10)

        missing = GeometricAveragePriceOption(
            option_type=OptionType.CALL, strike=100, averaging_start_date=effective,
            effective_date=effective, expiry_date=expiry,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(missing, context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

        starting = GeometricAveragePriceOption(
            option_type=OptionType.CALL, strike=100, averaging_start_date=date(2025, 7, 1),
            effective_date=effective, expiry_date=expiry,
        )
        greeks = engine.price_with_greeks(starting, context, GreeksLevel.FULL)
        self.assertIsNotNone(greeks.delta)
        self.assertIsNone(greeks.theta)

        ending = GeometricAveragePriceOption(
            option_type=OptionType.CALL, strike=100, averaging_start_date=effective,
            realized_average=120, effective_date=effective, expiry_date=expiry,
        )
        near_expiry = PricingContext(
            model_parameters=self.parameters, spot_price=100, valuation_time=date(2025, 12, 31),
        )
        at_expiry = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=expiry)
        self.assertAlmostEqual(engine.price(ending, near_expiry), 19.937346821828626, delta=1e-10)
        self.assertEqual(engine.price(ending, at_expiry), 20)

    def test_single_arithmetic_fixing_has_european_time_value(self):
        effective = date(2025, 1, 1)
        expiry = date(2026, 1, 1)
        asian_engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        vanilla_engine = AnalyticVanillaEngine()
        for option_type in (OptionType.CALL, OptionType.PUT):
            asian = ArithmeticAveragePriceOption(
                option_type=option_type, strike=100, averaging_start_date=expiry,
                effective_date=effective, expiry_date=expiry,
            )
            vanilla = EuropeanOption(
                option_type=option_type, strike=100, effective_date=effective, expiry_date=expiry,
            )
            for valuation in (effective, date(2025, 12, 31)):
                context = PricingContext(
                    model_parameters=self.parameters, spot_price=100, valuation_time=valuation,
                )
                self.assertEqual(asian_engine.price(asian, context), vanilla_engine.price(vanilla, context))
                if option_type == OptionType.CALL and valuation == effective:
                    self.assertAlmostEqual(asian_engine.price(asian, context), 9.227005508154036, delta=1e-12)

        fixed = ArithmeticAveragePriceOption(
            option_type=OptionType.CALL, strike=100, averaging_start_date=expiry,
            realized_average=110, effective_date=effective, expiry_date=expiry,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            asian_engine.price(fixed, self.context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
        at_expiry = PricingContext(model_parameters=self.parameters, spot_price=110, valuation_time=expiry)
        self.assertEqual(asian_engine.price(fixed, at_expiry), 10)

    def test_arithmetic_asian_requires_elapsed_average(self):
        effective = date(2025, 1, 1)
        start = date(2025, 2, 1)
        expiry = date(2026, 1, 1)
        missing = ArithmeticAveragePriceOption(
            option_type=OptionType.CALL, strike=100, averaging_start_date=start,
            effective_date=effective, expiry_date=expiry,
        )
        known = ArithmeticAveragePriceOption(
            option_type=OptionType.CALL, strike=100, averaging_start_date=start,
            realized_average=110, effective_date=effective, expiry_date=expiry,
        )
        engine = pricing.TurnbullWakemanArithmeticAveragePriceEngine()
        for valuation in (effective, start):
            context = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=valuation)
            self.assertIsInstance(engine.price(missing, context), float)
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine.price(known, context)
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

        during = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=date(2025, 6, 1))
        at_expiry = PricingContext(model_parameters=self.parameters, spot_price=100, valuation_time=expiry)
        self.assertIsInstance(engine.price(known, during), float)
        for context in (during, at_expiry):
            with self.assertRaises(kiyosi.KiyosiError) as error:
                engine.price(missing, context)
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(missing, during, GreeksLevel.BASIC)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
        self.assertEqual(engine.price(known, at_expiry), 10)

    def test_digital_barrier_schedule_and_analytics(self):
        digital = CashOrNothingOption(option_type=OptionType.CALL, strike=100, payout=10, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1))
        self.assertGreater(AnalyticDigitalEngine().price(digital, self.context), 0)
        barrier = BarrierOption(option_type=OptionType.CALL, strike=100, effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1), barrier_level=80, barrier_type=BarrierType.DOWN_AND_OUT, rebate=1, rebate_timing=RebateTiming.AT_EXPIRY)
        self.assertGreater(AnalyticBarrierEngine().price(barrier, self.context), 0)
        self.assertEqual(barrier.rebate_timing, RebateTiming.AT_EXPIRY)
        self.assertEqual(barrier.observation_mode, ObservationMode.CONTINUOUS)
        self.assertIn("observation_dates=[]", repr(barrier))
        self.assertFalse(hasattr(barrier, "rebate_payment"))
        self.assertFalse(hasattr(barrier, "observation"))
        self.assertEqual(len(fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)), 5)
        engine = AnalyticVanillaEngine()
        self.assertIsNotNone(calculate_numerical_risk_measures(engine, self.option, self.context).vega)
        observed_price = engine.price(self.option, self.context)
        self.assertAlmostEqual(
            implied_volatility(engine, self.option, self.context, observed_price),
            self.parameters.volatility,
        )
        with self.assertRaises(kiyosi.KiyosiError) as error:
            implied_volatility(engine, self.option, self.context, observed_price, lower_bound=0.5, upper_bound=0.1)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_calculate_numerical_risk_measures_rejects_invalid_shift_settings(self):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            calculate_numerical_risk_measures(AnalyticVanillaEngine(), self.option, self.context, spot_shift=0.0)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

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
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price_with_greeks(self.option, self.context, GreeksLevel.BASIC)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_calculate_numerical_risk_measures_retains_valid_boundary_results(self):
        engine = AnalyticVanillaEngine()

        low_volatility = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                volatility=0.00005,
            ),
            spot_price=100.0,
            valuation_time=date(2025, 1, 1),
        )
        result = calculate_numerical_risk_measures(engine, self.option, low_volatility)
        self.assertAlmostEqual(result.price, engine.price(self.option, low_volatility))
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
        result = calculate_numerical_risk_measures(engine, self.option, low_spot)
        self.assertAlmostEqual(result.price, engine.price(self.option, low_spot))
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
        self.assertGreater(engine.price(cash, self.context), 0)
        self.assertGreater(engine.price(binary, self.context), 0)

    def test_barrier_history_is_required_and_changes_remaining_value(self):
        terms = dict(option_type=OptionType.CALL, strike=100,
                     effective_date=date(2025, 1, 1), expiry_date=date(2026, 1, 1),
                     barrier_level=120, barrier_type=BarrierType.UP_AND_OUT)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2),
            spot_price=100, valuation_time=date(2025, 7, 1))
        engine = AnalyticBarrierEngine()
        self.assertFalse(hasattr(BarrierTouchState, "UNKNOWN"))
        self.assertIsNone(BarrierOption(**terms).touch_state)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(BarrierOption(**terms, touch_state=None), context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
        self.assertGreater(engine.price(BarrierOption(**terms, touch_state=BarrierTouchState.UNTOUCHED), context), 0)
        self.assertEqual(engine.price(BarrierOption(**terms, touch_state=BarrierTouchState.TOUCHED), context), 0)
        knocked_in = BarrierOption(**(terms | {"barrier_type": BarrierType.UP_AND_IN}),
                                   touch_state=BarrierTouchState.TOUCHED)
        self.assertAlmostEqual(engine.price(knocked_in, context), AnalyticVanillaEngine().price(
            EuropeanOption(option_type=OptionType.CALL, strike=100,
                           effective_date=terms["effective_date"], expiry_date=terms["expiry_date"]), context))
        touch_terms = dict(effective_date=terms["effective_date"], expiry_date=terms["expiry_date"],
                           barrier_level=120, payout=10, touch_state=BarrierTouchState.TOUCHED)
        binary_engine = AnalyticBinaryBarrierEngine()
        self.assertEqual(binary_engine.price(cash_one_touch_up(
            **touch_terms, settlement_timing=SettlementTiming.AT_HIT), context), 0)
        self.assertAlmostEqual(binary_engine.price(cash_one_touch_up(
            **touch_terms, settlement_timing=SettlementTiming.AT_EXPIRY), context),
            10 * math.exp(-0.04 * 184 / 365))

    def test_public_api_matches_shared_language_parity_cases(self):
        cases = parity_cases()
        self.assertEqual(len(cases), 10)
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
                elif kind in ("pricing", "pricing_greeks"):
                    parameters = BlackScholesMertonParameters(risk_free_rate=float(values["risk_free_rate"]), dividend_yield=float(values["dividend_yield"]), volatility=float(values["volatility"]))
                    context = PricingContext(model_parameters=parameters, spot_price=float(values["spot_price"]), valuation_time=date.fromisoformat(values["valuation_date"]))
                    option = EuropeanOption(option_type=getattr(OptionType, values["type"].upper()), strike=float(values["strike"]), effective_date=date.fromisoformat(values["effective_date"]), expiry_date=date.fromisoformat(values["expiry_date"]))
                    result = AnalyticVanillaEngine().price(option, context)
                    self.assertAlmostEqual(result, float(expected["price"]), delta=float(case["tolerance"]))
                    if kind == "pricing_greeks":
                        result = AnalyticVanillaEngine().price_with_greeks(
                            option, context, getattr(GreeksLevel, values["level"].upper())
                        )
                        for name, value in expected.items():
                            with self.subTest(measure=name):
                                if value == "none":
                                    self.assertIsNone(getattr(result, name))
                                else:
                                    self.assertAlmostEqual(getattr(result, name), float(value), delta=float(case["tolerance"]))
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
