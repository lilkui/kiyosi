from datetime import datetime
from math import isclose

from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticEuropeanEngine, implied_volatility

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
option = EuropeanOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)
model = BlackScholesMertonParameters(
    risk_free_rate=0.05, dividend_yield=0.02, volatility=0.20
)
context = PricingContext(model_parameters=model, spot_price=100, valuation_time=start)
engine = AnalyticEuropeanEngine()
result = engine.price_with_greeks(
    option, context, greeks=["delta", "gamma", "vega", "theta"]
)
print(f"看涨期权价格：{result.price:.6f}")
for name in ("delta", "gamma", "vega", "theta"):
    print(f"{name}：{result.require(name):.6f}")

# Greeks 是未缩放偏导：vega 按波动率小数计，theta 按年计。
print(f"波动率上升 1 个百分点的近似价格变化：{result.require('vega') * 0.01:.6f}")
print(f"未请求的 rho：{result.rho}，状态：{result.status('rho')}")

# replace 返回独立对象；情景计算不修改原始市场快照或合约。
for spot in (90, 100, 110):
    scenario = context.replace(spot_price=spot)
    print(f"现货 {spot}：{engine.price(option, scenario):.6f}")
put = option.replace(option_type="put")
print(f"相同条款的看跌期权：{engine.price(put, context):.6f}")

# 用模拟市场报价演示反解；实际使用时替换为观测到的期权价格。
sigma = implied_volatility(
    engine,
    option,
    context,
    observed_price=result.price,
    lower_bound=0.01,
    upper_bound=1.0,
)
print(f"隐含波动率：{sigma:.4%}")
assert isclose(sigma, model.volatility, abs_tol=1e-7)
