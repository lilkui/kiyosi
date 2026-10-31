import inspect
import math
import unittest
from datetime import UTC, date, datetime, timedelta

import QuantLib as ql
from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi.instruments import Accumulator
from kiyosi.market import monthly_schedule, weekdays_calendar
from test_accumulator import START, market

END = date(2025, 1, 6)


def contract(**changes):
    return Accumulator(
        **(
            {
                "strike": 100,
                "knock_out_level": 110,
                "daily_quantity": 1,
                "acceleration_factor": 2,
                "accumulated_quantity": 3,
                "effective_date": START,
                "expiry_date": END,
            }
            | changes
        )
    )


def engines():
    return (
        pricing.QuadratureAccumulatorEngine(),
        pricing.FiniteDifferenceAccumulatorEngine(
            asset_step_count=1600,
            time_step_count=2000,
            asset_upper_boundary=440,
        ),
        pricing.MonteCarloAccumulatorEngine(path_count=32768, seed=42),
    )


def terminal_reference(option, context):
    """Independent truncated-lognormal expectation for one remaining event."""
    end = datetime.combine(option.expiry_date, datetime.min.time(), UTC)
    time = (end - context.valuation_time).total_seconds() / (365 * 86400)
    model = context.model_parameters
    deviation = model.volatility * math.sqrt(time)
    stock = context.spot_price * math.exp(-model.dividend_yield * time)
    cash = option.strike * math.exp(-model.risk_free_rate * time)
    forward = stock - cash

    def below(level):
        d2 = (
            math.log(context.spot_price / level)
            + (model.risk_free_rate - model.dividend_yield - model.volatility**2 / 2)
            * time
        ) / deviation
        return (
            stock * math.erfc((d2 + deviation) / math.sqrt(2))
            - cash * math.erfc(d2 / math.sqrt(2))
        ) / 2

    observations = option.knock_out_observation_dates
    observes = observations is None or option.expiry_date in observations
    strike, barrier = option.strike, option.knock_out_level
    multiplier = option.acceleration_factor
    if option.option_type == "call":
        purchase = (
            multiplier * below(min(strike, barrier))
            + (below(barrier) - below(strike) if barrier > strike else 0)
            if observes
            else forward + (multiplier - 1) * below(strike)
        )
        sign = 1
    else:
        purchase = (
            multiplier * (forward - below(max(strike, barrier)))
            + (below(strike) - below(barrier) if barrier < strike else 0)
            if observes
            else multiplier * forward + (1 - multiplier) * below(strike)
        )
        sign = -1
    return sign * (
        option.accumulated_quantity * forward + option.daily_quantity * purchase
    )


class AccumulatorExtensionTests(unittest.TestCase):
    def test_direction_dates_defaults_history_replacement_and_ownership(self):
        default = contract()
        self.assertEqual(default.option_type, "call")
        self.assertIsNone(default.knock_out_observation_dates)
        self.assertEqual(
            default, contract(option_type="call", knock_out_observation_dates=None)
        )
        source = [START, END]
        option = contract(option_type="put", knock_out_observation_dates=source)
        source.clear()
        self.assertEqual(option.knock_out_observation_dates, [START, END])
        returned = option.knock_out_observation_dates
        returned.clear()
        self.assertEqual(option.knock_out_observation_dates, [START, END])
        updated = option.replace(accumulated_quantity=0.25)
        self.assertEqual(updated.option_type, "put")
        self.assertEqual(updated.knock_out_observation_dates, [START, END])
        self.assertEqual(updated.accumulated_quantity, 0.25)
        self.assertEqual(option.accumulated_quantity, 3)
        self.assertEqual(updated.replace(accumulated_quantity=3), option)
        self.assertNotEqual(default, contract(knock_out_observation_dates=[]))
        self.assertNotEqual(default, contract(option_type="put"))
        self.assertIn('option_type="put"', repr(option))
        self.assertIn("knock_out_observation_dates=[datetime.date", repr(option))
        self.assertIn("knock_out_observation_dates=None", repr(default))
        self.assertIn(
            "knock_out_observation_dates=[]",
            repr(contract(knock_out_observation_dates=[])),
        )
        for field, value in (
            ("option_type", "call"),
            ("knock_out_observation_dates", []),
        ):
            with self.subTest(field=field), self.assertRaises(AttributeError):
                setattr(option, field, value)
        with self.assertRaises(InvalidInputError):
            option.replace(accumulated_quantity=-1)
        self.assertEqual(option.knock_out_observation_dates, [START, END])
        signature = inspect.signature(Accumulator)
        self.assertIsNone(signature.parameters["knock_out_observation_dates"].default)
        self.assertEqual(
            signature.parameters["option_type"].kind, inspect.Parameter.KEYWORD_ONLY
        )
        schedule = monthly_schedule(
            start=START,
            end=date(2025, 2, 3),
            first_observation_month=1,
            calendar=weekdays_calendar(),
        )
        self.assertEqual(
            contract(
                expiry_date=date(2025, 2, 3),
                knock_out_observation_dates=schedule,
            ).knock_out_observation_dates,
            list(schedule),
        )
        self.assertEqual(
            contract(
                knock_out_observation_dates=iter([END])
            ).knock_out_observation_dates,
            [END],
        )

    def test_representation_and_explicit_schedule_validation(self):
        for value in (None, True, 1, [], object()):
            with self.subTest(direction=value), self.assertRaises(TypeError):
                contract(option_type=value)
        for value in ("Call", "PUT", "decumulator", ""):
            with self.subTest(direction=value), self.assertRaises(ValueError):
                contract(option_type=value)
        for value in (
            True,
            1,
            "2025-01-06",
            [None],
            [True],
            ["2025-01-06"],
            [datetime(2025, 1, 6, tzinfo=UTC)],
        ):
            with self.subTest(dates=value), self.assertRaises(TypeError):
                contract(knock_out_observation_dates=value)
        for value in (
            [END, END],
            [END, START],
            [START - timedelta(days=1)],
            [END + timedelta(days=1)],
        ):
            with self.subTest(dates=value), self.assertRaises(InvalidInputError):
                contract(knock_out_observation_dates=value)

    def test_trading_calendar_validation_is_deferred_until_pricing(self):
        weekend = contract(knock_out_observation_dates=[date(2025, 1, 4)])
        weekend_expiry = contract(
            expiry_date=date(2025, 1, 5), knock_out_observation_dates=[]
        )
        for engine in engines():
            with self.subTest(engine=type(engine).__name__):
                for option in (weekend, weekend_expiry):
                    with self.assertRaises(InvalidInputError):
                        engine.price(option, market(110, START))
                self.assertEqual(
                    engine.price(weekend, market(110, END, daily=True)), 40
                )
                self.assertEqual(
                    engine.price(
                        weekend_expiry,
                        market(90, date(2025, 1, 5), daily=True),
                    ),
                    -50,
                )

    def test_terminal_events_mirror_directions_and_observe_only_listed_dates(self):
        cases = (
            ("call", 110, 90, None, -50),
            ("call", 110, 100, None, 0),
            ("call", 110, 110, None, 30),
            ("call", 110, 110, [], 40),
            ("call", 110, 120, None, 60),
            ("call", 110, 120, [], 80),
            ("put", 90, 110, None, -50),
            ("put", 90, 100, None, 0),
            ("put", 90, 90, [END], 30),
            ("put", 90, 90, [], 40),
            ("put", 90, 80, None, 60),
            ("put", 90, 80, [], 80),
            ("call", 90, 90, None, -30),
            ("call", 90, 90, [], -50),
            ("put", 110, 110, None, -30),
            ("put", 110, 110, [], -50),
        )
        for engine in engines():
            for direction, barrier, spot, dates, expected in cases:
                with self.subTest(
                    engine=type(engine).__name__, case=(direction, spot, dates)
                ):
                    option = contract(
                        option_type=direction,
                        knock_out_level=barrier,
                        knock_out_observation_dates=dates,
                    )
                    self.assertEqual(engine.price(option, market(spot, END)), expected)
            for direction, spot in (("call", 90), ("put", 110)):
                option = contract(
                    option_type=direction,
                    daily_quantity=0.5,
                    acceleration_factor=0.5,
                    accumulated_quantity=0.25,
                    knock_out_observation_dates=[],
                )
                self.assertEqual(engine.price(option, market(spot, END)), -5)

    def test_sparse_and_empty_schedules_keep_every_daily_accumulation(self):
        end = date(2025, 1, 3)
        for engine in engines():
            for direction, spot, barrier in (
                ("call", 120, 110),
                ("put", 80, 90),
                ("call", 110, 90),
                ("put", 90, 110),
            ):
                context = market(spot, volatility=1e-8, rate=0, dividend=0)
                unit = abs(spot - 100)
                for observations, quantity in ((None, 3), ([end], 4), ([], 5)):
                    with self.subTest(
                        engine=type(engine).__name__,
                        direction=direction,
                        barrier=barrier,
                        observations=observations,
                    ):
                        option = contract(
                            option_type=direction,
                            knock_out_level=barrier,
                            acceleration_factor=0,
                            expiry_date=end,
                            knock_out_observation_dates=observations,
                        )
                        self.assertAlmostEqual(
                            engine.price(option, context), quantity * unit, delta=1e-6
                        )

    def test_explicit_daily_schedule_matches_default_prices_and_greeks(self):
        dates = [START, date(2025, 1, 2), date(2025, 1, 3), END]
        context = market()
        for engine in engines():
            for direction, barrier in (("call", 110), ("put", 90)):
                default = contract(option_type=direction, knock_out_level=barrier)
                explicit = contract(
                    option_type=direction,
                    knock_out_level=barrier,
                    knock_out_observation_dates=dates,
                )
                with self.subTest(engine=type(engine).__name__, direction=direction):
                    self.assertEqual(
                        engine.price(default, context), engine.price(explicit, context)
                    )
                    first = engine.price_with_greeks(
                        default, context, ["delta", "gamma", "vega"]
                    )
                    second = engine.price_with_greeks(
                        explicit, context, ["delta", "gamma", "vega"]
                    )
                    for field in ("price", "delta", "gamma", "vega"):
                        self.assertEqual(getattr(first, field), getattr(second, field))

    def test_midnight_and_intraday_history_and_first_observation_knock_out(self):
        observation = date(2025, 1, 3)
        midnight = datetime(2025, 1, 3, tzinfo=UTC)
        for engine in engines():
            for direction, spot, barrier, quantity in (
                ("call", 100, 110, 4),
                ("put", 100, 90, 4),
                ("call", 90, 110, 5),
                ("put", 110, 90, 5),
            ):
                option = contract(
                    option_type=direction,
                    knock_out_level=barrier,
                    knock_out_observation_dates=[END],
                )
                context = market(spot, midnight)
                expected = terminal_reference(
                    option.replace(accumulated_quantity=quantity), context
                )
                actual = engine.price(option, context)
                if isinstance(engine, pricing.MonteCarloAccumulatorEngine):
                    statistics = engine.price_with_statistics(option, context)
                    limit = 4 * statistics.standard_error + 0.003
                else:
                    limit = 0.003
                with self.subTest(
                    engine=type(engine).__name__, direction=direction, spot=spot
                ):
                    self.assertAlmostEqual(actual, expected, delta=limit)
                    intraday = market(spot, midnight + timedelta(hours=12))
                    seasoned = option.replace(accumulated_quantity=quantity)
                    self.assertAlmostEqual(
                        engine.price(seasoned, intraday),
                        terminal_reference(seasoned, intraday),
                        delta=limit,
                    )
            for direction, spot, barrier in (("call", 110, 110), ("put", 90, 90)):
                first_hit = contract(
                    option_type=direction,
                    knock_out_level=barrier,
                    knock_out_observation_dates=[observation],
                )
                self.assertEqual(engine.price(first_hit, market(spot, observation)), 30)

    def test_terminal_prices_match_independent_lognormal_expectation(self):
        context = market(
            100.25,
            datetime(2025, 1, 3, 12, tzinfo=UTC),
            volatility=0.6,
            rate=-0.02,
            dividend=0.03,
        )
        for engine in engines():
            for direction, barrier in (
                ("call", 110),
                ("call", 95),
                ("put", 90),
                ("put", 105),
            ):
                for observations in (None, []):
                    for acceleration in (0, 2):
                        option = contract(
                            option_type=direction,
                            knock_out_level=barrier,
                            acceleration_factor=acceleration,
                            knock_out_observation_dates=observations,
                        )
                        expected = terminal_reference(option, context)
                        if isinstance(engine, pricing.MonteCarloAccumulatorEngine):
                            estimate = engine.price_with_statistics(option, context)
                            actual = estimate.price
                            limit = 4 * estimate.standard_error + 0.003
                        else:
                            actual = engine.price(option, context)
                            limit = (
                                1e-9
                                if isinstance(
                                    engine, pricing.QuadratureAccumulatorEngine
                                )
                                else 0.003
                            )
                        with self.subTest(
                            engine=type(engine).__name__,
                            direction=direction,
                            barrier=barrier,
                            observations=observations,
                            acceleration=acceleration,
                        ):
                            self.assertAlmostEqual(actual, expected, delta=limit)

    def test_decumulator_risk_and_unidentifiable_no_knock_out_volatility(self):
        context = market(100.25, datetime(2025, 1, 3, 12, tzinfo=UTC), volatility=0.4)
        option = contract(
            option_type="put", knock_out_level=90, knock_out_observation_dates=[]
        )
        for engine in engines():
            with self.subTest(engine=type(engine).__name__):
                result = engine.price_with_greeks(
                    option, context, ["delta", "gamma", "vega"]
                )
                self.assertAlmostEqual(
                    result.price, engine.price(option, context), delta=1e-10
                )
                self.assertLess(result.delta, 0)
                self.assertLess(result.gamma, 0)
                self.assertLess(result.vega, 0)
                recovered = pricing.implied_volatility(
                    engine,
                    option,
                    context,
                    result.price,
                    lower_bound=0.2,
                    upper_bound=0.6,
                )
                self.assertAlmostEqual(recovered, 0.4, delta=1e-6)
                for terms in (
                    {"acceleration_factor": 1},
                    {"daily_quantity": 0},
                ):
                    linear = contract(
                        option_type="put", knock_out_observation_dates=[], **terms
                    )
                    with self.assertRaises(UnsupportedOperationError):
                        pricing.implied_volatility(
                            engine, linear, context, engine.price(linear, context)
                        )

    def test_sparse_knock_out_prices_survive_drift_back_inside_barrier(self):
        end = date(2025, 2, 3)
        time = 32.5 / 365
        for direction, spot, barrier, rate, dividend, sign in (
            ("call", 110.05, 110, 0.04, 0.06, 1),
            ("put", 89.95, 90, 0.06, 0.04, -1),
        ):
            option = contract(
                option_type=direction,
                knock_out_level=barrier,
                acceleration_factor=1,
                expiry_date=end,
                knock_out_observation_dates=[date(2025, 1, 15)],
            )
            observation = 13.5 / 365
            remaining = time - observation
            discount = math.exp(-rate * observation)
            forward = spot * math.exp((rate - dividend) * observation)
            alive = ql.Option.Put if direction == "call" else ql.Option.Call
            for sigma in (1e-8, 0.0005, 0.0008):
                deviation = sigma * math.sqrt(observation)
                asset = ql.BlackCalculator(
                    ql.AssetOrNothingPayoff(alive, barrier),
                    forward,
                    deviation,
                    discount,
                ).value()
                cash = ql.BlackCalculator(
                    ql.CashOrNothingPayoff(alive, barrier, 100),
                    forward,
                    deviation,
                    discount,
                ).value()
                expected = sign * (
                    12 * (discount * (forward - 100) - asset + cash)
                    + 26
                    * (
                        asset * math.exp(-dividend * remaining)
                        - cash * math.exp(-rate * remaining)
                    )
                )
                if sigma == 1e-8:
                    limit = (
                        sign
                        * 26
                        * (
                            spot * math.exp(-dividend * time)
                            - 100 * math.exp(-rate * time)
                        )
                    )
                    self.assertAlmostEqual(expected, limit, delta=1e-9)
                context = market(spot, volatility=sigma, rate=rate, dividend=dividend)
                for nodes, tolerance in ((512, 0.002), (2048, 0.00002)):
                    with self.subTest(direction=direction, sigma=sigma, nodes=nodes):
                        actual = pricing.QuadratureAccumulatorEngine(
                            asset_step_count=nodes
                        ).price(option, context)
                        self.assertAlmostEqual(actual, expected, delta=tolerance)

    def test_explicit_daily_zero_payoffs_keep_known_statistics_and_skip_grid_validation(
        self,
    ):
        context = market(100)
        monte_carlo = pricing.MonteCarloAccumulatorEngine(path_count=1, seed=42)
        clipped = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=50)
        for direction, barrier in (("call", 90), ("put", 110)):
            for observations in (
                None,
                [START, date(2025, 1, 2), date(2025, 1, 3)],
                [date(2025, 1, 2), date(2025, 1, 3)],
            ):
                option = contract(
                    option_type=direction,
                    knock_out_level=barrier,
                    acceleration_factor=0,
                    accumulated_quantity=0,
                    expiry_date=date(2025, 1, 3),
                    knock_out_observation_dates=observations,
                )
                with self.subTest(direction=direction, observations=observations):
                    result = monte_carlo.price_with_statistics(option, context)
                    self.assertEqual(result.price, 0)
                    self.assertEqual(result.standard_error, 0)
                    self.assertEqual(result.confidence_interval, (0, 0))
                    self.assertEqual(clipped.price(option, context), 0)
                    holding = option.replace(accumulated_quantity=3)
                    holding_context = market(
                        85 if direction == "call" else 115, rate=0, dividend=0
                    )
                    self.assertEqual(clipped.price(holding, holding_context), -45)
                    self.assertEqual(
                        monte_carlo.price_with_statistics(
                            holding, holding_context
                        ).standard_error,
                        0,
                    )

    def test_adjacent_barrier_levels_preserve_forward_prices_and_greeks(self):
        time = 4.5 / 365
        levels = (
            math.nextafter(100, 0),
            100,
            math.nextafter(100, math.inf),
            100 + 1e-11,
            100 + 1e-8,
        )
        for direction, sign in (("call", 1), ("put", -1)):
            expected = (
                sign
                * 6
                * (100.001 * math.exp(-0.06 * time) - 100 * math.exp(-0.04 * time))
            )
            delta = sign * 6 * math.exp(-0.06 * time)
            for sigma in (1e-8, 0.0001, 0.2):
                context = market(100.001, volatility=sigma, rate=0.04, dividend=0.06)
                for barrier in levels:
                    option = contract(
                        option_type=direction,
                        knock_out_level=barrier,
                        acceleration_factor=1,
                        knock_out_observation_dates=[],
                    )
                    for nodes in (512, 1024):
                        with self.subTest(
                            direction=direction,
                            sigma=sigma,
                            barrier=barrier,
                            nodes=nodes,
                        ):
                            result = pricing.QuadratureAccumulatorEngine(
                                asset_step_count=nodes
                            ).price_with_greeks(option, context, ["delta", "gamma"])
                            self.assertAlmostEqual(result.price, expected, delta=1e-8)
                            self.assertAlmostEqual(result.delta, delta, delta=1e-7)
                            self.assertAlmostEqual(result.gamma, 0, delta=1e-6)

    def test_terminal_digital_portfolios_match_quantlib(self):
        context = market(100.25, datetime(2025, 1, 3, 12, tzinfo=UTC))
        time = 2.5 / 365
        model = context.model_parameters
        discount = math.exp(-model.risk_free_rate * time)
        forward = context.spot_price * math.exp(
            (model.risk_free_rate - model.dividend_yield) * time
        )
        deviation = model.volatility * math.sqrt(time)
        unit = discount * (forward - 100)
        for direction, barrier, side, sign in (
            ("call", 110, ql.Option.Put, 1),
            ("put", 90, ql.Option.Call, -1),
        ):

            def digital(level, side=side):
                asset = ql.AssetOrNothingPayoff(side, level)
                cash = ql.CashOrNothingPayoff(side, level, 1)
                return (
                    ql.BlackCalculator(asset, forward, deviation, discount).value()
                    - 100
                    * ql.BlackCalculator(cash, forward, deviation, discount).value()
                )

            for observations in (None, []):
                option = contract(
                    option_type=direction,
                    knock_out_level=barrier,
                    knock_out_observation_dates=observations,
                )
                expected = sign * (
                    3 * unit
                    + (digital(barrier) if observations is None else unit)
                    + digital(100)
                )
                with self.subTest(direction=direction, observations=observations):
                    self.assertAlmostEqual(
                        pricing.QuadratureAccumulatorEngine().price(option, context),
                        expected,
                        delta=1e-9,
                    )


if __name__ == "__main__":
    unittest.main()
