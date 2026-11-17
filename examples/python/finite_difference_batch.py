"""复用一次 FD 求解，批量计算不同现货价格下的价格、delta 和 gamma。"""

from datetime import datetime

from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import FiniteDifferenceVanillaEngine

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
option = EuropeanOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)

# 合约、模型和估值时刻固定；批量查询只改变现货价格，不重复回溯网格。
engine = FiniteDifferenceVanillaEngine()
solution = engine.solve(option, context, spot_range=(70, 130))
spots = (80, 90, 100, 110, 120)
prices = solution.price_many(spots)
results = solution.price_many_with_greeks(spots, greeks=("delta", "gamma"))

# 查询区间需为 Greeks 的差分采样留出余量；不可用的指标返回 None。
print(f"{'Spot':>8} {'Price':>12} {'Delta':>12} {'Gamma':>12}")
for spot, price, result in zip(spots, prices, results, strict=True):
    assert result.delta is not None and result.gamma is not None
    print(f"{spot:8.2f} {result.price:12.6f} {result.delta:12.6f} {result.gamma:12.6f}")
    assert price == result.price
