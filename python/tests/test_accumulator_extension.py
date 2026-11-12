import inspect
import math
from datetime import UTC, datetime, timedelta
from datetime import time as clock_time

import pytest
import QuantLib as ql
from kiyosi import InvalidInputError, UnsupportedOperationError, pricing
from kiyosi.instruments import Accumulator
from kiyosi.market import all_days_calendar, monthly_schedule, weekdays_calendar
from test_accumulator import START, market

END = datetime(2025, 1, 6)


def contract(**changes):
    return Accumulator(
        observation_clock_time=clock_time(),
        **{
            "knock_out_settled": False,
            "strike": 100,
            "knock_out_level": 110,
            "daily_quantity": 1,
            "acceleration_factor": 2,
            "accumulated_quantity": 3,
            "effective_time": START,
            "expiry_time": END,
        }
        | changes,
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
    end = datetime.combine(option.expiry_time, datetime.min.time())
    time = (end - context.valuation_time).total_seconds() / (365 * 86400)
    model = context.model_parameters
    deviation = model.volatility * math.sqrt(time)
    stock = context.spot_price * math.exp(-model.flat_dividend_yield * time)
    cash = option.strike * math.exp(-model.flat_risk_free_rate * time)
    forward = stock - cash

    def below(level):
        d2 = (
            math.log(context.spot_price / level)
            + (
                model.flat_risk_free_rate
                - model.flat_dividend_yield
                - model.volatility**2 / 2
            )
            * time
        ) / deviation
        return (
            stock * math.erfc((d2 + deviation) / math.sqrt(2))
            - cash * math.erfc(d2 / math.sqrt(2))
        ) / 2

    observations = option.knock_out_observation_times
    observes = observations is None or option.expiry_time in observations
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


def test_direction_dates_defaults_history_replacement_and_ownership(subtests):
    default = contract()
    assert default.option_type == "call"
    assert default.knock_out_observation_times is None
    assert default == contract(option_type="call", knock_out_observation_times=None)
    source = [START, END]
    option = contract(option_type="put", knock_out_observation_times=source)
    source.clear()
    assert option.knock_out_observation_times == ((START, END))
    returned = list(option.knock_out_observation_times)
    returned.clear()
    assert option.knock_out_observation_times == ((START, END))
    updated = option.replace(
        accumulated_quantity=0.25,
    )
    assert updated.option_type == "put"
    assert updated.knock_out_observation_times == ((START, END))
    assert updated.accumulated_quantity == 0.25
    assert option.accumulated_quantity == 3
    assert (
        updated.replace(
            accumulated_quantity=3,
        )
        == option
    )
    assert default != contract(knock_out_observation_times=[])
    assert default != contract(option_type="put")
    assert "option_type='put'" in repr(option)
    assert "knock_out_observation_times=[datetime.date" in repr(option)
    assert "knock_out_observation_times=None" in repr(default)
    assert "knock_out_observation_times=[]" in repr(
        contract(knock_out_observation_times=[])
    )
    for field, value in (
        ("option_type", "call"),
        ("knock_out_observation_times", []),
    ):
        with subtests.test(field=field), pytest.raises(AttributeError):
            setattr(option, field, value)
    with pytest.raises(InvalidInputError):
        option.replace(
            accumulated_quantity=-1,
        )
    assert option.knock_out_observation_times == ((START, END))
    signature = inspect.signature(Accumulator)
    assert signature.parameters["knock_out_observation_times"].default is None
    assert signature.parameters["option_type"].kind == inspect.Parameter.KEYWORD_ONLY
    schedule = monthly_schedule(
        observation_clock_time=clock_time(),
        start_time=START,
        end_time=datetime(2025, 2, 3),
        first_observation_month=1,
        calendar=weekdays_calendar(),
    )
    assert contract(
        expiry_time=datetime(2025, 2, 3),
        knock_out_observation_times=schedule,
    ).knock_out_observation_times == tuple(schedule)
    assert contract(
        knock_out_observation_times=iter([END])
    ).knock_out_observation_times == ((END,))


def test_representation_and_explicit_schedule_validation(subtests):
    for value in (None, True, 1, [], object()):
        with subtests.test(direction=value), pytest.raises(TypeError):
            contract(option_type=value)
    for value in ("Call", "PUT", "decumulator", ""):
        with subtests.test(direction=value), pytest.raises(ValueError):
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
        with subtests.test(dates=value), pytest.raises(TypeError):
            contract(knock_out_observation_times=value)
    for value in (
        [END, END],
        [END, START],
        [START - timedelta(days=1)],
        [END + timedelta(days=1)],
    ):
        with subtests.test(dates=value), pytest.raises(InvalidInputError):
            contract(knock_out_observation_times=value)


def test_trading_calendar_validation_precedes_pricing(subtests):
    with pytest.raises(InvalidInputError):
        contract(knock_out_observation_times=[datetime(2025, 1, 4)])
    weekend = contract(
        knock_out_observation_times=[datetime(2025, 1, 4)],
        calendar=all_days_calendar(),
    )
    weekend_expiry = contract(
        expiry_time=datetime(2025, 1, 5),
        knock_out_observation_times=[],
    )
    for engine in engines():
        with subtests.test(engine=type(engine).__name__):
            assert (
                engine.price(
                    weekend.replace(calendar=all_days_calendar()),
                    market(110, END),
                )
                == 40
            )
            assert engine.price(
                weekend_expiry,
                market(90, datetime(2025, 1, 5)),
            ) == (-30)


def test_terminal_events_mirror_directions_and_observe_only_listed_dates(subtests):
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
            with subtests.test(
                engine=type(engine).__name__, case=(direction, spot, dates)
            ):
                option = contract(
                    option_type=direction,
                    knock_out_level=barrier,
                    knock_out_observation_times=dates,
                )
                assert engine.price(option, market(spot, END)) == expected
        for direction, spot in (("call", 90), ("put", 110)):
            option = contract(
                option_type=direction,
                daily_quantity=0.5,
                acceleration_factor=0.5,
                accumulated_quantity=0.25,
                knock_out_observation_times=[],
            )
            assert engine.price(option, market(spot, END)) == (-5)


def test_sparse_and_empty_schedules_keep_every_daily_accumulation(subtests):
    end = datetime(2025, 1, 3)
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
                with subtests.test(
                    engine=type(engine).__name__,
                    direction=direction,
                    barrier=barrier,
                    observations=observations,
                ):
                    option = contract(
                        option_type=direction,
                        knock_out_level=barrier,
                        acceleration_factor=0,
                        expiry_time=end,
                        knock_out_observation_times=observations,
                    )
                    assert engine.price(option, context) == pytest.approx(
                        quantity * unit, rel=0, abs=1e-6
                    )


def test_explicit_daily_schedule_matches_default_prices_and_greeks(subtests):
    dates = [START, datetime(2025, 1, 2), datetime(2025, 1, 3), END]
    context = market()
    for engine in engines():
        for direction, barrier in (("call", 110), ("put", 90)):
            default = contract(option_type=direction, knock_out_level=barrier)
            explicit = contract(
                option_type=direction,
                knock_out_level=barrier,
                knock_out_observation_times=dates,
            )
            with subtests.test(engine=type(engine).__name__, direction=direction):
                assert engine.price(default, context) == engine.price(explicit, context)
                first = engine.price_with_greeks(
                    default,
                    context,
                    ["delta", "gamma", "vega"],
                )
                second = engine.price_with_greeks(
                    explicit,
                    context,
                    ["delta", "gamma", "vega"],
                )
                for field in (
                    "price",
                    "delta",
                    "gamma",
                    "vega",
                ):
                    assert getattr(first, field) == getattr(second, field)


def test_midnight_and_intraday_history_and_first_observation_knock_out(subtests):
    observation = datetime(2025, 1, 3)
    midnight = datetime(2025, 1, 3)
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
                knock_out_observation_times=[END],
            )
            context = market(spot, midnight)
            expected = terminal_reference(
                option.replace(
                    accumulated_quantity=quantity,
                ),
                context,
            )
            actual = engine.price(option, context)
            if isinstance(engine, pricing.MonteCarloAccumulatorEngine):
                statistics = engine.price_with_statistics(option, context)
                limit = 4 * statistics.standard_error + 0.003
            else:
                limit = 0.003
            with subtests.test(
                engine=type(engine).__name__, direction=direction, spot=spot
            ):
                assert actual == pytest.approx(expected, rel=0, abs=limit)
                intraday = market(spot, midnight + timedelta(hours=12))
                seasoned = option.replace(
                    accumulated_quantity=quantity,
                )
                assert engine.price(seasoned, intraday) == pytest.approx(
                    terminal_reference(seasoned, intraday), rel=0, abs=limit
                )
        for direction, spot, barrier in (("call", 110, 110), ("put", 90, 90)):
            first_hit = contract(
                option_type=direction,
                knock_out_level=barrier,
                knock_out_observation_times=[observation],
            )
            assert (
                engine.price(
                    first_hit,
                    market(spot, observation),
                )
                == 30
            )


def test_terminal_prices_match_independent_lognormal_expectation(subtests):
    context = market(
        100.25,
        datetime(2025, 1, 3, 12),
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
                        knock_out_observation_times=observations,
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
                            if isinstance(engine, pricing.QuadratureAccumulatorEngine)
                            else 0.003
                        )
                    with subtests.test(
                        engine=type(engine).__name__,
                        direction=direction,
                        barrier=barrier,
                        observations=observations,
                        acceleration=acceleration,
                    ):
                        assert actual == pytest.approx(expected, rel=0, abs=limit)


def test_decumulator_risk_and_unidentifiable_no_knock_out_volatility(subtests):
    context = market(100.25, datetime(2025, 1, 3, 12), volatility=0.4)
    option = contract(
        option_type="put", knock_out_level=90, knock_out_observation_times=[]
    )
    for engine in engines():
        with subtests.test(engine=type(engine).__name__):
            result = engine.price_with_greeks(
                option, context, ["delta", "gamma", "vega"]
            )
            assert result.price == pytest.approx(
                engine.price(option, context),
                rel=0,
                abs=1e-10,
            )
            assert result.delta < 0
            assert result.gamma < 0
            assert result.vega < 0
            recovered = pricing.implied_volatility(
                engine,
                option,
                context,
                result.price,
                lower_bound=0.2,
                upper_bound=0.6,
            )
            assert recovered == pytest.approx(0.4, rel=0, abs=1e-6)
            for terms in (
                {"acceleration_factor": 1},
                {"daily_quantity": 0},
            ):
                linear = contract(
                    option_type="put", knock_out_observation_times=[], **terms
                )
                with pytest.raises(UnsupportedOperationError):
                    pricing.implied_volatility(
                        engine,
                        linear,
                        context,
                        engine.price(linear, context),
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )


@pytest.mark.parametrize(
    "direction,spot,barrier,rate,dividend,sign",
    (("call", 110.05, 110, 0.04, 0.06, 1), ("put", 89.95, 90, 0.06, 0.04, -1)),
)
@pytest.mark.parametrize("sigma", (1e-08, 0.0005, 0.0008))
def test_sparse_knock_out_prices_survive_drift_back_inside_barrier(
    subtests, direction, spot, barrier, rate, dividend, sign, sigma
):
    end = datetime(2025, 2, 3)
    time = 32.5 / 365
    option = contract(
        option_type=direction,
        knock_out_level=barrier,
        acceleration_factor=1,
        expiry_time=end,
        knock_out_observation_times=[datetime(2025, 1, 15)],
    )
    observation = 13.5 / 365
    remaining = time - observation
    discount = math.exp(-rate * observation)
    forward = spot * math.exp((rate - dividend) * observation)
    alive = ql.Option.Put if direction == "call" else ql.Option.Call
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
        * (asset * math.exp(-dividend * remaining) - cash * math.exp(-rate * remaining))
    )
    if sigma == 1e-8:
        limit = (
            sign
            * 26
            * (spot * math.exp(-dividend * time) - 100 * math.exp(-rate * time))
        )
        assert expected == pytest.approx(limit, rel=0, abs=1e-9)
    context = market(spot, volatility=sigma, rate=rate, dividend=dividend)
    for nodes, tolerance in ((512, 0.002), (2048, 0.00002)):
        with subtests.test(direction=direction, sigma=sigma, nodes=nodes):
            actual = pricing.QuadratureAccumulatorEngine(asset_step_count=nodes).price(
                option, context
            )
            assert actual == pytest.approx(expected, rel=0, abs=tolerance)


@pytest.mark.parametrize("direction,barrier", (("call", 90), ("put", 110)))
def test_explicit_daily_zero_payoffs_keep_known_statistics_and_skip_grid_validation(
    subtests, direction, barrier
):
    context = market(100)
    monte_carlo = pricing.MonteCarloAccumulatorEngine(path_count=1, seed=42)
    clipped = pricing.FiniteDifferenceAccumulatorEngine(asset_upper_boundary=50)
    for observations in (
        None,
        [START, datetime(2025, 1, 2), datetime(2025, 1, 3)],
        [datetime(2025, 1, 2), datetime(2025, 1, 3)],
    ):
        option = contract(
            option_type=direction,
            knock_out_level=barrier,
            acceleration_factor=0,
            accumulated_quantity=0,
            expiry_time=datetime(2025, 1, 3),
            knock_out_observation_times=observations,
        )
        with subtests.test(direction=direction, observations=observations):
            result = monte_carlo.price_with_statistics(option, context)
            assert result.price == 0
            assert result.standard_error == 0
            assert result.confidence_interval == ((0, 0))
            assert clipped.price(option, context) == 0
            holding = option.replace(
                accumulated_quantity=3,
                knock_out_settled=option.knock_out_settled,
            )
            holding_context = market(
                85 if direction == "call" else 115, rate=0, dividend=0
            )
            assert clipped.price(holding, holding_context) == (-45)
            assert (
                monte_carlo.price_with_statistics(
                    holding, holding_context
                ).standard_error
                == 0
            )


@pytest.mark.parametrize("direction,sign", (("call", 1), ("put", -1)))
@pytest.mark.parametrize("sigma", (1e-08, 0.0001, 0.2))
def test_adjacent_barrier_levels_preserve_forward_prices_and_greeks(
    subtests, direction, sign, sigma
):
    engine = pricing.QuadratureAccumulatorEngine(asset_step_count=128)
    time = 4.5 / 365
    levels = (
        math.nextafter(100, 0),
        100,
        math.nextafter(100, math.inf),
        100 + 1e-11,
        100 + 1e-8,
    )
    expected = (
        sign * 6 * (100.001 * math.exp(-0.06 * time) - 100 * math.exp(-0.04 * time))
    )
    delta = sign * 6 * math.exp(-0.06 * time)
    context = market(100.001, volatility=sigma, rate=0.04, dividend=0.06)
    for barrier in levels:
        option = contract(
            option_type=direction,
            knock_out_level=barrier,
            acceleration_factor=1,
            knock_out_observation_times=[],
        )
        with subtests.test(direction=direction, sigma=sigma, barrier=barrier):
            result = engine.price_with_greeks(
                option,
                context,
                ["delta", "gamma"],
            )
            assert result.price == pytest.approx(expected, rel=0, abs=1e-8)
            assert result.delta == pytest.approx(delta, rel=0, abs=1e-7)
            assert result.gamma == pytest.approx(0, rel=0, abs=1e-6)


@pytest.mark.parametrize(
    "direction,barrier,side,sign",
    (("call", 110, ql.Option.Put, 1), ("put", 90, ql.Option.Call, -1)),
)
def test_terminal_digital_portfolios_match_quantlib(
    subtests, direction, barrier, side, sign
):
    context = market(100.25, datetime(2025, 1, 3, 12))
    time = 2.5 / 365
    model = context.model_parameters
    discount = math.exp(-model.flat_risk_free_rate * time)
    forward = context.spot_price * math.exp(
        (model.flat_risk_free_rate - model.flat_dividend_yield) * time
    )
    deviation = model.volatility * math.sqrt(time)
    unit = discount * (forward - 100)

    def digital(level, side=side):
        asset = ql.AssetOrNothingPayoff(side, level)
        cash = ql.CashOrNothingPayoff(side, level, 1)
        return (
            ql.BlackCalculator(asset, forward, deviation, discount).value()
            - 100 * ql.BlackCalculator(cash, forward, deviation, discount).value()
        )

    for observations in (None, []):
        option = contract(
            option_type=direction,
            knock_out_level=barrier,
            knock_out_observation_times=observations,
        )
        expected = sign * (
            3 * unit
            + (digital(barrier) if observations is None else unit)
            + digital(100)
        )
        with subtests.test(direction=direction, observations=observations):
            assert pricing.QuadratureAccumulatorEngine().price(
                option, context
            ) == pytest.approx(expected, rel=0, abs=1e-9)
