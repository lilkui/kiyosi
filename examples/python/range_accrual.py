"""Price a range accrual with a daily observation clock and prior hit counts."""

from datetime import datetime, time

from kiyosi.instruments import RangeAccrualOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticRangeAccrualEngine

start, end = datetime(2025, 1, 2, 15), datetime(2025, 2, 3, 15)
option = RangeAccrualOption(
    notional=1_000_000,
    annual_coupon_rate=0.12,
    lower_bound=95,
    upper_bound=110,
    effective_time=start,
    expiry_time=end,
    observation_clock_time=time(15),
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.02, dividend_yield=0, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
engine = AnalyticRangeAccrualEngine()
price = engine.price(option, context)

# 按交易日 15:00 观察，含区间端点；生效时刻不计入观察。
# 票息 = 本金 × 年化票息 × Actual/365 期限 × 命中次数 / 总观察次数。
# 引擎返回到期票息的折现价值，已包含 notional，不含本金返还。
print(f"区间累计票息现值：{price:,.2f}")
full_coupon = option.notional * option.annual_coupon_rate * (end - start).days / 365
discount = context.model_parameters.risk_free_curve.factor(start, end)
print(f"全部观察均命中时的票息现值：{full_coupon * discount:,.2f}")
assert 0 < price < full_coupon * discount

# 假设估值时刻之前已有 7 次命中；估值时刻的观察由当前现货决定。
later = context.replace(valuation_time=datetime(2025, 1, 16, 15), spot_price=102)
seasoned = option.replace(realized_hit_count=7)
print(f"含历史命中的存续票息现值：{engine.price(seasoned, later):,.2f}")
