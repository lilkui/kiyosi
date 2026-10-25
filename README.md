# kiyosi

[![PyPI](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![MIT License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

简体中文 | [English](README.en.md)

kiyosi 是一个面向 Python 用户的金融衍生品定价库，支持 Black–Scholes–Merton 模型下的香草及奇异衍生品定价，也提供可独立使用的 C++ 库。

项目目前处于 Alpha 阶段，API 可能发生不兼容变更。

## 支持的产品与功能

| 产品 | 定价方法 |
| --- | --- |
| 欧式、美式香草期权 | 解析解、美式近似、二叉树、数值积分、有限差分、蒙特卡洛，具体支持取决于行权方式 |
| 现金或无、资产或无数字期权 | 解析解、数值积分、有限差分、CRR 二叉树、蒙特卡洛 |
| 单障碍香草期权 | 解析方法、有限差分、蒙特卡洛、CRR 二叉树、数值积分 |
| 二元障碍与单边触碰类期权 | 解析方法 |
| 双障碍期权与现金双边触碰类期权 | 有限差分 |
| 几何平均、算术平均亚式期权 | 几何平均解析解、算术平均近似 |
| 雪球、二元雪球、三元雪球、凤凰 | 数值积分、有限差分、蒙特卡洛 |
| 累购产品 | 数值积分、有限差分、蒙特卡洛 |

此外提供 Greeks、数值敏感度、隐含波动率及隐含票息求解、交易日历和观察日程。蒙特卡洛引擎支持 CPU 及可选 CUDA 后端。

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
