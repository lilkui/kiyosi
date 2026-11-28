"""Static constructor contracts; checked with unused ignores reported as errors."""

from datetime import datetime, time
from typing import Literal, assert_type

from kiyosi.instruments import (
    Accumulator,
    BinaryBarrierOption,
    CouponObservation,
    DoubleTouchOption,
    KnockInObservation,
    KnockOutObservation,
    PhoenixOption,
    SnowballObservation,
    SnowballOption,
    TernarySnowballOption,
    TouchOption,
)
from kiyosi.market import (
    HestonParameters,
    PricingContext,
    TradingCalendar,
)
from kiyosi.pricing import (
    CouponQuoteConvention,
    QuadratureAutocallableEngine,
    SnowballCouponQuote,
    implied_snowball_coupon,
)

BinaryBarrierOption()  # ty: ignore[no-matching-overload]
TouchOption()  # ty: ignore[no-matching-overload]
DoubleTouchOption()  # ty: ignore[missing-argument]


def heston_uses_fixed_variance_time() -> None:
    model = HestonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        initial_variance=0.04,
        long_run_variance=0.04,
        mean_reversion_speed=1.5,
        vol_of_vol=0.3,
        correlation=-0.7,
    )
    assert_type(model.variance_time_convention, Literal["actual_365_fixed"])
    assert_type(model.calendar, None)
    assert_type(
        model.replace(variance_time_convention="actual_365_fixed", calendar=None),
        HestonParameters,
    )
    model.replace(variance_time_convention="business")  # ty: ignore[invalid-argument-type]
    model.replace(calendar=TradingCalendar.weekdays())  # ty: ignore[invalid-argument-type]
    HestonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        initial_variance=0.04,
        long_run_variance=0.04,
        mean_reversion_speed=1.5,
        vol_of_vol=0.3,
        correlation=-0.7,
        variance_time_convention="business",  # ty: ignore[invalid-argument-type]
        calendar=TradingCalendar.weekdays(),  # ty: ignore[invalid-argument-type]
    )


def touch_payment_conditions(start: datetime, end: datetime) -> None:
    assert_type(
        TouchOption(
            payoff_type="cash",
            effective_time=start,
            expiry_time=end,
            barrier_level=120,
            barrier_type="up_and_in",
            payout=10,
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
        TouchOption(
            payoff_type="asset",
            effective_time=start,
            expiry_time=end,
            barrier_level=80,
            barrier_type="down_and_in",
        ),
        TouchOption,
    )
    assert_type(
        TouchOption(
            payoff_type="asset",
            effective_time=start,
            expiry_time=end,
            barrier_level=80,
            barrier_type="down_and_out",
        ),
        TouchOption,
    )
    TouchOption(  # ty: ignore[no-matching-overload]
        payoff_type="cash",
        effective_time=start,
        expiry_time=end,
        barrier_level=120,
        barrier_type="invalid",
        payout=10,
    )
    TouchOption(
        payoff_type="asset",
        effective_time=start,
        expiry_time=end,
        barrier_level=80,
        barrier_type="invalid",  # ty: ignore[invalid-argument-type]
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
    assert_type(note.with_coupon(**quote), SnowballOption)


def observation_records(at: datetime) -> None:
    record = SnowballObservation(at, 110, coupon_rate=0)
    assert_type(record.coupon_rate, float)
    assert_type(record.replace(coupon_rate=record.coupon_rate), SnowballObservation)
    assert_type(
        CouponObservation(at, 0),
        CouponObservation,
    )
    assert_type(
        SnowballObservation.from_columns([at], [110], knock_out_coupon_rates=[0]),
        list[SnowballObservation],
    )
    assert_type(
        KnockOutObservation(at, 110).replace(knock_out_level=120),
        KnockOutObservation,
    )
    SnowballObservation.from_columns([at], [110])  # ty: ignore[missing-argument]
    record.replace(coupon_rate=None)  # ty: ignore[invalid-argument-type]


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
    observations = [KnockOutObservation(end, 110)]
    assert_type(
        PhoenixOption(
            calendar=TradingCalendar.weekdays(),
            knock_in_observation=KnockInObservation.at_expiry(),
            coupon_per_period=0.01,
            initial_spot=100,
            knock_in_level=80,
            upper_strike=100,
            lower_strike=0,
            effective_time=start,
            expiry_time=end,
            knock_out_observations=observations,
            coupon_observations=[CouponObservation(end, 0)],
        ),
        PhoenixOption,
    )
    PhoenixOption(  # ty: ignore[missing-argument]
        calendar=TradingCalendar.weekdays(),
        knock_in_observation=KnockInObservation.at_expiry(),
        initial_spot=100,
        knock_in_level=80,
        upper_strike=100,
        lower_strike=0,
        effective_time=start,
        expiry_time=end,
        knock_out_observations=observations,
        coupon_observations=[CouponObservation(end, 0)],
    )


def phoenix_replacements(note: PhoenixOption) -> None:
    assert_type(note.knock_out_times, tuple[datetime, ...])
    assert_type(note.coupon_times, tuple[datetime, ...])
    assert_type(note.knock_out_observations, tuple[KnockOutObservation, ...])
    assert_type(note.coupon_observations, tuple[CouponObservation, ...])
    assert_type(
        note.replace(knock_out_observations=note.knock_out_observations), PhoenixOption
    )
    note.replace(lockout_periods=1)  # ty: ignore[unknown-argument]
    assert_type(note.coupon_per_period, float)
    assert_type(note.with_coupon(coupon_per_period=0.01), PhoenixOption)
    note.with_coupon(0.1)  # ty: ignore[too-many-positional-arguments, missing-argument]
    note.with_coupon()  # ty: ignore[missing-argument]
    note.with_coupon(coupon_rate=0.1, coupon_per_period=0.01)  # ty: ignore[unknown-argument]
    assert_type(note.replace(), PhoenixOption)
    assert_type(note.replace(coupon_per_period=0.01), PhoenixOption)
    assert_type(note.replace(coupon_per_period=Ellipsis), PhoenixOption)
    note.replace(coupon_rate=0.1)  # ty: ignore[unknown-argument]
    note.replace(coupon_per_period=None)  # ty: ignore[invalid-argument-type]


def phoenix_presets(start: datetime, end: datetime) -> None:
    assert_type(
        PhoenixOption.standard(
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
        PhoenixOption.step_down(
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
    observations = [SnowballObservation(end, 110, coupon_rate=0)]
    assert_type(
        SnowballOption(
            calendar=TradingCalendar.weekdays(),
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
            calendar=TradingCalendar.weekdays(),
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
