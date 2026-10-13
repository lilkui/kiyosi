import math
import os
import unittest
from datetime import date
from unittest.mock import patch

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    BarrierOption,
    BinarySnowballOption,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    asset_no_touch_down,
    asset_no_touch_up,
    asset_one_touch_down,
    asset_one_touch_up,
    autocallable_observations,
    cash_no_touch_down,
    cash_no_touch_up,
    cash_one_touch_down,
    cash_one_touch_up,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
    fixed_interval_schedule,
)


class AuditFixTests(unittest.TestCase):
    def test_schedule_iteration_is_lazy_and_retains_its_owner(self):
        schedule = fixed_interval_schedule(
            start=date(2025, 1, 1),
            end=date(2025, 1, 5),
            interval_days=1,
            calendar=all_days_calendar(),
        )
        expected = schedule.dates
        with patch("builtins.__import__", wraps=__import__) as converted:
            iterator = iter(schedule)
            independent = iter(schedule)
            self.assertIs(iter(iterator), iterator)
            self.assertEqual(converted.call_count, 0)
            self.assertEqual(next(iterator), expected[0])
            self.assertEqual(converted.call_count, 1)
            self.assertEqual(next(iterator), expected[1])
            self.assertEqual(next(independent), expected[0])
            self.assertEqual(converted.call_count, 3)
        del schedule
        self.assertEqual(list(iterator), expected[2:])
        self.assertEqual(list(independent), expected[1:])
        for _ in range(2):
            with self.assertRaises(StopIteration):
                next(iterator)

    def test_knocked_in_finite_difference_autocallables(self):
        start, valuation, middle, end = (
            date(2025, 1, 1),
            date(2025, 1, 2),
            date(2025, 7, 1),
            date(2026, 1, 1),
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=70,
            valuation_time=valuation,
        )

        def put_price(strike):
            return pricing.AnalyticVanillaEngine().price(
                EuropeanOption(
                    option_type="put",
                    strike=strike,
                    effective_date=start,
                    expiry_date=end,
                ),
                context,
            )

        def discount(value):
            return math.exp(-0.04 * (value - valuation).days / 365)

        downside = discount(end) - (put_price(100) - put_price(60)) / 100
        coupons = 0.08 * (
            (valuation - start).days / 365
            + (middle - valuation).days / 365 * discount(middle)
            + (end - middle).days / 365 * discount(end)
        )
        for state in ("none", "knocked_in"):
            observation_dates = [valuation, middle, end]
            knock_out_levels = [200, 200, 200]
            terms = {
                "knock_in_level": 75,
                "knock_in_observation_mode": "every_trading_day",
                "barrier_state": state,
                "effective_date": start,
                "expiry_date": end,
            }
            downside_terms = {
                "initial_spot": 100,
                "upper_strike": 100,
                "lower_strike": 60,
            }
            snowball = SnowballOption(
                **terms,
                **downside_terms,
                maturity_coupon_rate=0.8,
                observations=autocallable_observations(
                    observation_dates,
                    knock_out_levels,
                    knock_out_coupon_rates=[0.1, 0.1, 0.1],
                ),
            )
            phoenix = PhoenixOption(
                **terms,
                **downside_terms,
                coupon_rate=0.08,
                observations=autocallable_observations(
                    observation_dates, knock_out_levels, coupon_barrier_levels=[0, 0, 0]
                ),
            )
            ternary = TernarySnowballOption(
                **terms,
                maturity_coupon_rate=0.8,
                minimum_coupon_rate=0.02,
                observations=autocallable_observations(
                    observation_dates,
                    knock_out_levels,
                    knock_out_coupon_rates=[0.1, 0.1, 0.1],
                ),
            )
            for scheme in ("crank_nicolson", "implicit_euler"):
                for note, engine_type, expected in (
                    (snowball, pricing.FiniteDifferenceAutocallableEngine, downside),
                    (
                        phoenix,
                        pricing.FiniteDifferenceAutocallableEngine,
                        downside + coupons,
                    ),
                    (
                        ternary,
                        pricing.FiniteDifferenceAutocallableEngine,
                        1.02 * discount(end),
                    ),
                ):
                    engine = engine_type(
                        asset_step_count=800,
                        time_step_count=400,
                        asset_upper_boundary=800,
                        scheme=scheme,
                    )
                    with self.subTest(state=state, scheme=scheme, note=type(note)):
                        result = engine.price_with_greeks(
                            note, context, ["delta", "gamma"]
                        )
                        self.assertAlmostEqual(result.price, expected, delta=0.0002)
                        self.assertEqual(result.price, engine.price(note, context))
                        self.assertIsNotNone(result.delta)
                        self.assertIsNotNone(result.gamma)

    def test_finite_difference_barrier_native_spot_greeks(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        )
        engine = pricing.FiniteDifferenceBarrierEngine(
            asset_step_count=800, time_step_count=400
        )
        for kind in ("down_and_in", "down_and_out", "up_and_in", "up_and_out"):
            up = kind.startswith("up")
            barrier = 120 if up else 80
            option = BarrierOption(
                option_type="call",
                strike=100,
                effective_date=start,
                expiry_date=end,
                barrier_level=barrier,
                barrier_type=kind,
            )
            context = PricingContext(
                model_parameters=parameters, spot_price=100, valuation_time=start
            )
            result = engine.price_with_greeks(option, context, ["delta", "gamma"])
            expected = pricing.AnalyticBarrierEngine().price_with_greeks(
                option, context, ["delta", "gamma"]
            )
            with self.subTest(kind=kind):
                self.assertEqual(result.price, engine.price(option, context))
                self.assertAlmostEqual(result.delta, expected.delta, delta=0.003)
                self.assertAlmostEqual(result.gamma, expected.gamma, delta=0.0005)
                self.assertIsNone(result.vega)
            for spot in (barrier, barrier + (-0.001 if up else 0.001)):
                context = PricingContext(
                    model_parameters=parameters, spot_price=spot, valuation_time=start
                )
                result = engine.price_with_greeks(option, context, ["delta", "gamma"])
                with self.subTest(kind=kind, spot=spot):
                    self.assertEqual(result.price, engine.price(option, context))
                    self.assertIsNone(result.delta)
                    self.assertIsNone(result.gamma)

    def test_implied_volatility_interpolates_expensive_price_curves(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for engine in (
            pricing.AnalyticVanillaEngine(),
            pricing.FiniteDifferenceVanillaEngine(),
            pricing.MonteCarloVanillaEngine(path_count=5000, step_count=10, seed=42),
        ):
            with self.subTest(engine=type(engine).__name__):
                quote = engine.price(option, context)
                solved = pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    quote,
                    parameter_tolerance=1e-12,
                    max_iterations=12,
                )
                self.assertAlmostEqual(solved, 0.2, delta=1e-8)
                shifted = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=solved
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                self.assertAlmostEqual(engine.price(option, shifted), quote, delta=1e-8)

    def test_builtin_implied_coupons_solve_affine_curve_directly(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0, volatility=0.3
            ),
            spot_price=100,
            valuation_time=start,
        )
        for engine in (
            pricing.MonteCarloAutocallableEngine(path_count=512, seed=42),
            pricing.FiniteDifferenceAutocallableEngine(),
        ):
            for coupon in (0.073, -0.17):
                note = BinarySnowballOption(
                    maturity_coupon_rate=coupon + 0.02,
                    effective_date=start,
                    expiry_date=end,
                    observations=autocallable_observations(
                        [date(2025, 7, 1), end],
                        [110, 110],
                        knock_out_coupon_rates=[coupon, coupon + 0.01],
                    ),
                )
                quote = engine.price(note, context)
                for convention in ("shift_maturity_coupon", "preserve_maturity_coupon"):
                    with self.subTest(
                        engine=type(engine).__name__,
                        coupon=coupon,
                        convention=convention,
                    ):
                        solved = pricing.implied_coupon(
                            engine,
                            note,
                            context,
                            quote,
                            quote_convention=convention,
                            lower_bound=-1,
                            upper_bound=2,
                            price_tolerance=1e-10,
                            parameter_tolerance=1e-12,
                            max_iterations=1,
                        )
                        self.assertAlmostEqual(solved, coupon, delta=1e-10)

    def test_monte_carlo_coupon_solve_preserves_monitoring_and_current_cashflows(self):
        start, fixing, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)
        backends = ["cpu"] + (
            ["cuda"] if os.environ.get("KIYOSI_TEST_CUDA") == "enabled" else []
        )
        for mode in ("at_expiry", "every_trading_day"):
            observation_dates = [fixing, end]
            knock_out_levels = [110, 110]
            common = {
                "effective_date": start,
                "expiry_date": end,
                "barrier_state": "none",
            }
            knock_in = {"knock_in_level": 80, "knock_in_observation_mode": mode}
            downside = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 100}
            coupons = {
                "maturity_coupon_rate": 0.073,
                "observations": autocallable_observations(
                    observation_dates,
                    knock_out_levels,
                    knock_out_coupon_rates=[0.073, 0.083],
                ),
            }
            cases = (
                (
                    PhoenixOption(
                        **common,
                        **knock_in,
                        **downside,
                        coupon_rate=0.073,
                        observations=autocallable_observations(
                            observation_dates,
                            knock_out_levels,
                            coupon_barrier_levels=[90, 90],
                        ),
                    ),
                    pricing.MonteCarloAutocallableEngine,
                ),
                (
                    SnowballOption(**common, **knock_in, **downside, **coupons),
                    pricing.MonteCarloAutocallableEngine,
                ),
                (
                    BinarySnowballOption(**common, **coupons),
                    pricing.MonteCarloAutocallableEngine,
                ),
                (
                    TernarySnowballOption(
                        **common, **knock_in, **coupons, minimum_coupon_rate=-2
                    ),
                    pricing.MonteCarloAutocallableEngine,
                ),
            )
            for note, engine_type in cases:
                for backend in backends:
                    engine = engine_type(path_count=512, seed=42, backend=backend)
                    for valuation in (start, fixing, end):
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0.03, dividend_yield=0, volatility=0.3
                            ),
                            spot_price=100,
                            valuation_time=valuation,
                        )
                        quote = engine.price(note, context)
                        conventions = (
                            (None,)
                            if isinstance(note, PhoenixOption)
                            else ("shift_maturity_coupon", "preserve_maturity_coupon")
                        )
                        for convention in conventions:
                            kwargs = (
                                {}
                                if convention is None
                                else {"quote_convention": convention}
                            )
                            with self.subTest(
                                note=type(note).__name__,
                                backend=backend,
                                valuation=valuation,
                                mode=mode,
                                convention=convention,
                            ):
                                if (
                                    valuation == end
                                    and convention == "preserve_maturity_coupon"
                                ):
                                    with self.assertRaises(kiyosi.KiyosiError) as error:
                                        pricing.implied_coupon(
                                            engine,
                                            note,
                                            context,
                                            quote,
                                            lower_bound=-1,
                                            upper_bound=2,
                                            **kwargs,
                                        )
                                    self.assertEqual(
                                        error.exception.category,
                                        kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                                    )
                                    continue
                                solved = pricing.implied_coupon(
                                    engine,
                                    note,
                                    context,
                                    quote,
                                    lower_bound=-1,
                                    upper_bound=2,
                                    price_tolerance=1e-10,
                                    max_iterations=1,
                                    **kwargs,
                                )
                                self.assertAlmostEqual(solved, 0.073, delta=1e-10)

    def test_finite_difference_implied_coupons_retain_nonlinear_interpolation(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0, volatility=0.3
            ),
            spot_price=71.46981808356617,
            valuation_time=start,
        )
        observation_dates = [date(2025, 7, 1), end]
        knock_out_levels = [93.43788934756687, 93.43788934756687]
        terms = {
            "initial_spot": 100,
            "knock_in_level": 72.54516454112279,
            "upper_strike": 100,
            "lower_strike": 0,
            "knock_in_observation_mode": "at_expiry",
            "effective_date": start,
            "expiry_date": end,
        }
        note = SnowballOption(
            **terms,
            maturity_coupon_rate=0.073,
            observations=autocallable_observations(
                observation_dates,
                knock_out_levels,
                knock_out_coupon_rates=[0.073, 0.073],
            ),
        )
        engine = pricing.FiniteDifferenceAutocallableEngine(
            asset_step_count=5, time_step_count=10
        )
        quote = engine.price(note, context)
        with self.assertRaises(kiyosi.KiyosiError) as error:
            pricing.implied_coupon(
                engine,
                note,
                context,
                quote,
                quote_convention="shift_maturity_coupon",
                lower_bound=-1,
                upper_bound=2,
                price_tolerance=1e-10,
                parameter_tolerance=1e-12,
                max_iterations=1,
            )
        self.assertEqual(
            error.exception.category, kiyosi.ErrorCategory.SOLVER_NON_CONVERGENCE
        )
        solved = pricing.implied_coupon(
            engine,
            note,
            context,
            quote,
            quote_convention="shift_maturity_coupon",
            lower_bound=-1,
            upper_bound=2,
            price_tolerance=1e-10,
            parameter_tolerance=1e-12,
        )
        self.assertAlmostEqual(solved, 0.073, delta=1e-9)
        repriced = SnowballOption(
            **terms,
            maturity_coupon_rate=solved,
            observations=autocallable_observations(
                observation_dates,
                knock_out_levels,
                knock_out_coupon_rates=[solved, solved],
            ),
        )
        self.assertAlmostEqual(engine.price(repriced, context), quote, delta=1e-10)

    def test_touch_prices_survive_volatility_variance_underflow(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticBinaryBarrierEngine()
        for up in (True, False):
            barrier = 101 if up else 99
            terms = {
                "effective_date": start,
                "expiry_date": end,
                "barrier_level": barrier,
            }
            for sigma in (1e-200, math.ulp(0.0)):
                for crossing in (False, True):
                    dividend = (0 if up else 0.1) if crossing else 0.05
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.05,
                            dividend_yield=dividend,
                            volatility=sigma,
                        ),
                        spot_price=100,
                        valuation_time=start,
                    )
                    hit_time = (
                        math.log1p((barrier - 100) / 100) / (0.05 - dividend)
                        if crossing
                        else 0
                    )
                    for timing in ("at_hit", "at_expiry"):
                        with self.subTest(
                            up=up, sigma=sigma, crossing=crossing, timing=timing
                        ):
                            cash = (cash_one_touch_up if up else cash_one_touch_down)(
                                **terms, payout=1, settlement_timing=timing
                            )
                            asset = (
                                asset_one_touch_up if up else asset_one_touch_down
                            )(**terms, settlement_timing=timing)
                            discount = math.exp(
                                -0.05 * (hit_time if timing == "at_hit" else 1)
                            )
                            self.assertAlmostEqual(
                                engine.price(cash, context),
                                discount if crossing else 0,
                                delta=1e-12,
                            )
                            expected = (
                                (
                                    barrier * discount
                                    if timing == "at_hit"
                                    else 100 * math.exp(-dividend)
                                )
                                if crossing
                                else 0
                            )
                            self.assertAlmostEqual(
                                engine.price(asset, context), expected, delta=1e-10
                            )
                    cash = (cash_no_touch_up if up else cash_no_touch_down)(
                        **terms, payout=1
                    )
                    asset = (asset_no_touch_up if up else asset_no_touch_down)(**terms)
                    self.assertAlmostEqual(
                        engine.price(cash, context),
                        0 if crossing else math.exp(-0.05),
                        delta=1e-12,
                    )
                    self.assertAlmostEqual(
                        engine.price(asset, context),
                        0 if crossing else 100 * math.exp(-dividend),
                        delta=1e-10,
                    )
            drift = math.log1p((barrier - 100) / 100)
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=-drift, volatility=1e-200
                ),
                spot_price=100,
                valuation_time=start,
            )
            boundary = (cash_one_touch_up if up else cash_one_touch_down)(
                **terms, payout=1, settlement_timing="at_expiry"
            )
            self.assertEqual(engine.price(boundary, context), 0.5)
