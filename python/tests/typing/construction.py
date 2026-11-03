"""Static constructor contracts; checked with unused ignores reported as errors."""

from datetime import date
from typing import assert_type

from kiyosi.instruments import (
    AutocallableObservation,
    BinaryBarrierOption,
    DoubleTouchOption,
    PhoenixOption,
    TouchOption,
)

BinaryBarrierOption()  # ty: ignore[missing-argument]
TouchOption()  # ty: ignore[missing-argument]
DoubleTouchOption()  # ty: ignore[missing-argument]


def phoenix_choices() -> None:
    # Keep date and observation types visible to the overload checker.
    start, end = date(2025, 1, 2), date(2026, 1, 2)
    observations = [AutocallableObservation(end, 110)]
    assert_type(
        PhoenixOption(
            coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            knock_in_observation_mode="every_trading_day",
            effective_date=start,
            expiry_date=end,
            observations=observations,
        ),
        PhoenixOption,
    )
    assert_type(
        PhoenixOption(
            coupon_per_period=0.01,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            knock_in_observation_mode="every_trading_day",
            effective_date=start,
            expiry_date=end,
            observations=observations,
        ),
        PhoenixOption,
    )
    PhoenixOption(
        coupon_rate=0.1,
        coupon_per_period=0.01,
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        effective_date=start,
        expiry_date=end,
        observations=observations,
    )  # ty: ignore[no-matching-overload]
    PhoenixOption(
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        effective_date=start,
        expiry_date=end,
        observations=observations,
    )  # ty: ignore[no-matching-overload]


def phoenix_replacements(note: PhoenixOption) -> None:
    assert_type(note.replace(), PhoenixOption)
    assert_type(note.replace(coupon_rate=0.1), PhoenixOption)
    assert_type(note.replace(coupon_per_period=0.01), PhoenixOption)
    note.replace(coupon_rate=0.1, coupon_per_period=0.01)  # ty: ignore[no-matching-overload]
