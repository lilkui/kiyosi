"""Caller-owned touch history at public construction, replacement and pricing seams."""

import copy
import pickle
from datetime import timedelta

import pytest
from kiyosi import InvalidInputError, MissingHistoryError
from kiyosi import instruments as i
from kiyosi import market as m
from kiyosi import pricing as p
from test_contract_replacements import contract_inputs


def cases():
    for factory, fields in contract_inputs():
        if "touch_state" not in fields:
            continue
        expected = (
            (100 if fields.get("payoff_type") == "asset" else 10)
            if factory in (i.TouchOption, i.DoubleTouchOption)
            else 0
        )
        engine = (
            p.AnalyticBarrierEngine()
            if factory is i.BarrierOption
            else p.AnalyticDoubleBarrierEngine()
            if factory is i.DoubleBarrierOption
            else p.AnalyticDoubleTouchEngine()
            if factory is i.DoubleTouchOption
            else p.AnalyticBinaryBarrierEngine()
        )
        yield pytest.param(factory, fields, engine, expected, id=factory.__name__)


@pytest.mark.parametrize("factory,fields,engine,expected", list(cases()))
@pytest.mark.parametrize("scheduled", [False, True])
def test_undated_touch_history_reuses_caller_state_and_validates_at_pricing(
    factory, fields, engine, expected, scheduled
):
    first = fields["effective_time"] + timedelta(days=int(scheduled))
    terms = dict(
        fields,
        calendar=m.all_days_calendar(),
        touch_state="touched",
        observation_times=[first, fields["expiry_time"]] if scheduled else None,
    )
    option = factory(**terms)
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0.2
        ),
        spot_price=100,
        valuation_time=first,
    )
    with pytest.raises(InvalidInputError) as rejected:
        engine.price(option, context)
    assert type(rejected.value) is InvalidInputError
    for at in (first + timedelta(microseconds=1), first + timedelta(days=2)):
        assert engine.price(option, context.replace(valuation_time=at)) == expected
    cleared = option.replace(touch_state=None)
    with pytest.raises(MissingHistoryError):
        engine.price(cleared, context.replace(valuation_time=first + timedelta(days=2)))
    field = "upper_barrier" if hasattr(option, "upper_barrier") else "barrier_level"
    changed = option.replace(**{field: 130})
    assert changed.touch_state == "touched"
    assert changed == factory(**(terms | {field: 130}))
    with pytest.raises(InvalidInputError):
        option.replace(**{field: -1}, touch_state=None)
    assert getattr(option, field) == 120
    assert option.touch_state == "touched"
    assert option.replace(touch_state=...) == option.replace() == option
    for cloned in (
        copy.copy(option),
        copy.deepcopy(option),
        pickle.loads(pickle.dumps(option)),
    ):
        assert cloned == option
        assert cloned.touch_state == "touched"


def test_scheduled_touch_at_valuation_is_processed_once_with_current_spot():
    from datetime import datetime

    start = datetime(2025, 1, 2)
    first = start + timedelta(days=1)
    option = i.BarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=start + timedelta(days=8),
        barrier_level=120,
        barrier_type="up_and_out",
        rebate=7,
        rebate_timing="at_hit",
        observation_times=[first],
        calendar=m.all_days_calendar(),
    )
    context = m.PricingContext(
        model_parameters=m.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0
        ),
        spot_price=100,
        valuation_time=first - timedelta(microseconds=1),
    )
    engine = p.QuadratureBarrierEngine()
    assert engine.price(option, context) == 0
    assert (
        engine.price(option, context.replace(valuation_time=first, spot_price=120)) == 7
    )
    after = context.replace(valuation_time=first + timedelta(microseconds=1))
    assert engine.price(option.replace(touch_state="touched"), after) == 0
    with pytest.raises(MissingHistoryError):
        engine.price(option, after)
