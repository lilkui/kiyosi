"""Static constructor contracts; checked with unused ignores reported as errors."""

from datetime import datetime
from datetime import time as clock_time
from typing import Literal, assert_type

from kiyosi.instruments import (
    Accumulator,
    AutocallableObservation,
    BinaryBarrierOption,
    DoubleTouchOption,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    TouchOption,
    autocallable_observations,
)

BinaryBarrierOption()  # ty: ignore[no-matching-overload]
TouchOption()  # ty: ignore[no-matching-overload]
DoubleTouchOption()  # ty: ignore[missing-argument]


def observation_choices(at: datetime) -> None:
    record = AutocallableObservation.snowball(at, 110, coupon_rate=0)
    assert_type(record.coupon_rate, float | None)
    assert_type(record.coupon_barrier_level, float | None)
    assert_type(
        record.replace(
            coupon_rate=record.coupon_rate,
            coupon_barrier_level=record.coupon_barrier_level,
        ),
        AutocallableObservation,
    )
    assert_type(
        AutocallableObservation.phoenix(at, 110, coupon_barrier_level=0).kind,
        Literal["snowball", "phoenix"],
    )
    assert_type(
        AutocallableObservation.snowball(at, 110, coupon_rate=0),
        AutocallableObservation,
    )
    assert_type(
        AutocallableObservation.phoenix(at, 110, coupon_barrier_level=0),
        AutocallableObservation,
    )
    assert_type(
        autocallable_observations([at], [110], knock_out_coupon_rates=[0]),
        list[AutocallableObservation],
    )
    assert_type(
        autocallable_observations([at], [110], coupon_barrier_levels=[0]),
        list[AutocallableObservation],
    )
    autocallable_observations([at], [110])  # ty: ignore[no-matching-overload]
    autocallable_observations(
        [at], [110], knock_out_coupon_rates=[0], coupon_barrier_levels=[0]
    )  # ty: ignore[no-matching-overload]


def accumulator_history(option: Accumulator) -> None:
    assert_type(option.knock_out_settled, bool | None)
    assert_type(option.replace_history(knock_out_settled=None), Accumulator)
    assert_type(option.replace(knock_out_settled=False), Accumulator)
    option.replace_history(knock_out_settled=0)  # ty: ignore[invalid-argument-type]


def phoenix_choices() -> None:
    # Keep date and observation types visible to the overload checker.
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    observations = [AutocallableObservation.phoenix(end, 110, coupon_barrier_level=0)]
    assert_type(
        PhoenixOption(
            observation_clock_time=clock_time(),
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
            knock_in_observation_mode="at_expiry",
            effective_time=start,
            expiry_time=end,
            observations=observations,
        ),
        PhoenixOption,
    )
    PhoenixOption(
        observation_clock_time=clock_time(),
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
        observation_clock_time=clock_time(),
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
    assert_type(note.with_quoted_coupon(coupon_rate=0.1), PhoenixOption)
    assert_type(note.with_quoted_coupon(coupon_per_period=0.01), PhoenixOption)
    note.with_quoted_coupon(0.1)  # ty: ignore[no-matching-overload]
    note.with_quoted_coupon()  # ty: ignore[no-matching-overload]
    note.with_quoted_coupon(coupon_rate=0.1, coupon_per_period=0.01)  # ty: ignore[no-matching-overload]
    assert_type(note.replace(), PhoenixOption)
    assert_type(note.replace(coupon_rate=0.1, coupon_per_period=None), PhoenixOption)
    assert_type(note.replace(coupon_per_period=0.01, coupon_rate=None), PhoenixOption)
    note.replace(coupon_rate=0.1, coupon_per_period=0.01)  # ty: ignore[no-matching-overload]
    note.replace(coupon_rate=None)  # ty: ignore[no-matching-overload]
    note.replace(coupon_per_period=None)  # ty: ignore[no-matching-overload]
    note.replace(coupon_rate=None, coupon_per_period=None)  # ty: ignore[no-matching-overload]


def binary_and_touch_constructors() -> None:
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    assert_type(
        BinaryBarrierOption(
            payoff_type="cash",
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
            payoff_type="asset",
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
            payoff_type="asset",
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
            payoff_type="cash",
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


def binary_and_touch_payoff_choices() -> None:
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    BinaryBarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="asset",
    )
    BinaryBarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="cash",  # ty: ignore[invalid-argument-type]
    )
    BinaryBarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="cash",
        payout=None,
    )  # ty: ignore[no-matching-overload]
    BinaryBarrierOption(
        option_type="call",
        strike=100,
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="asset",
        payout=10,
    )  # ty: ignore[no-matching-overload]
    TouchOption(
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="asset",
    )
    TouchOption(
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="cash",  # ty: ignore[invalid-argument-type]
    )
    TouchOption(
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="cash",
        payout=None,
    )  # ty: ignore[no-matching-overload]
    TouchOption(
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_in",
        payoff_type="asset",
        payout=10,
    )  # ty: ignore[no-matching-overload]


def settlement_replacement_types(
    binary: BinaryBarrierOption, touch: TouchOption
) -> None:
    assert_type(binary.replace(payout=None), BinaryBarrierOption)
    assert_type(binary.replace(payout=10), BinaryBarrierOption)
    assert_type(touch.replace(payout=None), TouchOption)
    assert_type(touch.replace(payout=10), TouchOption)


def expiry_only_snowballs() -> None:
    start, end = datetime(2025, 1, 2), datetime(2026, 1, 2)
    observations = [AutocallableObservation.snowball(end, 110, coupon_rate=0)]
    assert_type(
        SnowballOption(
            maturity_coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=start,
            expiry_time=end,
            observations=observations,
            knock_in_observation_mode="at_expiry",
            observation_clock_time=None,
        ),
        SnowballOption,
    )
    assert_type(
        TernarySnowballOption(
            maturity_coupon_rate=0.1,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            effective_time=start,
            expiry_time=end,
            observations=observations,
            knock_in_observation_mode="at_expiry",
            observation_clock_time=None,
        ),
        TernarySnowballOption,
    )
