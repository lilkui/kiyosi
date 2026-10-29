"""Binary barrier FD prices, monitoring and settlement through the public API."""

import importlib
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, date, datetime

import QuantLib as ql
from kiyosi import InvalidInputError, RootNotBracketedError, UnsupportedOperationError
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import (
    AnalyticBinaryBarrierEngine,
    AnalyticDigitalEngine,
    FiniteDifferenceBinaryBarrierEngine,
    implied_volatility,
)

START = date(2025, 1, 1)
END = date(2026, 1, 1)


def market(spot=100, time=START, rate=0.04, dividend=0.01, sigma=0.2):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=sigma
        ),
        spot_price=spot,
        valuation_time=time,
    )


def touch(factory=ins.cash_one_touch_up, **changes):
    terms = {"effective_date": START, "expiry_date": END, "barrier_level": 120.13}
    if factory.__name__.startswith("cash"):
        terms["payout"] = 10
    return factory(**(terms | changes))


class BinaryBarrierFiniteDifferenceTests(unittest.TestCase):
    def test_continuous_touch_settlements_match_analytics(self):
        engine = FiniteDifferenceBinaryBarrierEngine()
        for factory in (
            ins.cash_one_touch_up,
            ins.cash_one_touch_down,
            ins.cash_no_touch_up,
            ins.cash_no_touch_down,
            ins.asset_one_touch_up,
            ins.asset_one_touch_down,
            ins.asset_no_touch_up,
            ins.asset_no_touch_down,
        ):
            for timing in ("at_expiry", "at_hit"):
                if "no_touch" in factory.__name__ and timing == "at_hit":
                    continue
                fields = {
                    "barrier_level": 79.87 if "down" in factory.__name__ else 120.13
                }
                if "one_touch" in factory.__name__:
                    fields["settlement_timing"] = timing
                option = touch(factory, **fields)
                self.assertAlmostEqual(
                    engine.price(option, market()),
                    AnalyticBinaryBarrierEngine().price(option, market()),
                    delta=0.02 if option.payoff_type == "asset" else 0.002,
                )

    def test_current_and_prior_at_hit_payment(self):
        engine = FiniteDifferenceBinaryBarrierEngine()
        for factory, amount in (
            (ins.cash_one_touch_up, 10),
            (ins.asset_one_touch_up, 130),
        ):
            option = touch(factory, settlement_timing="at_hit")
            self.assertEqual(engine.price(option, market(130)), amount)
            self.assertEqual(
                engine.price(
                    option.replace(touch_state="touched"), market(time=date(2025, 1, 2))
                ),
                0,
            )

    def assert_rejection(self, expected_error, operation, *args, **kwargs):
        with self.assertRaises(expected_error) as error:
            operation(*args, **kwargs)
        self.assertIs(type(error.exception), expected_error)

    def test_two_fixing_prices_and_grid_convergence_against_independent_integrals(self):
        coarse = FiniteDifferenceBinaryBarrierEngine(
            asset_step_count=100, time_step_count=100, asset_upper_boundary=400
        )
        fine = FiniteDifferenceBinaryBarrierEngine(
            asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
        )
        for up in (False, True):
            for asset in (False, True):
                for kind, at_hit in (
                    ("one_touch", False),
                    ("one_touch", True),
                    ("no_touch", False),
                ):
                    factory = getattr(
                        ins,
                        f"{'asset' if asset else 'cash'}_{kind}_{'up' if up else 'down'}",
                    )
                    fields = {
                        "barrier_level": 120.125 if up else 79.875,
                        "observation_mode": "scheduled",
                        "observation_dates": [date(2025, 6, 30), END],
                    }
                    if kind == "one_touch":
                        fields["settlement_timing"] = (
                            "at_hit" if at_hit else "at_expiry"
                        )
                    option = touch(factory, **fields)
                    expected = two_fixing_price(
                        up=up, knock_in=kind == "one_touch", asset=asset, at_hit=at_hit
                    )
                    value = fine.price(option, market())
                    self.assertAlmostEqual(
                        value, expected, delta=0.001 if asset else 0.0001
                    )
                    self.assertLess(
                        abs(value - expected),
                        abs(coarse.price(option, market()) - expected),
                    )
                for knock_in in (False, True):
                    for direction in ("call", "put"):
                        factory = (
                            ins.asset_binary_barrier_option
                            if asset
                            else ins.cash_binary_barrier_option
                        )
                        fields = {
                            "option_type": direction,
                            "strike": 100,
                            "effective_date": START,
                            "expiry_date": END,
                            "barrier_level": 120.125 if up else 79.875,
                            "barrier_type": f"{'up' if up else 'down'}_and_{'in' if knock_in else 'out'}",
                            "observation_mode": "scheduled",
                            "observation_dates": [date(2025, 6, 30), END],
                        }
                        if not asset:
                            fields["payout"] = 10
                        option = factory(**fields)
                        expected = two_fixing_price(
                            up=up, knock_in=knock_in, asset=asset, direction=direction
                        )
                        self.assertAlmostEqual(
                            fine.price(option, market()),
                            expected,
                            delta=0.002 if asset else 0.0002,
                        )

    def test_node_curves_and_early_at_hit_horizon(self):
        fixing = date(2025, 6, 30)
        first = (fixing - START).days / 365
        rate1, rate2, dividend1, dividend2 = -0.02, 0.08, 0.03, -0.01
        rate = RateTermStructure(
            reference_time=START,
            times=[fixing, END],
            factors=[
                math.exp(-rate1 * first),
                math.exp(-rate1 * first - rate2 * (1 - first)),
            ],
        )
        dividend = RateTermStructure(
            reference_time=START,
            times=[fixing, END],
            factors=[
                math.exp(-dividend1 * first),
                math.exp(-dividend1 * first - dividend2 * (1 - first)),
            ],
        )
        ctx = market(rate=rate, dividend=dividend)
        engine = FiniteDifferenceBinaryBarrierEngine(
            asset_step_count=1600, time_step_count=1600, asset_upper_boundary=400
        )
        for asset in (False, True):
            for at_hit in (False, True):
                option = touch(
                    ins.asset_one_touch_up if asset else ins.cash_one_touch_up,
                    barrier_level=120.125,
                    observation_mode="scheduled",
                    observation_dates=[fixing, END],
                    settlement_timing="at_hit" if at_hit else "at_expiry",
                )
                expected = two_fixing_price(
                    up=True,
                    knock_in=True,
                    asset=asset,
                    at_hit=at_hit,
                    rate1=rate1,
                    rate2=rate2,
                    dividend1=dividend1,
                    dividend2=dividend2,
                )
                self.assertAlmostEqual(
                    engine.price(option, ctx),
                    expected,
                    delta=0.002 if asset else 0.0002,
                )
        short = RateTermStructure(reference_time=START, times=[fixing], factors=[0.97])
        ctx = market(rate=short, dividend=short)
        option = touch(
            observation_mode="scheduled",
            observation_dates=[fixing],
            settlement_timing="at_hit",
        )
        digital = ins.CashOrNothingOption(
            option_type="call",
            strike=option.barrier_level,
            payout=10,
            effective_date=START,
            expiry_date=fixing,
        )
        self.assertAlmostEqual(
            engine.price(option, ctx),
            AnalyticDigitalEngine().price(digital, ctx),
            delta=0.0004,
        )
        later_payment = touch(observation_mode="scheduled", observation_dates=[fixing])
        self.assert_rejection(InvalidInputError, engine.price, later_payment, ctx)
        known = option.replace(touch_state="untouched")
        after = market(130, datetime(2025, 6, 30, 0, 0, 0, 1, tzinfo=UTC))
        self.assert_rejection(InvalidInputError, engine.price, option, after)
        self.assertEqual(engine.price(known, after), 0)
        self.assertEqual(engine.price(option, market(130, fixing)), 10)
        no_touch = touch(
            ins.cash_no_touch_up,
            observation_mode="scheduled",
            observation_dates=[fixing],
            touch_state="untouched",
        )
        self.assertAlmostEqual(
            engine.price(no_touch, after),
            10 * math.exp(-0.04 * (END - fixing).days / 365),
            delta=1e-8,
        )

    def test_history_expiry_greeks_and_solvers_preserve_inputs(self):
        engine = FiniteDifferenceBinaryBarrierEngine(asset_upper_boundary=400)
        option = touch(touch_state="untouched")
        ctx = market(time=date(2025, 1, 2))
        result = engine.price_with_greeks(option, ctx, all_greeks=True)
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
        self.assertTrue(all(math.isfinite(getattr(result, g)) for g in greeks))
        h = 0.01
        up = engine.price(option, market(100 + h, date(2025, 1, 2)))
        down = engine.price(option, market(100 - h, date(2025, 1, 2)))
        self.assertAlmostEqual(result.delta, (up - down) / (2 * h), delta=1e-6)
        self.assertAlmostEqual(
            result.gamma, (up - 2 * result.price + down) / h**2, delta=1e-6
        )
        self.assertAlmostEqual(
            implied_volatility(
                engine, option, ctx, result.price, lower_bound=0.1, upper_bound=0.3
            ),
            0.2,
            delta=1e-7,
        )
        self.assert_rejection(
            RootNotBracketedError, implied_volatility, engine, option, ctx, -1
        )
        self.assert_rejection(InvalidInputError, engine.price, touch(), ctx)
        self.assert_rejection(
            InvalidInputError,
            engine.price,
            option.replace(touch_state="touched"),
            market(),
        )
        self.assert_rejection(
            InvalidInputError, engine.price_with_greeks, option, ctx, []
        )
        self.assert_rejection(
            InvalidInputError,
            engine.price_with_greeks,
            option,
            ctx,
            ["delta"],
            spot_shift=0,
        )
        for factory in (
            ins.cash_one_touch_up,
            ins.cash_no_touch_up,
            ins.asset_one_touch_up,
            ins.asset_no_touch_up,
        ):
            contract = touch(factory, touch_state="untouched")
            expiry = engine.price_with_greeks(
                contract, market(130, END), all_greeks=True
            )
            self.assertTrue(all(getattr(expiry, g) is None for g in greeks))
        equality = engine.price_with_greeks(touch(), market(120.13), all_greeks=True)
        self.assertTrue(all(getattr(equality, g) is None for g in greeks))
        paid = touch(settlement_timing="at_hit", touch_state="touched")
        self.assert_rejection(
            UnsupportedOperationError, implied_volatility, engine, paid, ctx, 0
        )
        self.assertEqual(engine.price(option, ctx), result.price)
        self.assertEqual(option.touch_state, "untouched")
        self.assertEqual(ctx.spot_price, 100)
        with ThreadPoolExecutor(4) as pool:
            self.assertEqual(
                list(pool.map(lambda _: engine.price(option, ctx), range(16))),
                [result.price] * 16,
            )

    def test_continuous_domain_retains_configured_nodes_and_stability(self):
        option = touch(barrier_level=120)

        def engine(upper, scheme="crank_nicolson"):
            return FiniteDifferenceBinaryBarrierEngine(
                asset_step_count=800,
                time_step_count=2000,
                scheme=scheme,
                asset_upper_boundary=upper,
            )

        expected = engine(400).price(option, market())
        for upper in (10_000, 1e9):
            self.assertEqual(engine(upper).price(option, market()), expected)
        self.assert_rejection(
            InvalidInputError, engine(480, "explicit_euler").price, option, market()
        )

    def test_settings_protocols_defaults_and_errors(self):
        engine = FiniteDifferenceBinaryBarrierEngine()
        self.assertEqual(
            (
                engine.asset_step_count,
                engine.time_step_count,
                engine.scheme,
                engine.asset_upper_boundary,
            ),
            (800, 200, "crank_nicolson", None),
        )
        for name in ("asset_step_count", "time_step_count"):
            for value in (True, 2.5, 4.0):
                with self.assertRaises(TypeError):
                    FiniteDifferenceBinaryBarrierEngine(**{name: value})
            with self.assertRaises(OverflowError):
                FiniteDifferenceBinaryBarrierEngine(**{name: 2**40})
        with self.assertRaises(TypeError):
            FiniteDifferenceBinaryBarrierEngine(asset_upper_boundary=True)
        with self.assertRaises(TypeError):
            FiniteDifferenceBinaryBarrierEngine(unexpected=1)
        with self.assertRaises(ValueError):
            FiniteDifferenceBinaryBarrierEngine(scheme="unknown")
        for fields in (
            {"asset_step_count": 2},
            {"time_step_count": 0},
            {"asset_upper_boundary": math.nan},
        ):
            self.assert_rejection(
                InvalidInputError, FiniteDifferenceBinaryBarrierEngine, **fields
            )
        for fields in (
            {"asset_upper_boundary": 120},
            {"scheme": "explicit_euler", "time_step_count": 1},
        ):
            self.assert_rejection(
                InvalidInputError,
                FiniteDifferenceBinaryBarrierEngine(**fields).price,
                touch(),
                market(),
            )
        weekend = touch(
            observation_mode="scheduled", observation_dates=[date(2025, 1, 4)]
        )
        self.assert_rejection(InvalidInputError, engine.price, weekend, market())
        with self.assertRaises(TypeError):
            engine.price(object(), market())
        with self.assertRaises(TypeError):
            engine.price_with_greeks(touch(), market(), ["delta"], spot_shift=True)
        with self.assertRaises(TypeError):
            implied_volatility(engine, touch(), market(), True)
        with self.assertRaises(AttributeError):
            engine.time_step_count = 1

    def test_existing_independent_quantlib_price_scenarios(self):

        reference = importlib.import_module("quantlib_reference.binary")
        saved = ql.SavedSettings()
        self.addCleanup(lambda: saved.__exit__(None, None, None))
        engine = FiniteDifferenceBinaryBarrierEngine(
            asset_step_count=3200, time_step_count=800
        )
        for identifier, inputs in reference.scenarios():
            with self.subTest(identifier=identifier):
                kind, asset = inputs["BarrierType"], inputs["asset_settlement"]
                fields = {
                    "effective_date": date.fromisoformat(inputs["effective_date"]),
                    "expiry_date": date.fromisoformat(inputs["expiry_date"]),
                    "barrier_level": inputs["barrier"],
                    "touch_state": "untouched",
                }
                if not asset:
                    fields["payout"] = inputs["payout"]
                if inputs["option"] == "none":
                    name = f"{'asset' if asset else 'cash'}_{'one_touch' if kind.endswith('_in') else 'no_touch'}_{'up' if kind.startswith('up') else 'down'}"
                    if kind.endswith("_in"):
                        fields["settlement_timing"] = inputs["settlement"]
                    option = getattr(ins, name)(**fields)
                else:
                    option = (
                        ins.asset_binary_barrier_option
                        if asset
                        else ins.cash_binary_barrier_option
                    )(
                        **fields,
                        option_type=inputs["option"],
                        strike=inputs["strike"],
                        barrier_type=kind,
                    )
                ctx = market(
                    inputs["spot"],
                    date.fromisoformat(inputs["valuation"]),
                    rate=inputs["rate"],
                    dividend=inputs["dividend"],
                    sigma=inputs["volatility"],
                )
                self.assertAlmostEqual(
                    engine.price(option, ctx),
                    reference.measure(inputs, "price"),
                    delta=reference.barrier.FD["price"],
                )

                if (
                    inputs["spot"],
                    inputs["strike"],
                    (option.expiry_date - ctx.valuation_time.date()).days,
                ) == (100, 100, 365):
                    result = engine.price_with_greeks(
                        option, ctx, all_greeks=True, spot_shift=0.1
                    )
                    for greek in (
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
                    ):
                        self.assertAlmostEqual(
                            getattr(result, greek),
                            reference.measure(inputs, greek),
                            delta=reference.barrier.FD[greek],
                            msg=f"{identifier}: {greek}",
                        )


def two_fixing_price(
    *,
    up,
    knock_in,
    asset,
    direction=None,
    at_hit=False,
    rate1=0.04,
    rate2=0.04,
    dividend1=0.01,
    dividend2=0.01,
):
    """Integrate conditional lognormal payoffs, splitting at the first trigger."""
    first = (date(2025, 6, 30) - START).days / 365
    remaining = 1 - first
    level = 120.125 if up else 79.875
    sigma = 0.2
    normal = lambda x: 0.5 * math.erfc(-x / math.sqrt(2))
    cut = (math.log(level / 100) - (rate1 - dividend1 - sigma**2 / 2) * first) / (
        sigma * math.sqrt(first)
    )

    def integrand(z):
        spot = 100 * math.exp(
            (rate1 - dividend1 - sigma**2 / 2) * first + sigma * math.sqrt(first) * z
        )
        hit = spot >= level if up else spot <= level
        if hit and not knock_in:
            return 0.0
        if hit and at_hit:
            value = spot if asset else 10.0
        else:
            lower, upper = (
                (100.0, math.inf)
                if direction == "call"
                else (0.0, 100.0)
                if direction == "put"
                else (0.0, math.inf)
            )
            if not hit:
                if up == knock_in:
                    lower = max(lower, level)
                else:
                    upper = min(upper, level)
            if lower >= upper:
                return 0.0
            drift = (
                rate2 - dividend2 + (0.5 if asset else -0.5) * sigma**2
            ) * remaining
            width = sigma * math.sqrt(remaining)
            low = (
                0.0 if lower == 0 else normal((math.log(lower / spot) - drift) / width)
            )
            high = (
                1.0
                if math.isinf(upper)
                else normal((math.log(upper / spot) - drift) / width)
            )
            value = (
                spot * math.exp(-dividend2 * remaining)
                if asset
                else 10 * math.exp(-rate2 * remaining)
            ) * (high - low)
        return math.exp(-rate1 * first - z * z / 2) / math.sqrt(2 * math.pi) * value

    total = 0.0
    for left, right in ((-10.0, cut), (cut, 10.0)):
        count = 4000
        width = (right - left) / count
        # Midpoints avoid assigning a finite weight to the trigger discontinuity.
        total += width * sum(integrand(left + (i + 0.5) * width) for i in range(count))
    return total


if __name__ == "__main__":
    unittest.main()
