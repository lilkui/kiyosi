"""Terminal risk follows the payoff while preserving engine coverage."""

import unittest
from datetime import datetime

from kiyosi import instruments as ins
from kiyosi import market, pricing
from kiyosi.pricing import numerical_greeks
from support.terminal import assert_terminal_greeks


class ExpiryGreekTests(unittest.TestCase):
    def test_native_terminal_greeks_honor_spot_bumps(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100.005,
            valuation_time=end,
        )
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_time": start,
            "expiry_time": end,
        }
        option = ins.EuropeanOption(**terms)
        digital = ins.CashOrNothingOption(**terms, payout=10)
        barrier = ins.BarrierOption(
            **terms,
            barrier_type="up_and_out",
            barrier_level=120,
            touch_state="untouched",
        )
        cases = [
            (pricing.AnalyticEuropeanEngine(), option, 1),
            (pricing.CoxRossRubinsteinDigitalEngine(), digital, 0),
            (pricing.CoxRossRubinsteinBarrierEngine(), barrier, 1),
            (pricing.MonteCarloDigitalEngine(path_count=32, seed=42), digital, 0),
        ]
        for kind in (ins.GeometricAveragePriceOption, ins.ArithmeticAveragePriceOption):
            cases.append(
                (
                    pricing.MonteCarloAsianEngine(path_count=32, seed=42),
                    kind(**terms, averaging_start_time=end),
                    1,
                )
            )
        for engine, instrument, expected in cases:
            with self.subTest(
                engine=type(engine).__name__, instrument=type(instrument).__name__
            ):
                self.assertIsNone(
                    engine.price_with_greeks(instrument, context, "delta").delta
                )
                result = numerical_greeks(
                    engine, instrument, context, "delta", spot_shift=0.001
                )
                self.assertAlmostEqual(result.require("delta"), expected, delta=1e-10)

    def test_terminal_autocallable_stencils_preserve_knock_in_and_redemption_regimes(
        self,
    ):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        note = ins.SnowballOption(
            maturity_coupon_rate=0.06,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=60,
            effective_time=start,
            expiry_time=end,
            knock_in_observation_mode="at_expiry",
            barrier_state="no_event",
            observations=ins.autocallable_observations(
                [end], [120], knock_out_coupon_rates=[0.08]
            ),
        )
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        for spot, delta in ((60, None), (70, 0.01), (80, None), (100, 0), (120, None)):
            with self.subTest(spot=spot):
                context = market.PricingContext(
                    model_parameters=model, spot_price=spot, valuation_time=end
                )
                result = pricing.QuadratureAutocallableEngine().price_with_greeks(
                    note, context, "all"
                )
                assert_terminal_greeks(self, result, delta)

    def test_terminal_vanilla_greeks_match_payoff_derivatives_across_engines(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=120,
            valuation_time=end,
        )
        for kind, analytic in (
            (ins.EuropeanOption, pricing.AnalyticEuropeanEngine()),
            (ins.AmericanOption, pricing.BjerksundStenslandVanillaEngine()),
        ):
            for direction, delta in (("call", 1), ("put", 0)):
                option = kind(
                    option_type=direction,
                    strike=100,
                    effective_time=start,
                    expiry_time=end,
                )
                for engine in (
                    analytic,
                    pricing.QuadratureVanillaEngine(),
                    pricing.CoxRossRubinsteinVanillaEngine(),
                    pricing.FiniteDifferenceVanillaEngine(),
                    pricing.MonteCarloVanillaEngine(
                        path_count=32, seed=42, thread_count=1
                    ),
                ):
                    with self.subTest(
                        kind=kind.__name__,
                        direction=direction,
                        engine=type(engine).__name__,
                    ):
                        result = engine.price_with_greeks(option, context, "all")
                        self.assertAlmostEqual(
                            result.require("delta"), delta, delta=1e-10
                        )
                        for greek in (
                            "gamma",
                            "speed",
                            "vega_per_percentage_point",
                            "volga_per_squared_percentage_point",
                            "vanna_per_percentage_point",
                            "zomma_per_percentage_point",
                            "rho_per_percentage_point",
                            "dividend_rho_per_percentage_point",
                        ):
                            self.assertAlmostEqual(result.require(greek), 0, delta=1e-8)
                        for greek in ("theta", "charm", "color"):
                            self.assertEqual(result.status(greek), "undefined")
                        numerical = pricing.numerical_greeks(
                            engine, option, context, "delta"
                        )
                        self.assertAlmostEqual(
                            numerical.require("delta"), delta, delta=1e-10
                        )

    def test_terminal_kinks_and_unsupported_measures_remain_explicit(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        context = market.PricingContext(
            model_parameters=market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=end,
        )
        option = ins.CashOrNothingOption(
            option_type="call",
            strike=100,
            payout=10,
            effective_time=start,
            expiry_time=end,
        )
        engine = pricing.AnalyticDigitalEngine()
        result = engine.price_with_greeks(
            option, context, ["delta", "gamma", "vega_per_percentage_point"]
        )
        self.assertEqual(result.status("delta"), "undefined")
        self.assertEqual(result.status("gamma"), "undefined")
        self.assertEqual(result.require("vega_per_percentage_point"), 0)
        self.assertEqual(result.status("rho_per_percentage_point"), "not_requested")
        result = pricing.MonteCarloDigitalEngine(
            path_count=32, seed=42
        ).price_with_greeks(
            option,
            context.replace(spot_price=120),
            ["delta", "gamma", "vega_per_percentage_point"],
        )
        self.assertEqual(result.require("delta"), 0)
        self.assertEqual(result.require("gamma"), 0)
        self.assertEqual(result.status("vega_per_percentage_point"), "unsupported")
