"""Compare approximation, binomial, and finite-difference American prices."""

from datetime import datetime

from kiyosi.instruments import AmericanOption, EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import (
    AnalyticEuropeanEngine,
    BjerksundStenslandVanillaEngine,
    CoxRossRubinsteinVanillaEngine,
    FiniteDifferenceVanillaEngine,
)

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
american = AmericanOption(
    option_type="put", strike=100, effective_time=start, expiry_time=end
)
european = EuropeanOption(
    option_type="put", strike=100, effective_time=start, expiry_time=end
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
european_price = AnalyticEuropeanEngine().price(european, context)
print(f"欧式看跌价格：{european_price:.6f}")

# 相同合约与市场输入下比较算法；解析近似并非精确解。
engines = {
    "Bjerksund–Stensland 近似": BjerksundStenslandVanillaEngine(),
    "CRR 二叉树": CoxRossRubinsteinVanillaEngine(step_count=1000),
    "有限差分": FiniteDifferenceVanillaEngine(),
}
for name, engine in engines.items():
    price = engine.price(american, context)
    print(f"{name}：{price:.6f}，提前行权溢价：{price - european_price:.6f}")
    assert price >= european_price - 0.01

# 网格加密用于检查数值收敛；生产估值还需检查空间范围和时间步数。
fine = CoxRossRubinsteinVanillaEngine(step_count=2000).price(american, context)
coarse = engines["CRR 二叉树"].price(american, context)
print(f"CRR 加密至 2000 步：{fine:.6f}，价格差：{fine - coarse:.6f}")
assert abs(fine - coarse) < 0.01
