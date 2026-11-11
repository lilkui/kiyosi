"""Terminal risk follows the payoff while preserving engine coverage."""

import math
import unittest
from datetime import datetime

from kiyosi import instruments as ins
from kiyosi import market, pricing
from kiyosi.pricing import numerical_greeks
from support.history import dated as dated_history
from support.history import history_fields
from support.terminal import assert_terminal_greeks


class ExpiryGreekTests(unittest.TestCase):
    def test_finite_difference_terminal_greeks_are_exact_for_bsm_and_local_volatility(
        self,
    ):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        models = (
            market.BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            market.LocalVolatilityParameters(
                risk_free_rate=0.05,
                dividend_yield=0.02,
                surface=market.LocalVolatilitySurface(
                    times=[start, end], spots=[80, 120], values=[[0.2, 0.2], [0.2, 0.2]]
                ),
            ),
        )
        engine = pricing.FiniteDifferenceVanillaEngine()
        for model in models:
            for kind in (ins.EuropeanOption, ins.AmericanOption):
                for direction, deltas in (
                    ("call", (0, None, 1)),
                    ("put", (-1, None, 0)),
                ):
                    option = kind(
                        option_type=direction,
                        strike=100,
                        effective_time=start,
                        expiry_time=end,
                    )
                    for spot, delta in zip(
                        (math.nextafter(100, 0), 100, math.nextafter(100, math.inf)),
                        deltas,
                        strict=True,
                    ):
                        with self.subTest(
                            model=type(model).__name__,
                            kind=kind.__name__,
                            direction=direction,
                            spot=spot,
                        ):
                            context = market.PricingContext(
                                model_parameters=model,
                                spot_price=spot,
                                valuation_time=end,
                            )
                            result = engine.price_with_greeks(
                                dated_history(option, context), context, "all"
                            )
                            assert_terminal_greeks(self, result, delta)
                            self.assertEqual(
                                engine.greek_method(option, context), "native"
                            )
                            self.assertEqual(
                                engine.active_greek_controls(option, context), []
                            )
                            if isinstance(model, market.LocalVolatilityParameters):
                                self.assertEqual(result.supported, ("delta", "gamma"))
                                self.assertEqual(result.status("vega"), "unsupported")
                            only_delta = engine.price_with_greeks(
                                dated_history(option, context), context, "delta"
                            )
                            self.assertEqual(
                                only_delta.status("gamma"), "not_requested"
                            )

    def test_resolved_barriers_have_zero_spot_greeks_at_inactive_strikes(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        for at, volatility in ((end, 0.2), (datetime(2025, 7, 1), 0)):
            context = market.PricingContext(
                model_parameters=market.BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=volatility
                ),
                spot_price=100,
                valuation_time=at,
            )
            common = {
                "option_type": "call",
                "strike": 100,
                "effective_time": start,
                "expiry_time": end,
                "touch_state": "touched",
            }
            for engine, option in (
                (
                    pricing.AnalyticBarrierEngine(),
                    ins.BarrierOption(
                        **history_fields(
                            **common, barrier_level=120, barrier_type="up_and_out"
                        )
                    ),
                ),
                (
                    pricing.AnalyticDoubleBarrierEngine(),
                    ins.DoubleBarrierOption(
                        **history_fields(
                            **common,
                            lower_barrier=80,
                            upper_barrier=120,
                            barrier_type="knock_out",
                        )
                    ),
                ),
                (
                    pricing.AnalyticBinaryBarrierEngine(),
                    ins.cash_binary_barrier_option(
                        **history_fields(
                            **common,
                            barrier_level=120,
                            barrier_type="up_and_out",
                            payout=10,
                        )
                    ),
                ),
            ):
                with self.subTest(engine=type(engine).__name__, at=at):
                    result = engine.price_with_greeks(
                        dated_history(option, context), context, ["delta", "gamma"]
                    )
                    self.assertEqual(result.price, 0)
                    for greek in ("delta", "gamma"):
                        self.assertEqual(result.require(greek), 0)
                        self.assertEqual(result.status(greek), "available")

    def test_analytic_terminal_greeks_are_exact_next_to_the_strike(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        engine = pricing.AnalyticEuropeanEngine()
        for volatility in (0, 0.2):
            for direction, deltas in (("call", (0, None, 1)), ("put", (-1, None, 0))):
                option = ins.EuropeanOption(
                    option_type=direction,
                    strike=100,
                    effective_time=start,
                    expiry_time=end,
                )
                for spot, delta in zip(
                    (math.nextafter(100, 0), 100, math.nextafter(100, math.inf)),
                    deltas,
                    strict=True,
                ):
                    with self.subTest(
                        volatility=volatility, direction=direction, spot=spot
                    ):
                        context = market.PricingContext(
                            model_parameters=market.BlackScholesMertonParameters(
                                risk_free_rate=0.05,
                                dividend_yield=0.02,
                                volatility=volatility,
                            ),
                            spot_price=spot,
                            valuation_time=end,
                        )
                        result = engine.price_with_greeks(
                            dated_history(option, context), context, "all"
                        )
                        assert_terminal_greeks(self, result, delta)
                        for greek in result.requested:
                            expected = (
                                "undefined"
                                if (
                                    greek in ("theta", "charm", "color")
                                    or (
                                        delta is None
                                        and greek
                                        in ("delta", "gamma", "speed", "vanna", "zomma")
                                    )
                                )
                                else "available"
                            )
                            self.assertEqual(result.status(greek), expected)
                        self.assertEqual(engine.greek_method(option, context), "native")
                        self.assertEqual(
                            engine.active_greek_controls(option, context), []
                        )

    def test_automatic_spot_bumps_fit_small_price_units(self):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        model = market.BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        engines = (
            pricing.AnalyticEuropeanEngine(),
            pricing.CoxRossRubinsteinVanillaEngine(),
            pricing.FiniteDifferenceVanillaEngine(),
            pricing.QuadratureVanillaEngine(),
            pricing.MonteCarloVanillaEngine(path_count=32, seed=42),
        )
        for spot in (0.005, 0.5, 50):
            context = market.PricingContext(
                model_parameters=model, spot_price=spot, valuation_time=end
            )
            option = ins.EuropeanOption(
                option_type="call",
                strike=0.2 * spot,
                effective_time=start,
                expiry_time=end,
            )
            for engine in engines:
                with self.subTest(spot=spot, engine=type(engine).__name__):
                    result = engine.price_with_greeks(
                        dated_history(option, context), context, ["delta", "gamma"]
                    )
                    self.assertAlmostEqual(result.require("delta"), 1, delta=1e-10)
                    self.assertAlmostEqual(
                        result.require("gamma") * spot, 0, delta=1e-8
                    )
            explicit = numerical_greeks(
                engines[0],
                dated_history(option, context),
                context,
                "delta",
                spot_shift=2 * spot,
            )
            self.assertEqual(explicit.status("delta"), "stencil_unavailable")
            active = context.replace(valuation_time=start)
            atm = option.replace(strike=spot)
            exact = engines[0].price_with_greeks(
                dated_history(atm, active), active, ["delta", "gamma"]
            )
            numerical = numerical_greeks(
                engines[0], dated_history(atm, active), active, ["delta", "gamma"]
            )
            self.assertAlmostEqual(
                numerical.require("delta"), exact.require("delta"), delta=1e-4
            )
            self.assertAlmostEqual(
                (numerical.require("gamma") - exact.require("gamma")) * spot,
                0,
                delta=1e-3,
            )

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
            **history_fields(
                **terms,
                barrier_type="up_and_out",
                barrier_level=120,
                touch_state="untouched",
            )
        )
        cases = [
            (pricing.CoxRossRubinsteinVanillaEngine(), option, 1),
            (pricing.CoxRossRubinsteinDigitalEngine(), digital, 0),
            (pricing.CoxRossRubinsteinBarrierEngine(), barrier, 1),
            (pricing.MonteCarloDigitalEngine(path_count=32, seed=42), digital, 0),
        ]
        for kind in (ins.GeometricAveragePriceOption, ins.ArithmeticAveragePriceOption):
            cases.append(
                (
                    pricing.MonteCarloAsianEngine(path_count=32, seed=42),
                    kind(**history_fields(**terms, averaging_start_time=end)),
                    1,
                )
            )
        for engine, instrument, expected in cases:
            with self.subTest(
                engine=type(engine).__name__, instrument=type(instrument).__name__
            ):
                self.assertIsNone(
                    engine.price_with_greeks(
                        dated_history(instrument, context), context, "delta"
                    ).delta
                )
                result = numerical_greeks(
                    engine,
                    dated_history(instrument, context),
                    context,
                    "delta",
                    spot_shift=0.001,
                )
                self.assertAlmostEqual(result.require("delta"), expected, delta=1e-10)

    def test_terminal_autocallable_stencils_preserve_knock_in_and_redemption_regimes(
        self,
    ):
        start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
        note = ins.SnowballOption(
            calendar=market.weekdays_calendar(),
            observation_clock_time=None,
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
            history_time=start,
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
                    dated_history(note, context), context, "all"
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
                        path_count=32, seed=42, max_active_workers=1
                    ),
                ):
                    with self.subTest(
                        kind=kind.__name__,
                        direction=direction,
                        engine=type(engine).__name__,
                    ):
                        result = engine.price_with_greeks(
                            dated_history(option, context), context, "all"
                        )
                        self.assertAlmostEqual(
                            result.require("delta"), delta, delta=1e-10
                        )
                        for greek in (
                            "gamma",
                            "speed",
                            "vega",
                            "volga",
                            "vanna",
                            "zomma",
                            "rho",
                            "dividend_rho",
                        ):
                            self.assertAlmostEqual(result.require(greek), 0, delta=1e-8)
                        for greek in ("theta", "charm", "color"):
                            self.assertEqual(result.status(greek), "undefined")
                        numerical = pricing.numerical_greeks(
                            engine, dated_history(option, context), context, "delta"
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
            dated_history(option, context), context, ["delta", "gamma", "vega"]
        )
        self.assertEqual(result.status("delta"), "undefined")
        self.assertEqual(result.status("gamma"), "undefined")
        self.assertEqual(result.require("vega"), 0)
        self.assertEqual(result.status("rho"), "not_requested")
        result = pricing.MonteCarloDigitalEngine(
            path_count=32, seed=42
        ).price_with_greeks(
            dated_history(option, context.replace(spot_price=120)),
            context.replace(spot_price=120),
            ["delta", "gamma", "vega"],
        )
        self.assertEqual(result.require("delta"), 0)
        self.assertEqual(result.require("gamma"), 0)
        self.assertEqual(result.status("vega"), "unsupported")
