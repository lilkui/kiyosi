[English](README.md) | [**简体中文**](README.zh-CN.md)

# Kiyosi

Kiyosi 是一个拥有 C++23 核心和 Python 绑定的期权定价库。它提供经过验证的市场与金融工具类型，以及解析法、树模型、有限差分、积分和蒙特卡洛定价引擎。

[![许可证：MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.txt)

> [!IMPORTANT]
> Kiyosi 目前处于 Alpha 阶段，API 可能发生不兼容的变更。

## 功能

- 香草、数字、亚式、障碍、累计、雪球和凤凰期权
- 解析法、二叉树、有限差分、积分和蒙特卡洛定价引擎
- 通过统一的结果类型返回价格和希腊字母
- 情景网格、数值分析和隐含值求解器
- 交易日历和观察日程构建器，包括上交所节假日
- Python 和 C++ API 具有一致的领域语义

## 使用 Python 快速开始

需要 Python 3.11 或更高版本：

```bash
python -m pip install kiyosi
```

使用 Black-Scholes 解析引擎为欧式看涨期权定价：

```python
from datetime import date

from kiyosi.instruments import EuropeanOption, OptionType
from kiyosi.market import BsmParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine

parameters = BsmParameters(
    risk_free_rate=0.05,
    dividend_yield=0.02,
    volatility=0.20,
)
context = PricingContext(
    parameters=parameters,
    asset_price=100.0,
    valuation_time=date(2025, 1, 1),
)
option = EuropeanOption(
    type=OptionType.CALL,
    strike=100.0,
    effective=date(2025, 1, 1),
    expiry=date(2026, 1, 1),
)

result = AnalyticVanillaEngine().price(option, context)
print(result.price, result.delta, result["vega"])
```

Python API 分为三个模块：

| 模块 | 内容 |
| --- | --- |
| `kiyosi.instruments` | 经过验证的衍生品工具和结构化产品预设 |
| `kiyosi.market` | 模型参数、估值上下文、日历和日程 |
| `kiyosi.pricing` | 定价引擎、分析工具、情景分析和隐含值求解器 |

领域验证失败会抛出带有稳定 `ErrorCategory` 的 `KiyosiError`。Python 转换失败会使用相应的内置异常，例如 `TypeError` 或 `OverflowError`。

## C++ 库

从源码构建 C++ 核心需要 CMake 3.28 或更高版本、Ninja，以及支持 C++23 的编译器。

使用当前平台对应的预设进行配置、构建和测试：

```bash
cmake --preset linux-release
cmake --build --preset linux-release
ctest --preset linux-release
```

在 Windows 上，请先打开 Visual Studio Developer PowerShell，再使用 `windows-release`。其他调试、CI 和内存安全检测预设见 [`CMakePresets.json`](CMakePresets.json)。

安装库并链接其导出的 CMake 目标：

```bash
cmake --install out/build/linux-release --prefix out/install/kiyosi
```

```cmake
find_package(kiyosi CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE kiyosi::kiyosi)
```

```cpp
#include <kiyosi/kiyosi.hpp>
```

完整的 C++ 示例见 [`examples/all_pricing_engines.cpp`](examples/all_pricing_engines.cpp)，其中涵盖了现有的金融工具和定价引擎类别。

## 许可证

Kiyosi 使用 [MIT 许可证](LICENSE.txt)。
