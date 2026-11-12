"""Price European and American options under the Heston model."""

from datetime import datetime

from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import HestonParameters, PricingContext
from kiyosi.pricing import (
    AnalyticHestonEuropeanEngine,
    FiniteDifferenceHestonVanillaEngine,
)

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
model = HestonParameters(
    risk_free_rate=0.05,
    dividend_yield=0.02,
    initial_variance=0.04,
    long_run_variance=0.04,
    mean_reversion_speed=1.5,
    vol_of_vol=0.3,
    correlation=-0.7,
)

# 方差 0.04 对应 20% 波动率；correlation 为价格与方差驱动噪声的相关系数。
context = PricingContext(model_parameters=model, spot_price=100, valuation_time=start)
call = EuropeanOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)
fourier = AnalyticHestonEuropeanEngine()
price = fourier.price(call, context)
print(f"欧式看涨 Fourier 积分价：{price:.6f}")

# Heston 引擎不提供 Greeks；请求的指标标记为 unsupported。
print(f"Greeks 计算方式：{fourier.greek_method(call, context)}")
fd = FiniteDifferenceHestonVanillaEngine()
fd_price = fd.price(call, context)
print(f"欧式看涨二维 FD 价：{fd_price:.6f}，差值：{fd_price - price:.6f}")

# 此处仅校验示例参数下两个算法在 0.05 价格单位内一致；不是普遍误差保证。
# 二维 FD 应分别检查现货网格、方差网格、时间步数及两个上边界的收敛。
assert abs(fd_price - price) < 0.05

put = call.replace(option_type="put")
american = AmericanOption(
    option_type="put", strike=100, effective_time=start, expiry_time=end
)
european_price = fourier.price(put, context)
american_price = fd.price(american, context)
print(f"欧式看跌：{european_price:.6f}，美式看跌：{american_price:.6f}")
assert american_price >= european_price - 0.05
