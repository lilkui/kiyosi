import math
import unittest
from datetime import date, timedelta
from itertools import product

from kiyosi import pricing
from kiyosi.instruments import (
    AssetOrNothingOption,
    BarrierOption,
    CashOrNothingOption,
    EuropeanOption,
)
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    all_days_calendar,
)

START, END = date(2025, 1, 1), date(2026, 1, 1)
MEASURES = ("price", "delta", "gamma")


def context(spot=100.25, *, deterministic=False):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=0.04,
            dividend_yield=0.04 if deterministic else 0.01,
            volatility=1e-12 if deterministic else 0.2,
        ),
        spot_price=spot,
        valuation_time=START,
        calendar=all_days_calendar(),
    )


def option(up, barrier, *, dates=(END,), timing="at_expiry", knock_in=False):
    return BarrierOption(
        option_type="call" if up else "put",
        strike=100,
        effective_date=START,
        expiry_date=END,
        barrier_level=barrier,
        barrier_type=f"{'up' if up else 'down'}_and_{'in' if knock_in else 'out'}",
        rebate=2,
        rebate_timing=timing,
        observation_mode="scheduled",
        observation_dates=dates,
    )


def terminal_reference(spot, up, barrier, time):
    """Independent cash/asset binary decomposition of the truncated payoff."""
    sign = 1 if up else -1
    deviation = 0.2 * math.sqrt(time)

    def components(level):
        d1 = (math.log(spot / level) + 0.05 * time) / deviation
        asset = (
            spot * math.exp(-0.01 * time) * 0.5 * math.erfc(-sign * d1 / math.sqrt(2))
        )
        cash = (
            math.exp(-0.04 * time)
            * 0.5
            * math.erfc(-sign * (d1 - deviation) / math.sqrt(2))
        )
        return asset, cash

    strike_asset, strike_cash = components(100)
    barrier_asset, barrier_cash = components(barrier)
    return (
        sign * (strike_asset - 100 * strike_cash - barrier_asset + 100 * barrier_cash)
        + 2 * barrier_cash
    )


def two_observation_reference(spot, up, barrier, timing, count=1024):
    """Integrate analytic continuation over the surviving first fixing states."""
    first = 180 / 365
    deviation = 0.2 * math.sqrt(first)
    drift = 0.01 * first
    cut = (math.log(barrier / spot) - drift) / deviation
    lower, upper = (-10, cut) if up else (cut, 10)
    width = (upper - lower) / count

    def integrand(z):
        asset = spot * math.exp(drift + deviation * z)
        return (
            terminal_reference(asset, up, barrier, 1 - first)
            * math.exp(-0.5 * z * z)
            / math.sqrt(2 * math.pi)
        )

    integral = integrand(lower) + integrand(upper)
    for index in range(1, count):
        integral += (4 if index % 2 else 2) * integrand(lower + index * width)
    touched = 0.5 * math.erfc((cut if up else -cut) / math.sqrt(2))
    return (
        math.exp(-0.04 * first) * width * integral / 3
        + 2 * math.exp(-0.04 * (first if timing == "at_hit" else 1)) * touched
    )


class BarrierFiniteDifferenceEventTests(unittest.TestCase):
    def test_terminal_projection_converges_across_grid_placements(self):
        market = context()
        for up in (False, True):
            direction = "call" if up else "put"
            common = {
                "option_type": direction,
                "effective_date": START,
                "expiry_date": END,
            }
            vanilla = pricing.AnalyticVanillaEngine().price_with_greeks(
                EuropeanOption(strike=100, **common), market, ["delta", "gamma"]
            )
            previous = [1.0] * 3
            for count in (200, 400, 800):
                engine = pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=count,
                    time_step_count=2000,
                    asset_upper_boundary=400,
                )
                worst = [0.0] * 3
                for offset in (-0.25, 0, 0.125, 0.25, 0.5):
                    barrier = (110 if up else 90) + offset
                    cash = pricing.AnalyticDigitalEngine().price_with_greeks(
                        CashOrNothingOption(strike=barrier, payout=1, **common),
                        market,
                        ["delta", "gamma"],
                    )
                    asset = pricing.AnalyticDigitalEngine().price_with_greeks(
                        AssetOrNothingOption(strike=barrier, **common),
                        market,
                        ["delta", "gamma"],
                    )
                    contract = option(up, barrier)
                    actual = engine.price_with_greeks(
                        contract, market, ["delta", "gamma"]
                    )
                    self.assertEqual(actual.price, engine.price(contract, market))
                    for index, measure in enumerate(MEASURES):
                        expected = (
                            getattr(vanilla, measure)
                            - (1 if up else -1)
                            * (getattr(asset, measure) - 100 * getattr(cash, measure))
                            + 2 * getattr(cash, measure)
                        )
                        worst[index] = max(
                            worst[index], abs(getattr(actual, measure) - expected)
                        )
                refinement = 200 / count
                limits = (
                    0.012 * refinement**2,
                    0.00013 * refinement,
                    0.00004 * refinement**2,
                )
                for index, measure in enumerate(MEASURES):
                    with self.subTest(up=up, count=count, measure=measure):
                        self.assertLess(worst[index], limits[index])
                        self.assertLess(worst[index], previous[index])
                previous = worst

    def test_projection_preserves_large_finite_rebates(self):
        engine = pricing.FiniteDifferenceBarrierEngine(
            asset_step_count=40,
            time_step_count=2000,
            scheme="implicit_euler",
            asset_upper_boundary=400,
        )
        market = context()

        def price(rebate):
            contract = BarrierOption(
                option_type="call",
                strike=100,
                effective_date=START,
                expiry_date=END,
                barrier_level=110,
                barrier_type="up_and_out",
                rebate=rebate,
                observation_mode="scheduled",
                observation_dates=[END],
            )
            return engine.price(contract, market)

        self.assertAlmostEqual(price(1e308) / 1e308, price(1) - price(0), delta=1e-12)

    def test_restarts_match_implicit_half_steps_without_extra_observations(self):
        end = START + timedelta(days=360)
        for up, timing in product((False, True), ("at_hit", "at_expiry")):
            contract = BarrierOption(
                option_type="call" if up else "put",
                strike=100,
                effective_date=START,
                expiry_date=end,
                barrier_level=110.125 if up else 90.125,
                barrier_type="up_and_out" if up else "down_and_out",
                rebate=2,
                rebate_timing=timing,
                observation_mode="scheduled",
                observation_dates=[
                    START + timedelta(days=i) for i in (90, 180, 270, 360)
                ],
            )
            results = [
                pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=200,
                    time_step_count=steps,
                    scheme=scheme,
                    asset_upper_boundary=400,
                ).price_with_greeks(contract, context(), ["delta", "gamma"])
                for scheme, steps in (("crank_nicolson", 4), ("implicit_euler", 8))
            ]
            for measure in MEASURES:
                with self.subTest(up=up, timing=timing, measure=measure):
                    self.assertAlmostEqual(
                        getattr(results[0], measure),
                        getattr(results[1], measure),
                        delta=1e-12,
                    )

    def test_two_observations_match_independent_integration(self):
        market = context()
        for up, timing in product((False, True), ("at_hit", "at_expiry")):
            barrier = 110.125 if up else 90.125
            expected = two_observation_reference(100.25, up, barrier, timing)
            self.assertAlmostEqual(
                expected,
                two_observation_reference(100.25, up, barrier, timing, 2048),
                delta=1e-9,
            )
            h = 0.025
            low = two_observation_reference(100.25 - h, up, barrier, timing)
            high = two_observation_reference(100.25 + h, up, barrier, timing)
            reference = (
                expected,
                (high - low) / (2 * h),
                (high - 2 * expected + low) / h**2,
            )
            # Check the independent derivative stencil before using it as a reference.
            low = two_observation_reference(100.25 - h / 2, up, barrier, timing)
            high = two_observation_reference(100.25 + h / 2, up, barrier, timing)
            self.assertAlmostEqual(reference[1], (high - low) / h, delta=1e-7)
            self.assertAlmostEqual(
                reference[2], (high - 2 * expected + low) / (h / 2) ** 2, delta=1e-8
            )
            contract = option(
                up, barrier, dates=(START + timedelta(days=180), END), timing=timing
            )
            for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
                count = 200 if scheme == "explicit_euler" else 800
                actual = pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=count,
                    time_step_count=2000,
                    scheme=scheme,
                    asset_upper_boundary=400,
                ).price_with_greeks(contract, market, ["delta", "gamma"])
                for measure, value, limit in zip(
                    MEASURES, reference, (0.0008, 0.00003, 0.000003), strict=True
                ):
                    with self.subTest(
                        up=up, timing=timing, scheme=scheme, measure=measure
                    ):
                        self.assertAlmostEqual(
                            getattr(actual, measure),
                            value,
                            delta=limit * (800 / count) ** 2,
                        )

                if scheme == "crank_nicolson":
                    previous = [math.inf] * 3
                    for steps in (16, 64, 256):
                        refined = pricing.FiniteDifferenceBarrierEngine(
                            asset_step_count=800,
                            time_step_count=steps,
                            asset_upper_boundary=400,
                        ).price_with_greeks(contract, market, ["delta", "gamma"])
                        errors = [
                            abs(getattr(refined, measure) - getattr(actual, measure))
                            for measure in MEASURES
                        ]
                        for measure, error, prior in zip(
                            MEASURES, errors, previous, strict=True
                        ):
                            with self.subTest(
                                up=up, timing=timing, steps=steps, measure=measure
                            ):
                                self.assertLess(error, prior)
                        previous = errors
                    for error, limit in zip(previous, (2e-6, 5e-7, 3e-8), strict=True):
                        self.assertLess(error, limit)

    def test_current_observations_use_actual_spot_before_projection(self):
        engine = pricing.FiniteDifferenceBarrierEngine(
            asset_step_count=200, time_step_count=2000, asset_upper_boundary=400
        )
        for up, knock_in in product((False, True), repeat=2):
            barrier = 110.125 if up else 90.125
            future = option(up, barrier, knock_in=knock_in)
            current = option(up, barrier, dates=(START, END), knock_in=knock_in)
            for spot in (
                math.nextafter(barrier, 0 if up else math.inf),
                barrier,
                math.nextafter(barrier, math.inf if up else 0),
            ):
                market = context(spot)
                touched = spot >= barrier if up else spot <= barrier
                actual = engine.price_with_greeks(current, market, ["delta", "gamma"])
                with self.subTest(up=up, knock_in=knock_in, spot=spot):
                    if touched:
                        expected = (
                            pricing.AnalyticVanillaEngine().price(
                                EuropeanOption(
                                    option_type="call" if up else "put",
                                    strike=100,
                                    effective_date=START,
                                    expiry_date=END,
                                ),
                                market,
                            )
                            if knock_in
                            else 2 * math.exp(-0.04)
                        )
                        self.assertAlmostEqual(actual.price, expected, delta=1e-12)
                    else:
                        expected = engine.price_with_greeks(
                            future, market, ["delta", "gamma"]
                        )
                        self.assertEqual(actual.price, expected.price)
                        self.assertIsNone(actual.delta)
                        self.assertIsNone(actual.gamma)

    def test_projection_preserves_vanishing_volatility_settlement_and_rebate_timing(
        self,
    ):
        for up, hit, timing in product(
            (False, True), (False, True), ("at_hit", "at_expiry")
        ):
            market = context(120 if up == hit else 80, deterministic=True)
            contract = BarrierOption(
                option_type="put" if up else "call",
                strike=100,
                effective_date=START,
                expiry_date=END,
                barrier_level=110.125 if up else 90.125,
                barrier_type="up_and_out" if up else "down_and_out",
                rebate=2,
                rebate_timing=timing,
                observation_mode="scheduled",
                observation_dates=[START + timedelta(days=180), END],
            )
            expected = (
                2 * math.exp(-0.04 * (180 / 365 if timing == "at_hit" else 1))
                if hit
                else 20 * math.exp(-0.04)
            )
            for scheme in ("explicit_euler", "implicit_euler", "crank_nicolson"):
                actual = pricing.FiniteDifferenceBarrierEngine(
                    asset_step_count=800,
                    time_step_count=2000,
                    scheme=scheme,
                    asset_upper_boundary=400,
                ).price_with_greeks(contract, market, ["delta", "gamma"])
                with self.subTest(up=up, hit=hit, timing=timing, scheme=scheme):
                    self.assertAlmostEqual(actual.price, expected, delta=1e-5)
                    self.assertAlmostEqual(
                        actual.delta,
                        0 if hit else (-1 if up else 1) * math.exp(-0.04),
                        delta=1e-6,
                    )
                    self.assertAlmostEqual(actual.gamma, 0, delta=2e-10)
