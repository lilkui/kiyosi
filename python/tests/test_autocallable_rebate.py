"""Terminal rebates, native boundaries, and quote workflows for autocallables."""

import inspect
import math
from datetime import time, timedelta
from functools import partial

import pytest
from kiyosi import InvalidInputError, instruments
from kiyosi.instruments import (
    BinarySnowballOption,
    CouponObservation,
    KnockOutObservation,
    TernarySnowballOption,
)
from kiyosi.pricing import (
    FiniteDifferenceAutocallableEngine,
    MonteCarloAutocallableEngine,
    QuadratureAutocallableEngine,
    implied_coupon,
)
from test_phoenix import contract as phoenix
from test_snowball import END, START, contract, market
from test_snowball_variants import make_fixed

FACTORIES = (
    contract,
    phoenix,
    partial(make_fixed, BinarySnowballOption),
    partial(make_fixed, TernarySnowballOption),
)


def engines():
    return (
        QuadratureAutocallableEngine(),
        FiniteDifferenceAutocallableEngine(),
        MonteCarloAutocallableEngine(path_count=4096, seed=42),
    )


@pytest.mark.parametrize("factory", FACTORIES)
def test_rebate_validation_defaults_and_immutable_replacement(factory, subtests):
    note = factory()
    for field in ("rebate_rate", "annual_rebate_rate"):
        assert getattr(note, field) == 0
        assert inspect.signature(type(note)).parameters[field].default == 0
        for value in (True, False, None, "0.02"):
            with subtests.test(field=field, value=value):
                with pytest.raises(TypeError):
                    factory(**{field: value})
                with pytest.raises(TypeError):
                    note.replace(**{field: value})
        for value in (-0.01, math.nan, math.inf, -math.inf):
            with subtests.test(field=field, value=value):
                with pytest.raises(InvalidInputError):
                    factory(**{field: value})
                with pytest.raises(InvalidInputError):
                    note.replace(**{field: value})
        edited = note.replace(**{field: 2})
        assert getattr(edited, field) == 2
        assert edited.replace(**{field: Ellipsis}) == edited
        assert edited.replace(**{field: 0}) == note
        assert factory(**{field: 2}) == edited
        assert getattr(note, field) == 0


@pytest.mark.parametrize("factory", FACTORIES)
@pytest.mark.parametrize("flat,annual", [(0.02, 0), (0, 0.06), (0.02, 0.06)])
def test_terminal_rebate_adds_to_principal_and_existing_coupons(factory, flat, annual):
    # These timestamps and payoff scenarios are shared with the Rust tests.
    start, end = START.replace(hour=12), END.replace(hour=18)
    fields = {"effective_time": start, "expiry_time": end}
    if factory is phoenix:
        fields.update(
            knock_out_observations=[KnockOutObservation(end, 110)],
            coupon_observations=[CouponObservation(end, 90)],
        )
    else:
        fields["autocall_observations"] = [
            instruments.SnowballObservation(end, 110, coupon_rate=0.08)
        ]
    plain = factory(**fields)
    rebated = plain.replace(rebate_rate=flat, annual_rebate_rate=annual)
    expected = flat + annual * 90.25 / 365
    for engine in engines():
        for spot in (70, 90, 120):
            ctx = market(spot, end)
            assert engine.price(rebated, ctx) - engine.price(
                plain, ctx
            ) == pytest.approx(expected, rel=0, abs=1e-12)


def test_rebate_uses_actual_exit_time_and_is_not_a_periodic_phoenix_coupon():
    middle = START + timedelta(days=33, hours=6)
    expiry = END.replace(hour=18)
    note = phoenix(
        rebate_rate=0.02,
        annual_rebate_rate=0.06,
        expiry_time=expiry,
        knock_out_observations=[KnockOutObservation(middle, 110)],
        coupon_observations=[CouponObservation(middle, 90)],
    )
    for engine in engines():
        assert engine.price(note, market(120, middle)) == pytest.approx(
            1.03 + 0.06 * 33.25 / 365, rel=0, abs=1e-12
        )
        assert (
            engine.price(
                note.replace(barrier_state="knocked_out"),
                market(120, middle + timedelta(microseconds=1)),
            )
            == 0
        )
        # With zero rates/volatility and no future coupon observation, only
        # today's ordinary coupon and the terminal principal/rebate remain.
        assert engine.price(
            note, market(100, middle, volatility=0, rate=0, dividend=0)
        ) == pytest.approx(1.03 + 0.06 * 90.75 / 365, rel=0, abs=1e-12)
        assert engine.price(note, market(100, expiry)) == pytest.approx(
            1.02 + 0.06 * 90.75 / 365, rel=0, abs=1e-12
        )
        immediate = contract(
            effective_time=END,
            rebate_rate=0.02,
            annual_rebate_rate=0.06,
        )
        for spot in (90, 120):
            assert engine.price(immediate, market(spot, END)) == pytest.approx(1.02)


@pytest.mark.parametrize("factory", (contract, phoenix))
def test_stochastic_terminal_rebate_matches_discounted_payment(factory):
    fields = {"principal_redemption_ratio": 0.4, "lower_strike": 0}
    if factory is phoenix:
        fields.update(
            knock_out_observations=[KnockOutObservation(END, 110)],
            coupon_observations=[CouponObservation(END, 90)],
        )
    plain = factory(**fields)
    rebated = plain.replace(rebate_rate=0.02, annual_rebate_rate=0.06)
    expected = (0.02 + 0.06 * 90 / 365) * math.exp(-0.04 * 90 / 365)
    for engine in engines():
        ctx = market(80)
        assert engine.price(rebated, ctx) - engine.price(plain, ctx) == pytest.approx(
            expected, rel=0, abs=2e-6
        )


@pytest.mark.parametrize(
    "factory,extra",
    [
        (
            instruments.SnowballOption.standard,
            {"coupon_rate": 0.08, "knock_in_observation_clock_time": time()},
        ),
        (instruments.SnowballOption.european, {"coupon_rate": 0.08}),
        (instruments.PhoenixOption.fixed_coupon, {"coupon_per_period": 0.01}),
        (
            instruments.PhoenixOption.digital_coupon,
            {"coupon_per_period": 0.01, "coupon_barrier_level": 90},
        ),
        (
            instruments.PhoenixOption.standard,
            {
                "coupon_per_period": 0.01,
                "coupon_barrier_level": 90,
                "knock_in_observation_clock_time": time(),
            },
        ),
    ],
)
def test_presets_preserve_rebates_and_discoverable_defaults(factory, extra):
    fields = dict(
        initial_spot=100,
        knock_in_level=80,
        knock_out_level=110,
        effective_time=START,
        expiry_time=END,
        autocall_times=[END],
        **extra,
    )
    note = factory(**fields, rebate_rate=0.02, annual_rebate_rate=0.06)
    assert note.rebate_rate == 0.02
    assert note.annual_rebate_rate == 0.06
    assert note.replace(rebate_rate=0, annual_rebate_rate=0) == factory(**fields)
    signature = inspect.signature(factory)
    assert signature.parameters["rebate_rate"].default == 0
    assert signature.parameters["annual_rebate_rate"].default == 0
    for field in ("rebate_rate", "annual_rebate_rate"):
        with pytest.raises(TypeError):
            factory(**fields, **{field: True})
        with pytest.raises(InvalidInputError):
            factory(**fields, **{field: -0.01})


def test_coupon_solvers_preserve_rebate_rates():
    engine = QuadratureAutocallableEngine()
    context = market()
    snow = contract(rebate_rate=0.02, annual_rebate_rate=0.06)
    convention = "shift_maturity_coupon"
    target = engine.price(snow.with_coupon(0.12, quote_convention=convention), context)
    quote = implied_coupon(engine, snow, context, target, quote_convention=convention)
    applied = snow.with_coupon(**quote)
    assert applied.rebate_rate == 0.02
    assert applied.annual_rebate_rate == 0.06
    assert quote["first_observation_coupon"] == pytest.approx(0.12, abs=1e-8)
    bird = phoenix(rebate_rate=0.02, annual_rebate_rate=0.06)
    target = engine.price(bird.with_coupon(coupon_per_period=0.03), context)
    quote = implied_coupon(engine, bird, context, target)
    applied = bird.with_coupon(**quote)
    assert applied.rebate_rate == 0.02
    assert applied.annual_rebate_rate == 0.06
    assert quote["coupon_per_period"] == pytest.approx(0.03, abs=1e-8)
