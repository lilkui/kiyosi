import math
import unittest
from datetime import UTC, date, datetime, timedelta

import kiyosi
from kiyosi import pricing
from kiyosi.instruments import (
    AssetOrNothingOption,
    BarrierOption,
    CashOrNothingOption,
    EuropeanOption,
    asset_binary_barrier_option,
    asset_no_touch_down,
    asset_no_touch_up,
    asset_one_touch_down,
    asset_one_touch_up,
    cash_binary_barrier_option,
    cash_no_touch_down,
    cash_no_touch_up,
    cash_one_touch_down,
    cash_one_touch_up,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)
from price_differences import price_differences


class AnalyticBarrierDomainTests(unittest.TestCase):
    def test_scheduled_barrier_prices_survive_shifted_level_overflow(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        observations = [date(2025, 4, 1), date(2025, 7, 1), date(2025, 10, 1), end]
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=1
        )
        references = {}
        for scale in (1, 1e306):
            context = PricingContext(
                model_parameters=parameters,
                spot_price=100 * scale,
                valuation_time=start,
            )
            common = {
                "effective_date": start,
                "expiry_date": end,
                "barrier_level": 170 * scale,
                "observation_mode": "scheduled",
                "observation_dates": observations,
            }
            engine = pricing.AnalyticBinaryBarrierEngine()
            for factory, expected in (
                (cash_one_touch_up, 0.25739250301126404),
                (cash_no_touch_up, 0.742607496988736),
            ):
                with self.subTest(scale=scale, factory=factory.__name__):
                    extra = (
                        {"settlement_timing": "at_expiry"}
                        if factory is cash_one_touch_up
                        else {}
                    )
                    self.assertAlmostEqual(
                        engine.price(factory(**common, payout=1, **extra), context),
                        expected,
                        delta=1e-12,
                    )
            for direction in ("call", "put"):
                for barrier_type in ("up_and_in", "up_and_out"):
                    terms = dict(
                        common,
                        option_type=direction,
                        strike=175 * scale,
                        barrier_type=barrier_type,
                    )
                    for engine, option, divisor in (
                        (
                            pricing.AnalyticBarrierEngine(),
                            BarrierOption(**terms),
                            scale,
                        ),
                        (
                            pricing.AnalyticBinaryBarrierEngine(),
                            cash_binary_barrier_option(**terms, payout=1),
                            1,
                        ),
                    ):
                        key = (direction, barrier_type, type(engine).__name__)
                        with self.subTest(scale=scale, key=key):
                            actual = engine.price(option, context) / divisor
                            if scale == 1:
                                references[key] = actual
                            else:
                                self.assertAlmostEqual(
                                    actual, references[key], delta=1e-10
                                )

    def test_scheduled_analytic_barriers_preserve_excluded_terminal_payoffs(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.03, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100,
            valuation_time=start,
        )
        for up, direction, barrier in ((True, "call", 110), (False, "put", 90)):
            for strike in (barrier, barrier + (5 if up else -5)):
                vanilla_terms = {
                    "option_type": direction,
                    "strike": strike,
                    "effective_date": start,
                    "expiry_date": end,
                }
                for knock_in in (False, True):
                    terms = dict(
                        vanilla_terms,
                        barrier_level=barrier,
                        barrier_type=("up" if up else "down")
                        + "_and_"
                        + ("in" if knock_in else "out"),
                        observation_mode="scheduled",
                        observation_dates=[end],
                    )
                    for factory, vanilla in (
                        (
                            cash_binary_barrier_option,
                            CashOrNothingOption(**vanilla_terms, payout=10),
                        ),
                        (
                            asset_binary_barrier_option,
                            AssetOrNothingOption(**vanilla_terms),
                        ),
                    ):
                        binary = factory(
                            **terms,
                            **(
                                {"payout": 10}
                                if factory is cash_binary_barrier_option
                                else {}
                            ),
                        )
                        expected = (
                            pricing.AnalyticDigitalEngine().price(vanilla, context)
                            if knock_in
                            else 0
                        )
                        with self.subTest(
                            direction=direction,
                            strike=strike,
                            knock_in=knock_in,
                            factory=factory,
                        ):
                            self.assertEqual(
                                pricing.AnalyticBinaryBarrierEngine().price(
                                    binary, context
                                ),
                                expected,
                            )
                    for rebate in (0, 3):
                        expected = (
                            pricing.AnalyticVanillaEngine().price(
                                EuropeanOption(**vanilla_terms), context
                            )
                            if knock_in
                            else 0
                        )
                        if rebate:
                            factory = (
                                (cash_no_touch_up if knock_in else cash_one_touch_up)
                                if up
                                else (
                                    cash_no_touch_down
                                    if knock_in
                                    else cash_one_touch_down
                                )
                            )
                            touch = factory(
                                effective_date=start,
                                expiry_date=end,
                                barrier_level=barrier,
                                payout=rebate,
                                observation_mode="scheduled",
                                observation_dates=[end],
                            )
                            expected += pricing.AnalyticBinaryBarrierEngine().price(
                                touch, context
                            )
                        with self.subTest(
                            direction=direction,
                            strike=strike,
                            knock_in=knock_in,
                            rebate=rebate,
                        ):
                            self.assertAlmostEqual(
                                pricing.AnalyticBarrierEngine().price(
                                    BarrierOption(**terms, rebate=rebate), context
                                ),
                                expected,
                                delta=1e-12,
                            )

    def test_small_volatility_barriers_with_nonzero_carry(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for sigma in (1e-160, 1e-200):
            for carry in (-0.3, -0.02, 0.02, 0.3):
                terminal = 100 * math.exp(carry)
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.03,
                        dividend_yield=0.03 - carry,
                        volatility=sigma,
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                for up in (False, True):
                    touched = terminal >= 120 if up else terminal <= 80
                    for knock_in in (False, True):
                        for direction in ("call", "put"):
                            terms = {
                                "option_type": direction,
                                "strike": 100,
                                "effective_date": start,
                                "expiry_date": end,
                                "barrier_level": 120 if up else 80,
                                "barrier_type": ("up" if up else "down")
                                + ("_and_in" if knock_in else "_and_out"),
                            }
                            payoff = max(
                                (1 if direction == "call" else -1) * (terminal - 100),
                                0,
                            )
                            exercised = (
                                terminal > 100
                                if direction == "call"
                                else terminal < 100
                            )
                            contracts = (
                                (
                                    pricing.AnalyticBarrierEngine(),
                                    BarrierOption(
                                        **terms, rebate=7, rebate_timing="at_expiry"
                                    ),
                                    (payoff if knock_in == touched else 7)
                                    * math.exp(-0.03),
                                ),
                                (
                                    pricing.AnalyticBinaryBarrierEngine(),
                                    cash_binary_barrier_option(**terms, payout=10),
                                    10 * math.exp(-0.03)
                                    if knock_in == touched and exercised
                                    else 0,
                                ),
                                (
                                    pricing.AnalyticBinaryBarrierEngine(),
                                    asset_binary_barrier_option(**terms),
                                    terminal * math.exp(-0.03)
                                    if knock_in == touched and exercised
                                    else 0,
                                ),
                            )
                            for engine, option, expected in contracts:
                                with self.subTest(
                                    sigma=sigma,
                                    carry=carry,
                                    terms=terms,
                                    engine=type(engine).__name__,
                                    option=type(option).__name__,
                                ):
                                    self.assertAlmostEqual(
                                        engine.price(option, context),
                                        expected,
                                        delta=1e-11,
                                    )

    def test_reflected_barrier_tail_survives_overflowing_exponent(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        carry = math.log(2)
        option = cash_binary_barrier_option(
            option_type="put",
            strike=1,
            effective_date=start,
            expiry_date=end,
            barrier_level=1,
            barrier_type="up_and_in",
            payout=1e300,
        )
        for sigma in (1e-160, 1e-200):
            context = PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=carry,
                    dividend_yield=0,
                    volatility=sigma,
                ),
                spot_price=0.5,
                valuation_time=start,
            )
            expected = (1e300 * sigma) / (4 * carry * math.sqrt(2 * math.pi))
            with self.subTest(sigma=sigma):
                actual = pricing.AnalyticBinaryBarrierEngine().price(option, context)
                self.assertAlmostEqual(actual / expected, 1, delta=1e-12)

    def test_small_volatility_hit_rebates_and_touch_payments(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for sigma in (1e-160, 1e-200):
            for carry in (-0.3, -0.02, 0.02, 0.3):
                terminal = 100 * math.exp(carry)
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=0.03,
                        dividend_yield=0.03 - carry,
                        volatility=sigma,
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                for up in (False, True):
                    barrier = 120 if up else 80
                    crossed = terminal >= barrier if up else terminal <= barrier
                    expected = (
                        7 * math.exp(-0.03 * math.log(barrier / 100) / carry)
                        if crossed
                        else 0
                    )
                    touch = (cash_one_touch_up if up else cash_one_touch_down)(
                        effective_date=start,
                        expiry_date=end,
                        barrier_level=barrier,
                        payout=7,
                        settlement_timing="at_hit",
                    )
                    option = BarrierOption(
                        option_type="call" if up else "put",
                        strike=barrier,
                        effective_date=start,
                        expiry_date=end,
                        barrier_level=barrier,
                        barrier_type="up_and_out" if up else "down_and_out",
                        rebate=7,
                        rebate_timing="at_hit",
                    )
                    for engine, instrument in (
                        (pricing.AnalyticBinaryBarrierEngine(), touch),
                        (pricing.AnalyticBarrierEngine(), option),
                    ):
                        with self.subTest(
                            sigma=sigma,
                            carry=carry,
                            up=up,
                            engine=type(engine).__name__,
                        ):
                            self.assertAlmostEqual(
                                engine.price(instrument, context), expected, delta=1e-11
                            )

    def test_equivalent_hit_rebates_and_touch_payments_share_discount_validation(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        for up in (False, True):
            level = 120 if up else 80
            barrier = BarrierOption(
                option_type="call" if up else "put",
                strike=level,
                effective_date=start,
                expiry_date=end,
                barrier_level=level,
                barrier_type="up_and_out" if up else "down_and_out",
                rebate=7,
                rebate_timing="at_hit",
            )
            touch = (cash_one_touch_up if up else cash_one_touch_down)(
                effective_date=start,
                expiry_date=end,
                barrier_level=level,
                payout=7,
                settlement_timing="at_hit",
            )
            for dividend in (-0.04, -0.08, -0.08000000000000002, -0.1):
                context = PricingContext(
                    model_parameters=BlackScholesMertonParameters(
                        risk_free_rate=-0.02, dividend_yield=dividend, volatility=0.2
                    ),
                    spot_price=100,
                    valuation_time=start,
                )
                with self.subTest(up=up, dividend=dividend):
                    contracts = (
                        (pricing.AnalyticBarrierEngine(), barrier),
                        (pricing.AnalyticBinaryBarrierEngine(), touch),
                    )
                    if dividend == -0.1:
                        prices = [
                            engine.price(option, context)
                            for engine, option in contracts
                        ]
                        self.assertAlmostEqual(*prices, delta=1e-12)
                        self.assertGreater(prices[1], 0)
                    else:
                        for engine, option in contracts:
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                engine.price(option, context)
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.INVALID_RESULT,
                            )

    def test_dense_scheduled_greeks_preserve_independent_stencils(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        common = {
            "effective_date": start,
            "expiry_date": end,
            "barrier_level": 80,
            "observation_mode": "scheduled",
            "touch_state": "untouched",
            "observation_dates": [start + timedelta(days=i) for i in range(1, 366)],
        }
        terms = dict(
            common, option_type="call", strike=100, barrier_type="down_and_out"
        )
        context = PricingContext(
            model_parameters=BlackScholesMertonParameters(
                risk_free_rate=0.04, dividend_yield=0.01, volatility=0.2
            ),
            spot_price=100.25,
            valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
            calendar=all_days_calendar(),
        )
        contracts = [
            (pricing.AnalyticBarrierEngine(), BarrierOption(**terms)),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                cash_binary_barrier_option(**terms, payout=10),
            ),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                asset_binary_barrier_option(**terms),
            ),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                cash_no_touch_down(**common, payout=10),
            ),
            (
                pricing.AnalyticBinaryBarrierEngine(),
                asset_one_touch_down(**common, settlement_timing="at_expiry"),
            ),
        ]
        greeks = (
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
        for engine, option in contracts:
            with self.subTest(option=type(option).__name__):
                time_greeks = ("theta", "charm", "color")
                expected = price_differences(
                    engine,
                    option,
                    context,
                    greeks=tuple(greek for greek in greeks if greek not in time_greeks),
                )
                actual = engine.price_with_greeks(option, context, greeks)
                self.assertEqual(actual.price, engine.price(option, context))
                for greek in greeks:
                    if greek in time_greeks:
                        self.assertIsNone(getattr(actual, greek))
                    else:
                        self.assertEqual(
                            getattr(actual, greek), getattr(expected, greek)
                        )
                solved = pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    actual.price,
                    lower_bound=0.15,
                    upper_bound=0.25,
                )
                self.assertAlmostEqual(solved, 0.2, delta=1e-7)
                with self.assertRaises(kiyosi.KiyosiError) as invalid:
                    pricing.implied_volatility(
                        engine,
                        option,
                        context,
                        actual.price,
                        lower_bound=0,
                    )
                self.assertEqual(
                    invalid.exception.category, kiyosi.ErrorCategory.INVALID_PARAMETER
                )

    def test_binary_barriers_survive_volatility_variance_underflow(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        engine = pricing.AnalyticBinaryBarrierEngine()
        for sigma in (1e-160, 1e-200):
            for rate in (0.0, 0.05):
                parameters = BlackScholesMertonParameters(
                    risk_free_rate=rate, dividend_yield=rate, volatility=sigma
                )
                for spot in (90, 100, 110):
                    context = PricingContext(
                        model_parameters=parameters,
                        spot_price=spot,
                        valuation_time=start,
                    )
                    for direction in ("call", "put"):
                        probability = (
                            0.5
                            if spot == 100
                            else float(
                                spot > 100 if direction == "call" else spot < 100
                            )
                        )
                        for up in (False, True):
                            for knock_in in (False, True):
                                terms = {
                                    "option_type": direction,
                                    "strike": 100,
                                    "effective_date": start,
                                    "expiry_date": end,
                                    "barrier_level": 120 if up else 80,
                                    "barrier_type": ("up" if up else "down")
                                    + ("_and_in" if knock_in else "_and_out"),
                                }
                                for asset in (False, True):
                                    with self.subTest(
                                        sigma=sigma,
                                        rate=rate,
                                        spot=spot,
                                        direction=direction,
                                        up=up,
                                        knock_in=knock_in,
                                        asset=asset,
                                    ):
                                        option = (
                                            asset_binary_barrier_option(**terms)
                                            if asset
                                            else cash_binary_barrier_option(
                                                **terms, payout=10
                                            )
                                        )
                                        expected = (
                                            0
                                            if knock_in
                                            else (
                                                (spot if asset else 10)
                                                * math.exp(-rate)
                                                * probability
                                            )
                                        )
                                        price = engine.price(option, context)
                                        self.assertAlmostEqual(
                                            price, expected, delta=1e-11
                                        )
                                        self.assertEqual(
                                            engine.price_with_greeks(
                                                option, context, "delta"
                                            ).price,
                                            price,
                                        )

    def test_scheduled_analytic_barriers_reject_spots_outside_shifted_domain(self):
        start, end = date(2025, 1, 1), date(2026, 1, 1)
        parameters = BlackScholesMertonParameters(
            risk_free_rate=0.05, dividend_yield=0.02, volatility=0.2
        )
        for up in (True, False):
            context = PricingContext(
                model_parameters=parameters,
                spot_price=150 if up else 50,
                valuation_time=datetime(2025, 1, 1, 12, tzinfo=UTC),
                calendar=all_days_calendar(),
            )
            common = {
                "effective_date": start,
                "expiry_date": end,
                "barrier_level": 120 if up else 80,
                "observation_mode": "scheduled",
                "observation_dates": [end],
            }
            for knock_in in (True, False):
                terms = dict(
                    common,
                    option_type="call",
                    strike=100,
                    barrier_type=("up" if up else "down")
                    + ("_and_in" if knock_in else "_and_out"),
                )
                contracts = [
                    (pricing.AnalyticBarrierEngine(), BarrierOption(**terms)),
                    (
                        pricing.AnalyticBinaryBarrierEngine(),
                        cash_binary_barrier_option(**terms, payout=10),
                    ),
                    (
                        pricing.AnalyticBinaryBarrierEngine(),
                        asset_binary_barrier_option(**terms),
                    ),
                ]
                if knock_in:
                    cash = cash_one_touch_up if up else cash_one_touch_down
                    asset = asset_one_touch_up if up else asset_one_touch_down
                    for timing in ("at_hit", "at_expiry"):
                        contracts.extend(
                            [
                                (
                                    pricing.AnalyticBinaryBarrierEngine(),
                                    cash(**common, payout=10, settlement_timing=timing),
                                ),
                                (
                                    pricing.AnalyticBinaryBarrierEngine(),
                                    asset(**common, settlement_timing=timing),
                                ),
                            ]
                        )
                else:
                    cash = cash_no_touch_up if up else cash_no_touch_down
                    asset = asset_no_touch_up if up else asset_no_touch_down
                    contracts.extend(
                        [
                            (
                                pricing.AnalyticBinaryBarrierEngine(),
                                cash(**common, payout=10),
                            ),
                            (pricing.AnalyticBinaryBarrierEngine(), asset(**common)),
                        ]
                    )
                for engine, option in contracts:
                    with self.subTest(up=up, knock_in=knock_in, option=repr(option)):
                        for evaluate, arguments in (
                            (engine.price, (option, context)),
                            (engine.price_with_greeks, (option, context, "delta")),
                        ):
                            with self.assertRaises(kiyosi.KiyosiError) as error:
                                evaluate(*arguments)
                            self.assertEqual(
                                error.exception.category,
                                kiyosi.ErrorCategory.UNSUPPORTED_OPERATION,
                            )
