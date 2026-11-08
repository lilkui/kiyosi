# Kiyosi

[![PyPI version](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![CI](https://github.com/lilkui/kiyosi/actions/workflows/ci.yml/badge.svg)](https://github.com/lilkui/kiyosi/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

**简体中文** | [English](README.en.md)

Kiyosi 是一个衍生品定价库，采用 Rust 构建核心引擎，并为 Python 用户提供易用的 API。

> [!WARNING]
> 项目目前处于 Alpha 阶段，API 尚不承诺向后兼容。

## 特性

- **Rust 核心**：Python 中建模，Rust 中计算，兼顾易用与速度
- **引擎可替换**：同一产品可以换用解析解、树、有限差分、蒙特卡洛等方法定价，方便交叉验证
- **风险指标**：支持希腊值计算和隐含波动率、隐含票息求解
- **结构化产品**：支持雪球、凤凰、累购/累沽、区间累计等国内常见结构
- **多种模型**：Black–Scholes–Merton（BSM）、局部波动率、Heston

## 安装

**要求：** Python ≥ 3.12

通过 PyPI 安装预编译 wheel（推荐）：

```bash
pip install kiyosi
```

若需从源码构建，请确保已安装 Rust 环境和本机编译工具链：

```bash
git clone https://github.com/lilkui/kiyosi.git
cd kiyosi
pip install .
```

## 快速上手

使用 BSM 解析解为欧式看涨期权定价：

```python
from datetime import datetime

from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticEuropeanEngine, implied_volatility

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
option = EuropeanOption(
    option_type="call",
    strike=100.0,
    effective_time=start,
    expiry_time=end,
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        volatility=0.20,
    ),
    spot_price=100.0,
    valuation_time=start,
)
engine = AnalyticEuropeanEngine()
result = engine.price_with_greeks(option, context, greeks="delta")
price, delta = result.price, result.delta
sigma = implied_volatility(engine, option, context, observed_price=price, lower_bound=0.1, upper_bound=0.3)
print(f"Price: {price:.6f}")  # Price: 9.227006
print(f"Delta: {delta:.6f}")  # Delta: 0.586851
print(f"Implied volatility: {sigma:.2%}")  # Implied volatility: 20.00%
```

更多详细用法请参阅 [`examples/`](examples/)。

## 支持产品与算法

| 金融产品 | 模型 | 定价算法 |
| --- | --- | --- |
| 欧式香草期权 | BSM、局部波动率、Heston | 解析解、数值积分、CRR、FD、MC |
| 美式香草期权 | BSM、局部波动率、Heston | Bjerksund–Stensland 2002 近似、数值积分、CRR、FD、最小二乘 MC |
| 数字期权 | BSM、局部波动率 | 解析解、数值积分、CRR、FD、MC |
| 几何平均亚式期权 | BSM | 解析解（连续平均）、MC |
| 算术平均亚式期权 | BSM | Turnbull–Wakeman 连续平均近似、MC |
| 单障碍期权 | BSM、局部波动率 | 解析解、数值积分、CRR、FD、MC |
| 双障碍期权 | BSM、局部波动率 | 解析解（级数）、FD、MC |
| 二元障碍、单触碰与单不触碰期权 | BSM、局部波动率 | 解析解、FD |
| 双触碰与双不触碰期权 | BSM、局部波动率 | 解析解（级数）、FD |
| 雪球、二元雪球、三元雪球 | BSM、局部波动率 | 数值积分、FD、MC |
| 凤凰结构 | BSM、局部波动率 | 数值积分、FD、MC |
| 累购、累沽 | BSM、局部波动率 | 数值积分、FD、MC |
| 区间累计 | BSM | 解析解（区间概率求和） |

缩写：CRR = Cox–Ross–Rubinstein 二叉树；FD = 有限差分；MC = 蒙特卡洛模拟。

## 免责声明

本项目仅供研究与学习，计算结果不构成任何投资建议，用于生产环境前请自行验证。

## 许可证

[MIT](LICENSE.txt)
