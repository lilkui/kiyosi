"""History snapshots have an explicit valuation time and cannot silently go stale."""

import math
import pickle
from datetime import UTC, datetime, time, timedelta
from functools import partial

import pytest
from kiyosi import InvalidInputError, MissingHistoryError, StaleHistoryError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from kiyosi.market import custom_calendar
from test_contract_replacements import contract_inputs, contracts

AT = datetime(2025, 1, 6, 12)


def context(at=AT, volatility=0.2):
    return m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=volatility
        ),
        spot_price=100,
        valuation_time=at,
    )


def engine(instrument):
    for cls, factory in (
        (i.BarrierOption, p.AnalyticBarrierEngine),
        (i.DoubleBarrierOption, p.AnalyticDoubleBarrierEngine),
        (i.BinaryBarrierOption, p.AnalyticBinaryBarrierEngine),
        (i.TouchOption, p.AnalyticBinaryBarrierEngine),
        (i.DoubleTouchOption, p.AnalyticDoubleTouchEngine),
        (i.RangeAccrualOption, p.AnalyticRangeAccrualEngine),
        (i.Accumulator, p.QuadratureAccumulatorEngine),
        (i.GeometricAveragePriceOption, p.AnalyticGeometricAveragePriceEngine),
        (i.ArithmeticAveragePriceOption, p.TurnbullWakemanArithmeticAveragePriceEngine),
    ):
        if isinstance(instrument, cls):
            return factory()
    return p.QuadratureAutocallableEngine()


def test_touched_history_must_be_possible_at_its_own_timestamp():
    expected_prices = {
        i.BarrierOption: 0,
        i.DoubleBarrierOption: 0,
        i.cash_binary_barrier_option: 0,
        i.asset_binary_barrier_option: 0,
        i.cash_touch_option: 10,
        i.asset_touch_option: 100,
        i.cash_double_touch: 10,
    }
    for factory, fields in contract_inputs():
        if factory not in expected_prices:
            continue
        for scheduled in (False, True):
            first = fields["effective_time"] + timedelta(days=int(scheduled))
            fields = dict(
                fields,
                touch_state="untouched",
                history_time=first,
                observation_times=[first, fields["expiry_time"]] if scheduled else None,
            )
            with pytest.raises(InvalidInputError):
                factory(**dict(fields, touch_state="touched"))
            option = factory(**fields)
            selected = engine(option)
            for method in (option.replace, option.replace_history):
                with pytest.raises(InvalidInputError) as raised:
                    method(touch_state="touched")
                assert type(raised.value) is InvalidInputError
            valid = option.replace_history(
                touch_state="touched",
                history_time=first + timedelta(microseconds=1),
            )
            assert (
                selected.price(valid, context(first + timedelta(days=1)))
                == expected_prices[factory]
            )


def test_history_timestamps_cannot_follow_valuation_without_payloads(subtests):
    for factory, fields in contract_inputs():
        fields = fields.copy()
        for name in (
            "touch_state",
            "barrier_state",
            "realized_average",
            "realized_hit_count",
            "accumulated_quantity",
            "knock_out_settled",
        ):
            if name in fields:
                fields[name] = None
        option = factory(**fields)
        selected = engine(option)
        market = context(option.effective_time)
        baseline = selected.price(option, market)
        with subtests.test(contract=factory.__name__):
            assert (
                selected.price(
                    option.replace(history_time=market.valuation_time), market
                )
                == baseline
            )
            future = option.replace(
                history_time=market.valuation_time + timedelta(microseconds=1)
            )
            with pytest.raises(InvalidInputError) as raised:
                selected.price(future, market)
            assert type(raised.value) is InvalidInputError


def test_autocallable_history_respects_first_daily_observation_on_sparse_calendar(
    subtests,
):
    start, first, end = (
        datetime(2025, 1, 2, 13),
        datetime(2025, 1, 6, 12),
        datetime(2025, 1, 6, 16),
    )
    note = i.SnowballOption(
        maturity_coupon_rate=0,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        effective_time=start,
        expiry_time=end,
        observations=[i.AutocallableObservation.snowball(end, 110, coupon_rate=0)],
        observation_clock_time=time(12),
        calendar=custom_calendar(
            base=m.all_days_calendar(),
            closed_dates=[(start + timedelta(days=day)).date() for day in (1, 2, 3)],
        ),
    )
    selected = p.QuadratureAutocallableEngine()
    for at in (
        start,
        first - timedelta(microseconds=1),
        first,
        first + timedelta(microseconds=1),
    ):
        snapshot = context(at, volatility=0)
        with subtests.test(at=at):
            if at <= first:
                assert selected.price(note, snapshot) == 1
            else:
                with pytest.raises(MissingHistoryError):
                    selected.price(note, snapshot)
            if at <= first:
                with pytest.raises(InvalidInputError):
                    note.replace_history(barrier_state="knocked_in", history_time=at)
            else:
                knocked_in = note.replace_history(
                    barrier_state="knocked_in", history_time=at
                )
                assert selected.price(knocked_in, snapshot) == 1
            known = note.replace_history(barrier_state="no_event", history_time=at)
            assert selected.price(known, snapshot) == 1


@pytest.mark.parametrize(
    "cls", (i.GeometricAveragePriceOption, i.ArithmeticAveragePriceOption)
)
def test_average_snapshots_require_elapsed_averaging_at_construction_and_replacement(
    subtests, cls
):
    start = datetime(2025, 1, 2)
    averaging, end = start + timedelta(days=1), start + timedelta(days=10)
    fields = {
        "option_type": "call",
        "strike": 100,
        "effective_time": start,
        "averaging_start_time": averaging,
        "expiry_time": end,
    }
    original = cls(**fields)
    for at in (start, averaging):
        with subtests.test(contract=cls.__name__, at=at):
            with pytest.raises(InvalidInputError):
                cls(**fields, realized_average=100, history_time=at)
            with pytest.raises(InvalidInputError):
                original.replace_history(realized_average=100, history_time=at)
    at = averaging + timedelta(microseconds=1)
    seasoned = original.replace_history(realized_average=100, history_time=at)
    assert seasoned.history_time == at
    with pytest.raises(InvalidInputError):
        seasoned.replace(averaging_start_time=at, realized_average=100)
    assert original.realized_average is None


def test_redating_an_average_requires_explicit_history_and_preserves_the_price():
    start = datetime(2025, 1, 2)
    at, later = start + timedelta(days=4), start + timedelta(days=18)
    option = i.ArithmeticAveragePriceOption(
        option_type="call",
        strike=100,
        effective_time=start,
        averaging_start_time=start,
        expiry_time=start + timedelta(days=32),
        realized_average=100,
        history_time=at,
    )
    for method in (option.replace, option.replace_history):
        with pytest.raises(StaleHistoryError):
            method(history_time=later)
    refreshed = option.replace_history(
        realized_average=(4 * 100 + 14 * 120) / 18,
        history_time=later,
    )
    market = context(later, volatility=0).replace(spot_price=120)
    assert engine(option).price(refreshed, market) == pytest.approx(
        17.5, rel=0, abs=1e-12
    )
    assert option.history_time == at
    assert option.realized_average == 100


@pytest.mark.parametrize(
    "cls,fields,payload",
    (
        (
            i.Accumulator,
            {
                "strike": 100,
                "knock_out_level": 120,
                "daily_quantity": 1,
                "acceleration_factor": 2,
            },
            "accumulated_quantity",
        ),
        (
            i.RangeAccrualOption,
            {
                "notional": 100,
                "annual_coupon_rate": 0.1,
                "lower_bound": 90,
                "upper_bound": 110,
            },
            "realized_hit_count",
        ),
    ),
)
def test_accrued_history_requires_observations_before_its_snapshot(
    subtests, cls, fields, payload
):
    start = datetime(2025, 1, 6)
    first = start.replace(hour=15)
    common = {
        "effective_time": start,
        "expiry_time": start + timedelta(days=3),
        "observation_clock_time": time(15),
    }
    original = cls(**common, **fields)
    for at in (
        first - timedelta(microseconds=1),
        first,
        first + timedelta(microseconds=1),
    ):
        changes = {payload: 1, "history_time": at}
        for make in (
            partial(cls, **common, **fields, **changes),
            partial(original.replace, **changes),
            partial(original.replace_history, **changes),
        ):
            with subtests.test(contract=cls.__name__, at=at):
                if at > first:
                    result = make()
                    assert getattr(result, payload) == 1
                    assert result.history_time == at
                else:
                    with pytest.raises(InvalidInputError):
                        make()
    assert getattr(original, payload) is None
    assert original.history_time is None


def test_accumulator_history_refresh_requires_only_known_payloads():
    start = datetime(2025, 1, 6)
    at, later = start + timedelta(days=1), start + timedelta(days=2)
    original = i.Accumulator(
        strike=100,
        knock_out_level=120,
        daily_quantity=1,
        acceleration_factor=2,
        effective_time=start,
        expiry_time=start + timedelta(days=4),
        observation_clock_time=time(),
        knock_out_observation_times=[],
        accumulated_quantity=1,
        history_time=at,
    )
    market = context(at, volatility=0).replace(spot_price=105)
    selected = engine(original)
    for method in (original.replace, original.replace_history):
        refreshed = method(accumulated_quantity=2, history_time=later)
        assert refreshed.accumulated_quantity == 2
        assert refreshed.knock_out_settled is None
        assert refreshed.history_time == later
        assert selected.price(
            refreshed, market.replace(valuation_time=later)
        ) == pytest.approx(25, rel=0, abs=1e-12)
        with pytest.raises(StaleHistoryError):
            method(history_time=later)
    assert selected.price(original, market) == pytest.approx(25, rel=0, abs=1e-12)
    known = original.replace(knock_out_settled=False)
    for changes in ({"accumulated_quantity": 2}, {"knock_out_settled": False}):
        with pytest.raises(StaleHistoryError):
            known.replace_history(**changes, history_time=later)
    settled = known.replace(
        accumulated_quantity=None,
        knock_out_settled=True,
        knock_out_observation_times=None,
    )
    refreshed = settled.replace_history(knock_out_settled=True, history_time=later)
    assert refreshed.accumulated_quantity is None
    assert refreshed.history_time == later
    assert selected.price(refreshed, market.replace(valuation_time=later)) == 0
    assert original.history_time == at
    assert not (known.knock_out_settled)


def test_clearing_history_clears_only_implicit_timestamps_in_every_family(subtests):
    names = (
        "touch_state",
        "barrier_state",
        "realized_average",
        "realized_hit_count",
        "accumulated_quantity",
        "knock_out_settled",
    )
    for original in contracts():
        if hasattr(original, "realized_average"):
            original = original.replace_history(realized_average=100, history_time=AT)
        original = original.replace(history_time=AT)
        fields = {name: None for name in names if hasattr(original, name)}
        for method in ("replace", "replace_history"):
            with subtests.test(contract=type(original).__name__, method=method):
                replace = getattr(original, method)
                cleared = replace(**fields)
                assert cleared.history_time is None
                assert all(getattr(cleared, name) is None for name in fields)
                assert replace() == original
                assert replace(**{name: ... for name in fields}) == original
                kept = replace(**fields, history_time=AT)
                assert kept.history_time == AT
                assert kept.replace_history() == kept
                with pytest.raises(InvalidInputError):
                    replace(
                        **fields,
                        history_time=original.expiry_time + timedelta(days=1),
                    )
                assert original.history_time == AT
                assert pickle.loads(pickle.dumps(cleared)) == cleared


@pytest.mark.parametrize(
    "cls", (i.ArithmeticAveragePriceOption, i.GeometricAveragePriceOption)
)
def test_cleared_average_can_shorten_expiry_before_its_old_snapshot(cls):
    start, at, end = (
        datetime(2025, 1, 2),
        datetime(2025, 2, 1),
        datetime(2026, 1, 2),
    )
    expiry = datetime(2025, 1, 15)
    original = cls(
        option_type="call",
        strike=100,
        effective_time=start,
        averaging_start_time=start,
        expiry_time=end,
        realized_average=100,
        history_time=at,
    )
    cleared = original.replace_history(realized_average=None)
    assert cleared.replace(expiry_time=expiry).expiry_time == expiry
    assert (
        original.replace(realized_average=None, expiry_time=expiry).history_time is None
    )
    with pytest.raises(InvalidInputError):
        original.replace(realized_average=None, expiry_time=expiry, history_time=at)
    assert original.history_time == at


def test_partial_accumulator_history_clears_retain_the_shared_timestamp(subtests):
    original = next(
        option for option in contracts() if isinstance(option, i.Accumulator)
    ).replace_history(accumulated_quantity=0, knock_out_settled=False, history_time=AT)
    for first, last in (
        ("accumulated_quantity", "knock_out_settled"),
        ("knock_out_settled", "accumulated_quantity"),
    ):
        for method in ("replace", "replace_history"):
            with subtests.test(first=first, method=method):
                partial = getattr(original, method)(**{first: None})
                assert partial.history_time == AT
                assert getattr(partial, last) == getattr(original, last)
                assert getattr(partial, method)(**{last: None}).history_time is None
    assert original.accumulated_quantity == 0
    assert not (original.knock_out_settled)


def test_dated_average_refresh_updates_value_and_time_atomically(subtests):
    start = datetime(2025, 1, 2)
    at, end = start + timedelta(days=4), start + timedelta(days=10)
    market = context(at, volatility=0)
    later = market.replace(valuation_time=at + timedelta(days=1))
    for cls, expected in (
        (i.GeometricAveragePriceOption, math.sqrt(104 * 100) - 100),
        (i.ArithmeticAveragePriceOption, 2),
    ):
        with subtests.test(contract=cls.__name__):
            original = cls(
                option_type="call",
                strike=100,
                effective_time=start,
                averaging_start_time=start,
                expiry_time=end,
                realized_average=100,
                history_time=at,
            )
            refreshed = original.replace(
                realized_average=104, history_time=later.valuation_time
            )
            assert refreshed.realized_average == 104
            assert refreshed.history_time == later.valuation_time
            assert engine(original).price(refreshed, later) == pytest.approx(
                expected, rel=0, abs=1e-12
            )
            with pytest.raises(InvalidInputError):
                original.replace(
                    realized_average=104, history_time=end + timedelta(days=1)
                )
            assert original.realized_average == 100
            assert original.history_time == at
            assert engine(original).price(original, market) == 0


def test_dated_history_survives_future_schedule_edits(subtests):
    past, future = datetime(2025, 1, 3), datetime(2025, 1, 8)
    for original in contracts():
        with subtests.test(contract=type(original).__name__):
            if hasattr(original, "touch_state"):
                original = original.replace(
                    observation_times=[past, original.expiry_time],
                    touch_state=original.touch_state,
                    history_time=AT,
                )
                updated = original.replace(
                    observation_times=[past, AT, future, original.expiry_time]
                )
                assert updated.observation_times == (
                    (past, AT, future, original.expiry_time)
                )
                assert updated.touch_state == original.touch_state
                with pytest.raises(InvalidInputError):
                    original.replace(observation_times=[future, original.expiry_time])
            elif hasattr(original, "observations"):
                record = original.observations[0]
                original = original.replace(
                    observations=[
                        record.replace(time=past),
                        record.replace(time=original.expiry_time),
                    ],
                    barrier_state=original.barrier_state,
                    history_time=AT,
                )
                updated = original.replace(
                    observations=[
                        original.observations[0],
                        record.replace(time=AT),
                        record.replace(time=future),
                        original.observations[-1],
                    ]
                )
                assert updated.observation_times == (
                    (past, AT, future, original.expiry_time)
                )
                assert updated.barrier_state == original.barrier_state
                with pytest.raises(InvalidInputError):
                    original.replace(observations=original.observations[1:])
            else:
                continue
            assert updated.history_time == AT
            assert original.observation_times == ((past, original.expiry_time))
            extended = original.replace(
                expiry_time=original.expiry_time + timedelta(days=1)
            )
            assert extended.history_time == AT


def test_dated_daily_history_survives_future_calendar_edits(subtests):
    for original in contracts():
        if not hasattr(original, "calendar") or hasattr(original, "observation_mode"):
            continue
        if isinstance(original, i.BinarySnowballOption):
            continue
        original = original.replace(history_time=AT)
        with subtests.test(contract=type(original).__name__):
            future_calendar = custom_calendar(
                base=original.calendar, closed_dates=[datetime(2025, 1, 8).date()]
            )
            updated = original.replace(calendar=future_calendar)
            assert updated.history_time == AT
            assert updated.calendar == future_calendar
            past_calendar = custom_calendar(
                base=original.calendar, closed_dates=[datetime(2025, 1, 3).date()]
            )
            with pytest.raises(InvalidInputError):
                original.replace(calendar=past_calendar)


def test_settled_history_must_be_possible_at_its_own_timestamp():
    start, end = datetime(2025, 1, 2), datetime(2025, 1, 10)
    option = i.Accumulator(
        strike=100,
        knock_out_level=120,
        daily_quantity=1,
        acceleration_factor=2,
        effective_time=start,
        expiry_time=end,
        observation_clock_time=time(15),
        knock_out_settled=True,
        history_time=AT,
    )
    selected = engine(option)
    with pytest.raises(InvalidInputError):
        option.replace_history(knock_out_settled=True, history_time=start)
    with pytest.raises(InvalidInputError):
        selected.price(
            option.replace_history(
                knock_out_settled=True,
                accumulated_quantity=option.accumulated_quantity,
                history_time=AT + timedelta(days=1),
            ),
            context(),
        )
    assert (
        selected.price(
            option.replace_history(
                knock_out_settled=True,
                accumulated_quantity=option.accumulated_quantity,
                history_time=AT,
            ),
            context(),
        )
        == 0
    )
    note = i.BinarySnowballOption(
        calendar=m.weekdays_calendar(),
        maturity_coupon_rate=0.1,
        effective_time=start,
        expiry_time=end,
        observations=[
            i.AutocallableObservation.snowball(datetime(2025, 1, 3), 110, coupon_rate=0)
        ],
        barrier_state="knocked_out",
        history_time=AT,
    )
    selected = engine(note)
    with pytest.raises(InvalidInputError):
        note.replace_history(barrier_state="knocked_out", history_time=start)
    assert (
        selected.price(
            note.replace_history(barrier_state=note.barrier_state, history_time=AT),
            context(),
        )
        == 0
    )


def test_every_path_dependent_family_requires_dated_history(subtests):
    for factory, fields in contract_inputs():
        if "realized_average" in fields:
            fields["realized_average"] = 100
        if "observations" in fields:
            fields["observations"] = [
                fields["observations"][0].replace(time=AT - timedelta(days=3)),
                *fields["observations"],
            ]
        with subtests.test(contract=factory.__name__):
            for omitted in ({}, {"history_time": None}):
                with pytest.raises(MissingHistoryError):
                    factory(**fields, **omitted)
            original = factory(**fields, history_time=AT)
            selected = engine(original)
            dated = original.replace(history_time=AT)
            assert math.isfinite(selected.price(dated, context()))
            payloads = {
                name: None
                for name in (
                    "touch_state",
                    "barrier_state",
                    "realized_average",
                    "realized_hit_count",
                    "accumulated_quantity",
                    "knock_out_settled",
                )
                if hasattr(dated, name)
            }
            missing = dated.replace_history(**payloads)
            with pytest.raises(MissingHistoryError):
                selected.price(missing, context())
            with pytest.raises(StaleHistoryError):
                dated.replace(history_time=AT - timedelta(microseconds=1))
            assert original.history_time == AT
            assert dated.history_time == AT
            assert pickle.loads(pickle.dumps(dated)) == dated
            assert dated.replace() == dated
            assert dated.replace_history() == dated
            for method in (dated.replace, dated.replace_history):
                with pytest.raises(StaleHistoryError):
                    method(history_time=AT + timedelta(microseconds=1))
            with pytest.raises(InvalidInputError):
                selected.price(dated.replace(history_time=None), context())
            for invalid in (
                original.effective_time - timedelta(microseconds=1),
                original.expiry_time + timedelta(microseconds=1),
            ):
                with pytest.raises(InvalidInputError):
                    dated.replace(history_time=invalid)
                with pytest.raises(InvalidInputError):
                    dated.replace_history(history_time=invalid)
            for boundary in (original.effective_time, original.expiry_time):
                assert (
                    dated.replace(**payloads, history_time=boundary).history_time
                    == boundary
                )
            assert dated.history_time == AT
            for bad in (True, AT.date(), AT.replace(tzinfo=UTC)):
                with pytest.raises(TypeError):
                    original.replace(history_time=bad)


def test_history_construction_rejects_timestamps_outside_contract_life():
    start, end = datetime(2025, 1, 2), datetime(2025, 1, 10)
    terms = {
        "option_type": "call",
        "strike": 100,
        "effective_time": start,
        "expiry_time": end,
        "barrier_level": 120,
        "barrier_type": "up_and_out",
        "touch_state": "untouched",
    }
    for at in (start - timedelta(microseconds=1), end + timedelta(microseconds=1)):
        with pytest.raises(InvalidInputError):
            i.BarrierOption(**terms, history_time=at)
    for at in (start, end):
        assert i.BarrierOption(**terms, history_time=at).history_time == at


def test_history_time_and_contract_life_are_replaced_together():
    start, end = datetime(2025, 1, 2), datetime(2025, 1, 10)
    option = i.BarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_out",
        touch_state="untouched",
        history_time=AT,
    )
    expiry = start + timedelta(days=1)
    with pytest.raises(InvalidInputError):
        option.replace(expiry_time=expiry, touch_state="untouched")
    replaced = option.replace(
        expiry_time=expiry, touch_state="untouched", history_time=expiry
    )
    assert replaced.history_time == expiry
    assert replaced.expiry_time == expiry
    assert option.history_time == AT
    assert option.expiry_time == end


def test_discrete_history_reuses_between_events_and_requires_refresh_after_them():
    option = i.RangeAccrualOption(
        notional=100,
        annual_coupon_rate=0.1,
        lower_bound=90,
        upper_bound=110,
        effective_time=datetime(2025, 1, 2),
        expiry_time=datetime(2025, 1, 10),
        observation_clock_time=time(15),
        realized_hit_count=1,
        history_time=AT,
    )
    selected = engine(option)
    market = context(volatility=0)
    price = selected.price(option, market)
    event = AT.replace(hour=15)
    for at in (AT, event - timedelta(microseconds=1), event):
        assert selected.price(option, market.replace(valuation_time=at)) == price
    rolled = market.replace(valuation_time=event + timedelta(microseconds=1))
    with pytest.raises(InvalidInputError):
        selected.price(option, rolled)
    refreshed = option.replace_history(
        realized_hit_count=2, history_time=rolled.valuation_time
    )
    assert selected.price(refreshed, rolled) == price
    assert option.history_time == AT
    with pytest.raises(InvalidInputError):
        selected.price(option, market.replace(valuation_time=option.effective_time))


@pytest.mark.parametrize(
    "cls,factory",
    (
        (i.GeometricAveragePriceOption, p.AnalyticGeometricAveragePriceEngine),
        (i.ArithmeticAveragePriceOption, p.TurnbullWakemanArithmeticAveragePriceEngine),
    ),
)
def test_continuous_history_rejects_reuse_but_numerical_time_risk_keeps_it_frozen(
    cls, factory
):
    option = cls(
        option_type="call",
        strike=100,
        effective_time=datetime(2025, 1, 2),
        averaging_start_time=datetime(2025, 1, 2),
        expiry_time=datetime(2025, 2, 2),
        realized_average=100,
        history_time=AT,
    )
    selected = factory()
    market = context()
    assert math.isfinite(
        selected.price_with_greeks(option, market, "theta").require("theta")
    )
    rolled = market.replace(valuation_time=AT + timedelta(microseconds=1))
    with pytest.raises(InvalidInputError):
        selected.price(option, rolled)
    assert math.isfinite(
        selected.price(
            option.replace_history(
                realized_average=100, history_time=rolled.valuation_time
            ),
            rolled,
        )
    )
    assert option.history_time == AT
