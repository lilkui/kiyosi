"""Live independent QuantLib comparisons through the public Python wrapper."""

import math
import unittest
from collections import Counter
from datetime import date
from unittest.mock import patch

from kiyosi import instruments, pricing
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    weekdays_calendar,
)
from price_differences import price_differences
from quantlib_reference import (
    american,
    asian,
    barrier,
    binary,
    digital,
    oracle,
    scenarios,
)


def market(inputs):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=inputs["rate"],
            dividend_yield=inputs["dividend"],
            volatility=inputs["volatility"],
        ),
        spot_price=inputs["spot"],
        valuation_time=date.fromisoformat(inputs["valuation"]),
        calendar=weekdays_calendar(),
    )


def contract(row):
    inputs = row["inputs"]
    dates = {
        key: date.fromisoformat(inputs[key])
        for key in ("effective_date", "expiry_date")
    }
    kind = row["instrument"]
    if kind == "TouchOption":
        asset = "asset" if inputs["asset_settlement"] else "cash"
        touch = "one_touch" if inputs["BarrierType"].endswith("in") else "no_touch"
        direction = "up" if inputs["BarrierType"].startswith("up") else "down"
        settings = dict(dates, barrier_level=inputs["barrier"], touch_state="untouched")
        if asset == "cash":
            settings["payout"] = inputs["payout"]
        if touch == "one_touch":
            settings["settlement_timing"] = inputs["settlement"]
        return getattr(instruments, f"{asset}_{touch}_{direction}")(**settings)

    settings = dict(dates, option_type=inputs["option"], strike=inputs["strike"])
    if kind in ("BarrierOption", "BinaryBarrierOption"):
        settings.update(
            barrier_level=inputs["barrier"],
            barrier_type=inputs["BarrierType"],
            touch_state="untouched",
        )
        if kind == "BinaryBarrierOption":
            asset = inputs["asset_settlement"]
            if not asset:
                settings["payout"] = inputs["payout"]
            factory = (
                instruments.asset_binary_barrier_option
                if asset
                else instruments.cash_binary_barrier_option
            )
            return factory(**settings)
        settings.update(rebate=inputs["rebate"], rebate_timing=inputs["settlement"])
    elif kind == "CashOrNothingOption":
        settings["payout"] = inputs["payout"]
    elif kind in ("GeometricAveragePriceOption", "ArithmeticAveragePriceOption"):
        settings.update(
            averaging_start_date=date.fromisoformat(inputs["averaging_start_date"]),
            realized_average=inputs["realized_average"],
        )
    return getattr(instruments, kind)(**settings)


def engine(row):
    inputs = row["inputs"]
    name = row["engine"]
    settings = {}
    if name.startswith("FiniteDifference"):
        settings = {
            key: inputs[key]
            for key in (
                "asset_step_count",
                "time_step_count",
                "scheme",
                "asset_upper_boundary",
            )
        }
    elif name == "CoxRossRubinsteinVanillaEngine":
        settings["step_count"] = inputs["step_count"]
    elif name == "MonteCarloVanillaEngine":
        settings = {key: inputs[key] for key in ("path_count", "step_count", "seed")}
    return getattr(pricing, name)(**settings)


def shifts(inputs):
    return {
        key: inputs[key]
        for key in ("spot_shift", "volatility_shift", "rate_shift", "time_shift_days")
        if key in inputs
    }


class QuantLibReferenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.previous_date = oracle.ql.Settings.instance().evaluationDate.serialNumber()
        cls.rows = scenarios.rows()
        cls.restored_date = oracle.ql.Settings.instance().evaluationDate.serialNumber()

    def test_reference_inventory_and_stability(self):
        self.assertEqual(oracle.ql.__version__, "1.43")
        self.assertEqual(self.previous_date, self.restored_date)
        self.assertEqual(len({row["case_id"] for row in self.rows}), 664)
        self.assertEqual(
            Counter(row["instrument"] for row in self.rows),
            {
                "EuropeanOption": 92,
                "AmericanOption": 40,
                "CashOrNothingOption": 60,
                "AssetOrNothingOption": 60,
                "BarrierOption": 120,
                "BinaryBarrierOption": 160,
                "TouchOption": 108,
                "GeometricAveragePriceOption": 6,
                "ArithmeticAveragePriceOption": 18,
            },
        )
        self.assertEqual(len({row["engine"] for row in self.rows}), 14)
        for row in self.rows:
            with self.subTest(case=row["case_id"]):
                inputs = row["inputs"]
                outputs = row["outputs"]
                self.assertEqual(set(outputs), set(row["tolerances"]))
                self.assertEqual(inputs["calendar"], "weekends_only")
                self.assertEqual(inputs["convention"], oracle.CONVENTION)
                unavailable = {
                    key.removeprefix("unavailable_")
                    for key in inputs
                    if key.startswith("unavailable_")
                }
                self.assertEqual(set(outputs), set(oracle.MEASURES) - unavailable)
                for name, value in outputs.items():
                    self.assertTrue(math.isfinite(value))
                    self.assertEqual(inputs[f"unit_{name}"], oracle.UNITS[name])
                    uncertainty = inputs[f"uncertainty_{name}"]
                    self.assertTrue(math.isfinite(uncertainty))
                    self.assertGreaterEqual(uncertainty, 0)
                    self.assertLessEqual(uncertainty, inputs[f"stability_limit_{name}"])
                    self.assertGreaterEqual(row["tolerances"][name], 0)

    def test_prices_and_greeks_match_quantlib(self):
        native_pairs = {
            "CoxRossRubinsteinVanillaEngine",
            "FiniteDifferenceVanillaEngine",
            "AnalyticDigitalEngine",
            "FiniteDifferenceDigitalEngine",
        }
        for row in self.rows:
            with self.subTest(case=row["case_id"]):
                inputs = row["inputs"]
                option, context, pricer = contract(row), market(inputs), engine(row)
                expected = row["outputs"]
                self.assertAlmostEqual(
                    pricer.price(option, context),
                    expected["price"],
                    delta=row["tolerances"]["price"],
                )
                analytic = row["engine"] == "AnalyticVanillaEngine"
                wrapped = analytic or inputs["wrapper"]
                native_names = (
                    set(expected) - {"price"}
                    if analytic
                    else {"delta", "gamma"} & expected.keys()
                    if row["engine"] in native_pairs
                    else set()
                )
                requested = set(expected) - {"price"} if wrapped else native_names
                if requested:
                    joint = pricer.price_with_greeks(
                        option, context, sorted(requested), **shifts(inputs)
                    )
                    self.assertAlmostEqual(
                        joint.price, expected["price"], delta=row["tolerances"]["price"]
                    )
                    for name in requested:
                        with self.subTest(measure=name, method="public"):
                            value = getattr(joint, name)
                            self.assertIsNotNone(value)
                            budget = (
                                row["tolerances"][name]
                                if name in native_names
                                else inputs[f"numerical_tolerance_{name}"]
                            )
                            self.assertAlmostEqual(
                                value,
                                expected[name],
                                delta=budget + inputs[f"uncertainty_{name}"],
                            )
                if not wrapped or row["engine"] == "MonteCarloVanillaEngine":
                    continue
                numerical = price_differences(
                    pricer,
                    option,
                    context,
                    sorted(set(expected) - {"price"}),
                    **shifts(inputs),
                )
                for name, value in expected.items():
                    with self.subTest(measure=name, method="scalar"):
                        self.assertAlmostEqual(
                            getattr(numerical, name),
                            value,
                            delta=inputs[f"numerical_tolerance_{name}"]
                            + inputs[f"uncertainty_{name}"],
                        )

    def test_implied_volatility_recovers_reference_inputs(self):
        for row in self.rows:
            if row["engine"] != "AnalyticVanillaEngine":
                continue
            with self.subTest(case=row["case_id"]):
                self.assertAlmostEqual(
                    pricing.implied_volatility(
                        engine(row),
                        contract(row),
                        market(row["inputs"]),
                        row["outputs"]["price"],
                    ),
                    row["inputs"]["volatility"],
                    delta=1e-7,
                )

    def test_reference_failure_restores_quantlib_settings(self):
        original = oracle.ql.Settings.instance().evaluationDate.serialNumber()

        def fail(inputs):
            oracle.market_process(inputs)
            raise ValueError("reference failure")

        with (
            patch.object(oracle, "reference", side_effect=fail),
            self.assertRaisesRegex(ValueError, "reference failure"),
        ):
            scenarios.rows()
        self.assertEqual(
            oracle.ql.Settings.instance().evaluationDate.serialNumber(), original
        )

    def test_contract_bindings_and_payoff_decompositions(self):
        with oracle.ql.SavedSettings():
            barrier.check_bindings()
            binary.check_bindings()
            asian.check_bindings()
        for direction, cash, asset in (
            (oracle.ql.Option.Call, [0, 0, 10], [0, 0, 101]),
            (oracle.ql.Option.Put, [10, 0, 0], [99, 0, 0]),
        ):
            with self.subTest(direction=direction):
                self.assertEqual(
                    [
                        oracle.ql.CashOrNothingPayoff(direction, 100, 10)(s)
                        for s in (99, 100, 101)
                    ],
                    cash,
                )
                self.assertEqual(
                    [
                        oracle.ql.AssetOrNothingPayoff(direction, 100)(s)
                        for s in (99, 100, 101)
                    ],
                    asset,
                )

    def test_unstable_and_nonfinite_references_are_rejected(self):
        inputs = next(
            row["inputs"]
            for row in self.rows
            if row["engine"] == "AnalyticVanillaEngine"
        )
        market_inputs = {key: inputs[key] for key in oracle.INPUTS}
        real_measure = oracle.measure
        for bad in (float("nan"), float("inf"), 1000.0):

            def corrupt(market, name, scale=1, price_only=False, bad=bad, **options):
                if name == "vega" and scale == 2:
                    return bad
                return real_measure(market, name, scale, price_only, **options)

            with (
                self.subTest(value=bad),
                oracle.ql.SavedSettings(),
                patch.object(oracle, "measure", side_effect=corrupt),
                self.assertRaises(ValueError),
            ):
                oracle.reference(market_inputs)

    def test_american_unstable_and_nonfinite_references_are_rejected(self):
        inputs = next(
            row["inputs"]
            for row in self.rows
            if row["case_id"].startswith("ql-american-put-100-365d-")
        )
        market_inputs = {key: inputs[key] for key in oracle.INPUTS}
        real_measure = american.measure
        for bad in (float("nan"), float("inf"), 1000.0):

            def corrupt(market, name, grid=american.GRIDS[-1], scale=1, bad=bad):
                if name == "vega" and scale == 2:
                    return bad
                return real_measure(market, name, grid, scale)

            with (
                self.subTest(value=bad),
                oracle.ql.SavedSettings(),
                patch.object(american, "measure", side_effect=corrupt),
                self.assertRaises(ValueError),
            ):
                american.reference(market_inputs)

    def test_reference_higher_greeks_agree_with_price_differences(self):
        for row in self.rows:
            if row["engine"] not in ("AnalyticVanillaEngine", "AnalyticDigitalEngine"):
                continue
            inputs = row["inputs"]
            if (
                date.fromisoformat(inputs["expiry_date"])
                - date.fromisoformat(inputs["valuation"])
            ).days <= 2:
                continue
            for name in ("speed", "charm", "color", "vanna", "zomma"):
                with (
                    self.subTest(case=row["case_id"], measure=name),
                    oracle.ql.SavedSettings(),
                ):
                    direct = oracle.measure(inputs, name)
                    fallback = oracle.measure(inputs, name, price_only=True)
                    limit = (
                        1e-7
                        if row["engine"] == "AnalyticVanillaEngine"
                        else digital.STABILITY[name]
                    )
                    self.assertLess(abs(direct - fallback), limit)

    def test_american_long_expiry_domain_matches_live_quantlib(self):
        inputs = {
            "option": "call",
            "strike": 100,
            "spot": 100,
            "rate": 0.03,
            "dividend": 0.05,
            "volatility": 1.0,
            "effective_date": "2025-01-01",
            "valuation": "2025-01-01",
            "expiry_date": "2035-01-01",
        }
        with oracle.ql.SavedSettings():
            reference = american.native(tuple(sorted(inputs.items())), (3200, 3200))[
                "price"
            ]
        option = instruments.AmericanOption(
            option_type="call",
            strike=100,
            effective_date=date(2025, 1, 1),
            expiry_date=date(2035, 1, 1),
        )
        context = market(inputs)
        coarse = pricing.FiniteDifferenceVanillaEngine().price(option, context)
        fine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=800, time_step_count=1600
        )
        refined = fine.price(option, context)
        self.assertAlmostEqual(coarse, reference, delta=0.2)
        self.assertAlmostEqual(refined, reference, delta=0.03)
        self.assertLess(abs(refined - reference), abs(coarse - reference))
        joint = fine.price_with_greeks(option, context, "vega")
        self.assertEqual(joint.price, refined)
        self.assertIsNotNone(joint.vega)
