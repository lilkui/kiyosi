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
    autocallable_observations,
    cash_binary_barrier_option,
    cash_one_touch_down,
    cash_one_touch_up,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)
from price_differences import price_differences


class PricingRegressionTests(unittest.TestCase):
    def test_scheduled_time_greeks_preserve_inclusive_observation_endpoints(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        event = datetime(2025, 7, 1, tzinfo=UTC)
        dates = [date(2025, 1, 2), event.date(), date(2025, 12, 31)]
        barrier = BarrierOption(
            option_type="call",
            strike=100,
            effective_date=start,
            expiry_date=end,
            barrier_level=120,
            barrier_type="up_and_out",
            observation_mode="scheduled",
            observation_dates=dates,
            touch_state="untouched",
        )
        note = BinarySnowballOption(
            maturity_coupon_rate=0.05,
            effective_date=start,
            expiry_date=end,
            barrier_state="none",
            observations=autocallable_observations(
                dates, [110] * 3, knock_out_coupon_rates=[0.08] * 3
            ),
        )
        cases = (
            (
                barrier,
                pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=80, time_step_count=40
                ),
            ),
            (note, pricing.QuadratureAutocallableEngine(asset_step_count=16)),
        )
        for option, engine in cases:
            for days, microseconds, available in (
                (-2, 0, True),
                (-1, -1, True),
                (-1, 0, False),
                (0, 0, False),
                (1, 0, False),
                (1, 1, True),
            ):
                valuation = event + timedelta(days=days, microseconds=microseconds)
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
                    ),
                    spot_price=100,
                    valuation_time=valuation,
                    calendar=all_days_calendar(),
                )
                with self.subTest(
                    option=type(option), days=days, microseconds=microseconds
                ):
                    result = engine.price_with_greeks(option, context, ["theta"])
                    if available:
                        expected = (
                            engine.price(
                                option,
                                context.replace(
                                    valuation_time=valuation + timedelta(days=1)
                                ),
                            )
                            - engine.price(
                                option,
                                context.replace(
                                    valuation_time=valuation - timedelta(days=1)
                                ),
                            )
                        ) / 2
                        self.assertAlmostEqual(result.theta, expected, delta=1e-10)
                    else:
                        self.assertIsNone(result.theta)

    def test_european_binomial_expectations_match_full_rollback(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for steps in (1, 2, 17, 128, 255, 256, 257, 512, 1000, 1024):
            rates = (-6.4, 6.4) if steps == 1024 else (-0.2, 0.04, 0.2)
            for rate in rates:
                up = math.exp(0.2 / math.sqrt(steps))
                down = 1 / up
                probability = math.expm1(
                    rate / steps + 0.2 / math.sqrt(steps)
                ) / math.expm1(0.4 / math.sqrt(steps))
                discount = math.exp(-rate / steps)
                for direction, sign in (("call", 1), ("put", -1)):
                    with self.subTest(steps=steps, rate=rate, direction=direction):
                        values = []
                        spot = 100 * down**steps
                        for _ in range(steps + 1):
                            values.append(max(sign * (spot - 100), 0))
                            spot *= up * up
                        for level in range(steps, 0, -1):
                            values = [
                                discount
                                * (
                                    probability * values[i + 1]
                                    + (1 - probability) * values[i]
                                )
                                for i in range(level)
                            ]
                        option = EuropeanOption(
                            option_type=direction,
                            strike=100,
                            effective_date=start,
                            expiry_date=end,
                        )
                        context = PricingContext(
                            model_parameters=BlackScholesMertonParameters(
                                risk_free_rate=rate, dividend_yield=0, volatility=0.2
                            ),
                            spot_price=100,
                            valuation_time=start,
                        )
                        engine = pricing.CoxRossRubinsteinVanillaEngine(steps)
                        price = engine.price(option, context)
                        self.assertAlmostEqual(
                            price, values[0], delta=max(1e-10, abs(values[0]) * 1e-11)
                        )
                        for request in (["delta"], ["gamma"], ["delta", "gamma"]):
                            self.assertEqual(
                                engine.price_with_greeks(
                                    option, context, request
                                ).price,
                                price,
                            )

    def test_american_fd_calls_retain_early_exercise_boundaries(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": start,
            "expiry_date": end,
        }
        american, european = AmericanOption(**terms), EuropeanOption(**terms)
        engine = pricing.FiniteDifferenceVanillaEngine(
            asset_step_count=400, time_step_count=800
        )
        for rate, dividend in (
            (0, 0),
            (0.05, 0),
            (0.05, -0.02),
            (-0.05, 0),
            (0.05, 0.08),
        ):
            for spot in (100, 200):
                with self.subTest(rate=rate, dividend=dividend, spot=spot):
                    context = PricingContext(
                        model_parameters=BlackScholesMertonParameters(
                            risk_free_rate=rate, dividend_yield=dividend, volatility=0.2
                        ),
                        spot_price=spot,
                        valuation_time=start,
                    )
                    actual = engine.price_with_greeks(
                        american, context, ["delta", "gamma"]
                    )
                    expected = pricing.AnalyticVanillaEngine().price_with_greeks(
                        european, context, ["delta", "gamma"]
                    )
                    self.assertEqual(actual.price, engine.price(american, context))
                    self.assertGreaterEqual(actual.price, max(spot - 100, 0))
                    if rate >= 0 and dividend <= 0:
                        self.assertAlmostEqual(actual.price, expected.price, delta=0.02)
                        self.assertAlmostEqual(
                            actual.delta, expected.delta, delta=0.002
                        )
                        self.assertAlmostEqual(
                            actual.gamma, expected.gamma, delta=0.001
                        )
                    elif spot == 200:
                        self.assertGreater(actual.price, expected.price + 1)

    def test_binomial_expectations_recover_underflowed_tail_weights(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        steps, spot = 2048, 1e280
        strike = spot * math.exp(8)
        log_up = 0.2 / math.sqrt(steps)
        up = math.exp(log_up)
        down = 1 / up
        probability = math.expm1(0.03 / steps + log_up) / math.expm1(2 * log_up)
        discount = math.exp(-0.04 / steps)
        values = []
        node_spot = spot * down**steps
        for _ in range(steps + 1):
            values.append(max(node_spot - strike, 0))
            node_spot *= up * up
        for level in range(steps, 0, -1):
            values = [
                discount * (probability * values[i + 1] + (1 - probability) * values[i])
                for i in range(level)
            ]
        self.assertGreater(values[0], 0)
        option = EuropeanOption(
            option_type="call", strike=strike, effective_date=start, expiry_date=end
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=spot,
            valuation_time=start,
        )
        engine = pricing.CoxRossRubinsteinVanillaEngine(steps)
        price = engine.price(option, context)
        self.assertAlmostEqual(price, values[0], delta=values[0] * 1e-9)
        self.assertEqual(
            engine.price_with_greeks(option, context, ["delta", "gamma"]).price, price
        )

    def test_american_binomial_calls_preserve_early_exercise_boundaries(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": start,
            "expiry_date": end,
        }
        american, european = AmericanOption(**terms), EuropeanOption(**terms)
        for rate, dividend in (
            (0, 0),
            (0.04, 0),
            (0.04, -0.01),
            (-0.04, 0),
            (0.04, 0.08),
        ):
            for spot in (100, 200):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=rate, dividend_yield=dividend, volatility=0.2
                    ),
                    spot_price=spot,
                    valuation_time=start,
                )
                for steps in (255, 256, 257, 511, 512, 1024):
                    with self.subTest(
                        rate=rate, dividend=dividend, spot=spot, steps=steps
                    ):
                        engine = pricing.CoxRossRubinsteinVanillaEngine(steps)
                        actual = engine.price_with_greeks(
                            american, context, ["delta", "gamma"]
                        )
                        expected = engine.price_with_greeks(
                            european, context, ["delta", "gamma"]
                        )
                        self.assertEqual(actual.price, engine.price(american, context))
                        self.assertGreaterEqual(actual.price, max(spot - 100, 0))
                        if rate >= 0 and dividend <= 0:
                            self.assertAlmostEqual(
                                actual.price, expected.price, delta=1e-10
                            )
                            self.assertAlmostEqual(
                                actual.delta, expected.delta, delta=1e-10
                            )
                            self.assertAlmostEqual(
                                actual.gamma, expected.gamma, delta=1e-10
                            )
                        elif spot == 200:
                            self.assertGreater(actual.price, expected.price + 1)
                            self.assertEqual(actual.delta, 1)
                            self.assertEqual(actual.gamma, 0)

    def test_vanilla_small_volatility_tail_prices(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        # Independent 100-digit Black-Scholes evaluations of the exact float inputs.
        for direction, strike, volatility, expected in (
            ("call", 100.00000000013, 1e-13, 4.6459080591698985e-51),
            ("put", 99.99999999987, 1e-13, 4.6459080581312582e-51),
            ("call", 100.00099900499002, 1e-6, 8.2763746965085081e-29),
            ("call", 100.00100100501001, 1e-6, 6.7498304307870833e-29),
            ("put", 99.99700004499955, 1e-6, 1.6319322561070434e-203),
        ):
            option = EuropeanOption(
                option_type=direction,
                strike=strike,
                effective_date=start,
                expiry_date=end,
            )
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0, dividend_yield=0, volatility=volatility
                ),
                spot_price=100,
                valuation_time=start,
            )
            for engine in (
                pricing.AnalyticVanillaEngine(),
                pricing.QuadratureVanillaEngine(),
            ):
                with self.subTest(direction=direction, strike=strike, engine=engine):
                    value = engine.price(option, context)
                    self.assertTrue(math.isclose(value, expected, rel_tol=1e-10))
                    self.assertEqual(
                        engine.price_with_greeks(option, context, "delta").price, value
                    )

    def test_quadrature_greeks_preserve_prices_near_the_numeric_limit(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        option = EuropeanOption(
            option_type="call",
            strike=1e300,
            effective_date=start,
            expiry_date=end,
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0, dividend_yield=0, volatility=0.2
            ),
            spot_price=1e308,
            valuation_time=date(2025, 6, 1),
        )
        for greek in ("gamma", "speed", "zomma", "color"):
            with self.subTest(greek=greek):
                result = pricing.QuadratureVanillaEngine().price_with_greeks(
                    option, context, greeks=["delta", greek], spot_shift=1e306
                )
                self.assertEqual(result.price, 1e308 - 1e300)
                self.assertEqual(result.delta, 1)
                self.assertEqual(getattr(result, greek), 0)

    def test_binomial_immediate_exercise_returns_payoff_greeks(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for direction in ("put", "call"):
            put = direction == "put"
            spot = 88.0 if put else 100.0
            parameters = BlackScholesMertonParameters(
                risk_free_rate=0.15 if put else 0.0,
                dividend_yield=0.0 if put else 0.15,
                volatility=0.2,
            )
            option = AmericanOption(
                option_type=direction,
                strike=100.0 if put else 88.0,
                effective_date=start,
                expiry_date=end,
            )

            def context(value, parameters=parameters):
                return PricingContext(
                    model_parameters=parameters, spot_price=value, valuation_time=start
                )

            for steps in (1, 2, 3, 4, 5, 6, 7, 8, 10, 255, 256, 257):
                with self.subTest(direction=direction, steps=steps):
                    engine = pricing.CoxRossRubinsteinVanillaEngine(steps)
                    result = engine.price_with_greeks(
                        option, context(spot), greeks=["delta", "gamma"]
                    )
                    self.assertEqual(result.price, 12.0)
                    self.assertEqual(result.delta, -1.0 if put else 1.0)
                    self.assertEqual(result.gamma, 0.0)
                    bumped = (
                        engine.price(option, context(spot + 0.001))
                        - engine.price(option, context(spot - 0.001))
                    ) / 0.002
                    self.assertAlmostEqual(bumped, result.delta, delta=1e-10)

    def test_analytic_rho_preserves_finite_large_scale_sensitivities(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0,
                dividend_yield=0,
                volatility=0.2,
            ),
            spot_price=1e308,
            valuation_time=start,
        )
        engine = pricing.AnalyticVanillaEngine()
        for direction, expected in (
            ("call", 3.7608790690553036e306),
            ("put", -6.244600382999492e306),
        ):
            option = EuropeanOption(
                option_type=direction,
                strike=1e308,
                effective_date=start,
                expiry_date=end,
            )
            price = engine.price(option, context)
            for request in ("rho", ["rho", "vega"]):
                with self.subTest(direction=direction, request=request):
                    actual = engine.price_with_greeks(option, context, greeks=request)
                    self.assertEqual(actual.price, price)
                    self.assertAlmostEqual(actual.rho / expected, 1, delta=1e-12)

    def test_numerical_charm_preserves_large_monetary_scaling(self):
        def charm(scale):
            option = EuropeanOption(
                option_type="call",
                strike=100 * scale,
                effective_date=date(2025, 1, 1),
                expiry_date=date(2026, 1, 1),
            )
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
                ),
                spot_price=100 * scale,
                valuation_time=date(2025, 6, 1),
            )
            return price_differences(
                pricing.AnalyticVanillaEngine(),
                option,
                context,
                greeks=("charm",),
                spot_shift=70 * scale,
            ).charm

        baseline = charm(1.0)
        self.assertGreater(abs(baseline), 1e-5)
        self.assertTrue(math.isclose(charm(1e306), baseline, rel_tol=1e-9))

    def test_scheduled_fd_greeks_preserve_scalar_stencils_and_failures(self):
        start, end = date(2025, 1, 1), date(2035, 1, 1)
        dates = [
            start + timedelta(days=offset)
            for offset in range((end - start).days + 1)
            if (start + timedelta(days=offset)).weekday() < 5
        ]
        valuation = datetime(2034, 12, 31, 12, tzinfo=UTC)

        def context(volatility=0.2, rate=0.04):
            return PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=0.01, volatility=volatility
                ),
                spot_price=100.25,
                valuation_time=valuation,
            )

        market = context()
        vol_shift = (0.2 + 0.002) - 0.2
        rate_shift = (0.04 + 0.001) - 0.04
        for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
            engine = pricing.FiniteDifferenceBarrierEngine(
                asset_step_count=80, time_step_count=1, scheme=scheme
            )
            for kind in ("down_and_out", "down_and_in", "up_and_out", "up_and_in"):
                with self.subTest(scheme=scheme, kind=kind):
                    terms = {
                        "option_type": "call",
                        "strike": 100,
                        "effective_date": start,
                        "expiry_date": end,
                        "barrier_level": 120 if kind.startswith("up") else 80,
                        "barrier_type": kind,
                        "observation_mode": "scheduled",
                        "observation_dates": dates,
                        "touch_state": "untouched",
                    }
                    option = BarrierOption(**terms)
                    result = engine.price_with_greeks(
                        option,
                        market,
                        ["vega", "rho"],
                        volatility_shift=0.002,
                        rate_shift=0.001,
                    )
                    self.assertEqual(result.price, engine.price(option, market))
                    self.assertEqual(
                        result.vega,
                        (
                            engine.price(option, context(0.2 + vol_shift))
                            - engine.price(option, context(0.2 - vol_shift))
                        )
                        / (200 * vol_shift),
                    )
                    self.assertEqual(
                        result.rho,
                        (
                            engine.price(option, context(rate=0.04 + rate_shift))
                            - engine.price(option, context(rate=0.04 - rate_shift))
                        )
                        / (200 * rate_shift),
                    )
                    self.assertIsNone(result.delta)
                    if scheme == "explicit_euler":
                        with self.assertRaises(kiyosi.KiyosiError) as unstable:
                            engine.price_with_greeks(
                                option, market, ["vega"], volatility_shift=0.19
                            )
                        self.assertEqual(
                            unstable.exception.category,
                            kiyosi.ErrorCategory.INVALID_PARAMETER,
                        )
                    invalid = BarrierOption(
                        **dict(terms, observation_dates=[date(2025, 1, 4), end])
                    )
                    with self.assertRaises(kiyosi.KiyosiError) as rejected:
                        engine.price_with_greeks(invalid, market, ["vega", "rho"])
                    self.assertEqual(
                        rejected.exception.category, kiyosi.ErrorCategory.INVALID_DATE
                    )
                    recovered = engine.price_with_greeks(
                        option,
                        market,
                        ["vega", "rho"],
                        volatility_shift=0.002,
                        rate_shift=0.001,
                    )
                    self.assertEqual(
                        (recovered.price, recovered.vega, recovered.rho),
                        (result.price, result.vega, result.rho),
                    )
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(
                pool.map(
                    lambda _: engine.price_with_greeks(
                        option,
                        market,
                        ["vega", "rho"],
                        volatility_shift=0.002,
                        rate_shift=0.001,
                    ),
                    range(8),
                )
            )
        self.assertEqual(
            [(value.price, value.vega, value.rho) for value in results],
            [(result.price, result.vega, result.rho)] * 8,
        )

    def test_shared_analytic_barriers_preserve_prices_and_errors_across_threads(self):
        start, end = date(2015, 1, 1), date(2025, 1, 1)
        dates = [
            start + timedelta(days=day)
            for day in range(1, (end - start).days + 1)
            if (start + timedelta(days=day)).weekday() < 5
        ]
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        terms = {
            "option_type": "call",
            "strike": 100,
            "effective_date": start,
            "expiry_date": end,
            "barrier_level": 80,
            "barrier_type": "down_and_out",
        }
        for family, continuous, scheduled in (
            ("barrier", 23.200671753767061, 23.628841998021954),
            ("binary", 2.0207068648871727, 2.0735108817276178),
            ("touch", 4.576604119557576, 4.514156543575421),
        ):
            engine = (
                pricing.AnalyticBarrierEngine()
                if family == "barrier"
                else pricing.AnalyticBinaryBarrierEngine()
            )
            cases = []
            for mode, observations, expected in (
                ("continuous", [], continuous),
                ("scheduled", dates, scheduled),
                ("scheduled", [date(2015, 1, 3)], None),
            ):
                monitoring = {
                    "observation_mode": mode,
                    "observation_dates": observations,
                }
                if family == "barrier":
                    option = BarrierOption(**terms, **monitoring)
                elif family == "binary":
                    option = cash_binary_barrier_option(
                        **terms, **monitoring, payout=10
                    )
                else:
                    option = cash_one_touch_down(
                        effective_date=start,
                        expiry_date=end,
                        barrier_level=80,
                        payout=10,
                        **monitoring,
                    )
                cases.append((option, expected))
            with self.subTest(family=family):
                with ThreadPoolExecutor(max_workers=4) as executor:
                    tasks = [
                        (executor.submit(engine.price, option, context), expected)
                        for option, expected in cases * 20
                    ]
                    for task, expected in tasks:
                        if expected is None:
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                task.result()
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.INVALID_DATE,
                            )
                        else:
                            self.assertAlmostEqual(task.result(), expected, places=12)
                self.assertAlmostEqual(
                    engine.price(cases[1][0], context), scheduled, places=12
                )

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
            expected = [
                engine.price_with_greeks(option, context, all_greeks=True)
                for option in options
            ]
            tasks = [
                executor.submit(
                    engine.price_with_greeks, option, context, all_greeks=True
                )
                for option in options * 50
            ]
            for index, task in enumerate(tasks):
                actual, reference = task.result(), expected[index % 2]
                self.assertEqual(actual.price, reference.price)
                self.assertEqual(
                    tuple(getattr(actual, name) for name in names),
                    tuple(getattr(reference, name) for name in names),
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
        # Fixed-grid references include CN startup damping.
        cases = (
            (
                "crank_nicolson",
                "at_hit",
                6.001294270333644,
                -0.46100321142270584,
                0.012094206387263214,
                -0.0007201663153294298,
            ),
            (
                "crank_nicolson",
                "at_expiry",
                5.9654519235374055,
                -0.46428364164012226,
                0.011885181449706947,
                -0.0007257305720597129,
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

    def test_accumulator_mc_selected_greeks_match_all_requests(self):
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
                            expected = engine.price_with_greeks(
                                option, context, all_greeks=True, spot_shift=0.2
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
        observation_dates = [end]
        knock_out_levels = [110]
        ladder = {**life}
        coupons = {
            "maturity_coupon_rate": 0.06,
            "observations": autocallable_observations(
                observation_dates, knock_out_levels, knock_out_coupon_rates=[0.08]
            ),
        }
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
                pricing.FiniteDifferenceAutocallableEngine,
                110,
            ),
            (
                BinarySnowballOption(**coupons, **ladder),
                pricing.FiniteDifferenceAutocallableEngine,
                110,
            ),
            (
                TernarySnowballOption(
                    minimum_coupon_rate=0, **coupons, **knock_in, **ladder
                ),
                pricing.FiniteDifferenceAutocallableEngine,
                110,
            ),
            (
                PhoenixOption(
                    coupon_rate=0.08,
                    **strikes,
                    **knock_in,
                    **ladder,
                    observations=autocallable_observations(
                        observation_dates, knock_out_levels, coupon_barrier_levels=[90]
                    ),
                ),
                pricing.FiniteDifferenceAutocallableEngine,
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
        result = engine.price_with_greeks(option, context, ["speed"], spot_shift=0.25)
        self.assertEqual(result.price, engine.price(option, context))
        shifted = [
            PricingContext(
                model_parameters=parameters,
                spot_price=context.spot_price + shift,
                valuation_time=start,
            )
            for shift in (-0.25, 0.25)
        ]
        low, high = [
            engine.price_with_greeks(option, market, ["gamma"]).gamma
            for market in shifted
        ]
        self.assertAlmostEqual(result.speed, (high - low) / 0.5, delta=1e-12)
        volatile_context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=10
            ),
            spot_price=399,
            valuation_time=start,
        )
        coarse = pricing.FiniteDifferenceDigitalEngine(
            asset_step_count=1000, time_step_count=3, asset_upper_boundary=400
        )
        self.assertTrue(math.isfinite(coarse.price(option, volatile_context)))
        with self.assertRaises(kiyosi.KiyosiError) as rejected:
            coarse.price_with_greeks(
                option, volatile_context, ["speed"], spot_shift=0.25
            )
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
            ("crank_nicolson", 200, -2160.414826198844),
            ("implicit_euler", 200, -2157.150729478969),
            ("explicit_euler", 40, -2248.038835478244),
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
            (99.99, 9.25238419180772),
            (math.nextafter(100, 0), 9.258315258670022),
            (100, 9.258315258670033),
            (math.nextafter(100, math.inf), 9.258315258670043),
            (100.01, 9.264248271960529),
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
                    path_count=19999, step_count=2, seed=42
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
            engine = pricing.MonteCarloVanillaEngine(
                path_count=16, step_count=2, seed=42, backend=backend
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
            maturity_coupon_rate=0,
            initial_spot=100,
            knock_in_level=75,
            upper_strike=100,
            lower_strike=0,
            knock_in_observation_mode="every_trading_day",
            barrier_state="knocked_in",
            effective_date=start,
            expiry_date=end,
            observations=autocallable_observations(
                [end], [200], knock_out_coupon_rates=[0]
            ),
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
                pricing.FiniteDifferenceAutocallableEngine(
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
