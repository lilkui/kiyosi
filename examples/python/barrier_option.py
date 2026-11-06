from datetime import datetime
from math import isclose

from kiyosi.instruments import (
    BarrierOption,
    BinaryBarrierOption,
    DoubleBarrierOption,
    DoubleTouchOption,
    EuropeanOption,
    TouchOption,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import (
    AnalyticBarrierEngine,
    AnalyticBinaryBarrierEngine,
    AnalyticDoubleBarrierEngine,
    AnalyticDoubleTouchEngine,
    AnalyticEuropeanEngine,
)

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
barrier = BarrierOption(
    option_type="call",
    strike=100,
    effective_time=start,
    expiry_time=end,
    barrier_level=120,
    barrier_type="up_and_out",
)
double_barrier = DoubleBarrierOption(
    option_type="call",
    strike=100,
    effective_time=start,
    expiry_time=end,
    lower_barrier=80,
    upper_barrier=120,
    barrier_type="knock_out",
)
binary = BinaryBarrierOption(
    option_type="call",
    strike=100,
    effective_time=start,
    expiry_time=end,
    barrier_level=120,
    barrier_type="up_and_in",
    payoff_type="cash",
    payout=10,
)
touch = TouchOption(
    effective_time=start,
    expiry_time=end,
    barrier_level=120,
    barrier_type="up_and_in",
    payoff_type="cash",
    payout=10,
)
double_touch = DoubleTouchOption(
    effective_time=start,
    expiry_time=end,
    lower_barrier=80,
    upper_barrier=120,
    barrier_type="knock_in",
    payout=10,
)

# 默认连续监控。二元障碍还检查到期价与行权价；触碰产品只检查是否触碰。
# 这里均在到期支付；单触碰可指定 settlement_timing="at_hit"。
engine = AnalyticBarrierEngine()
out_price = engine.price(barrier, context)
print(f"向上敲出看涨：{out_price:.6f}")
print(
    f"双障碍敲出看涨：{AnalyticDoubleBarrierEngine().price(double_barrier, context):.6f}"
)
binary_engine = AnalyticBinaryBarrierEngine()
print(f"现金二元障碍：{binary_engine.price(binary, context):.6f}")
print(f"单触碰：{binary_engine.price(touch, context):.6f}")
print(f"双触碰：{AnalyticDoubleTouchEngine().price(double_touch, context):.6f}")

# 无返还金时，敲入与敲出之和等于对应香草期权。
knock_in = barrier.replace(barrier_type="up_and_in")
vanilla = EuropeanOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)
assert isclose(
    out_price + engine.price(knock_in, context),
    AnalyticEuropeanEngine().price(vanilla, context),
    abs_tol=1e-10,
)

# 历史只描述估值时刻之前的事件；当前时刻的触碰由引擎根据现货判断。
later = context.replace(valuation_time=datetime(2025, 7, 1), spot_price=105)
live = barrier.replace_history(touch_state="untouched")
settled = barrier.replace_history(touch_state="touched")
print(f"此前未触碰的存续价格：{engine.price(live, later):.6f}")
print(f"此前已敲出且无返还金的价格：{engine.price(settled, later):.6f}")
assert engine.price(settled, later) == 0
