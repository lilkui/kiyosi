"""Price options with an explicitly supplied local-volatility surface."""

from datetime import datetime

from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    PricingContext,
)
from kiyosi.pricing import FiniteDifferenceVanillaEngine

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)

# 输入的是局部波动率，不是隐含波动率；行对应时间，列对应绝对现货价格。
# 时间与现货间双线性插值；现货两端常数外推，时间不外推。
surface = LocalVolatilitySurface(
    times=[start, end],
    spots=[80, 100, 120],
    values=[[0.25, 0.20, 0.18], [0.27, 0.22, 0.20]],
)
print(
    f"中间时点、现货 110 的局部波动率：{surface.volatility(datetime(2025, 7, 1), 110):.4%}"
)
context = PricingContext(
    model_parameters=LocalVolatilityParameters(
        risk_free_rate=0.05, dividend_yield=0.02, surface=surface
    ),
    spot_price=100,
    valuation_time=start,
)
option = EuropeanOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)

# 局部波动率由一维 FD 引擎支持，提供固定绝对曲面下的 delta 和 gamma。
engine = FiniteDifferenceVanillaEngine(asset_upper_boundary=400)
result = engine.price_with_greeks(option, context, greeks=["delta", "gamma", "vega"])
print(f"价格：{result.price:.6f}")
print(f"delta：{result.require('delta'):.6f}，gamma：{result.require('gamma'):.6f}")
print(f"vega：{result.vega}，状态：{result.status('vega')}")

# 更改现货时，曲面绝对节点保持原位，不随新现货平移。
scenario = context.replace(spot_price=105)
print(f"现货升至 105：{engine.price(option, scenario):.6f}")

# 固定边界后加密网格；生产估值还需独立扩大边界检查截断误差。
refined = FiniteDifferenceVanillaEngine(
    asset_step_count=1600, time_step_count=800, asset_upper_boundary=400
).price(option, context)
print(f"加密价格：{refined:.6f}，价格差：{refined - result.price:.6f}")
assert abs(refined - result.price) < 0.01
