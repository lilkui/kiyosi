import csv
import unittest
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import kiyosi
import kiyosi.market as market
import kiyosi.pricing as pricing
from kiyosi.instruments import (
    Accumulator,
    BarrierOption,
    BarrierType,
    CashOrNothingOption,
    EuropeanOption,
    GeometricAverageOption,
    ObservationMode,
    OptionType,
    PayoffType,
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
from kiyosi.market import BsmParameters, PricingContext, fixed_interval_schedule, monthly_schedule
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
        self.parameters = BsmParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2)
        self.context = PricingContext(parameters=self.parameters, asset_price=100.0, valuation_time=date(2025, 1, 1))
        self.option = EuropeanOption(type=OptionType.CALL, strike=100.0, effective=date(2025, 1, 1), expiry=date(2026, 1, 1))

    def test_explicit_pricing_result_surface(self):
        self.assertFalse(hasattr(kiyosi, "price"))
        self.assertFalse(hasattr(pricing, "price"))
        result = AnalyticVanillaEngine().price(self.option, self.context)
        self.assertEqual(len(result), 11)
        self.assertEqual(result["price"], result.price)
        self.assertIn("speed", result)

    def test_domain_values_have_value_equality_and_readable_representations(self):
        same_parameters = BsmParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2)
        same_option = EuropeanOption(type=OptionType.CALL, strike=100.0, effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        self.assertEqual(self.parameters, same_parameters)
        self.assertEqual(self.option, same_option)
        self.assertNotEqual(self.option, EuropeanOption(type=OptionType.PUT, strike=100.0, effective=date(2025, 1, 1), expiry=date(2026, 1, 1)))
        with self.assertRaises(TypeError):
            hash(self.parameters)

        schedule = fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)
        same_schedule = fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)
        self.assertEqual(schedule, same_schedule)

        note_terms = dict(coupon_rate=0.1, initial_price=100, knock_in_price=80, knock_out_price=105, observation_dates=[date(2026, 1, 1)], effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        note = standard_snowball(**note_terms)
        self.assertEqual(note, standard_snowball(**note_terms))

        result = AnalyticVanillaEngine().price(self.option, self.context)
        cases = (
            (self.parameters, "BsmParameters(", "volatility=0.2"),
            (self.option, "EuropeanOption(", "strike=100.0"),
            (schedule, "ObservationSchedule(", "dates=["),
            (market.weekdays_calendar(), "TradingCalendar(", "annual_trading_days=252"),
            (self.context, "PricingContext(", "asset_price=100.0"),
            (note, "SnowballOption(", "knock_out_coupon_rates=[0.1]"),
            (pricing.FiniteDifferenceVanillaEngine(), "FiniteDifferenceVanillaEngine(", "asset_steps="),
            (result, "PricingResult(", "price="),
        )
        for value, prefix, field in cases:
            with self.subTest(type=type(value).__name__):
                representation = repr(value)
                self.assertTrue(representation.startswith(prefix))
                self.assertIn(field, representation)

    def test_snowball_implied_coupon_uses_explicit_quote_convention(self):
        terms = dict(
            initial_price=100,
            knock_in_price=70,
            observation_dates=[date(2025, 7, 1), date(2026, 1, 1)],
            effective=date(2025, 1, 1),
            expiry=date(2026, 1, 1),
        )
        standard = standard_snowball(coupon_rate=0.10, knock_out_price=105, **terms)
        both_down = both_down_snowball(
            coupon_start=0.10,
            coupon_step=0.01,
            knock_out_start=110,
            knock_out_step=5,
            **terms,
        )
        dual = dual_coupon_snowball(
            knock_out_coupon=0.10,
            maturity_coupon=0.03,
            knock_out_price=105,
            **terms,
        )
        target_standard = standard_snowball(
            coupon_rate=0.12, knock_out_price=105, **terms
        )
        target_both_down = both_down_snowball(
            coupon_start=0.12,
            coupon_step=0.01,
            knock_out_start=110,
            knock_out_step=5,
            **terms,
        )
        target_dual = dual_coupon_snowball(
            knock_out_coupon=0.12,
            maturity_coupon=0.03,
            knock_out_price=105,
            **terms,
        )

        linked = pricing.CouponQuoteConvention.LINKED_MATURITY
        fixed = pricing.CouponQuoteConvention.FIXED_MATURITY
        engine = pricing.FiniteDifferenceSnowballEngine(asset_steps=40, time_steps=40)
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
        self.assertIn("validated", BsmParameters.__doc__.lower())
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
        self.assertEqual(calendar.annual_trading_days, 252)
        self.assertFalse(calendar.is_trading_day(date(2025, 1, 4)))
        self.assertFalse(self.context.calendar.is_trading_day(date(2025, 1, 4)))
        self.assertEqual(calendar.trading_days_between(date(2025, 1, 4), date(2025, 1, 6)), 0)
        for operation in (calendar.trading_days_between, calendar.trading_year_fraction):
            with self.subTest(operation=operation.__name__), self.assertRaises(kiyosi.KiyosiError) as error:
                operation(date(2025, 1, 6), date(2025, 1, 4))
            self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_EXPIRY)

    def test_native_domain_errors_expose_categories(self):
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BsmParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.0)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_VOLATILITY)

    def test_accumulator_errors_identify_rejected_terms(self):
        terms = {
            "strike": 100.0,
            "knock_out": 110.0,
            "daily_quantity": 1.0,
            "acceleration": 2.0,
            "effective": date(2025, 1, 1),
            "expiry": date(2026, 1, 1),
        }
        cases = (
            ("strike", 0.0, kiyosi.ErrorCategory.INVALID_STRIKE, "strike must be finite and positive"),
            ("knock_out", 0.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "knock-out price must be finite and positive"),
            ("daily_quantity", -1.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "daily quantity must be finite and non-negative"),
            ("acceleration", -1.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "acceleration must be finite and non-negative"),
            ("accumulated_quantity", -1.0, kiyosi.ErrorCategory.INVALID_PARAMETER, "accumulated quantity must be finite and non-negative"),
        )
        for field, value, category, message in cases:
            with self.subTest(field=field), self.assertRaises(kiyosi.KiyosiError) as error:
                Accumulator(**{**terms, field: value})
            self.assertEqual(error.exception.category, category)
            self.assertEqual(str(error.exception), message)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            Accumulator(**{**terms, "effective": terms["expiry"], "expiry": terms["effective"]})
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_EXPIRY)
        self.assertEqual(str(error.exception), "expiry must not precede effective")

    def test_numeric_and_date_boundaries_are_checked(self):
        with self.assertRaises(TypeError):
            EuropeanOption(type=OptionType.CALL, strike="100", effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        with self.assertRaises(TypeError):
            BsmParameters(risk_free_rate=True, dividend_yield=0.02, volatility=0.2)
        with self.assertRaises(TypeError):
            pricing.FiniteDifferenceVanillaEngine(asset_steps=1.5)
        with self.assertRaises(TypeError):
            pricing.MonteCarloVanillaEngine(seed=True)
        with self.assertRaises(OverflowError):
            pricing.FiniteDifferenceVanillaEngine(asset_steps=2**40)
        with self.assertRaises(OverflowError):
            BsmParameters(risk_free_rate=10**400, dividend_yield=0.02, volatility=0.2)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            BsmParameters(risk_free_rate=float("inf"), dividend_yield=0.02, volatility=0.2)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_RATE)
        for seed in (-1, 2**64):
            with self.subTest(seed=seed), self.assertRaises(OverflowError):
                pricing.MonteCarloVanillaEngine(seed=seed)
        maximum_seed = 2**64 - 1
        self.assertEqual(pricing.MonteCarloVanillaEngine(seed=maximum_seed).seed, maximum_seed)
        with self.assertRaises(TypeError):
            PricingContext(parameters=self.parameters, asset_price=100, valuation_time=datetime(2025, 1, 1))
        aware = PricingContext(parameters=self.parameters, asset_price=100, valuation_time=datetime(2025, 1, 1, 8, tzinfo=timezone.utc))
        self.assertIsNotNone(AnalyticVanillaEngine().price(self.option, aware).price)

    def test_temporal_accessors_preserve_date_and_timestamp_semantics(self):
        average_start = date(2025, 2, 1)
        average = GeometricAverageOption(type=OptionType.CALL, strike=100, average_start=average_start, effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        self.assertIs(type(average.average_start), date)
        self.assertEqual(average.average_start, average_start)

        expiry = date(2026, 1, 1)
        note = standard_snowball(coupon_rate=0.1, initial_price=100, knock_in_price=80, knock_out_price=105, observation_dates=[expiry], effective=date(2025, 1, 1), expiry=expiry)
        self.assertEqual(note.observation_dates, [expiry])
        self.assertIs(type(note.effective), date)
        self.assertIs(type(note.expiry), date)

        local_time = datetime(2025, 1, 1, 8, 9, 10, 123456, tzinfo=timezone(timedelta(hours=8)))
        context = PricingContext(parameters=self.parameters, asset_price=100, valuation_time=local_time)
        self.assertEqual(context.valuation_time, datetime(2025, 1, 1, 0, 9, 10, 123456, tzinfo=timezone.utc))
        midnight = PricingContext(parameters=self.parameters, asset_price=100, valuation_time=date(2025, 1, 1))
        self.assertEqual(midnight.valuation_time, datetime(2025, 1, 1, tzinfo=timezone.utc))

    def test_digital_barrier_schedule_and_analytics(self):
        digital = CashOrNothingOption(type=OptionType.CALL, strike=100, payout=10, effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        self.assertGreater(AnalyticDigitalEngine().price(digital, self.context).price, 0)
        barrier = BarrierOption(type=OptionType.CALL, strike=100, effective=date(2025, 1, 1), expiry=date(2026, 1, 1), barrier=80, barrier_kind=BarrierType.DOWN_AND_OUT, rebate=1, rebate_timing=RebateTiming.AT_EXPIRY)
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

    def test_numerical_analytics_forwards_explicit_shift_settings(self):
        analytics = NumericalAnalyticsEngine(AnalyticVanillaEngine(), spot_shift=0.0)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            analytics.price(self.option, self.context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
        self.assertFalse(hasattr(pricing, "numerical_analytics"))

    def test_engine_settings_are_validated_when_pricing(self):
        configured_engines = (
            pricing.CrrVanillaEngine,
            pricing.FiniteDifferenceVanillaEngine,
            pricing.MonteCarloVanillaEngine,
            pricing.MonteCarloSnowballEngine,
        )
        for engine_type in configured_engines:
            with self.subTest(engine=engine_type.__name__):
                self.assertIn("validated when price() is called", engine_type.__doc__)
                self.assertIn("validated when price() is called", engine_type.__init__.__doc__)

        engine = pricing.FiniteDifferenceVanillaEngine(asset_steps=0)
        self.assertEqual(engine.asset_steps, 0)

        with self.assertRaises(kiyosi.KiyosiError) as error:
            engine.price(self.option, self.context)
        self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)

    def test_numerical_analytics_retains_valid_boundary_results(self):
        engine = AnalyticVanillaEngine()
        analytics = NumericalAnalyticsEngine(engine)

        low_volatility = PricingContext(
            parameters=BsmParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                volatility=0.00005,
            ),
            asset_price=100.0,
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
            parameters=self.parameters,
            asset_price=0.005,
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
        terms = dict(effective=date(2025, 1, 1), expiry=date(2026, 1, 1))
        cash = cash_one_touch_up(
            **terms, barrier=130, payout=10,
            settlement_timing=SettlementTiming.AT_HIT,
            observation_mode=ObservationMode.SCHEDULED,
            observation_dates=[date(2025, 6, 2), terms["expiry"]])
        asset = asset_no_touch_down(**terms, barrier=70)
        self.assertIsInstance(cash, TouchOption)
        self.assertTrue(cash.is_one_touch)
        self.assertTrue(cash.is_up)
        self.assertEqual(cash.payout, 10)
        self.assertEqual(cash.payoff_type, PayoffType.CASH)
        self.assertEqual(cash.settlement_timing, SettlementTiming.AT_HIT)
        self.assertEqual(cash.observation_dates, [date(2025, 6, 2), terms["expiry"]])
        self.assertFalse(asset.is_one_touch)
        self.assertFalse(asset.is_up)
        self.assertIsNone(asset.payout)
        self.assertEqual(asset.payoff_type, PayoffType.ASSET)
        with self.assertRaises(TypeError):
            cash_one_touch_up(**terms, barrier=130, payout=10, strike=100)
        with self.assertRaises(TypeError):
            asset_no_touch_down(**terms, barrier=70, payout=10)

        binary = cash_binary_barrier_option(
            **terms, type=OptionType.CALL, strike=100, barrier=80,
            barrier_kind=BarrierType.DOWN_AND_OUT, payout=10)
        self.assertEqual(binary.type, OptionType.CALL)
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
                    option = EuropeanOption(type=getattr(OptionType, values["type"].upper()), strike=float(values["strike"]), effective=date.fromisoformat(values["effective"]), expiry=date.fromisoformat(values["expiry"]))
                    self.assertEqual(option.type, getattr(OptionType, expected["type"].upper()))
                    self.assertEqual(option.strike, float(expected["strike"]))
                    self.assertEqual(option.effective, date.fromisoformat(expected["effective"]))
                    self.assertEqual(option.expiry, date.fromisoformat(expected["expiry"]))
                elif kind == "defaults":
                    engine = pricing.FiniteDifferenceVanillaEngine()
                    self.assertEqual(engine.asset_steps, int(expected["asset_steps"]))
                    self.assertEqual(engine.time_steps, int(expected["time_steps"]))
                    self.assertEqual(engine.scheme, getattr(FiniteDifferenceScheme, expected["scheme"].upper()))
                    self.assertEqual(expected["upper_boundary"], "none")
                    self.assertIsNone(engine.upper_boundary)
                elif kind == "pricing":
                    parameters = BsmParameters(risk_free_rate=float(values["risk_free_rate"]), dividend_yield=float(values["dividend_yield"]), volatility=float(values["volatility"]))
                    context = PricingContext(parameters=parameters, asset_price=float(values["asset_price"]), valuation_time=date.fromisoformat(values["valuation_date"]))
                    option = EuropeanOption(type=getattr(OptionType, values["type"].upper()), strike=float(values["strike"]), effective=date.fromisoformat(values["effective"]), expiry=date.fromisoformat(values["expiry"]))
                    result = AnalyticVanillaEngine().price(option, context)
                    self.assertAlmostEqual(result.price, float(expected["price"]), delta=float(case["tolerance"]))
                elif kind == "domain_error":
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        BsmParameters(risk_free_rate=float(values["risk_free_rate"]), dividend_yield=float(values["dividend_yield"]), volatility=float(values["volatility"]))
                    self.assertEqual(error.exception.category, getattr(kiyosi.ErrorCategory, expected["category"].upper()))
                elif kind == "date_round_trip":
                    option = GeometricAverageOption(type=getattr(OptionType, values["type"].upper()), strike=float(values["strike"]), average_start=date.fromisoformat(values["average_start"]), effective=date.fromisoformat(values["effective"]), expiry=date.fromisoformat(values["expiry"]))
                    self.assertEqual(option.average_start, date.fromisoformat(expected["average_start"]))
                    self.assertEqual(option.effective, date.fromisoformat(expected["effective"]))
                    self.assertEqual(option.expiry, date.fromisoformat(expected["expiry"]))
                elif kind == "timestamp_round_trip":
                    parameters = BsmParameters(risk_free_rate=float(values["risk_free_rate"]), dividend_yield=float(values["dividend_yield"]), volatility=float(values["volatility"]))
                    context = PricingContext(parameters=parameters, asset_price=float(values["asset_price"]), valuation_time=utc_timestamp(values["valuation_time"]))
                    self.assertEqual(context.valuation_time, utc_timestamp(expected["valuation_time"]))
                    self.assertEqual(context.valuation_date, date.fromisoformat(expected["valuation_date"]))
                elif kind == "numeric_boundary":
                    engine = pricing.MonteCarloVanillaEngine(seed=int(values["seed"]))
                    self.assertEqual(engine.seed, int(expected["seed"]))
                else:
                    self.fail(f"unknown parity case kind: {kind}")


if __name__ == "__main__":
    unittest.main()
