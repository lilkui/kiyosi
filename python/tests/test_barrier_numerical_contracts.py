"""Shared public contracts for numerical single-barrier engines."""

import math
from datetime import datetime, timedelta
from functools import partial

import pytest
from kiyosi import (
    InvalidInputError,
    MissingHistoryError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.pricing import numerical_greeks
from support.assertions import assert_rejection
from support.terminal import assert_terminal_greeks
from test_barrier_binomial import END, KINDS, START, market, option


def engines():
    return (
        pricing.CoxRossRubinsteinBarrierEngine(step_count=720),
        pricing.FiniteDifferenceBarrierEngine(),
        pricing.QuadratureBarrierEngine(),
    )


def test_history_is_required_and_failure_does_not_change_engine(subtests):
    for engine in engines():
        with subtests.test(engine=type(engine).__name__):
            contract = option()
            expected = engine.price(contract, market())
            assert_rejection(
                MissingHistoryError,
                engine.price,
                contract,
                market(valuation=START + timedelta(days=1)),
            )
            assert engine.price(contract, market()) == expected
            assert_rejection(
                InvalidInputError,
                engine.price,
                option(touch_state="touched"),
                market(),
            )


@pytest.mark.parametrize("kind", KINDS)
def test_scheduled_touch_occurs_only_at_original_midnight(subtests, kind):
    observation = START + timedelta(days=90)
    midday = datetime.combine(observation, datetime.min.time()) + timedelta(hours=12)
    level = 120 if kind.startswith("up") else 80
    terms = {
        "rebate": 5,
        "rebate_timing": "at_hit" if kind.endswith("out") else "at_expiry",
        "observation_times": [observation],
    }
    contract = option(kind, **terms)
    prior_untouched = option(kind, **terms, touch_state="untouched")
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    at_observation = market(spot=level, valuation=observation)
    after_observation = market(spot=level, valuation=midday)
    for engine in engines():
        with subtests.test(engine=type(engine).__name__, kind=kind):
            current = (
                5
                if kind.endswith("out")
                else pricing.AnalyticEuropeanEngine().price(vanilla, at_observation)
            )
            later = (
                pricing.AnalyticEuropeanEngine().price(vanilla, after_observation)
                if kind.endswith("out")
                else 5 * math.exp(-0.03 * 269.5 / 365)
            )
            assert engine.price(contract, at_observation) == pytest.approx(
                current, rel=0, abs=1e-12
            )
            assert engine.price(
                prior_untouched,
                after_observation,
            ) == pytest.approx(later, rel=0, abs=1e-12)


def test_completed_monitoring_returns_vanilla_or_expiry_rebate(subtests):
    dates = [START + timedelta(days=90)]
    valuation = START + timedelta(days=180)
    context = market(valuation=valuation)
    vanilla = EuropeanOption(
        option_type="call", strike=100, effective_time=START, expiry_time=END
    )
    value = pricing.AnalyticEuropeanEngine().price(vanilla, context)
    for engine in engines():
        with subtests.test(engine=type(engine).__name__):
            for kind in KINDS:
                contract = option(
                    kind,
                    rebate=3,
                    observation_times=dates,
                    touch_state="untouched",
                )
                expected = (
                    3 * math.exp(-0.03 * 180 / 365) if kind.endswith("in") else value
                )
                assert engine.price(contract, context) == pytest.approx(
                    expected, rel=0, abs=1e-12
                )


def test_valuation_and_greek_validation_preserve_core_errors(subtests):
    for engine in engines():
        with subtests.test(engine=type(engine).__name__):
            for valuation in (START - timedelta(days=1), END + timedelta(days=1)):
                assert_rejection(
                    InvalidInputError,
                    engine.price,
                    option(),
                    market(valuation=valuation),
                )
            assert_rejection(
                InvalidInputError,
                partial(numerical_greeks, engine),
                option(),
                market(),
                "delta",
                spot_shift=0,
            )


def test_expiry_settlement_has_terminal_spot_greeks(subtests):
    for engine in engines():
        with subtests.test(engine=type(engine).__name__):
            for kind in KINDS:
                contract = option(kind, rebate=3, touch_state="untouched")
                result = engine.price_with_greeks(
                    contract,
                    market(spot=110, valuation=END),
                    greeks="all",
                )
                assert result.price == (3 if kind.endswith("in") else 10)
                assert_terminal_greeks(result, int(kind.endswith("out")))


def test_implied_volatility_reprices_and_rejects_fixed_cashflows(subtests):
    context = market()
    contract = option(barrier_level=60)
    for engine in engines():
        with subtests.test(engine=type(engine).__name__):
            quote = engine.price(contract, context)
            volatility = pricing.implied_volatility(
                engine,
                contract,
                context,
                quote,
                lower_bound=0.15,
                upper_bound=0.25,
                price_tolerance=1e-7,
            )
            assert volatility == pytest.approx(0.2, rel=0, abs=0.0002)
            fixed = option(rebate=7, rebate_timing="at_expiry", touch_state="touched")
            later = market(valuation=START + timedelta(days=1))
            assert_rejection(
                UnsupportedOperationError,
                pricing.implied_volatility,
                engine,
                fixed,
                later,
                engine.price(fixed, later),
                lower_bound=0.0,
                upper_bound=4.0,
            )
