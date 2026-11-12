from datetime import datetime

from kiyosi.instruments import ArithmeticAveragePriceOption, GeometricAveragePriceOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import (
    AnalyticGeometricAveragePriceEngine,
    MonteCarloAsianEngine,
    TurnbullWakemanArithmeticAveragePriceEngine,
)

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
geometric = GeometricAveragePriceOption(
    option_type="call",
    strike=100,
    effective_time=start,
    averaging_start_time=start,
    expiry_time=end,
)
arithmetic = ArithmeticAveragePriceOption(
    option_type="call",
    strike=100,
    effective_time=start,
    averaging_start_time=start,
    expiry_time=end,
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
exact = AnalyticGeometricAveragePriceEngine().price(geometric, context)
approximation = TurnbullWakemanArithmeticAveragePriceEngine().price(arithmetic, context)
print(f"几何平均解析价：{exact:.6f}")
print(f"算术平均 Turnbull–Wakeman 近似价：{approximation:.6f}")

# 固定种子可在同一构建中重放随机抽样；使用偶数条反向路径以获得标准误。
mc = MonteCarloAsianEngine(
    path_count=16384, step_count=100, seed=42, antithetic=True, max_active_workers=1
)
for name, option in (("几何平均", geometric), ("算术平均", arithmetic)):
    estimate = mc.price_with_statistics(option, context)
    print(f"{name} MC：{estimate.price:.6f}，标准误：{estimate.standard_error:.6f}")
    print(f"95% 置信区间：{estimate.confidence_interval}")

# 几何平均的抽样分布可直接与连续平均解析解核对，容许五个标准误的偏差。
estimate = mc.price_with_statistics(geometric, context)
assert abs(estimate.price - exact) <= 5 * estimate.standard_error + 1e-10

# 算术平均的置信区间只衡量抽样误差，不包括时间离散偏差；还需加密 step_count。
# 估值进入平均期后，必须提供估值时刻之前对应种类的已实现平均值。
later = context.replace(valuation_time=datetime(2025, 7, 1), spot_price=103)
seasoned = arithmetic.replace(realized_average=102)
print(f"含历史算术平均值的存续价格：{mc.price(seasoned, later):.6f}")
