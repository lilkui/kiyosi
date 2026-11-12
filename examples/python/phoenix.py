"""Build Phoenix notes and solve their implied coupon rates."""

from datetime import datetime, time

from kiyosi.instruments import (
    fixed_coupon_note,
    standard_phoenix,
    step_down_phoenix,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext, monthly_schedule
from kiyosi.pricing import FiniteDifferenceAutocallableEngine, implied_phoenix_coupon

start, end = datetime(2025, 1, 2, 15), datetime(2025, 7, 2, 15)
times = monthly_schedule(
    start_time=start,
    end_time=end,
    first_observation_month=1,
    observation_clock_time=time(15),
)

context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.02, dividend_yield=0, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
engine = FiniteDifferenceAutocallableEngine()

# 前两期正常观察票息和敲入，从第三期开始允许敲出。
standard = standard_phoenix(
    coupon_per_period=0.01,
    initial_spot=100,
    knock_in_level=80,
    knock_out_level=103,
    coupon_barrier_level=90,
    lockout_periods=2,
    knock_in_observation_clock_time=time(15),
    autocall_times=times,
    effective_time=start,
    expiry_time=end,
)
# 首次允许敲出时障碍为 103，此后每期递减 1；锁定期不消耗降敲阶数。
step_down = step_down_phoenix(
    coupon_per_period=0.01,
    initial_spot=100,
    knock_in_level=80,
    initial_knock_out_level=103,
    knock_out_level_decrement=1,
    coupon_barrier_level=90,
    lockout_periods=2,
    knock_in_observation_clock_time=time(15),
    autocall_times=times,
    effective_time=start,
    expiry_time=end,
)

# FCN 工厂返回 Phoenix 合约：无条件固定每期票息，到期才观察敲入。
# coupon_per_period=0.01 表示每期本金的 1%，不按天数折算。
fcn = fixed_coupon_note(
    coupon_per_period=0.01,
    initial_spot=100,
    knock_in_level=80,
    knock_out_level=103,
    autocall_times=times,
    effective_time=start,
    expiry_time=end,
)

# 三种结构都返回单位本金价值，求解值均为每期绝对票息率。
for name, note in (
    ("标准凤凰", standard),
    ("降敲凤凰", step_down),
    ("FCN", fcn),
):
    print(f"{name} 单位本金价格：{engine.price(note, context):.6f}")
    target = 1.0
    quote = implied_phoenix_coupon(
        engine, note, context, target, lower_bound=0, upper_bound=0.5
    )
    quoted = note.replace(**quote)
    coupon = quote["coupon_per_period"]
    repriced = engine.price(quoted, context)
    print(f"{name} 平价票息：{coupon:.4%}（每期），回代价格：{repriced:.8f}")
    assert abs(repriced - target) < 1e-7
