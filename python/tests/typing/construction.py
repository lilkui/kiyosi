"""Static constructor contracts; checked with unused ignores reported as errors."""

from datetime import datetime, time
from typing import Literal, assert_type

from kiyosi.instruments import (
    Accumulator,
    AutocallableObservation,
    BinaryBarrierOption,
    DoubleTouchOption,
    KnockInObservation,
    PhoenixOption,
    SnowballOption,
    TernarySnowballOption,
    TouchOption,
    asset_no_touch_option,
    asset_touch_option,
    autocallable_observations,
    cash_no_touch_option,
    cash_touch_option,
    standard_phoenix,
    step_down_phoenix,
)
from kiyosi.market import PricingContext, weekdays_calendar
from kiyosi.pricing import (
    CouponQuoteConvention,
    QuadratureAutocallableEngine,
    SnowballCouponQuote,
    implied_snowball_coupon,
)

BinaryBarrierOption()  # ty: ignore[no-matching-overload]
TouchOption()  # ty: ignore[no-matching-overload]
DoubleTouchOption()  # ty: ignore[missing-argument]


def touch_payment_conditions(start: datetime, end: datetime) -> None:
    assert_type(
        cash_touch_option(
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_in",
            payout=10,
        ),
        TouchOption,
    )
    assert_type(
        cash_no_touch_option(
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_out",
            payout=10,
        ),
        TouchOption,
    )
    assert_type(
        asset_touch_option(
            effective_time=start,
            expiry_time=end,
            barrier_level=80,
            barrier_type="down_and_in",
        ),
        TouchOption,
    )
    assert_type(
        asset_no_touch_option(
            effective_time=start,
            expiry_time=end,
            barrier_level=80,
            barrier_type="down_and_out",
        ),
        TouchOption,
    )
    cash_touch_option(
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="up_and_out",  # ty: ignore[invalid-argument-type]
        payout=10,
    )
    asset_no_touch_option(
        effective_time=start,
        expiry_time=end,
        barrier_level=80,
        barrier_type="down_and_in",  # ty: ignore[invalid-argument-type]
    )


def snowball_quote_application(note: SnowballOption, context: PricingContext) -> None:
    quote = implied_snowball_coupon(
        QuadratureAutocallableEngine(),
        note,
        context,
        1.0,
        quote_convention="preserve_maturity_coupon",
    )
    assert_type(quote, SnowballCouponQuote)
    assert_type(quote["first_observation_coupon"], float)
    assert_type(quote["quote_convention"], CouponQuoteConvention)
    assert_type(note.with_quoted_coupon(**quote), SnowballOption)


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
    assert_type(option.replace(knock_out_settled=None), Accumulator)
    assert_type(option.replace(knock_out_settled=False), Accumulator)
    option.replace(knock_out_settled=0)  # ty: ignore[invalid-argument-type]


def knock_in_observation_values(note: SnowballOption) -> None:
    daily = KnockInObservation.every_trading_day(time(15))
    assert_type(daily, KnockInObservation)
    assert_type(KnockInObservation.at_expiry(), KnockInObservation)
    assert_type(daily.observation_clock_time, time | None)
    assert_type(daily.kind, Literal["every_trading_day", "at_expiry"])
    assert_type(note.knock_in_observation, KnockInObservation)
    assert_type(note.replace(knock_in_observation=daily), SnowballOption)
    KnockInObservation.every_trading_day(None)  # ty: ignore[invalid-argument-type]
    note.replace(knock_in_observation=None)  # ty: ignore[invalid-argument-type]
    note.replace(knock_in_observation="at_expiry")  # ty: ignore[invalid-argument-type]


def phoenix_periodic_coupon() -> None:
    start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
    observations = [AutocallableObservation.phoenix(end, 110, coupon_barrier_level=0)]
    assert_type(
        PhoenixOption(
            calendar=weekdays_calendar(),
            knock_in_observation=KnockInObservation.at_expiry(),
            coupon_per_period=0.01,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=start,
            expiry_time=end,
            autocall_observations=observations,
        ),
        PhoenixOption,
    )
    PhoenixOption(  # ty: ignore[missing-argument]
        calendar=weekdays_calendar(),
        knock_in_observation=KnockInObservation.at_expiry(),
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        effective_time=start,
        expiry_time=end,
        autocall_observations=observations,
    )


def phoenix_replacements(note: PhoenixOption) -> None:
    assert_type(note.lockout_periods, int)
    assert_type(note.replace(lockout_periods=1), PhoenixOption)
    note.replace(lockout_periods=1.5)  # ty: ignore[invalid-argument-type]
    assert_type(note.coupon_per_period, float)
    assert_type(note.with_quoted_coupon(coupon_per_period=0.01), PhoenixOption)
    note.with_quoted_coupon(0.1)  # ty: ignore[too-many-positional-arguments, missing-argument]
    note.with_quoted_coupon()  # ty: ignore[missing-argument]
    note.with_quoted_coupon(coupon_rate=0.1, coupon_per_period=0.01)  # ty: ignore[unknown-argument]
    assert_type(note.replace(), PhoenixOption)
    assert_type(note.replace(coupon_per_period=0.01), PhoenixOption)
    assert_type(note.replace(coupon_per_period=Ellipsis), PhoenixOption)
    note.replace(coupon_rate=0.1)  # ty: ignore[unknown-argument]
    note.replace(coupon_per_period=None)  # ty: ignore[invalid-argument-type]


def phoenix_presets(start: datetime, end: datetime) -> None:
    assert_type(
        standard_phoenix(
            coupon_per_period=0.01,
            initial_spot=100,
            knock_in_level=80,
            knock_out_level=110,
            coupon_barrier_level=90,
            autocall_times=[start, end],
            knock_in_observation_clock_time=time(15),
            effective_time=start,
            expiry_time=end,
            lockout_periods=1,
        ),
        PhoenixOption,
    )
    assert_type(
        step_down_phoenix(
            coupon_per_period=0.01,
            initial_spot=100,
            knock_in_level=80,
            initial_knock_out_level=110,
            knock_out_level_decrement=2,
            coupon_barrier_level=90,
            knock_in_observation_clock_time=time(15),
            autocall_times=[start, end],
            effective_time=start,
            expiry_time=end,
            lockout_periods=1,
        ),
        PhoenixOption,
    )


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
            calendar=weekdays_calendar(),
            maturity_coupon_rate=0.1,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=start,
            expiry_time=end,
            autocall_observations=observations,
            knock_in_observation=KnockInObservation.at_expiry(),
        ),
        SnowballOption,
    )
    assert_type(
        TernarySnowballOption(
            calendar=weekdays_calendar(),
            maturity_coupon_rate=0.1,
            minimum_coupon_rate=0.02,
            knock_in_level=80,
            effective_time=start,
            expiry_time=end,
            autocall_observations=observations,
            knock_in_observation=KnockInObservation.at_expiry(),
        ),
        TernarySnowballOption,
    )
