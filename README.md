# kiyosi

[![PyPI](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![MIT License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

简体中文 | [English](README.en.md)

kiyosi 是一个面向 Python 用户的金融衍生品定价库，支持 Black–Scholes–Merton 模型下的香草及奇异衍生品定价，也提供可独立使用的 C++ 库。

项目目前处于 Alpha 阶段，API 可能发生不兼容变更。

Rust 重写正在分阶段进行。[候选版](docs/rust-candidate.md)提供欧式解析定价、美式近似定价、香草与数字期权 CRR、数值积分和有限差分、单障碍期权 CRR、数值积分与有限差分、单/双障碍、二元障碍与单边触碰/不触碰解析定价及事件历史、Greeks 与隐含波动率、期限结构、交易日历与观察日程，以及独立 Rust 核心与隔离安装的同名 Python 包；下文仍描述现有交付。

## 支持的产品与功能

| 产品 | 定价方法 |
| --- | --- |
| 欧式、美式香草期权 | 解析解、美式近似、二叉树、数值积分、有限差分、蒙特卡洛 |
| 现金或无、资产或无数字期权 | 解析解、数值积分、有限差分、CRR 二叉树、蒙特卡洛 |
| 单障碍香草期权 | 解析方法、有限差分、蒙特卡洛、CRR 二叉树、数值积分 |
| 二元障碍与单边触碰/不触碰期权 | 解析方法、有限差分 |
| 双障碍期权 | 连续观察解析定价、有限差分、蒙特卡洛 |
| 现金双边触碰/不触碰期权 | 有限差分 |
| 几何平均、算术平均亚式期权 | 几何平均解析解、算术平均近似、蒙特卡洛 |
| 雪球、凤凰 | 数值积分、有限差分、蒙特卡洛 |
| 累购产品 | 数值积分、有限差分、蒙特卡洛 |
| 区间累计产品 | 边际概率解析定价 |

此外提供：

- Greeks、数值敏感度、隐含波动率及隐含票息求解。
- 无风险利率与股息率期限结构、交易日历和观察日程。
- 蒙特卡洛 CPU 并行及可选 CUDA 后端、伪随机采样、Sobol 准蒙特卡洛采样和对偶变量。

## 安装 Python 包

需要 Python 3.11 或更新版本。从 PyPI 安装：

```sh
python -m pip install kiyosi
```

预编译 wheel 无需本地 C++ 工具链；从源码构建需要 C++23 编译器、CMake 3.28+ 和 oneTBB 2023.1+（未安装时自动下载）。

Windows 源码构建和 CMake 预设使用 clang-cl。请安装 Visual Studio C++ 工具及 C++ Clang 编译器组件，并在 Visual Studio Developer PowerShell 中运行构建命令。

## Python 快速上手

计算一年期欧式看涨期权的价格。利率、股息率和波动率使用小数，利率和股息率采用连续复利。

```python
from datetime import date

from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine

option = EuropeanOption(
    option_type="call",
    strike=100.0,
    effective_date=date(2025, 1, 1),
    expiry_date=date(2026, 1, 1),
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.03,
        dividend_yield=0.01,
        volatility=0.20,
    ),
    spot_price=100.0,
    valuation_time=date(2025, 1, 1),
)

price = AnalyticVanillaEngine().price(option, context)
print(f"Price: {price:.6f}")  # 8.827321
```

## 许可证

采用 [MIT 许可证](LICENSE.txt)。第三方组件的版权和许可条款见 [第三方声明](THIRD_PARTY_NOTICES.txt)。
