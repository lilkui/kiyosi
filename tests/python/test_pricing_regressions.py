import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    Accumulator,
    AmericanOption,
    AssetOrNothingOption,
    BarrierOption,
    BinarySnowballOption,
    CashOrNothingOption,
    EuropeanOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    asset_binary_barrier_option,
    cash_binary_barrier_option,
    cash_one_touch_down,
    cash_one_touch_up,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)


class PricingRegressionTests(unittest.TestCase):
    def test_analytic_delta_and_rho_selections_retain_full_valuation_results(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticVanillaEngine()
        names = (
            "delta",
            "gamma",
            "speed",
            "theta",
            "charm",
            "color",
            "vega",
            "vanna",
            "zomma",
            "rho",
        )
        for direction in ("call", "put"):
            option = EuropeanOption(
                option_type=direction, strike=100, effective_date=start, expiry_date=end
            )
            for volatility in (0.2, 1e-20, 1e-200, 1e308):
                for spot in (0.01, 100, 1e100):
                    for valuation in (start, datetime(2025, 1, 1, 12, tzinfo=UTC), end):
                        with self.subTest(
                            direction=direction,
                            volatility=volatility,
                            spot=spot,
                            valuation=valuation,
                        ):
                            context = PricingContext(
                                model_parameters=BlackScholesMertonParameters(
                                    risk_free_rate=0.05,
                                    dividend_yield=0.02,
                                    volatility=volatility,
                                ),
                                spot_price=spot,
                                valuation_time=valuation,
                            )
                            full = engine.price_with_greeks(
                                option, context, all_greeks=True
                            )
                            scalar = engine.price(option, context)
                            for request in (["delta"], ["rho"], ["delta", "rho"]):
                                selected = engine.price_with_greeks(
                                    option, context, request
                                )
                                self.assertEqual(selected.price, scalar)
                                self.assertEqual(selected.price, full.price)
                                for name in names:
                                    self.assertEqual(
                                        getattr(selected, name),
                                        getattr(full, name)
                                        if name in request
                                        else None,
                                    )

    def test_shared_analytic_engine_prices_are_consistent_across_threads(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.AnalyticVanillaEngine()
        options = [
            EuropeanOption(
                option_type=direction, strike=100, effective_date=start, expiry_date=end
            )
            for direction in ("call", "put")
        ]
        with ThreadPoolExecutor(max_workers=4) as executor:
            tasks = [
                executor.submit(engine.price, option, context)
                for option in options * 50
            ]
            for index, task in enumerate(tasks):
                self.assertAlmostEqual(
                    task.result(),
                    (9.227005508154036, 6.330080627549918)[index % 2],
                    places=12,
                )

    def test_continuous_up_barriers_preserve_put_boundaries_and_rebate_timing(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04,
                dividend_yield=0.01,
                volatility=0.2,
            ),
            spot_price=100.25,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        # References before skipping the discarded vanilla upper-boundary formula.
        cases = (
            (
                "crank_nicolson",
                "at_hit",
                6.001295273660669,
                -0.46100327414769354,
                0.012094195366788239,
                -0.000720165080800371,
            ),
            (
                "crank_nicolson",
                "at_expiry",
                5.965452933440826,
                -0.46428370491099064,
                0.01188517039188602,
                -0.0007257293326437646,
            ),
            (
                "implicit_euler",
                "at_hit",
                6.000291212030822,
                -0.46094055012576535,
                0.012105234989577161,
                -0.0007214011673570921,
            ),
            (
                "implicit_euler",
                "at_expiry",
                5.964442597640509,
                -0.46422044616223535,
                0.011896243851421412,
                -0.0007269705471180208,
            ),
            (
                "explicit_euler",
                "at_hit",
                6.002297864958332,
                -0.4610659998773655,
                0.012083194022974213,
                -0.0007189321115608181,
            ),
            (
                "explicit_euler",
                "at_expiry",
                5.966461797161821,
                -0.46434696546546267,
                0.011874135281227763,
                -0.0007244912412884592,
            ),
        )
        for scheme, timing, price, delta, gamma, speed in cases:
            with self.subTest(scheme=scheme, timing=timing):
                option = BarrierOption(
                    option_type="put",
                    strike=100,
                    effective_date=start,
                    expiry_date=end,
                    barrier_level=110,
                    barrier_type="up_and_out",
                    rebate=2,
                    rebate_timing=timing,
                )
                engine = pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=80,
                    time_step_count=1000,
                    scheme=scheme,
                    asset_upper_boundary=400,
                )
                result = engine.price_with_greeks(
                    option, context, ["delta", "gamma", "speed"]
                )
                for name, expected in zip(
                    ("price", "delta", "gamma", "speed"), (price, delta, gamma, speed)
                ):
                    self.assertIsNotNone(getattr(result, name))
                    self.assertAlmostEqual(getattr(result, name), expected, delta=1e-11)
                self.assertEqual(result.price, engine.price(option, context))

    def test_accumulator_mc_numerical_and_selected_greeks_match_the_engine_api(self):
        start, end = date(2025, 1, 1), date(2025, 1, 13)
        option = Accumulator(
            strike=100,
            knock_out_level=115,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=3,
            effective_date=start,
            expiry_date=end,
        )
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04,
            dividend_yield=0.01,
            volatility=0.2,
        )
        names = (
            "delta",
            "gamma",
            "speed",
            "theta",
            "charm",
            "color",
            "vega",
            "vanna",
            "zomma",
            "rho",
        )
        for seed in (42, 2**64 - 1):
            for paths in (1, 31, 32):
                engine = pricing.MonteCarloAccumulatorEngine(
                    path_count=paths, seed=seed
                )
                for spot in (95, 100, 114.9, 116):
                    for valuation in (start, datetime(2025, 1, 1, 12, tzinfo=UTC), end):
                        with self.subTest(
                            seed=seed, paths=paths, spot=spot, valuation=valuation
                        ):
                            context = PricingContext(
                                model_parameters=parameters,
                                spot_price=spot,
                                valuation_time=valuation,
                                calendar=all_days_calendar(),
                            )
                            expected = pricing.calculate_numerical_greeks(
                                engine, option, context, spot_shift=0.2
                            )
                            for request in (names, ["delta", "gamma"], ["speed"]):
                                actual = engine.price_with_greeks(
                                    option, context, request, spot_shift=0.2
                                )
                                self.assertEqual(actual.price, expected.price)
                                for name in names:
                                    self.assertEqual(
                                        getattr(actual, name),
                                        getattr(expected, name)
                                        if name in request
                                        else None,
                                    )

    def test_bjerksund_quadrature_preserves_continuation_across_maturities(self):
        start = date(2025, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05,
                dividend_yield=0.03,
                volatility=0.2,
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.BjerksundStenslandVanillaEngine()
        for days, call, put in (
            (1, 0.4203293406666333, 0.41497263683106667),
            (7, 1.1233367180705613, 1.0862318403299156),
            (30, 2.362502879143129, 2.205771411369753),
            (91, 4.1945462483931095, 3.7306674960129627),
            (180, 5.986257668860937, 5.094433820244056),
            (365, 8.652695436032069, 6.932464292493037),
            (1000, 14.425584357882556, 10.310794114806157),
            (3650, 25.33817727859211, 15.052781075279043),
        ):
            for kind, expected in (("call", call), ("put", put)):
                with self.subTest(days=days, kind=kind):
                    option = AmericanOption(
                        option_type=kind,
                        strike=100,
                        effective_date=start,
                        expiry_date=start + timedelta(days=days),
                    )
                    self.assertAlmostEqual(
                        engine.price(option, context), expected, delta=1e-11
                    )

    def test_product_fd_speed_preserves_shifted_gamma_and_stencil_errors(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        )
        valuation = datetime(2025, 1, 1, 12, tzinfo=UTC)
        calendar = all_days_calendar()
        context = PricingContext(
            model_parameters=parameters,
            spot_price=100.25,
            valuation_time=valuation,
            calendar=calendar,
        )
        life = {"effective_date": start, "expiry_date": end}
        ladder = {"knock_out_levels": [110], "observation_dates": [end], **life}
        coupons = {"knock_out_coupon_rates": [0.08], "maturity_coupon_rate": 0.06}
        knock_in = {
            "knock_in_level": 75,
            "knock_in_observation_mode": "every_trading_day",
            "barrier_state": "none",
        }
        strikes = {"initial_spot": 100, "upper_strike": 100, "lower_strike": 60}
        cases = (
            (
                CashOrNothingOption(option_type="call", strike=100, payout=10, **life),
                pricing.FiniteDifferenceDigitalEngine,
                100.25,
            ),
            (
                AssetOrNothingOption(option_type="put", strike=100, **life),
                pricing.FiniteDifferenceDigitalEngine,
                100.25,
            ),
            (
                BarrierOption(
                    option_type="call",
                    strike=100,
                    barrier_level=80,
                    barrier_type="down_and_in",
                    rebate=2,
                    observation_mode="scheduled",
                    observation_dates=[end],
                    **life,
                ),
                pricing.FiniteDifferenceBarrierEngine,
                100.25,
            ),
            (
                Accumulator(
                    strike=100,
                    knock_out_level=115,
                    daily_quantity=1,
                    acceleration_factor=2,
                    accumulated_quantity=3,
                    **life,
                ),
                pricing.FiniteDifferenceAccumulatorEngine,
                115,
            ),
            (
                SnowballOption(**coupons, **strikes, **knock_in, **ladder),
                pricing.FiniteDifferenceSnowballEngine,
                110,
            ),
            (
                BinarySnowballOption(**coupons, **ladder),
                pricing.FiniteDifferenceBinarySnowballEngine,
                110,
            ),
            (
                TernarySnowballOption(
                    minimum_coupon_rate=0, **coupons, **knock_in, **ladder
                ),
                pricing.FiniteDifferenceTernarySnowballEngine,
                110,
            ),
            (
                PhoenixOption(
                    coupon_rate=0.08,
                    coupon_barrier_levels=[90],
                    **strikes,
                    **knock_in,
                    **ladder,
                ),
                pricing.FiniteDifferencePhoenixEngine,
                110,
            ),
        )
        for option, engine_class, relevant in cases:
            for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
                for upper in (None, 400):
                    settings = {
                        "asset_step_count": 40,
                        "time_step_count": 400,
                        "scheme": scheme,
                    }
                    engine = engine_class(asset_upper_boundary=upper, **settings)
                    fixed_engine = engine_class(
                        asset_upper_boundary=upper or 4 * relevant, **settings
                    )
                    for shift in (0.01, 0.25):
                        with self.subTest(
                            product=type(option).__name__,
                            scheme=scheme,
                            upper=upper,
                            shift=shift,
                        ):
                            h = (context.spot_price + shift) - context.spot_price
                            gammas = [
                                fixed_engine.price_with_greeks(
                                    option,
                                    PricingContext(
                                        model_parameters=parameters,
                                        spot_price=context.spot_price + bump,
                                        valuation_time=valuation,
                                        calendar=calendar,
                                    ),
                                    ["gamma"],
                                ).gamma
                                for bump in (h, -h)
                            ]
                            result = engine.price_with_greeks(
                                option, context, ["speed"], spot_shift=shift
                            )
                            self.assertEqual(
                                result.speed, (gammas[0] - gammas[1]) / (2 * h)
                            )
                            self.assertEqual(
                                result.price, engine.price(option, context)
                            )
                            self.assertIsNone(result.delta)
                            self.assertIsNone(result.gamma)
            engine = engine_class()
            for shift in (1e-300, 200):
                self.assertIsNone(
                    engine.price_with_greeks(
                        option, context, ["speed"], spot_shift=shift
                    ).speed
                )
            expiry = PricingContext(
                model_parameters=parameters,
                spot_price=context.spot_price,
                valuation_time=end,
                calendar=calendar,
            )
            self.assertIsNone(engine.price_with_greeks(option, expiry, ["speed"]).speed)
            narrow = engine_class(
                asset_step_count=40,
                time_step_count=400,
                scheme="implicit_euler",
                asset_upper_boundary=relevant + 0.5,
            )
            with self.assertRaises(kiyosi.KiyosiError) as clipped:
                narrow.price_with_greeks(
                    option,
                    context,
                    ["speed"],
                    spot_shift=relevant + 0.5 - context.spot_price,
                )
            self.assertEqual(
                clipped.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )

    def test_fd_speed_validates_bumped_digital_prices_and_event_boundaries(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        context = PricingContext(
            model_parameters=parameters, spot_price=99.75, valuation_time=start
        )
        option = CashOrNothingOption(
            option_type="call",
            strike=100.5,
            payout=100,
            effective_date=start,
            expiry_date=end,
        )
        engine = pricing.FiniteDifferenceDigitalEngine(
            asset_step_count=10000, time_step_count=1, asset_upper_boundary=400
        )
        self.assertTrue(math.isfinite(engine.price(option, context)))
        with self.assertRaises(kiyosi.KiyosiError) as rejected:
            engine.price_with_greeks(option, context, ["speed"], spot_shift=0.25)
        self.assertEqual(
            rejected.exception.category, kiyosi.ErrorCategory.INVALID_RESULT
        )
        accumulator = Accumulator(
            strike=100,
            knock_out_level=115,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=0,
            effective_date=start,
            expiry_date=end,
        )
        context = PricingContext(
            model_parameters=parameters, spot_price=100, valuation_time=start
        )
        self.assertIsNone(
            pricing.FiniteDifferenceAccumulatorEngine()
            .price_with_greeks(accumulator, context, ["speed"])
            .speed
        )

    def test_scheduled_fd_knock_in_layers_preserve_rebate_parity(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
            settings = {
                "asset_step_count": 80,
                "time_step_count": 400,
                "scheme": scheme,
                "asset_upper_boundary": 400,
            }
            engine = pricing.FiniteDifferenceBarrierEngine(**settings)
            for option_type in ("call", "put"):
                terms = {
                    "option_type": option_type,
                    "strike": 100,
                    "effective_date": start,
                    "expiry_date": end,
                }
                vanilla = pricing.FiniteDifferenceVanillaEngine(
                    **settings
                ).price_with_greeks(
                    EuropeanOption(**terms), context, ["delta", "gamma"]
                )
                for direction, level in (("down", 80), ("up", 120)):
                    for rebate in (0, 2):
                        for observes_expiry in (False, True):
                            with self.subTest(
                                scheme=scheme,
                                option_type=option_type,
                                direction=direction,
                                rebate=rebate,
                                observes_expiry=observes_expiry,
                            ):
                                dates = [
                                    start + timedelta(days=73),
                                    start + timedelta(days=219),
                                ]
                                if observes_expiry:
                                    dates.append(end)
                                barrier_terms = dict(
                                    **terms,
                                    barrier_level=level,
                                    rebate=rebate,
                                    observation_mode="scheduled",
                                    observation_dates=dates,
                                )
                                knock_in = BarrierOption(
                                    **barrier_terms, barrier_type=f"{direction}_and_in"
                                )
                                knock_out = BarrierOption(
                                    **barrier_terms, barrier_type=f"{direction}_and_out"
                                )
                                inside = engine.price_with_greeks(
                                    knock_in, context, ["delta", "gamma"]
                                )
                                outside = engine.price_with_greeks(
                                    knock_out, context, ["delta", "gamma"]
                                )
                                self.assertEqual(
                                    inside.price, engine.price(knock_in, context)
                                )
                                self.assertAlmostEqual(
                                    inside.price + outside.price,
                                    vanilla.price + rebate * math.exp(-0.04),
                                    delta=1e-10,
                                )
                                for greek in ("delta", "gamma"):
                                    self.assertAlmostEqual(
                                        getattr(inside, greek)
                                        + getattr(outside, greek),
                                        getattr(vanilla, greek),
                                        delta=1e-12,
                                    )

    def test_accumulator_daily_leap_year_grid_preserves_settlement_price(self):
        start, end = date(2024, 1, 1), date(2025, 1, 1)
        option = Accumulator(
            strike=100,
            knock_out_level=115,
            daily_quantity=1,
            acceleration_factor=2,
            accumulated_quantity=0,
            effective_date=start,
            expiry_date=end,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
            calendar=all_days_calendar(),
        )
        for scheme, nodes, expected in (
            ("crank_nicolson", 200, -2058.336718606278),
            ("implicit_euler", 200, -2048.4071966441415),
            ("explicit_euler", 40, -2253.4061717943714),
        ):
            with self.subTest(scheme=scheme):
                engine = pricing.FiniteDifferenceAccumulatorEngine(
                    asset_step_count=nodes, time_step_count=366, scheme=scheme
                )
                self.assertAlmostEqual(
                    engine.price(option, context), expected, delta=1e-8
                )

    def test_vanilla_fd_speed_reuses_fixed_grid_gamma_with_custom_shifts(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        context = PricingContext(
            model_parameters=parameters, spot_price=100.25, valuation_time=start
        )
        engine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=80, time_step_count=200, asset_upper_boundary=410
        )
        for option_class in (EuropeanOption, AmericanOption):
            option = option_class(
                option_type="put", strike=100, effective_date=start, expiry_date=end
            )
            for shift in (0.01, 0.25):
                with self.subTest(option=option_class.__name__, shift=shift):
                    h = (context.spot_price + shift) - context.spot_price
                    gammas = [
                        engine.price_with_greeks(
                            option,
                            PricingContext(
                                model_parameters=parameters,
                                spot_price=context.spot_price + bump,
                                valuation_time=start,
                            ),
                            ["gamma"],
                        ).gamma
                        for bump in (h, -h)
                    ]
                    result = engine.price_with_greeks(
                        option, context, ["speed"], spot_shift=shift
                    )
                    self.assertEqual(result.speed, (gammas[0] - gammas[1]) / (2 * h))
                    self.assertEqual(result.price, engine.price(option, context))
                    self.assertIsNone(result.gamma)
            unavailable = engine.price_with_greeks(
                option, context, ["speed"], spot_shift=1e-300
            )
            self.assertIsNone(unavailable.speed)
            narrow = pricing.FiniteDifferenceVanillaEngine(asset_upper_boundary=100.5)
            with self.assertRaises(kiyosi.KiyosiError) as clipped:
                narrow.price_with_greeks(option, context, ["speed"], spot_shift=0.25)
            self.assertEqual(
                clipped.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )

    def test_fd_interpolation_preserves_prices_at_and_around_grid_nodes(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        engine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=80, time_step_count=80, asset_upper_boundary=400
        )
        # Numerical regression values from the unchanged spline implementation.
        for spot, expected in (
            (99.99, 9.252556506551684),
            (math.nextafter(100, 0), 9.258487593783041),
            (100, 9.258487593783052),
            (math.nextafter(100, math.inf), 9.258487593783062),
            (100.01, 9.264420627312544),
        ):
            with self.subTest(spot=spot):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                    ),
                    spot_price=spot,
                    valuation_time=start,
                )
                self.assertAlmostEqual(
                    engine.price(option, context), expected, delta=1e-12
                )

    def test_accumulator_fixed_position_uses_exact_zero_carry_value(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engines = [
            pricing.MonteCarloAccumulatorEngine(path_count=32, seed=7),
            pricing.MonteCarloAccumulatorEngine(
                path_count=2000, seed=42, backend="cuda"
            ),
            pricing.FiniteDifferenceAccumulatorEngine(),
        ]
        for strike in (50, 105, 110):
            option = Accumulator(
                strike=strike,
                knock_out_level=105,
                daily_quantity=0 if strike == 50 else 1,
                acceleration_factor=0,
                accumulated_quantity=3,
                effective_date=start,
                expiry_date=end,
            )
            for hour in (0, 12):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0, dividend_yield=0, volatility=0.2
                    ),
                    spot_price=100,
                    valuation_time=datetime(2025, 1, 1, hour, tzinfo=UTC),
                )
                for engine in engines:
                    with self.subTest(
                        strike=strike, hour=hour, engine=type(engine).__name__
                    ):
                        self.assertEqual(
                            engine.price(option, context), 3 * (100 - strike)
                        )
                        greeks = engine.price_with_greeks(
                            option, context, ["delta", "gamma"]
                        )
                        self.assertAlmostEqual(greeks.delta, 3, delta=1e-9)
                        self.assertAlmostEqual(greeks.gamma, 0, delta=1e-8)

    def test_binomial_speed_tracks_analytic_sensitivity_with_default_shifts(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.CoxRossRubinsteinVanillaEngine(step_count=1000)
        for dividend in (0.0, 0.01):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=dividend, volatility=0.2
                ),
                spot_price=100,
                valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
            )
            for direction in ("call", "put"):
                terms = {
                    "option_type": direction,
                    "strike": 100,
                    "effective_date": start,
                    "expiry_date": end,
                }
                european = EuropeanOption(**terms)
                expected = pricing.AnalyticVanillaEngine().price_with_greeks(
                    european, context, ["speed"]
                )
                options = [european]
                if direction == "call" and dividend == 0:
                    options.append(AmericanOption(**terms))
                for option in options:
                    with self.subTest(
                        dividend=dividend, direction=direction, option=type(option)
                    ):
                        actual = engine.price_with_greeks(option, context, ["speed"])
                        self.assertEqual(actual.price, engine.price(option, context))
                        self.assertAlmostEqual(
                            actual.speed,
                            expected.speed,
                            delta=abs(expected.speed) * 0.02,
                        )
                        self.assertIsNone(actual.gamma)

    def test_accumulator_fixed_quantity_preserves_accrual_linearity(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
        )
        for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
            with self.subTest(scheme=scheme):
                engine = pricing.FiniteDifferenceAccumulatorEngine(
                    asset_step_count=40,
                    time_step_count=200,
                    asset_upper_boundary=440,
                    scheme=scheme,
                )
                results = [
                    engine.price_with_greeks(
                        Accumulator(
                            strike=90,
                            knock_out_level=110,
                            daily_quantity=daily,
                            acceleration_factor=2,
                            accumulated_quantity=accumulated,
                            effective_date=start,
                            expiry_date=end,
                        ),
                        context,
                        ["delta", "gamma"],
                    )
                    for daily, accumulated in ((0, 3), (1, 3), (1, 0))
                ]
                fixed, with_purchases, purchases_only = results
                for name in ("price", "delta", "gamma"):
                    self.assertAlmostEqual(
                        getattr(fixed, name),
                        getattr(with_purchases, name) - getattr(purchases_only, name),
                        delta=1e-9 if name == "price" else 1e-10,
                    )

    def test_accumulator_inactive_accrual_preserves_fixed_position_fd_values(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
        )
        for strike in (105, 110):
            for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
                with self.subTest(strike=strike, scheme=scheme):
                    engine = pricing.FiniteDifferenceAccumulatorEngine(
                        asset_step_count=40,
                        time_step_count=200,
                        asset_upper_boundary=440,
                        scheme=scheme,
                    )
                    results = [
                        engine.price_with_greeks(
                            Accumulator(
                                strike=strike,
                                knock_out_level=105,
                                daily_quantity=daily,
                                acceleration_factor=0,
                                accumulated_quantity=3,
                                effective_date=start,
                                expiry_date=end,
                            ),
                            context,
                            ["delta", "gamma"],
                        )
                        for daily in (0, 1)
                    ]
                    for name in ("price", "delta", "gamma"):
                        self.assertEqual(
                            getattr(results[0], name), getattr(results[1], name)
                        )

    def test_binary_barrier_branches_preserve_in_out_parity(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.AnalyticBinaryBarrierEngine()
        for up in (False, True):
            for direction in ("call", "put"):
                for strike in (70, 80, 100, 120, 130):
                    for asset in (False, True):
                        with self.subTest(
                            up=up, direction=direction, strike=strike, asset=asset
                        ):
                            terms = {
                                "option_type": direction,
                                "strike": strike,
                                "effective_date": start,
                                "expiry_date": end,
                            }
                            vanilla = (
                                AssetOrNothingOption(**terms)
                                if asset
                                else CashOrNothingOption(**terms, payout=10)
                            )
                            factory = (
                                asset_binary_barrier_option
                                if asset
                                else cash_binary_barrier_option
                            )
                            if not asset:
                                terms["payout"] = 10
                            terms["barrier_level"] = 120 if up else 80
                            prefix = "up" if up else "down"
                            knock_in = factory(**terms, barrier_type=f"{prefix}_and_in")
                            knock_out = factory(
                                **terms, barrier_type=f"{prefix}_and_out"
                            )
                            self.assertAlmostEqual(
                                engine.price(knock_in, context)
                                + engine.price(knock_out, context),
                                pricing.AnalyticDigitalEngine().price(vanilla, context),
                                delta=1e-10,
                            )

    def test_binomial_mixed_greeks_track_analytic_sensitivities(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        request = ["vanna", "zomma", "charm", "color"]
        engine = pricing.CoxRossRubinsteinVanillaEngine(1000)
        for dividend in (0, 0.01):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.04, dividend_yield=dividend, volatility=0.2
                ),
                spot_price=100,
                valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
            )
            for direction in ("call", "put"):
                terms = {
                    "option_type": direction,
                    "strike": 100,
                    "effective_date": start,
                    "expiry_date": end,
                }
                european = EuropeanOption(**terms)
                expected = pricing.AnalyticVanillaEngine().price_with_greeks(
                    european, context, request
                )
                options = [european]
                if direction == "call" and dividend == 0:
                    options.append(AmericanOption(**terms))
                for option in options:
                    with self.subTest(
                        dividend=dividend,
                        option=type(option).__name__,
                        direction=direction,
                    ):
                        actual = engine.price_with_greeks(option, context, request)
                        self.assertEqual(actual.price, engine.price(option, context))
                        for name in request:
                            reference = getattr(expected, name)
                            self.assertAlmostEqual(
                                getattr(actual, name),
                                reference,
                                delta=abs(reference) * 0.02 + 1e-7,
                            )
                        self.assertIsNone(actual.delta)
                        self.assertIsNone(actual.gamma)

    def test_analytic_barrier_branches_preserve_in_out_parity_and_rebate_timing(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        engine = pricing.AnalyticBarrierEngine()
        for up in (False, True):
            for direction in ("call", "put"):
                for strike in (70, 80, 100, 120, 130):
                    common = {
                        "option_type": direction,
                        "strike": strike,
                        "effective_date": start,
                        "expiry_date": end,
                    }
                    vanilla = pricing.AnalyticVanillaEngine().price(
                        EuropeanOption(**common), context
                    )
                    for rebate in (0, 5):
                        for timing in ("at_hit", "at_expiry"):
                            with self.subTest(
                                up=up,
                                direction=direction,
                                strike=strike,
                                rebate=rebate,
                                timing=timing,
                            ):
                                level = 120 if up else 80
                                prefix = "up" if up else "down"
                                out = BarrierOption(
                                    **common,
                                    barrier_level=level,
                                    barrier_type=f"{prefix}_and_out",
                                    rebate=rebate,
                                    rebate_timing=timing,
                                )
                                knock_in = BarrierOption(
                                    **common,
                                    barrier_level=level,
                                    barrier_type=f"{prefix}_and_in",
                                    rebate=0 if timing == "at_hit" else rebate,
                                )
                                extra = rebate * math.exp(-0.04)
                                if timing == "at_hit":
                                    touch = (
                                        cash_one_touch_up if up else cash_one_touch_down
                                    )(
                                        effective_date=start,
                                        expiry_date=end,
                                        barrier_level=level,
                                        payout=1,
                                        settlement_timing="at_hit",
                                    )
                                    extra = (
                                        rebate
                                        * pricing.AnalyticBinaryBarrierEngine().price(
                                            touch, context
                                        )
                                    )
                                self.assertAlmostEqual(
                                    engine.price(knock_in, context)
                                    + engine.price(out, context),
                                    vanilla + extra,
                                    delta=1e-10,
                                )

    def test_continuous_fd_barriers_preserve_rebates_across_schemes(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
            explicit = scheme == "explicit_euler"
            engine = pricing.FiniteDifferenceBarrierEngine(
                asset_step_count=80 if explicit else 800,
                time_step_count=400,
                scheme=scheme,
                asset_upper_boundary=400,
            )
            for rate in (-0.04, 0.04):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=rate,
                        dividend_yield=0.01,
                        volatility=0.2,
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                for up in (False, True):
                    for timing in ("at_hit", "at_expiry"):
                        with self.subTest(
                            scheme=scheme, rate=rate, up=up, timing=timing
                        ):
                            option = BarrierOption(
                                option_type="call",
                                strike=100,
                                effective_date=start,
                                expiry_date=end,
                                barrier_level=120 if up else 80,
                                barrier_type="up_and_out" if up else "down_and_out",
                                rebate=5,
                                rebate_timing=timing,
                            )
                            actual = engine.price_with_greeks(
                                option, context, greeks=["delta", "gamma"]
                            )
                            expected = (
                                pricing.AnalyticBarrierEngine().price_with_greeks(
                                    option, context, greeks=["delta", "gamma"]
                                )
                            )
                            self.assertEqual(
                                actual.price, engine.price(option, context)
                            )
                            self.assertAlmostEqual(
                                actual.price,
                                expected.price,
                                delta=0.1 if explicit else 0.01,
                            )
                            self.assertAlmostEqual(
                                actual.delta,
                                expected.delta,
                                delta=0.01 if explicit else 0.003,
                            )
                            self.assertAlmostEqual(
                                actual.gamma,
                                expected.gamma,
                                delta=0.002 if explicit else 0.0005,
                            )

    def test_american_mc_skips_grids_without_early_exercise_value(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": start,
            "expiry_date": end,
        }
        american, european = AmericanOption(**terms), EuropeanOption(**terms)
        for rate in (0.0, 0.05):
            for dividend in (-0.02, 0.0):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=rate, dividend_yield=dividend, volatility=0.2
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                expected = pricing.MonteCarloVanillaEngine(
                    path_count=20000, step_count=2, seed=42
                ).price(european, context)
                for steps in (3, 50, 10000):
                    with self.subTest(rate=rate, dividend=dividend, steps=steps):
                        engine = pricing.MonteCarloVanillaEngine(
                            path_count=19999, step_count=steps, seed=42
                        )
                        self.assertEqual(engine.price(american, context), expected)
                        self.assertEqual(engine.step_count, steps)
        deep = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=2000,
            valuation_time=start,
        )
        for seed in (1, 42, 73):
            engine = pricing.MonteCarloVanillaEngine(
                path_count=1, step_count=3, seed=seed
            )
            self.assertEqual(
                engine.price(american, deep), max(engine.price(european, deep), 1900)
            )
        for backend in ("cpu", "cuda"):
            for paths, steps in ((0, 50), (16, 2), (16, 10001)):
                engine = pricing.MonteCarloVanillaEngine(
                    path_count=paths, step_count=steps, seed=42, backend=backend
                )
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(american, context)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                )
            expired = PricingContext(
                model_parameters=context.model_parameters,
                spot_price=110,
                valuation_time=end,
            )
            engine = pricing.MonteCarloVanillaEngine(
                path_count=16, step_count=2, seed=42, backend=backend
            )
            self.assertEqual(engine.price(american, expired), 10)

    def test_american_mc_rate_greeks_retain_one_estimator_across_zero(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = AmericanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        engine = pricing.MonteCarloVanillaEngine(
            path_count=20000, step_count=7, seed=42
        )
        shift = 1 / 8192
        for rate in (-1 / 16384, 0.0, 1 / 16384):
            with self.subTest(rate=rate):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=rate, dividend_yield=0, volatility=0.2
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                rho = engine.price_with_greeks(option, context, "rho", rate_shift=shift)
                all_greeks = engine.price_with_greeks(
                    option, context, all_greeks=True, rate_shift=shift
                )
                self.assertEqual(rho.price, engine.price(option, context))
                self.assertEqual(all_greeks.price, rho.price)

                # A subnormal dividend retains the grid without changing the coefficients.
                def exercise_value(bumped_rate):
                    shifted = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=bumped_rate,
                            dividend_yield=math.nextafter(0.0, 1.0),
                            volatility=0.2,
                        ),
                        spot_price=100,
                        valuation_time=start,
                    )
                    return engine.price(option, shifted)

                expected = (
                    exercise_value(rate + shift) - exercise_value(rate - shift)
                ) / (200 * shift)
                self.assertAlmostEqual(rho.rho, expected, delta=1e-10)
                self.assertEqual(all_greeks.rho, rho.rho)
                spot = engine.price_with_greeks(option, context, ["delta", "gamma"])
                self.assertEqual(
                    (all_greeks.delta, all_greeks.gamma), (spot.delta, spot.gamma)
                )

    def test_zero_exposure_accumulators_skip_numerical_work_after_validation(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for strike, knock_out, daily, acceleration in (
            (100, 110, 0, 2),
            (100, 100, 1, 0),
            (110, 105, 1, 0),
        ):
            option = Accumulator(
                strike=strike,
                knock_out_level=knock_out,
                daily_quantity=daily,
                acceleration_factor=acceleration,
                accumulated_quantity=0,
                effective_date=start,
                expiry_date=end,
            )
            parameters = BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            )
            context = PricingContext(
                model_parameters=parameters, spot_price=90, valuation_time=start
            )
            for engine in (
                pricing.MonteCarloAccumulatorEngine(path_count=2000, seed=42),
                pricing.MonteCarloAccumulatorEngine(
                    path_count=2000, seed=42, backend="cuda"
                ),
                pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=1),
            ):
                with self.subTest(engine=type(engine).__name__):
                    self.assertEqual(engine.price(option, context), 0)
                    result = engine.price_with_greeks(
                        option, context, ["delta", "gamma", "vega", "rho"]
                    )
                    self.assertEqual(
                        (
                            result.price,
                            result.delta,
                            result.gamma,
                            result.vega,
                            result.rho,
                        ),
                        (0, 0, 0, 0, 0),
                    )
                    expired = PricingContext(
                        model_parameters=parameters,
                        spot_price=90,
                        valuation_time=date(2026, 1, 2),
                    )
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        engine.price(option, expired)
                    self.assertEqual(
                        error.exception.category,
                        kiyosi.ErrorCategory.INVALID_TIME_RANGE,
                    )
            for engine in (
                pricing.MonteCarloAccumulatorEngine(path_count=0),
                pricing.FiniteDifferenceAccumulatorEngine(time_step_count=0),
            ):
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    engine.price(option, context)
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                )

    def test_structured_finite_difference_spot_greeks_reuse_native_layers(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=70,
            valuation_time=date(2025, 1, 2),
        )
        accumulator = Accumulator(
            strike=50,
            knock_out_level=200,
            daily_quantity=0,
            acceleration_factor=1,
            accumulated_quantity=3,
            effective_date=start,
            expiry_date=end,
        )
        note = SnowballOption(
            knock_out_coupon_rates=[0],
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=75,
            knock_out_levels=[200],
            upper_strike=100,
            lower_strike=0,
            observation_dates=[end],
            knock_in_observation_mode="every_trading_day",
            barrier_state="knocked_in",
            effective_date=start,
            expiry_date=end,
        )
        put = EuropeanOption(
            option_type="put", strike=100, effective_date=start, expiry_date=end
        )
        expected = pricing.AnalyticVanillaEngine().price_with_greeks(
            put, context, ["delta", "gamma"]
        )
        time = (end - date(2025, 1, 2)).days / 365
        for engine, option, delta, gamma in (
            (
                pricing.FiniteDifferenceAccumulatorEngine(),
                accumulator,
                3 * math.exp(-0.01 * time),
                0,
            ),
            (
                pricing.FiniteDifferenceSnowballEngine(
                    asset_step_count=800, asset_upper_boundary=800
                ),
                note,
                -expected.delta / 100,
                -expected.gamma / 100,
            ),
        ):
            with self.subTest(engine=type(engine).__name__):
                result = engine.price_with_greeks(option, context, ["delta", "gamma"])
                shifted = engine.price_with_greeks(
                    option, context, ["delta", "gamma"], spot_shift=0.1
                )
                self.assertEqual(result.price, engine.price(option, context))
                self.assertEqual(
                    (result.delta, result.gamma), (shifted.delta, shifted.gamma)
                )
                self.assertAlmostEqual(result.delta, delta, delta=1e-5)
                self.assertAlmostEqual(result.gamma, gamma, delta=1e-6)
                self.assertIsNone(result.vega)

    def test_analytic_barriers_retain_tiny_positive_volatility_time_value(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticBarrierEngine()
        for kind in ("down_and_in", "down_and_out", "up_and_in", "up_and_out"):
            for direction in ("call", "put"):
                for spot in (1e16, 1e150):
                    for sigma in (1e-12, 1e-16, 1e-20, 1e-200):
                        with self.subTest(
                            kind=kind, direction=direction, spot=spot, sigma=sigma
                        ):
                            context = PricingContext(
                                model_parameters=BlackScholesMertonParameters(
                                    risk_free_rate=0, dividend_yield=0, volatility=sigma
                                ),
                                spot_price=spot,
                                valuation_time=start,
                            )
                            option = BarrierOption(
                                option_type=direction,
                                strike=spot,
                                effective_date=start,
                                expiry_date=end,
                                barrier_level=(1.2 if kind.startswith("up") else 0.8)
                                * spot,
                                barrier_type=kind,
                            )
                            actual = engine.price(option, context)
                            expected = (
                                0
                                if kind.endswith("in")
                                else spot * sigma / math.sqrt(2 * math.pi)
                            )
                            self.assertTrue(
                                math.isclose(actual, expected, rel_tol=1e-12)
                            )
                            self.assertEqual(
                                engine.price_with_greeks(
                                    option, context, "delta"
                                ).price,
                                actual,
                            )

    def test_bjerksund_retains_small_volatility_early_exercise_value(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        engine = pricing.BjerksundStenslandVanillaEngine()
        for direction in ("call", "put"):
            call = direction == "call"
            option = AmericanOption(
                option_type=direction,
                strike=100 if call else 150,
                effective_date=start,
                expiry_date=end,
            )
            for sigma in (1e-6, 1e-10, 1e-20, 1e-100, 1e-150, 1e-200, 1e200):
                with self.subTest(direction=direction, sigma=sigma):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0.1 if call else 0.05,
                            dividend_yield=0.05 if call else 0.1,
                            volatility=sigma,
                        ),
                        spot_price=150 if call else 100,
                        valuation_time=start,
                    )
                    if sigma in (1e-200, 1e200):
                        for method, extra in (
                            (engine.price, ()),
                            (engine.price_with_greeks, ("delta",)),
                        ):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                method(option, context, *extra)
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.INVALID_RESULT,
                            )
                    else:
                        actual = engine.price(option, context)
                        self.assertAlmostEqual(actual, 56.25, delta=1e-8)
                        self.assertEqual(
                            engine.price_with_greeks(option, context, "delta").price,
                            actual,
                        )

    def test_implicit_euler_preserves_large_finite_currency_scales(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
        )
        engine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=200, time_step_count=200, scheme="implicit_euler"
        )
        for direction in ("call", "put"):
            prices = []
            for amount in (100, 1e305, 1e306):
                option = EuropeanOption(
                    option_type=direction,
                    strike=amount,
                    effective_date=start,
                    expiry_date=end,
                )
                context = PricingContext(
                    model_parameters=parameters, spot_price=amount, valuation_time=start
                )
                prices.append(engine.price(option, context) / amount)
            for scaled in prices[1:]:
                self.assertTrue(math.isclose(scaled, prices[0], rel_tol=1e-12))

    def test_american_fd_domains_preserve_long_expiry_exercise_value(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0.05, volatility=1.0
            ),
            spot_price=100,
            valuation_time=start,
        )
        option = AmericanOption(
            option_type="call", strike=100, effective_date=start, expiry_date=end
        )
        # QuantLib 1.43 FD, 3200 time/asset steps and 2 damping steps.
        reference = 69.73204196706111
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
