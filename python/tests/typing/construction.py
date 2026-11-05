"""Static constructor contracts; checked with unused ignores reported as errors."""

from datetime import datetime
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
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    observations = [AutocallableObservation(end, 110)]
    assert_type(
        PhoenixOption(
            coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            knock_in_observation_mode="every_trading_day",
            effective_time=start,
            expiry_time=end,
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
            effective_time=start,
            expiry_time=end,
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
        effective_time=start,
        expiry_time=end,
        observations=observations,
    )  # ty: ignore[no-matching-overload]
    PhoenixOption(
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        knock_in_observation_mode="every_trading_day",
        effective_time=start,
        expiry_time=end,
        observations=observations,
    )  # ty: ignore[no-matching-overload]


def phoenix_replacements(note: PhoenixOption) -> None:
    assert_type(note.replace(), PhoenixOption)
    assert_type(note.replace(coupon_rate=0.1), PhoenixOption)
    assert_type(note.replace(coupon_per_period=0.01), PhoenixOption)
    note.replace(coupon_rate=0.1, coupon_per_period=0.01)  # ty: ignore[no-matching-overload]


def binary_and_touch_constructors() -> None:
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    assert_type(
        BinaryBarrierOption(
            option_type="call",
            strike=100,
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_in",
            payout=10,
        ),
        BinaryBarrierOption,
    )
    assert_type(
        BinaryBarrierOption(
            option_type="put",
            strike=100,
            effective_time=start,
            expiry_time=end,
            barrier_level=80,
            barrier_type="down_and_out",
            payout=None,
        ),
        BinaryBarrierOption,
    )
    assert_type(
        TouchOption(
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_in",
            payout=None,
        ),
        TouchOption,
    )
    assert_type(
        TouchOption(
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_out",
            payout=10,
        ),
        TouchOption,
    )
    assert_type(
        DoubleTouchOption(
            effective_time=start,
            expiry_time=end,
            lower_barrier=80,
            upper_barrier=120,
            barrier_type="knock_out",
            payout=10,
        ),
        DoubleTouchOption,
    )
    DoubleTouchOption(
        effective_time=start,
        expiry_time=end,
        lower_barrier=80,
        upper_barrier=120,
        barrier_type="knock_in",
        payout=None,  # ty: ignore[invalid-argument-type]
    )


def settlement_replacement_types(
    binary: BinaryBarrierOption, touch: TouchOption
) -> None:
    assert_type(binary.replace(payout=None), BinaryBarrierOption)
    assert_type(binary.replace(payout=10), BinaryBarrierOption)
    assert_type(touch.replace(payout=None), TouchOption)
    assert_type(touch.replace(payout=10), TouchOption)
