"""Price European cash-or-nothing and asset-or-nothing options."""

from datetime import datetime
from math import isclose

from kiyosi.instruments import AssetOrNothingOption, CashOrNothingOption, EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticDigitalEngine, AnalyticEuropeanEngine

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
cash = CashOrNothingOption(
    option_type="call", strike=100, payout=10, effective_time=start, expiry_time=end
)
asset = AssetOrNothingOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)
vanilla = EuropeanOption(
    option_type="call", strike=100, effective_time=start, expiry_time=end
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05, dividend_yield=0.02, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)

# 到期现货严格大于行权价才支付；恰好等于行权价时，两种数字期权均不支付。
engine = AnalyticDigitalEngine()
cash_price = engine.price(cash, context)
asset_price = engine.price(asset, context)
print(f"现金数字看涨（到期支付 10）：{cash_price:.6f}")
print(f"资产数字看涨（到期支付一单位资产）：{asset_price:.6f}")
print(f"现金数字看跌：{engine.price(cash.replace(option_type='put'), context):.6f}")

# (S-K)+ = S·1(S>K) - K·1(S>K)，可用数字期权复制香草看涨。
replicated = asset_price - vanilla.strike / cash.payout * cash_price
vanilla_price = AnalyticEuropeanEngine().price(vanilla, context)
print(f"数字期权组合：{replicated:.6f}，香草看涨：{vanilla_price:.6f}")
assert isclose(replicated, vanilla_price, abs_tol=1e-10)
