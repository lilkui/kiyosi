import math
import unittest
from datetime import date, datetime, timedelta, timezone

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
    GeometricAveragePriceOption,
    PhoenixOption,
    SnowballOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)
from kiyosi.pricing import implied_volatility


class PricingAuditTests(unittest.TestCase):
    def test_quadrature_vanilla_retains_small_positive_volatility_time_value(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction in ("call", "put"):
            for spot in (100, 1e14):
                for sigma in (1e-13, 1e-12, 1e-11, 1e-6):
                    with self.subTest(direction=direction, spot=spot, sigma=sigma):
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=0, dividend_yield=0, volatility=sigma,
                            ),
                            spot_price=spot, valuation_time=start,
                        )
                        option = EuropeanOption(
                            option_type=direction, strike=spot,
                            effective_date=start, expiry_date=end,
                        )
                        actual = pricing.QuadratureVanillaEngine().price(option, context)
                        expected = spot * math.erf(sigma / (2 * math.sqrt(2)))
                        self.assertAlmostEqual(actual, expected, delta=max(1e-24, expected * 1e-12))

    def test_single_fixing_geometric_asians_retain_european_time_value(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticGeometricAveragePriceEngine()
        for direction in ("call", "put"):
            for sigma in (1e-13, 0.2):
                with self.subTest(direction=direction, sigma=sigma):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=0, dividend_yield=0, volatility=sigma,
                        ),
                        spot_price=1e14, valuation_time=start,
                    )
                    terms = dict(
                        option_type=direction, strike=1e14,
                        effective_date=start, averaging_start_date=end, expiry_date=end,
                    )
                    actual = engine.price(GeometricAveragePriceOption(**terms), context)
                    expected = 1e14 * math.erf(sigma / (2 * math.sqrt(2)))
                    self.assertAlmostEqual(actual, expected, delta=max(1e-12, expected * 1e-12))
                    with self.assertRaises(kiyosi.KiyosiError) as error:
                        engine.price(GeometricAveragePriceOption(**terms, realized_average=1e14), context)
                    self.assertEqual(error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER)
            expired = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2,
                ),
                spot_price=110, valuation_time=end,
            )
            settled = engine.price(
                GeometricAveragePriceOption(
                    option_type=direction, strike=100,
                    effective_date=start, averaging_start_date=end, expiry_date=end,
                ),
                expired,
            )
            self.assertEqual(settled, 10 if direction == "call" else 0)

    def test_implied_volatility_rejects_unconditional_fixed_phoenix_cashflows(self):
        start, fixing, end = date(2025, 1, 1), date(2025, 1, 6), date(2025, 1, 10)
        for rate in (0, 0.05):
            for dates in ([end], [fixing], [fixing, end]):
                for barrier in (0, 90):
                    for lower in (100, 60):
                        note = PhoenixOption(
                            coupon_rate=0.1,
                            initial_spot=100,
                            knock_in_level=80,
                            knock_out_levels=[120] * len(dates),
                            coupon_barrier_levels=[barrier] * len(dates),
                            upper_strike=100,
                            lower_strike=lower,
                            observation_dates=dates,
                            knock_in_observation_mode="at_expiry",
                            effective_date=start,
                            expiry_date=end,
                        )
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=rate,
                                dividend_yield=0.02,
                                volatility=0.05,
                            ),
                            spot_price=100,
                            valuation_time=start,
                            calendar=all_days_calendar(),
                        )
                        exposed = (
                            lower != 100 or barrier != 0 or len(dates) > 1
                            or (rate != 0 and dates[0] != end)
                        )
                        for engine in (
                            pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
                            pricing.FiniteDifferencePhoenixEngine(),
                        ):
                            with self.subTest(
                                rate=rate, dates=dates, barrier=barrier,
                                lower=lower, engine=type(engine).__name__,
                            ):
                                quote = engine.price(note, context)
                                if exposed:
                                    self.assertEqual(
                                        implied_volatility(
                                            engine, note, context, quote,
                                            lower_bound=0.05, upper_bound=0.4,
                                        ),
                                        0.05,
                                    )
                                else:
                                    days = (dates[0] - start).days
                                    expected = math.exp(-rate * 9 / 365) + (
                                        0.1 * days / 365 * math.exp(-rate * days / 365)
                                    )
                                    self.assertAlmostEqual(quote, expected, delta=1e-10)
                                    with self.assertRaises(kiyosi.KiyosiError) as error:
                                        implied_volatility(
                                            engine, note, context, quote,
                                            lower_bound=0.05, upper_bound=0.4,
                                        )
                                    self.assertEqual(
                                        error.exception.category,
                                        kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                                    )

    def test_trading_finite_difference_prices_are_currency_scale_invariant(self):
        start, end = date(2025, 1, 1), date(2025, 1, 10)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )

        def prices(scale):
            context = PricingContext(
                model_parameters=parameters,
                spot_price=100 * scale,
                valuation_time=start,
                calendar=all_days_calendar(),
            )
            accumulator = Accumulator(
                strike=100 * scale,
                knock_out_level=120 * scale,
                daily_quantity=1,
                acceleration_factor=2,
                accumulated_quantity=0,
                effective_date=start,
                expiry_date=end,
            )
            snowball = BinarySnowballOption(
                knock_out_coupon_rates=[0.1],
                maturity_coupon_rate=0.2,
                knock_out_levels=[101 * scale],
                observation_dates=[end],
                effective_date=start,
                expiry_date=end,
            )
            return (
                pricing.FiniteDifferenceAccumulatorEngine().price(accumulator, context)
                / scale,
                pricing.FiniteDifferenceBinarySnowballEngine().price(snowball, context),
            )

        expected = prices(1)
        for scale in (0.01, 0.0001, 0.000001):
            with self.subTest(scale=scale):
                for actual, baseline in zip(prices(scale), expected):
                    self.assertAlmostEqual(actual, baseline, delta=1e-9)

    def test_numerical_time_greeks_preserve_microsecond_shifts(self):
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        for year in (1900, 2026, 9999):
            expiry = datetime(year, 1, 1, tzinfo=timezone.utc)
            option = EuropeanOption(
                option_type="call",
                strike=100,
                effective_date=date(year - 1, 1, 1),
                expiry_date=expiry.date(),
            )
            for microseconds in (1, 10):
                shift = timedelta(microseconds=microseconds)
                context = PricingContext(
                    model_parameters=parameters,
                    spot_price=100,
                    valuation_time=expiry - shift,
                )
                before = PricingContext(
                    model_parameters=parameters,
                    spot_price=100,
                    valuation_time=expiry - 2 * shift,
                )
                elapsed_days = (2 * shift).total_seconds() / 86400
                analytic = pricing.AnalyticVanillaEngine()
                quadrature = pricing.QuadratureVanillaEngine()
                for engine, evaluate in (
                    (analytic, pricing.calculate_numerical_greeks),
                    (
                        quadrature,
                        lambda engine, option, context: engine.price_with_greeks(
                            option, context, "theta"
                        ),
                    ),
                ):
                    with self.subTest(
                        year=year,
                        microseconds=microseconds,
                        engine=type(engine).__name__,
                    ):
                        expected = -engine.price(option, before) / elapsed_days
                        self.assertLess(expected, 0)
                        result = evaluate(engine, option, context)
                        self.assertAlmostEqual(
                            result.theta, expected, delta=abs(expected) * 1e-10
                        )

    def test_bjerksund_rejects_nonphysical_exercise_boundaries(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.BjerksundStenslandVanillaEngine()
        for direction in ("call", "put"):
            call = direction == "call"
            for rate, dividend, volatility in (
                (0.05, 0.1, 0.02),
                (0, 0.02, 0.01),
                (0, 0.1, 0.05),
            ):
                with self.subTest(direction=direction, volatility=volatility):
                    option = AmericanOption(
                        option_type=direction,
                        strike=100 if call else 99,
                        effective_date=start,
                        expiry_date=end,
                    )
                    parameters = BlackScholesMertonParameters(
                        risk_free_rate=rate if call else dividend,
                        dividend_yield=dividend if call else rate,
                        volatility=volatility,
                    )
                    context = PricingContext(
                        model_parameters=parameters,
                        spot_price=99 if call else 100,
                        valuation_time=start,
                    )
                    for method, extra in (
                        (engine.price, ()),
                        (engine.price_with_greeks, ("delta",)),
                    ):
                        with self.assertRaises(kiyosi.KiyosiError) as error:
                            method(option, context, *extra)
                        self.assertEqual(
                            error.exception.category,
                            kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                        )
                    expired = PricingContext(
                        model_parameters=parameters,
                        spot_price=context.spot_price,
                        valuation_time=end,
                    )
                    self.assertEqual(engine.price(option, expired), 0)

    def test_fixed_barrier_settlements_bypass_irrelevant_vanilla_overflow(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for rate in (0, 0.04):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=-0.1, volatility=0.2
                ),
                spot_price=1.7e308,
                valuation_time=date(2025, 1, 2),
            )
            terms = {
                "option_type": "call",
                "strike": 100,
                "barrier_level": 120,
                "rebate": 3,
                "effective_date": start,
                "expiry_date": end,
            }
            for kind, timing, state, scheduled, fixed, paid in (
                ("up_and_out", "at_expiry", "touched", False, True, False),
                ("up_and_out", "at_hit", "touched", False, True, True),
                ("up_and_out", "at_hit", "untouched", False, True, False),
                ("up_and_in", "at_expiry", "untouched", True, True, False),
                ("up_and_in", "at_expiry", "touched", False, False, False),
                ("up_and_out", "at_expiry", "untouched", True, False, False),
            ):
                option = BarrierOption(
                    **terms,
                    barrier_type=kind,
                    rebate_timing=timing,
                    touch_state=state,
                    observation_mode="scheduled" if scheduled else "continuous",
                    observation_dates=[start] if scheduled else [],
                )
                for engine in (
                    pricing.AnalyticBarrierEngine(),
                    pricing.FiniteDifferenceBarrierEngine(),
                ):
                    with self.subTest(
                        rate=rate,
                        kind=kind,
                        timing=timing,
                        state=state,
                        scheduled=scheduled,
                        engine=type(engine).__name__,
                    ):
                        if fixed:
                            result = engine.price_with_greeks(option, context, "vega")
                            expected = (
                                0
                                if paid
                                else 3
                                * (
                                    1
                                    if timing == "at_hit"
                                    else math.exp(-rate * 364 / 365)
                                )
                            )
                            self.assertAlmostEqual(result.price, expected)
                            self.assertEqual(result.vega, 0)
                        else:
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                engine.price_with_greeks(option, context, "vega")
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.INVALID_RESULT,
                            )
            with self.assertRaises(kiyosi.KiyosiError) as error:
                pricing.AnalyticBarrierEngine().price(
                    BarrierOption(**terms, barrier_type="up_and_out"), context
                )
            self.assertEqual(
                error.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
            )

    def test_finite_difference_knock_in_prices_preserve_small_positive_values(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for direction, strike, barrier, kind in (
            ("call", 150, 70, "down_and_in"),
            ("put", 50, 130, "up_and_in"),
        ):
            option = BarrierOption(
                option_type=direction,
                strike=strike,
                barrier_level=barrier,
                barrier_type=kind,
                effective_date=start,
                expiry_date=end,
            )
            expected = pricing.AnalyticBarrierEngine().price(option, context)
            for steps in (200, 800):
                with self.subTest(direction=direction, steps=steps):
                    engine = pricing.FiniteDifferenceBarrierEngine(
                        asset_step_count=steps
                    )
                    value = engine.price(option, context)
                    self.assertGreater(value, 0)
                    self.assertAlmostEqual(value, expected, delta=5e-7)
                    self.assertEqual(
                        engine.price_with_greeks(option, context, "delta").price,
                        value,
                    )
            coarse = pricing.FiniteDifferenceBarrierEngine(asset_step_count=3)
            self.assertGreaterEqual(coarse.price(option, context), 0)

    def test_analytic_and_quadrature_prices_preserve_scaled_normal_tails(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        )
        # Independent 100-digit Decimal references, scaled before rounding to float.
        for ratio, vanilla, call_cash, call_asset, put_asset in (
            (
                2392.274820537378,
                6.592380619674553e-32,
                5.3531191121506334e-33,
                1.287205586953211e-29,
                1.2806132063335364e-29,
            ),
            (
                10000,
                1.1367038364232515e-163,
                2.6141386421114433e-165,
                2.625505680475676e-161,
                2.6141386421114432e-161,
            ),
        ):
            for direction in ("call", "put"):
                call = direction == "call"
                context = PricingContext(
                    model_parameters=parameters,
                    spot_price=1e300 if call else 1e300 * ratio,
                    valuation_time=start,
                )
                kwargs = {
                    "option_type": direction,
                    "strike": 1e300 * ratio if call else 1e300,
                    "effective_date": start,
                    "expiry_date": end,
                }
                option = EuropeanOption(**kwargs)
                cash = CashOrNothingOption(**kwargs, payout=1e300)
                asset = AssetOrNothingOption(**kwargs)
                for engine, instrument, expected in (
                    (pricing.AnalyticVanillaEngine(), option, vanilla),
                    (pricing.QuadratureVanillaEngine(), option, vanilla),
                    (
                        pricing.AnalyticDigitalEngine(),
                        cash,
                        call_cash if call else call_asset,
                    ),
                    (
                        pricing.QuadratureDigitalEngine(),
                        cash,
                        call_cash if call else call_asset,
                    ),
                    (
                        pricing.AnalyticDigitalEngine(),
                        asset,
                        call_asset if call else put_asset,
                    ),
                    (
                        pricing.QuadratureDigitalEngine(),
                        asset,
                        call_asset if call else put_asset,
                    ),
                ):
                    with self.subTest(
                        ratio=ratio,
                        direction=direction,
                        engine=engine,
                        instrument=instrument,
                    ):
                        value = engine.price(instrument, context)
                        self.assertTrue(math.isclose(value, expected, rel_tol=1e-8))
                        self.assertEqual(
                            engine.price_with_greeks(
                                instrument, context, "delta"
                            ).price,
                            value,
                        )
                rho = (
                    pricing.AnalyticVanillaEngine()
                    .price_with_greeks(option, context, "rho")
                    .rho
                )
                self.assertTrue(
                    math.isclose(
                        rho,
                        (call_cash * ratio if call else -call_asset) / 100,
                        rel_tol=1e-8,
                    )
                )

    def test_analytic_vanilla_greeks_preserve_scaled_normal_tails(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        )
        engine = pricing.AnalyticVanillaEngine()
        for direction in ("call", "put"):
            context = PricingContext(
                model_parameters=parameters, spot_price=1e-300, valuation_time=start
            )
            option = EuropeanOption(
                option_type=direction,
                strike=1e-296,
                effective_date=start,
                expiry_date=end,
            )
            expected = {
                "gamma": 6.035176823596672e-159,
                "speed": 1.3805980535242903e144,
                "color": -1.7524741795041444e-158,
                "zomma": 6.396530755190127e-157,
            }
            result = engine.price_with_greeks(option, context, greeks=list(expected))
            for name, value in expected.items():
                self.assertTrue(
                    math.isclose(result.require(name), value, rel_tol=1e-10)
                )
            context = PricingContext(
                model_parameters=parameters, spot_price=1e300, valuation_time=start
            )
            option = EuropeanOption(
                option_type=direction,
                strike=1e304,
                effective_date=start,
                expiry_date=end,
            )
            result = engine.price_with_greeks(option, context, greeks=["vega", "theta"])
            self.assertTrue(
                math.isclose(result.vega, 1.2070353647193343e-161, rel_tol=1e-10)
            )
            self.assertTrue(
                math.isclose(result.theta, -3.306946204710505e-163, rel_tol=1e-10)
            )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=2
            ),
            spot_price=1e308,
            valuation_time=start,
        )
        option = EuropeanOption(
            option_type="call", strike=1e308, effective_date=start, expiry_date=end
        )
        result = engine.price_with_greeks(option, context, greeks=["gamma"])
        self.assertTrue(
            math.isclose(result.gamma, 1.2098536225957167e-309, rel_tol=1e-10)
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=1e155
            ),
            spot_price=100,
            valuation_time=start,
        )
        option = EuropeanOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=start + timedelta(days=1),
        )
        result = engine.price_with_greeks(option, context, all_greeks=True)
        self.assertEqual(result.price, 100)
        self.assertEqual(result.color, 0)

    def test_quadrature_retains_scaled_prices_beyond_the_former_tail_cutoff(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        )
        for direction, sign, spot, strike in (
            ("call", 1, 1e100, 1.5e101),
            ("put", -1, 1.5e101, 1e100),
        ):
            context = PricingContext(
                model_parameters=parameters, spot_price=spot, valuation_time=start
            )
            option = EuropeanOption(
                option_type=direction,
                strike=strike,
                effective_date=start,
                expiry_date=end,
            )
            value = pricing.QuadratureVanillaEngine().price(option, context)
            self.assertTrue(math.isclose(value, 2.5478923549273412e57, rel_tol=1e-7))
            for threshold in (11.0, 11.9, 11.99, 12.0, 13.6, 30.0):
                strike = spot * math.exp(sign * 0.2 * threshold - 0.02)
                digital = CashOrNothingOption(
                    option_type=direction,
                    strike=strike,
                    payout=1e100,
                    effective_date=start,
                    expiry_date=end,
                )
                value = pricing.QuadratureDigitalEngine().price(digital, context)
                expected = 1e100 * 0.5 * math.erfc(threshold / math.sqrt(2))
                self.assertTrue(math.isclose(value, expected, rel_tol=1e-8))

    def test_implied_volatility_counts_only_future_phoenix_coupons(self):
        start, fixing, end = date(2025, 1, 1), date(2025, 1, 3), date(2025, 1, 6)
        for valuation in (date(2025, 1, 2), fixing, date(2025, 1, 4)):
            for lower in (100, 60):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0, dividend_yield=0, volatility=0.05
                    ),
                    spot_price=100,
                    valuation_time=valuation,
                    calendar=all_days_calendar(),
                )
                option = PhoenixOption(
                    coupon_rate=0.1,
                    initial_spot=100,
                    knock_in_level=80,
                    knock_out_levels=[110],
                    coupon_barrier_levels=[90],
                    upper_strike=100,
                    lower_strike=lower,
                    observation_dates=[fixing],
                    knock_in_observation_mode="every_trading_day",
                    barrier_state="knocked_in",
                    effective_date=start,
                    expiry_date=end,
                )
                for engine in (
                    pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
                    pricing.FiniteDifferencePhoenixEngine(),
                ):
                    with self.subTest(
                        valuation=valuation, lower=lower, engine=type(engine).__name__
                    ):
                        quote = engine.price(option, context)
                        args = {"lower_bound": 0.05, "upper_bound": 0.4}
                        if valuation < fixing or lower != 100:
                            self.assertEqual(
                                implied_volatility(
                                    engine, option, context, quote, **args
                                ),
                                0.05,
                            )
                        else:
                            self.assertAlmostEqual(
                                quote,
                                1 + (0.1 * 2 / 365 if valuation == fixing else 0),
                                places=10,
                            )
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                implied_volatility(
                                    engine, option, context, quote, **args
                                )
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                            )

    def test_implied_volatility_rejects_fixed_participation_and_zero_accrual(self):
        start, fixing, end = date(2025, 1, 1), date(2025, 7, 1), date(2026, 1, 1)

        def check(engine, option, context, exposed):
            quote = engine.price(option, context)
            if exposed:
                self.assertEqual(
                    implied_volatility(
                        engine,
                        option,
                        context,
                        quote,
                        lower_bound=0.05,
                        upper_bound=0.4,
                    ),
                    0.05,
                )
            else:
                with self.assertRaises(kiyosi.KiyosiError) as error:
                    implied_volatility(
                        engine,
                        option,
                        context,
                        quote,
                        lower_bound=0.05,
                        upper_bound=0.4,
                    )
                self.assertEqual(
                    error.exception.category, kiyosi.ErrorCategory.UNSUPPORTED_OPERATION
                )

        for rate in (0, 0.04):
            for lower in (100, 60):
                for coupon in (0, 0.1):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate, dividend_yield=0, volatility=0.05
                        ),
                        spot_price=100,
                        valuation_time=start,
                    )
                    terms = {
                        "initial_spot": 100,
                        "knock_in_level": 80,
                        "knock_out_levels": [120, 120],
                        "upper_strike": 100,
                        "lower_strike": lower,
                        "observation_dates": [fixing, end],
                        "knock_in_observation_mode": "at_expiry",
                        "effective_date": start,
                        "expiry_date": end,
                    }
                    snowball = SnowballOption(
                        **terms,
                        knock_out_coupon_rates=[coupon, coupon],
                        maturity_coupon_rate=coupon,
                    )
                    phoenix = PhoenixOption(
                        **terms, coupon_rate=coupon, coupon_barrier_levels=[90, 90]
                    )
                    for engine, option in (
                        (
                            pricing.MonteCarloSnowballEngine(path_count=64, seed=73),
                            snowball,
                        ),
                        (pricing.FiniteDifferenceSnowballEngine(), snowball),
                        (
                            pricing.MonteCarloPhoenixEngine(path_count=64, seed=73),
                            phoenix,
                        ),
                        (pricing.FiniteDifferencePhoenixEngine(), phoenix),
                    ):
                        with self.subTest(
                            engine=type(engine).__name__,
                            rate=rate,
                            lower=lower,
                            coupon=coupon,
                        ):
                            check(
                                engine,
                                option,
                                context,
                                rate != 0 or lower != 100 or coupon != 0,
                            )
        for historical in (False, True):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=0.05
                ),
                spot_price=100 if historical else 70,
                valuation_time=fixing if historical else start,
            )
            note = SnowballOption(
                knock_out_coupon_rates=[0, 0],
                maturity_coupon_rate=0.1,
                initial_spot=100,
                knock_in_level=80,
                knock_out_levels=[120, 120],
                upper_strike=100,
                lower_strike=100,
                observation_dates=[fixing, end],
                knock_in_observation_mode="every_trading_day",
                barrier_state="knocked_in" if historical else "none",
                effective_date=start,
                expiry_date=end,
            )
            for engine in (
                pricing.MonteCarloSnowballEngine(path_count=64, seed=73),
                pricing.FiniteDifferenceSnowballEngine(),
            ):
                check(engine, note, context, False)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.05
            ),
            spot_price=80,
            valuation_time=start,
        )
        for knock_out in (90, 100, 120):
            for acceleration in (0, 1):
                for quantity in (0, 1):
                    option = Accumulator(
                        strike=100,
                        knock_out_level=knock_out,
                        daily_quantity=1,
                        acceleration_factor=acceleration,
                        accumulated_quantity=quantity,
                        effective_date=start,
                        expiry_date=end,
                    )
                    for engine in (
                        pricing.MonteCarloAccumulatorEngine(path_count=64, seed=73),
                        pricing.FiniteDifferenceAccumulatorEngine(),
                    ):
                        with self.subTest(
                            engine=type(engine).__name__,
                            knock_out=knock_out,
                            acceleration=acceleration,
                            quantity=quantity,
                        ):
                            check(
                                engine,
                                option,
                                context,
                                quantity != 0 or acceleration != 0 or knock_out > 100,
                            )
