import math
import pickle
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime, time, timedelta
from datetime import time as clock_time
from functools import partial
from itertools import pairwise

import pytest
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    MissingHistoryError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import Accumulator, EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    custom_calendar,
    weekdays_calendar,
)
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks

START = datetime(2025, 1, 1)


def test_termination_history_is_required_after_prior_knock_out_observations(subtests):
    option = Accumulator(
        strike=100,
        knock_out_level=110,
        daily_quantity=1,
        acceleration_factor=1,
        effective_time=datetime(2025, 1, 6),
        expiry_time=datetime(2025, 1, 10),
        observation_clock_time=clock_time(),
        accumulated_quantity=2,
    )
    context = market(
        spot=105, valuation=datetime(2025, 1, 8), volatility=0, rate=0, dividend=0
    )
    assert option.knock_out_settled is None
    active = option.replace(
        knock_out_settled=False,
    )
    settled = option.replace(
        knock_out_settled=True,
    )
    assert active.knock_out_settled is False
    assert settled.knock_out_settled is True
    assert active.replace(knock_out_settled=None) == option
    for value in (option, active, settled):
        assert pickle.loads(pickle.dumps(value)) == value
    for engine in (
        pricing.QuadratureAccumulatorEngine(),
        pricing.FiniteDifferenceAccumulatorEngine(),
        pricing.MonteCarloAccumulatorEngine(),
    ):
        with subtests.test(engine=type(engine).__name__):
            with pytest.raises(InvalidInputError):
                engine.price(option, context)
            assert engine.price(active, context) == 25
            assert engine.price(settled, context) == 0
            initial = context.replace(valuation_time=option.effective_time)
            assert (
                engine.price(
                    option.replace(accumulated_quantity=None),
                    initial,
                )
                == 25
            )
            with pytest.raises(InvalidInputError):
                engine.price(settled, initial)
            no_knock_out = option.replace(
                knock_out_observation_times=[],
                accumulated_quantity=2,
                knock_out_settled=None,
            )
            assert engine.price(no_knock_out, context) == 25
    for invalid in (0, 1, "yes"):
        with subtests.test(invalid=invalid), pytest.raises(TypeError):
            option.replace(
                knock_out_settled=invalid,
            )


def contract(
    end=datetime(2025, 2, 3),
    knock_out=110,
    acceleration=2,
    daily=1,
    quantity=3,
    calendar=None,
):
    return Accumulator(
        knock_out_settled=False,
        observation_clock_time=clock_time(),
        strike=100,
        knock_out_level=knock_out,
        daily_quantity=daily,
        acceleration_factor=acceleration,
        accumulated_quantity=quantity,
        effective_time=START,
        expiry_time=end,
        calendar=weekdays_calendar() if calendar is None else calendar,
    )


def market(spot=100.25, valuation=None, volatility=0.2, rate=0.04, dividend=0.01):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=volatility
        ),
        spot_price=spot,
        valuation_time=valuation or datetime(2025, 1, 1, 12),
    )


def terminal_reference(option, context):
    horizon = (
        datetime.combine(option.expiry_time, time()) - context.valuation_time
    ).total_seconds() / (365 * 86400)
    parameters = context.model_parameters
    deviation = parameters.volatility * math.sqrt(horizon)
    stock = context.spot_price * math.exp(-parameters.flat_dividend_yield * horizon)
    cash = option.strike * math.exp(-parameters.flat_risk_free_rate * horizon)

    def lower_payoff(level):
        d2 = (
            math.log(context.spot_price / level)
            + (
                parameters.flat_risk_free_rate
                - parameters.flat_dividend_yield
                - parameters.volatility**2 / 2
            )
            * horizon
        ) / deviation
        return (
            stock * math.erfc((d2 + deviation) / math.sqrt(2))
            - cash * math.erfc(d2 / math.sqrt(2))
        ) / 2

    purchase = (
        lower_payoff(option.knock_out_level)
        + (option.acceleration_factor - 1) * lower_payoff(option.strike)
        if option.knock_out_level > option.strike
        else option.acceleration_factor * lower_payoff(option.knock_out_level)
    )
    return (
        option.accumulated_quantity * (stock - cash) + option.daily_quantity * purchase
    )


def exposure(option, context):
    current = context.valuation_time
    observations = 0
    while current <= option.expiry_time:
        if datetime.combine(
            current, time()
        ) >= context.valuation_time and option.calendar.is_trading_day(current.date()):
            observations += 1
        current += timedelta(days=1)
    return option.strike * (
        option.accumulated_quantity
        + observations * option.daily_quantity * max(1, option.acceleration_factor)
    )


@pytest.mark.parametrize(
    "direction,spot,barrier,sign,purchase",
    (
        ("call", 120, 110, 1, 1),
        ("call", 80, 110, 1, 2),
        ("put", 80, 90, -1, 1),
        ("put", 120, 90, -1, 2),
    ),
)
@pytest.mark.parametrize("knocks_out", (False, True))
def test_quadrature_tails_exclude_closed_dates_and_purchases_at_knock_out(
    direction, spot, barrier, sign, purchase, knocks_out
):
    expiry = datetime(2025, 1, 4)
    knock_out_time = datetime(2025, 1, 3)
    calendar = custom_calendar(
        base=weekdays_calendar(), closed_dates=[datetime(2025, 1, 2).date()]
    )
    engine = pricing.QuadratureAccumulatorEngine()
    context = market(spot=spot, volatility=1e-8)
    option = Accumulator(
        option_type=direction,
        strike=100,
        knock_out_level=barrier,
        daily_quantity=1,
        acceleration_factor=2,
        effective_time=START,
        expiry_time=expiry,
        observation_clock_time=clock_time(),
        accumulated_quantity=3,
        knock_out_settled=False,
        knock_out_observation_times=[knock_out_time] if knocks_out else [],
        calendar=calendar,
    )
    terminates = knocks_out and purchase == 1
    settlement = knock_out_time if terminates else expiry
    horizon = (settlement - context.valuation_time).total_seconds() / (365 * 86400)
    quantity = 3 + (0 if terminates else purchase)
    expected = (
        sign
        * quantity
        * (spot * math.exp(-0.01 * horizon) - 100 * math.exp(-0.04 * horizon))
    )
    assert engine.price(option, context) == pytest.approx(expected, rel=0, abs=1e-7)


def test_unknown_history_requires_an_explicit_quantity_after_prior_observations(
    subtests,
):
    for start, first in (
        (datetime(2025, 1, 2), datetime(2025, 1, 2)),
        (datetime(2025, 1, 4), datetime(2025, 1, 6)),
    ):
        option = Accumulator(
            knock_out_settled=False,
            observation_clock_time=clock_time(),
            strike=90,
            knock_out_level=150,
            daily_quantity=1,
            acceleration_factor=2,
            effective_time=start,
            expiry_time=datetime(2025, 1, 7),
        )
        for engine in (
            pricing.QuadratureAccumulatorEngine(),
            pricing.FiniteDifferenceAccumulatorEngine(),
            pricing.MonteCarloAccumulatorEngine(path_count=64, seed=42),
        ):
            with subtests.test(start=start, engine=type(engine).__name__):
                assert option.accumulated_quantity is None
                current = market(spot=100, valuation=first, rate=0, dividend=0)
                assert math.isfinite(engine.price(option, current))
                for valuation in (start, first):
                    assert_rejection(
                        InvalidInputError,
                        engine.price,
                        option.replace(accumulated_quantity=3),
                        current.replace(valuation_time=valuation),
                    )
                prior = current.replace(
                    valuation_time=datetime.combine(first, time(12))
                )
                assert_rejection(
                    MissingHistoryError,
                    engine.price,
                    option,
                    prior,
                )
                assert math.isfinite(
                    engine.price(
                        option.replace(
                            accumulated_quantity=0,
                        ),
                        prior,
                    )
                )
                expiry = current.replace(valuation_time=option.expiry_time)
                assert_rejection(
                    MissingHistoryError,
                    engine.price,
                    option,
                    expiry,
                )
                assert (
                    engine.price(
                        option.replace(
                            accumulated_quantity=0,
                        ),
                        expiry,
                    )
                    == 10
                )
                assert (
                    engine.price(
                        option.replace(
                            accumulated_quantity=10,
                            knock_out_settled=option.knock_out_settled,
                        ),
                        expiry,
                    )
                    == 110
                )


def test_contract_history_protocols_validation_and_ownership(subtests):
    fields = {
        "strike": 100,
        "knock_out_level": 110,
        "daily_quantity": 1,
        "acceleration_factor": 2,
        "effective_time": START,
        "expiry_time": datetime(2025, 2, 3),
    }
    option = Accumulator(observation_clock_time=clock_time(), **fields)
    assert option.accumulated_quantity is None
    assert option.strike == 100
    assert option.knock_out_level == 110
    assert option.daily_quantity == 1
    assert option.acceleration_factor == 2
    assert option.effective_time == START
    assert option.expiry_time == fields["expiry_time"]
    updated = option.replace(
        accumulated_quantity=3,
    )
    assert updated.accumulated_quantity == 3
    assert updated.replace(accumulated_quantity=None) == option
    for replacement in (
        updated.replace(),
        updated.replace(
            accumulated_quantity=Ellipsis,
        ),
    ):
        assert replacement == updated
        assert replacement is not updated
    settled_update = updated.replace(knock_out_settled=False)
    assert settled_update.accumulated_quantity == 3
    assert settled_update.knock_out_settled is False
    assert "accumulated_quantity=3" in repr(updated)
    with pytest.raises(AttributeError):
        updated.accumulated_quantity = 9
    for field in (
        "strike",
        "knock_out_level",
        "daily_quantity",
        "acceleration_factor",
        "accumulated_quantity",
    ):
        for value in (True, False, "3", 1 + 2j, object()) + (
            () if field == "accumulated_quantity" else (None,)
        ):
            with (
                subtests.test(field=field, value=value),
                pytest.raises(TypeError),
            ):
                Accumulator(
                    observation_clock_time=clock_time(), **fields | {field: value}
                )
        with subtests.test(field=field), pytest.raises(OverflowError):
            Accumulator(
                observation_clock_time=clock_time(), **fields | {field: 10**400}
            )
        for value in (-1, math.nan, math.inf):
            with pytest.raises(InvalidInputError, match=field):
                Accumulator(
                    observation_clock_time=clock_time(), **fields | {field: value}
                )
    for value in (True, "3", object()):
        with pytest.raises(TypeError):
            updated.replace(
                accumulated_quantity=value,
            )
    for value in (-1, math.nan, math.inf):
        with pytest.raises(InvalidInputError, match="accumulated_quantity"):
            updated.replace(
                accumulated_quantity=value,
            )
    with pytest.raises(OverflowError):
        updated.replace(
            accumulated_quantity=10**400,
        )
    for changes in ({"extra": 0}, {"accumulated_quantity": 3, "extra": 0}):
        with pytest.raises(TypeError):
            updated.replace(**changes)
    with pytest.raises(TypeError):
        updated.replace(3)
    for field in ("effective_time", "expiry_time"):
        for value in (datetime(2025, 1, 1, tzinfo=UTC), "2025-01-01", None):
            with pytest.raises(TypeError):
                Accumulator(
                    observation_clock_time=clock_time(), **fields | {field: value}
                )
    assert_rejection(
        InvalidInputError,
        Accumulator,
        observation_clock_time=clock_time(),
        **fields | {"expiry_time": START - timedelta(days=1)},
    )
    assert_rejection(
        InvalidInputError,
        Accumulator,
        observation_clock_time=clock_time(),
        **fields | {"strike": 0, "expiry_time": START - timedelta(days=1)},
    )
    assert (
        Accumulator(
            observation_clock_time=clock_time(),
            **fields
            | {
                "expiry_time": START,
                "daily_quantity": 0,
                "acceleration_factor": 0,
            },
        ).daily_quantity
        == 0
    )
    assert updated.accumulated_quantity == 3
    # Historical corrections are caller assertions, including zero and fractional quantities.
    assert (
        updated.replace(
            accumulated_quantity=0.25,
        ).accumulated_quantity
        == 0.25
    )
    assert (
        pricing.QuadratureAccumulatorEngine().price(
            contract(datetime(2025, 1, 6), quantity=0),
            market(110, START + timedelta(days=1)),
        )
        == 0
    )


def test_applicability_order_and_unidentifiable_volatility():
    engine = pricing.QuadratureAccumulatorEngine()
    option = contract(datetime(2025, 1, 6))
    context = market(110, datetime(2025, 1, 6))
    curve = RateTermStructure(
        reference_time=START,
        times=[
            datetime(2025, 2, 3),
            datetime(2025, 2, 3) + (datetime(2025, 2, 3) - START),
        ],
        factors=[0.99, (0.99) * 0.5],
    )
    for field in ("risk_free_rate", "dividend_yield"):
        curved = context.replace(
            model_parameters=context.model_parameters.replace(**{field: curve})
        )
        assert_rejection(
            UnsupportedOperationError,
            engine.price,
            option,
            curved,
        )
        assert_rejection(
            UnsupportedOperationError,
            engine.price_with_greeks,
            option,
            curved,
            [],
        )
        assert_rejection(
            UnsupportedOperationError,
            pricing.implied_volatility,
            engine,
            option,
            curved,
            math.nan,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    for instrument, scenario in (
        (option, market(110, START + timedelta(days=1))),
        (option, context),
        (contract(daily=0), market(90, rate=0, dividend=0)),
        (contract(daily=0, quantity=0), market()),
    ):
        quote = engine.price(instrument, scenario)
        assert_rejection(
            UnsupportedOperationError,
            pricing.implied_volatility,
            engine,
            instrument,
            scenario,
            quote,
            lower_bound=0.0,
            upper_bound=4.0,
        )
    assert_rejection(
        InvalidInputError,
        partial(numerical_greeks, engine),
        option,
        market(),
        "delta",
        spot_shift=0,
    )
    for value in (True, "1", None):
        with pytest.raises(TypeError):
            pricing.implied_volatility(
                engine,
                option,
                market(),
                value,
                lower_bound=0.0,
                upper_bound=4.0,
            )
    assert (
        engine.price(
            option,
            market(110, START + timedelta(days=1)),
        )
        == 30
    )


@pytest.mark.parametrize("ko", (95, 100, 110))
def test_two_events_match_independent_conditional_terminal_integral(ko):
    engine = pricing.QuadratureAccumulatorEngine()
    option = contract(datetime(2025, 1, 3), knock_out=ko)
    context = market(100.25, volatility=0.6, rate=-0.02, dividend=0.03)
    t = 0.5 / 365
    width = 0.6 * math.sqrt(t)
    drift = (-0.02 - 0.03 - 0.6**2 / 2) * t
    breaks = sorted(
        set(
            [-12, 12]
            + [
                (math.log(level / 100.25) - drift) / width
                for level in (100, ko)
                if -12 < (math.log(level / 100.25) - drift) / width < 12
            ]
        )
    )
    reference = 0
    for left, right in pairwise(breaks):
        side = 100.25 * math.exp(drift + width * (left + right) / 2)

        def payoff(z, drift=drift, width=width, side=side, ko=ko, option=option):
            spot = 100.25 * math.exp(drift + width * z)
            if side >= ko:
                value = 3 * (spot - 100)
            else:
                quantity = 3 + (2 if side < 100 else 1)
                value = terminal_reference(
                    option.replace(
                        accumulated_quantity=quantity,
                    ),
                    market(spot, datetime(2025, 1, 2), 0.6, -0.02, 0.03),
                )
            return value * math.exp(-z * z / 2) / math.sqrt(2 * math.pi)

        step = (right - left) / 4096
        total = payoff(left) + payoff(right)
        total += sum(
            (2 if i % 2 == 0 else 4) * payoff(left + i * step) for i in range(1, 4096)
        )
        reference += total * step / 3
    reference *= math.exp(0.02 * t)
    assert engine.price(option, context) == pytest.approx(reference, rel=0, abs=2e-5)


def test_configuration_protocols_and_ownership(subtests):
    engine = pricing.QuadratureAccumulatorEngine()
    assert engine.asset_step_count == 512
    assert engine.integration_standard_deviations == 8
    assert "asset_step_count=512" in repr(engine)
    with pytest.raises(AttributeError):
        engine.asset_step_count = 1024
    for value in (True, 4.5, "512"):
        with subtests.test(value=value), pytest.raises(TypeError):
            pricing.QuadratureAccumulatorEngine(asset_step_count=value)
    with pytest.raises(OverflowError):
        pricing.QuadratureAccumulatorEngine(asset_step_count=2**40)
    with pytest.raises(TypeError):
        pricing.QuadratureAccumulatorEngine(integration_standard_deviations=True)
    for kwargs in (
        {"asset_step_count": 3},
        {"integration_standard_deviations": 0},
        {"integration_standard_deviations": math.inf},
        {"integration_standard_deviations": math.nan},
    ):
        with (
            subtests.test(kwargs=kwargs),
            pytest.raises(InvalidInputError) as error,
        ):
            pricing.QuadratureAccumulatorEngine(**kwargs)
        assert type(error.value) is InvalidInputError
    vanilla = EuropeanOption(
        option_type="call",
        strike=100,
        effective_time=START,
        expiry_time=datetime(2025, 2, 3),
    )
    with pytest.raises(UnsupportedOperationError):
        engine.price(vanilla, market())


def test_single_observation_matches_independent_cash_asset_decomposition(subtests):
    valuation = datetime(2025, 1, 3, 12)
    default = pricing.QuadratureAccumulatorEngine()
    minimal = pricing.QuadratureAccumulatorEngine(
        asset_step_count=4, integration_standard_deviations=0.1
    )
    for knock_out in (90, 100, 110):
        for acceleration in (0, 1, 2):
            for spot in (80, 100, 120):
                with subtests.test(
                    knock_out=knock_out, acceleration=acceleration, spot=spot
                ):
                    option = contract(datetime(2025, 1, 6), knock_out, acceleration)
                    context = market(spot, valuation, 0.6, -0.02, 0.03)
                    actual = default.price(option, context)
                    assert actual == pytest.approx(
                        terminal_reference(option, context), rel=0, abs=1e-9
                    )
                    assert actual == minimal.price(option, context)
    option = contract(datetime(2025, 1, 6))
    context = market(100, datetime(2025, 1, 5, 23, 59, 59))
    assert default.price(option, context) == pytest.approx(
        terminal_reference(option, context), rel=0, abs=1e-9
    )


def test_settlement_boundaries_validation_and_failure_reuse(subtests):
    end = datetime(2025, 1, 6)
    option = contract(end)
    engine = pricing.QuadratureAccumulatorEngine()
    for spot, expected in ((90, -50), (100, 0), (109, 36), (110, 30), (111, 33)):
        with subtests.test(spot=spot):
            context = market(spot, end)
            assert engine.price(option, context) == expected
            result = engine.price_with_greeks(option, context, greeks="all")
            assert result.price == expected
            assert_terminal_greeks(
                result,
                None if spot in (100, 110) else expected / (spot - 100),
            )
    assert (
        engine.price(
            option,
            market(110, START + timedelta(days=1)),
        )
        == 30
    )
    for instrument, context, expected_error in (
        (
            option,
            market(100, end + timedelta(days=1)),
            InvalidInputError,
        ),
        (
            contract(end, quantity=sys.float_info.max),
            market(111, end),
            ComputationFailureError,
        ),
    ):
        with (
            subtests.test(expected_error=expected_error),
            pytest.raises(expected_error) as error,
        ):
            engine.price(instrument, context)
        assert type(error.value) is expected_error
    assert engine.price(option, market(110, end)) == 30


def test_quantity_affinity_exact_limits_and_shared_instance_threads():
    engine = pricing.QuadratureAccumulatorEngine()
    context = market(90)
    values = [
        engine.price(contract(quantity=quantity), context) for quantity in (0, 3, 6)
    ]
    assert values[2] - values[1] == pytest.approx(
        values[1] - values[0], rel=0, abs=1e-10
    )
    assert engine.price(contract(daily=0, quantity=0), context) == 0
    assert engine.price(
        contract(daily=0),
        market(90, rate=0, dividend=0),
    ) == (-30)
    assert engine.price(
        contract(datetime(2025, 1, 2)),
        market(90, volatility=1e-8, rate=0, dividend=0),
    ) == pytest.approx((-50), rel=0, abs=1e-9)
    assert engine.price(
        contract(),
        market(90, volatility=1e-8, rate=0, dividend=0),
    ) == pytest.approx((-490), rel=0, abs=1e-9)
    assert engine.price(
        contract(),
        market(120, volatility=1e-8, rate=0, dividend=0),
    ) == pytest.approx(60, rel=0, abs=1e-9)
    option = contract()
    with ThreadPoolExecutor(max_workers=2) as pool:
        values = list(
            pool.map(
                lambda _: engine.price(option, context),
                range(2),
            )
        )
    assert values == [engine.price(option, context)] * 2


def test_current_observation_adjacent_thresholds_and_extreme_result():
    end = datetime(2025, 1, 6)
    option = contract(end)
    engine = pricing.QuadratureAccumulatorEngine()
    for spot, quantity in ((90, 5), (100, 4), (109, 4)):
        context = market(spot, datetime(2025, 1, 3))
        assert engine.price(option, context) == pytest.approx(
            terminal_reference(contract(end, quantity=quantity), context),
            rel=0,
            abs=1e-9,
        )
    for spot, quantity in (
        (math.nextafter(100, 0), 5),
        (math.nextafter(100, 110), 4),
        (math.nextafter(110, 0), 4),
        (math.nextafter(110, 120), 3),
    ):
        assert engine.price(option, market(spot, end)) == quantity * (spot - 100)
    assert engine.price(contract(end, 90), market(90, end)) == (-30)
    assert engine.price(contract(end, 90), market(89, end)) == (-55)
    with pytest.raises(ComputationFailureError) as error:
        engine.price(
            option,
            market(volatility=1e308),
        )
    assert type(error.value) is ComputationFailureError


def test_greek_availability_at_boundaries():
    option = contract()
    engine = pricing.QuadratureAccumulatorEngine()
    boundary = engine.price_with_greeks(
        option,
        market(100, START + timedelta(days=1)),
        greeks="all",
    )
    for greek in ("delta", "gamma", "speed", "theta"):
        assert getattr(boundary, greek) is None
    small = engine.price_with_greeks(
        option,
        market(90, volatility=1e-4),
        greeks=[
            "vega",
            "vanna",
            "zomma",
        ],
        volatility_shift=0.0001,
    )
    for greek in (
        "vega",
        "vanna",
        "zomma",
    ):
        assert getattr(small, greek) is None


def test_reused_spot_and_volatility_curves_match_scalar_prices(subtests):
    engine = pricing.QuadratureAccumulatorEngine(asset_step_count=128)
    option = contract(datetime(2025, 1, 20))
    spot, h = 100.25, 0.25
    volatilities = (0.25, 0.3)
    valuation = datetime(2025, 1, 6) - timedelta(seconds=1)

    def values(vol):
        return [
            engine.price(
                option,
                market(spot + offset * h, valuation, vol, -0.02),
            )
            for offset in (0, 1, -1, 2, -2)
        ]

    expected = []
    for volatility in volatilities:
        v = (volatility + 0.002) - volatility
        center, up, down, up2, down2 = values(volatility)
        high, high_up, high_down, _, _ = values(volatility + v)
        low, low_up, low_down, _, _ = values(volatility - v)
        expected.append(
            (
                center,
                {
                    "delta": (up - down) / (2 * h),
                    "gamma": (up - 2 * center + down) / h**2,
                    "speed": (up2 - 2 * up + 2 * down - down2) / (2 * h**3),
                    "vanna": (high_up - high_down - low_up + low_down) / (4 * h * v),
                    "zomma": (
                        (high_up - 2 * high + high_down) - (low_up - 2 * low + low_down)
                    )
                    / (2 * h**2 * v),
                },
            )
        )

    def evaluate(volatility):
        return numerical_greeks(
            engine,
            option,
            market(spot, valuation, volatility, -0.02),
            [
                "delta",
                "gamma",
                "speed",
                "vanna",
                "zomma",
            ],
            spot_shift=h,
            volatility_shift=0.002,
        )

    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(evaluate, volatilities))
    for result, (center, greeks) in zip(results, expected, strict=True):
        assert result.price == center
        for greek, reference in greeks.items():
            with subtests.test(greek=greek):
                assert getattr(result, greek) == pytest.approx(
                    reference, rel=0, abs=1e-10
                )


def test_implied_volatility_uses_shared_solver():
    option = contract(datetime(2025, 1, 6))
    context = market(100, datetime(2025, 1, 3, 12), volatility=0.4)
    engine = pricing.QuadratureAccumulatorEngine()
    quote = engine.price(option, context)
    volatility = pricing.implied_volatility(
        engine,
        option,
        context,
        quote,
        lower_bound=0.2,
        upper_bound=0.6,
    )
    assert volatility == pytest.approx(0.4, rel=0, abs=1e-6)
