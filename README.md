# Kiyosi

**简体中文** | [English](README.en.md)

Kiyosi 是基于 Rust 的 Python 衍生品定价库，支持 Black–Scholes–Merton 模型下的香草及奇异期权定价、希腊值计算和隐含参数求解。

> [!WARNING]
> 项目目前处于 Alpha 阶段，API 尚不承诺向后兼容。

## 支持的产品与算法

| 金融产品 | 定价算法 |
| --- | --- |
| 欧式香草期权 | BSM 解析解、CRR、数值积分、FD、MC |
| 美式香草期权 | Bjerksund–Stensland 2002 近似、CRR、FD、最小二乘 MC |
| 数字期权（现金或资产支付） | 解析解、CRR、数值积分、FD、MC |
| 几何平均亚式期权 | 连续平均解析解、MC |
| 算术平均亚式期权 | Turnbull–Wakeman 连续平均近似、MC |
| 单障碍期权 | 解析法、CRR、数值积分、FD、MC |
| 双障碍期权 | 解析级数、FD、MC |
| 二元障碍、单触碰与单不触碰期权 | 解析法、FD |
| 双触碰与双不触碰期权 | FD |
| 雪球、二元雪球、三元雪球 | 数值积分、FD、MC |
| 凤凰结构 | 数值积分、FD、MC |
| 累购／累沽 | 数值积分、FD、MC |
| 区间累计 | BSM 区间概率解析求和 |

## 安装

需要 **Python 3.11+**：

```bash
python -m pip install kiyosi
```

wheel 安装无需 Rust；源码安装需 Rust/Cargo 和本机编译工具。

## 快速上手

使用有限差分（FD）引擎为标准雪球定价：

```python
from datetime import date

from kiyosi.instruments import standard_snowball
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    monthly_schedule,
    weekdays_calendar,
)
from kiyosi.pricing import FiniteDifferenceAutocallableEngine

start, end = date(2025, 1, 2), date(2026, 1, 2)
calendar = weekdays_calendar()
observations = monthly_schedule(
    start=start,
    end=end,
    first_observation_month=1,
    calendar=calendar,
)
snowball = standard_snowball(
    coupon_rate=0.12,
    initial_spot=100.0,
    knock_in_level=80.0,
    knock_out_level=103.0,
    observation_dates=observations,
    effective_date=start,
    expiry_date=end,
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.02,
        dividend_yield=0.00,
        volatility=0.20,
    ),
    spot_price=100.0,
    valuation_time=start,
    calendar=calendar,
)
engine = FiniteDifferenceAutocallableEngine(
    asset_step_count=800,
    time_step_count=2000,
    scheme="crank_nicolson",
)
price = engine.price(snowball, context)
notional = 1_000_000
print(f"NPV / notional: {price:.6f}")
print(f"NPV: {price * notional:,.2f}")
```

## 许可证

[MIT](LICENSE.txt)
