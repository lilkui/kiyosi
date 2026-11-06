from datetime import datetime
from math import isclose

from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import AnalyticEuropeanEngine

start = datetime(2025, 1, 1)
middle, end = datetime(2025, 7, 1), datetime(2026, 1, 1)

# 节点输入为从 reference_time 到节点的折现因子，不是利率。
# 曲线使用 Actual/365 Fixed，节点间对数折现因子线性插值，时间范围需覆盖估值。
curve = RateTermStructure(
    reference_time=start, times=[middle, end], factors=[0.98, 0.95]
)
print(f"全年折现因子：{curve.factor(start, end):.6f}")
print(f"后半段折现因子：{curve.factor(middle, end):.6f}")
print(f"中间时点远期利率：{curve.forward_rate(datetime(2025, 4, 1)):.4%}")

# 无风险利率可传曲线，股息率可同时使用标量；标量为年化连续复利小数。
model = BlackScholesMertonParameters(
    risk_free_rate=curve, dividend_yield=0.02, volatility=0.20
)
context = PricingContext(model_parameters=model, spot_price=100, valuation_time=start)
option = EuropeanOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)
engine = AnalyticEuropeanEngine()
base_price = engine.price(option, context)

# 平行上移 10 bp，返回新曲线；原曲线和原市场快照可继续用于基准估值。
shifted = curve.shifted(0.001)
scenario = context.replace(model_parameters=model.replace(risk_free_rate=shifted))
print(f"基准价格：{base_price:.6f}")
print(f"利率上移 10 bp：{engine.price(option, scenario):.6f}")
assert isclose(
    curve.factor(start, middle) * curve.factor(middle, end),
    curve.factor(start, end),
    abs_tol=1e-12,
)
