"""Binary and touch contracts with independent QuantLib comparisons."""

import importlib
import math
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    UnsupportedOperationError,
)
from kiyosi import instruments as ins
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
    all_days_calendar,
)
from kiyosi.pricing import (
    AnalyticBinaryBarrierEngine,
    implied_volatility,
    numerical_greeks,
)
from support.assertions import assert_decimal_equal, assert_rejection
from support.terminal import assert_terminal_greeks

GREEKS = (
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


def make_terms(**changes):
    return {
        "option_type": "call",
        "strike": 100,
        "effective_time": datetime(2025, 1, 1),
        "expiry_time": datetime(2026, 1, 1),
        "barrier_level": 120,
        "barrier_type": "up_and_out",
    } | changes


def make_touch(payoff_type="cash", **changes):
    terms = make_terms()
    for key in ("option_type", "strike"):
        terms.pop(key)
    terms["barrier_type"] = "up_and_in"
    terms["payoff_type"] = payoff_type
    if payoff_type == "cash":
        terms["payout"] = 10
    return ins.TouchOption(**(terms | changes))


def make_context(
    spot=100,
    time=datetime(2025, 1, 1),
    *,
    rate=0.04,
    dividend=0.01,
    sigma=0.2,
    **changes,
):
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=rate, dividend_yield=dividend, volatility=sigma
        ),
        spot_price=spot,
        valuation_time=time,
        **changes,
    )


def test_strict_history_current_and_paid_cash_and_asset_settlement():
    engine = AnalyticBinaryBarrierEngine()
    for payoff_type in ("cash", "asset"):
        asset = payoff_type == "asset"
        for timing in ("at_hit", "at_expiry"):
            option = make_touch(payoff_type, settlement_timing=timing)
            assert option.touch_state is None
            assert option.observation_mode == "continuous"
            amount = 130 if asset else 10
            expected = (
                amount
                if timing == "at_hit"
                else amount * math.exp(-(0.01 if asset else 0.04))
            )
            assert engine.price(option, make_context(130)) == pytest.approx(
                expected, rel=0, abs=1e-12
            )
            assert_rejection(
                MissingHistoryError,
                lambda option=option: engine.price(
                    option,
                    make_context(130, datetime(2025, 1, 2)),
                ),
            )
            known = option.replace(
                touch_state="touched",
            )
            assert_rejection(
                InvalidInputError,
                lambda known=known: engine.price(known, make_context(130)),
            )
            context = make_context(time=datetime(2025, 7, 1))
            expected = (
                0
                if timing == "at_hit"
                else (100 if asset else 10)
                * math.exp(-(0.01 if asset else 0.04) * 184 / 365)
            )
            assert engine.price(known, context) == pytest.approx(
                expected, rel=0, abs=1e-12
            )
            assert_rejection(
                UnsupportedOperationError,
                lambda context=context, expected=expected, known=known: (
                    implied_volatility(
                        engine,
                        known,
                        context,
                        expected,
                        lower_bound=0.0,
                        upper_bound=4.0,
                    )
                ),
            )
            assert known.replace(touch_state=None) == option
            assert option.replace() == option
            with pytest.raises(ValueError):
                option.replace(
                    touch_state="unknown",
                )
            with pytest.raises(TypeError):
                option.replace(
                    touch_state=True,
                )
            assert option.replace(payoff_type="cash", payout=7).payout == 7
            assert option.touch_state is None
    for kind in ("up_and_in", "up_and_out"):
        option = ins.BinaryBarrierOption(
            payoff_type="cash", **make_terms(barrier_type=kind), payout=10
        )
        assert_rejection(
            MissingHistoryError,
            lambda option=option: engine.price(
                option,
                make_context(130, datetime(2025, 7, 1)),
            ),
        )
        known = option.replace(
            touch_state="touched",
        )
        assert (
            engine.price(
                known,
                make_context(100, datetime(2025, 7, 1)),
            )
            == 0
        ) == (kind == "up_and_out")
        assert known.replace(touch_state=None) == option


def test_representation_domain_validation_and_failure_state(subtests):
    for payoff_type in (
        "cash",
        "asset",
    ):
        extra = {"payout": 10} if payoff_type == "cash" else {}
        option = ins.BinaryBarrierOption(**make_terms(payoff_type=payoff_type), **extra)
        for field in ("strike", "barrier_level", *extra):
            for value in (True, "100", None, 1 + 0j):
                with (
                    subtests.test(field=field, value=value),
                    pytest.raises(TypeError),
                ):
                    ins.BinaryBarrierOption(
                        **(make_terms(payoff_type=payoff_type) | extra | {field: value})
                    )
        for field in (
            "option_type",
            "barrier_type",
            "touch_state",
        ):
            with subtests.test(field=field), pytest.raises(TypeError):
                ins.BinaryBarrierOption(
                    **(make_terms(payoff_type=payoff_type) | extra | {field: True})
                )
            with subtests.test(field=field), pytest.raises(ValueError):
                ins.BinaryBarrierOption(
                    **(make_terms(payoff_type=payoff_type) | extra | {field: "unknown"})
                )
        for fields, expected_error in (
            ({"strike": 0, "barrier_level": 0}, InvalidInputError),
            ({"barrier_level": 0}, InvalidInputError),
            ({"expiry_time": datetime(2024, 1, 1)}, InvalidInputError),
            ({"observation_times": []}, InvalidInputError),
            (
                {
                    "observation_times": [datetime(2026, 1, 2)],
                },
                InvalidInputError,
            ),
            (
                {
                    "observation_times": [datetime(2025, 1, 2)] * 2,
                },
                InvalidInputError,
            ),
        ):
            assert_rejection(
                expected_error,
                lambda fields=fields, payoff_type=payoff_type, extra=extra: (
                    ins.BinaryBarrierOption(
                        **(make_terms(payoff_type=payoff_type) | extra | fields)
                    )
                ),
            )
        for fields in ({"strike": True}, {"touch_state": True}):
            with pytest.raises(TypeError):
                option.replace(**fields)
        assert option == ins.BinaryBarrierOption(
            **make_terms(payoff_type=payoff_type), **extra
        )
        with pytest.raises(AttributeError):
            option.strike = 0
    for payout in (0, -1, math.inf, math.nan):
        assert_rejection(
            InvalidInputError, lambda payout=payout: make_touch(payout=payout)
        )
        assert_rejection(
            InvalidInputError,
            lambda payout=payout: ins.BinaryBarrierOption(
                payoff_type="cash", **make_terms(strike=0), payout=payout
            ),
        )
    for field in (
        "barrier_level",
        "payout",
        "barrier_type",
        "settlement_timing",
        "touch_state",
    ):
        with subtests.test(field=field), pytest.raises(TypeError):
            make_touch(**{field: True})
    for field in (
        "barrier_type",
        "settlement_timing",
        "touch_state",
    ):
        with subtests.test(field=field), pytest.raises(ValueError):
            make_touch(**{field: "unknown"})
    with pytest.raises(InvalidInputError):
        make_touch(barrier_type="up_and_out", settlement_timing="at_hit")
    with pytest.raises(TypeError):
        make_touch(effective_time=datetime(2025, 1, 1).date())
    with pytest.raises(TypeError):
        AnalyticBinaryBarrierEngine().price(object(), make_context())


@pytest.mark.parametrize("payoff_type", ("cash", "asset"))
def test_touch_payoffs_select_all_barrier_types_and_validate_settlement(
    subtests, payoff_type
):
    for kind in ("up_and_in", "up_and_out", "down_and_in", "down_and_out"):
        with subtests.test(payoff_type=payoff_type, barrier_type=kind):
            option = make_touch(payoff_type, barrier_type=kind)
            assert option.barrier_type == kind
            assert option.is_up == kind.startswith("up")
            assert option.is_one_touch == kind.endswith("_in")
            assert option.settlement_timing == "at_expiry"
            assert option.payout == (10 if payoff_type == "cash" else None)
            if kind.endswith("_in"):
                at_hit = make_touch(
                    payoff_type, barrier_type=kind, settlement_timing="at_hit"
                )
                assert at_hit.settlement_timing == "at_hit"
            else:
                assert_rejection(
                    InvalidInputError,
                    lambda payoff_type=payoff_type, kind=kind: make_touch(
                        payoff_type, barrier_type=kind, settlement_timing="at_hit"
                    ),
                )
    for invalid, error in (
        (True, TypeError),
        (None, TypeError),
        ("up", ValueError),
    ):
        with subtests.test(payoff_type=payoff_type, invalid=invalid):
            assert_rejection(
                error,
                lambda payoff_type=payoff_type, invalid=invalid: make_touch(
                    payoff_type, barrier_type=invalid
                ),
            )


def test_scheduled_boundaries_history_and_owned_dates():
    engine = AnalyticBinaryBarrierEngine()
    fixing = datetime(2025, 1, 2)
    for kind in ("up_and_in", "up_and_out"):
        one = kind == "up_and_in"
        option = make_touch(
            barrier_type=kind,
            observation_times=(d for d in [fixing]),
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda option=option: engine.price(option, make_context()),
        )
        assert_rejection(
            InvalidInputError,
            lambda option=option: engine.price(
                option.replace(
                    touch_state="touched",
                ),
                make_context(120, fixing),
            ),
        )
        expected = 10 * math.exp(-0.04 * 364 / 365) if one else 0
        assert engine.price(
            option,
            make_context(120, fixing),
        ) == pytest.approx(expected, rel=0, abs=1e-12)
        noon = datetime(2025, 1, 2, 12)
        assert_rejection(
            MissingHistoryError,
            lambda option=option, noon=noon: engine.price(
                option,
                make_context(130, noon),
            ),
        )
        known = option.replace(
            touch_state="untouched",
        )
        expected = 0 if one else 10 * math.exp(-0.04 * 363 / 365)
        assert engine.price(
            known,
            make_context(130, datetime(2025, 1, 3)),
        ) == pytest.approx(expected, rel=0, abs=1e-12)
        dates = list(known.observation_times)
        dates.clear()
        assert known.observation_schedule.times == ((fixing,))
        assert known.observation_times == ((fixing,))
        assert_decimal_equal(known.mean_observation_year_fraction, 1 / 365)
    with pytest.raises(InvalidInputError):
        make_touch(
            observation_times=[datetime(2025, 1, 4)],
            touch_state="touched",
        )


def test_sensitivities_solve_expiry_curves_and_concurrent_reads():
    engine = AnalyticBinaryBarrierEngine()
    options = [
        ins.BinaryBarrierOption(
            payoff_type="cash", **make_terms(touch_state="untouched"), payout=10
        ),
        make_touch(touch_state="untouched"),
    ]
    for option in options:
        context = make_context(time=datetime(2025, 7, 1))
        result = engine.price_with_greeks(option, context, greeks="all")
        assert all(getattr(result, g) is not None for g in GREEKS)
        sigma = implied_volatility(
            engine,
            option,
            context,
            result.price,
            lower_bound=0.15,
            upper_bound=0.25,
        )
        assert sigma == pytest.approx(0.2, rel=0, abs=1e-7)
        assert_rejection(
            InvalidInputError,
            lambda context=context, option=option: numerical_greeks(
                engine,
                option,
                context,
                "delta",
                spot_shift=0,
            ),
        )
        boundary = engine.price_with_greeks(
            option,
            make_context(120),
            greeks="all",
        )
        assert boundary.delta is None
        assert boundary.gamma is None
        near = engine.price_with_greeks(
            option,
            make_context(119.995),
            greeks="all",
        )
        assert near.delta is None
        assert near.speed is None
        assert near.vega is not None
        expiry = make_context(time=datetime(2026, 1, 1))
        terminal = engine.price_with_greeks(option, expiry, greeks="all")
        assert_terminal_greeks(
            terminal,
            None if isinstance(option, ins.BinaryBarrierOption) else 0,
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda option=option, expiry=expiry, terminal=terminal: implied_volatility(
                engine,
                option,
                expiry,
                terminal.price,
                lower_bound=0.0,
                upper_bound=4.0,
            ),
        )
        assert_rejection(
            InvalidInputError,
            lambda option=option: engine.price(
                option,
                make_context(time=datetime(2026, 1, 1, 0, 0, 0, 1)),
            ),
        )
        curve = RateTermStructure(
            reference_time=datetime(2025, 1, 1),
            times=[
                datetime(2026, 1, 1),
                datetime(2026, 1, 1) + (datetime(2026, 1, 1) - datetime(2025, 1, 1)),
            ],
            factors=[0.95, (0.95) * 0.5],
        )
        curves = make_context(rate=curve)
        assert_rejection(
            UnsupportedOperationError,
            lambda curves=curves, option=option: engine.price(option, curves),
        )
        assert_rejection(
            UnsupportedOperationError,
            lambda curves=curves, option=option: engine.price_with_greeks(
                option, curves, []
            ),
        )
        with ThreadPoolExecutor(4) as pool:
            prices = list(
                pool.map(
                    lambda _, context=context, option=option: engine.price(
                        option, context
                    ),
                    range(16),
                )
            )
        assert all(p == result.price for p in prices)
    dense = make_touch(
        observation_times=[
            datetime(2025, 1, 1) + timedelta(days=i) for i in range(1, 366)
        ],
        touch_state="untouched",
        calendar=all_days_calendar(),
    )
    result = engine.price_with_greeks(
        dense,
        make_context(time=datetime(2025, 1, 1, 12)),
        greeks="all",
    )
    assert all(math.isfinite(result.require(g)) for g in ("theta", "charm", "color"))
    assert result.delta is not None


def test_existing_quantlib_price_and_greek_scenarios(subtests, quantlib_settings):

    reference = importlib.import_module("quantlib_reference.binary")
    engine = AnalyticBinaryBarrierEngine()
    for identifier, inputs in reference.scenarios():
        with subtests.test(identifier=identifier):
            kind = inputs["BarrierType"]
            asset = inputs["asset_settlement"]
            terms = {
                "effective_time": datetime.fromisoformat(inputs["effective_time"]),
                "expiry_time": datetime.fromisoformat(inputs["expiry_time"]),
                "barrier_level": inputs["barrier"],
                "touch_state": "untouched",
            }
            if not asset:
                terms["payout"] = inputs["payout"]
            terms["payoff_type"] = "asset" if asset else "cash"
            if inputs["option"] == "none":
                option = ins.TouchOption(
                    **terms, barrier_type=kind, settlement_timing=inputs["settlement"]
                )
            else:
                option = ins.BinaryBarrierOption(
                    **terms,
                    option_type=inputs["option"],
                    strike=inputs["strike"],
                    barrier_type=kind,
                )
            context = make_context(
                inputs["spot"],
                datetime.fromisoformat(inputs["valuation"]),
                rate=inputs["rate"],
                dividend=inputs["dividend"],
                sigma=inputs["volatility"],
            )
            expected = reference.measure(inputs, "price")
            assert engine.price(option, context) == pytest.approx(
                expected, rel=0, abs=reference.BUDGET["price"]
            )
            if not reference.exclusions(inputs):
                result = numerical_greeks(
                    engine,
                    option,
                    context,
                    greeks="all",
                    spot_shift=0.02,
                )
                expected_greeks, metadata = reference.g.reference(
                    inputs,
                    reference.STABILITY,
                    measure_fn=reference.measure,
                    unavailable=reference.exclusions(inputs),
                )
                for greek in GREEKS:
                    value = expected_greeks[greek]
                    assert getattr(result, greek) == pytest.approx(
                        value,
                        rel=0,
                        abs=reference.BUDGET[greek] + metadata[f"uncertainty_{greek}"],
                    ), f"{identifier}: {greek}"


def test_subnormal_ratio_retains_half_probability_at_expiry():
    spot, barrier = 1e20, 1e-300
    drift = math.log(barrier) - math.log(spot)
    option = make_touch(barrier_type="down_and_in", barrier_level=barrier, payout=1)
    context = make_context(spot, rate=0, dividend=-drift, sigma=1e-200)
    assert AnalyticBinaryBarrierEngine().price(option, context) == 0.5
