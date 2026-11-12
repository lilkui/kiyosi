"""Construct binary and touch contracts through their public classes."""

import inspect
import pickle
from datetime import datetime

import pytest
from kiyosi import InvalidInputError, market, pricing
from kiyosi import instruments as ins


@pytest.mark.parametrize("direction,level", (("up", 120), ("down", 80)))
@pytest.mark.parametrize(
    "factory,condition,payout,expected",
    (
        (ins.cash_touch_option, "in", {"payout": 10}, 0),
        (ins.cash_no_touch_option, "out", {"payout": 10}, 10),
        (ins.asset_touch_option, "in", {}, 0),
        (ins.asset_no_touch_option, "out", {}, 100),
    ),
)
def test_named_touch_factories_enforce_the_payment_condition(
    direction, level, factory, condition, payout, expected
):
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0, dividend_yield=0, volatility=0
        ),
        spot_price=100,
        valuation_time=start,
    )
    terms = {
        "effective_time": start,
        "expiry_time": end,
        "barrier_level": level,
        **payout,
    }
    option = factory(**terms, barrier_type=f"{direction}_and_{condition}")
    assert pricing.AnalyticBinaryBarrierEngine().price(option, context) == expected
    opposite = "out" if condition == "in" else "in"
    with pytest.raises(InvalidInputError):
        factory(**terms, barrier_type=f"{direction}_and_{opposite}")


@pytest.mark.parametrize(
    "cls,extra",
    (
        (ins.TouchOption, {}),
        (ins.BinaryBarrierOption, {"strike": 100, "option_type": "call"}),
    ),
)
def test_payoff_kind_is_explicit_and_null_amount_cannot_change_cash_settlement(
    cls, extra
):
    common = {
        "effective_time": datetime(2025, 1, 3),
        "expiry_time": datetime(2025, 1, 6),
        "barrier_level": 120,
        "barrier_type": "up_and_in",
    }
    cash = cls(**common, **extra, payoff_type="cash", payout=10)
    for fields, error in (
        ({"payoff_type": "cash"}, TypeError),
        ({"payoff_type": "cash", "payout": None}, TypeError),
        ({"payoff_type": "asset", "payout": 10}, TypeError),
        ({"payoff_type": "unknown"}, ValueError),
        ({"payoff_type": True}, TypeError),
    ):
        with pytest.raises(error):
            cls(**common, **extra, **fields)
    with pytest.raises(TypeError):
        cash.replace(payout=None)
    assert ((cash.payoff_type, cash.payout)) == (("cash", 10))
    asset = cash.replace(payoff_type="asset")
    assert ((asset.payoff_type, asset.payout)) == (("asset", None))
    assert asset.replace(payoff_type="cash", payout=10) == cash


def test_constructors_preserve_settlement_monitoring_and_terminal_payments(subtests):
    start, end = datetime(2025, 1, 3), datetime(2025, 1, 4)
    life = {"effective_time": start, "expiry_time": end}
    single = {"barrier_level": 120, "barrier_type": "up_and_in"}
    binary = dict(single, option_type="call", strike=100)
    double = {"lower_barrier": 80, "upper_barrier": 120}
    calendar = market.all_days_calendar()
    monitoring = {
        "observation_times": [end],
        "touch_state": "untouched",
        "calendar": calendar,
    }
    context = market.PricingContext(
        model_parameters=market.BlackScholesMertonParameters(
            risk_free_rate=0.02, dividend_yield=0, volatility=0.2
        ),
        spot_price=125,
        valuation_time=end,
    )
    cases = [
        (
            ins.BinaryBarrierOption,
            ins.cash_binary_barrier_option,
            dict(binary, payout=10),
            10,
        ),
        (
            ins.BinaryBarrierOption,
            ins.asset_binary_barrier_option,
            dict(binary, payout=None),
            125,
        ),
        (ins.TouchOption, ins.cash_touch_option, dict(single, payout=10), 10),
        (ins.TouchOption, ins.asset_touch_option, dict(single, payout=None), 125),
        (
            ins.TouchOption,
            ins.cash_no_touch_option,
            dict(single, barrier_type="up_and_out", payout=10),
            0,
        ),
        (
            ins.TouchOption,
            ins.asset_no_touch_option,
            dict(single, barrier_type="up_and_out", payout=None),
            0,
        ),
        (
            ins.DoubleTouchOption,
            ins.cash_double_touch,
            dict(double, barrier_type="knock_in", payout=10),
            10,
        ),
        (
            ins.DoubleTouchOption,
            ins.cash_double_no_touch,
            dict(double, barrier_type="knock_out", payout=10),
            0,
        ),
    ]
    for cls, factory, terms, expected in cases:
        with subtests.test(contract=cls.__name__, factory=factory.__name__):
            fields = {**life, **terms, **monitoring}
            if cls is not ins.DoubleTouchOption:
                fields["payoff_type"] = (
                    "cash" if terms["payout"] is not None else "asset"
                )
            arguments = inspect.signature(cls).bind(**fields)
            arguments.apply_defaults()
            contract = cls(*arguments.args, **arguments.kwargs)
            factory_fields = fields.copy()
            factory_fields.pop("payoff_type", None)
            if cls is ins.DoubleTouchOption:
                factory_fields.pop("barrier_type")
            elif fields["payout"] is None:
                factory_fields.pop("payout")
            assert contract == factory(**factory_fields)
            assert contract.payout == terms["payout"]
            assert contract.calendar == calendar
            assert contract.observation_times == ((end,))
            assert contract.touch_state == "untouched"
            engine = (
                pricing.AnalyticDoubleTouchEngine()
                if cls is ins.DoubleTouchOption
                else pricing.AnalyticBinaryBarrierEngine()
            )
            assert engine.price(contract, context) == expected
            restored = pickle.loads(pickle.dumps(contract))
            assert restored == contract
            assert engine.price(restored, context) == expected
            if terms["payout"] is None:
                cash = contract.replace(payoff_type="cash", payout=5)
                assert cash.payout == 5
                assert engine.price(cash, context) == (5 if expected else 0)
            else:
                assert engine.price(contract.replace(payout=5), context) == expected / 2
            with pytest.raises(InvalidInputError):
                contract.replace(calendar=market.weekdays_calendar())
            assert contract.calendar == calendar


def test_constructors_validate_representations_and_domain_inputs(subtests):
    start, end = datetime(2025, 1, 3), datetime(2025, 1, 4)
    life = {"effective_time": start, "expiry_time": end, "payout": 10}
    for cls, specific in (
        (
            ins.BinaryBarrierOption,
            {
                "option_type": "call",
                "strike": 100,
                "barrier_level": 120,
                "barrier_type": "up_and_in",
            },
        ),
        (ins.TouchOption, {"barrier_level": 120, "barrier_type": "up_and_in"}),
        (
            ins.DoubleTouchOption,
            {"lower_barrier": 80, "upper_barrier": 120, "barrier_type": "knock_in"},
        ),
    ):
        fields = dict(life, **specific, calendar=market.all_days_calendar())
        if cls is not ins.DoubleTouchOption:
            fields["payoff_type"] = "cash"
        for changes, error in (
            ({"payout": True}, TypeError),
            ({"payout": "10"}, TypeError),
            ({"payout": -1}, InvalidInputError),
            ({"payout": float("inf")}, InvalidInputError),
            ({"barrier_type": "invalid"}, ValueError),
            ({"expiry_time": start.replace(year=2024)}, InvalidInputError),
            (
                {
                    "calendar": market.weekdays_calendar(),
                    "observation_times": [end],
                },
                InvalidInputError,
            ),
        ):
            with (
                subtests.test(contract=cls.__name__, changes=changes),
                pytest.raises(error),
            ):
                cls(**(fields | changes))
        without_payout = fields.copy()
        without_payout.pop("payout")
        with pytest.raises(TypeError):
            cls(**without_payout)
        with pytest.raises(TypeError):
            cls(10)
    with pytest.raises(TypeError):
        ins.DoubleTouchOption(
            **(life | {"payout": None}),
            lower_barrier=80,
            upper_barrier=120,
            barrier_type="knock_in",
        )


def test_at_hit_settlement_requires_one_touch(subtests):
    life = {
        "effective_time": datetime(2025, 1, 3),
        "expiry_time": datetime(2025, 1, 6),
    }
    for payout in (10, None):
        contract = ins.TouchOption(
            payoff_type="cash" if payout is not None else "asset",
            **life,
            barrier_level=120,
            barrier_type="up_and_in",
            payout=payout,
            settlement_timing="at_hit",
        )
        assert contract.settlement_timing == "at_hit"
        assert contract.is_one_touch
        with subtests.test(payout=payout), pytest.raises(InvalidInputError):
            ins.TouchOption(
                payoff_type="cash" if payout is not None else "asset",
                **life,
                barrier_level=120,
                barrier_type="up_and_out",
                payout=payout,
                settlement_timing="at_hit",
            )
    contract = ins.DoubleTouchOption(
        **life,
        lower_barrier=80,
        upper_barrier=120,
        barrier_type="knock_in",
        payout=10,
        settlement_timing="at_hit",
    )
    assert contract.settlement_timing == "at_hit"
    assert contract.is_one_touch
    with pytest.raises(InvalidInputError):
        ins.DoubleTouchOption(
            **life,
            lower_barrier=80,
            upper_barrier=120,
            barrier_type="knock_out",
            payout=10,
            settlement_timing="at_hit",
        )
