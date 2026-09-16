import csv
import unittest
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

import kiyosi
import kiyosi.pricing as pricing
from kiyosi.instruments import Accumulator, BarrierOption, BarrierType, CashOrNothingOption, EuropeanOption, GeometricAverageOption, OptionType, standard_snowball
from kiyosi.market import BsmParameters, PricingContext, fixed_interval_schedule
from kiyosi.pricing import AnalyticBarrierEngine, AnalyticDigitalEngine, AnalyticVanillaEngine, FiniteDifferenceScheme, numerical_analytics


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
        note = standard_snowball(coupon_rate=0.1, initial_price=100, knock_in_price=80, knock_out_price=105, observations=[expiry], effective=date(2025, 1, 1), expiry=expiry)
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
        barrier = BarrierOption(type=OptionType.CALL, strike=100, effective=date(2025, 1, 1), expiry=date(2026, 1, 1), barrier=80, barrier_kind=BarrierType.DOWN_AND_OUT)
        self.assertGreater(AnalyticBarrierEngine().price(barrier, self.context).price, 0)
        self.assertEqual(len(fixed_interval_schedule(start=date(2025, 1, 1), end=date(2025, 3, 1), interval_days=10)), 5)
        self.assertIsNotNone(numerical_analytics(AnalyticVanillaEngine(), self.option, self.context).vega)

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
