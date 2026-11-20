# Kiyosi

[![PyPI version](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![CI](https://github.com/lilkui/kiyosi/actions/workflows/ci.yml/badge.svg)](https://github.com/lilkui/kiyosi/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

**简体中文** | [English](README.en.md)

Kiyosi 是一个衍生品定价库，核心引擎用 Rust 编写，为 Python 用户提供易用的 API。

## 特性

- **高性能内核**：核心计算由 Rust 实现，执行时释放 GIL，支持 Python 多线程并行。
- **产品覆盖**：包括香草、数字、障碍和亚式期权，以及雪球、凤凰、累购/累沽、区间累计等国内常见的场外结构化产品。
- **合约与引擎解耦**：同一合约可切换解析解、数值积分、二叉树、有限差分和蒙特卡洛等定价引擎，便于交叉验证。
- **模型支持**：提供 Black–Scholes–Merton（BSM）、局部波动率和 Heston 模型。

## 安装

**要求：** Python ≥ 3.12

通过 PyPI 安装预编译 wheel（推荐）：

```bash
pip install kiyosi
```

若需从源码构建，请先安装 Rust 和本机编译工具链：

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
| 欧式香草期权 | BSM、局部波动率、Heston | 解析解、数值积分、二叉树、有限差分、蒙特卡洛模拟 |
| 美式香草期权 | BSM、局部波动率、Heston | Bjerksund–Stensland 2002 近似、数值积分、二叉树、有限差分、最小二乘蒙特卡洛模拟 |
| 数字期权 | BSM、局部波动率 | 解析解、数值积分、二叉树、有限差分、蒙特卡洛模拟 |
| 几何平均亚式期权 | BSM | 解析解（连续平均）、蒙特卡洛模拟 |
| 算术平均亚式期权 | BSM | Turnbull–Wakeman 连续平均近似、蒙特卡洛模拟 |
| 单障碍期权 | BSM、局部波动率 | 解析解、数值积分、二叉树、有限差分、蒙特卡洛模拟 |
| 双障碍期权 | BSM、局部波动率 | 解析解（级数）、有限差分、蒙特卡洛模拟 |
| 二元障碍、单触碰与单不触碰期权 | BSM、局部波动率 | 解析解、有限差分 |
| 双触碰与双不触碰期权 | BSM、局部波动率 | 解析解（级数）、有限差分 |
| 雪球、二元雪球、三元雪球 | BSM、局部波动率 | 数值积分、有限差分、蒙特卡洛模拟 |
| 凤凰结构 | BSM、局部波动率 | 数值积分、有限差分、蒙特卡洛模拟 |
| 累购、累沽 | BSM、局部波动率 | 数值积分、有限差分、蒙特卡洛模拟 |
| 区间累计 | BSM | 解析解（区间概率求和） |

## 免责声明

本项目仅供研究与学习，用于生产环境前请自行验证。本库不承诺冻结公开 API，可能为优化设计而大幅重构。

## 许可证

[MIT](LICENSE.txt)
