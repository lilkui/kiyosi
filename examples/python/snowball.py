from datetime import datetime, time

from kiyosi.instruments import standard_snowball
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    monthly_schedule,
    sse_calendar,
)
from kiyosi.pricing import FiniteDifferenceAutocallableEngine, implied_snowball_coupon

start, end = datetime(2025, 1, 2, 15), datetime(2026, 1, 5, 15)
calendar = sse_calendar()
observations = monthly_schedule(
    start_time=start,
    end_time=end,
    first_observation_month=1,
    observation_clock_time=time(15),
    calendar=calendar,
)
note = standard_snowball(
    coupon_rate=0.12,
    initial_spot=100,
    knock_in_level=80,
    knock_out_level=103,
    observation_times=observations,
    observation_clock_time=time(15),
    effective_time=start,
    expiry_time=end,
    calendar=calendar,
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.02, dividend_yield=0, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
engine = FiniteDifferenceAutocallableEngine()
price = engine.price(note, context)

# 雪球返回单位本金的价值，含条款规定的本金与收益；名义本金在外部缩放。
notional = 1_000_000
print(f"单位本金价格：{price:.6f}")
print(f"名义本金 {notional:,.0f} 对应的价值：{price * notional:,.2f}")

# 用平价报价 1.0 反解年化票息，并将报价应用回独立合约。
# 此约定同时平移观察票息和到期票息；另一个约定可固定到期票息。
convention = "shift_maturity_coupon"
target = 1.0
coupon = implied_snowball_coupon(
    engine,
    note,
    context,
    target,
    quote_convention=convention,
    lower_bound=0,
    upper_bound=0.5,
)
quoted = note.with_first_observation_coupon(coupon, quote_convention=convention)
repriced = engine.price(quoted, context)
print(f"平价年化票息：{coupon:.4%}，回代价格：{repriced:.8f}")
assert abs(repriced - target) < 1e-7

# 假设此前既未敲入也未敲出；历史来自实际观察记录，不由当前现货推断。
# 存续价格只计尚未结算的现金流。
live = note.replace_history(barrier_state="no_event")
later = context.replace(valuation_time=datetime(2025, 7, 1, 15), spot_price=95)
print(f"存续期单位本金价格：{engine.price(live, later):.6f}")
