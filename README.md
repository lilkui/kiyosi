# kiyosi

简体中文 | [English](README.en.md)

kiyosi 是一个面向 Python 用户的金融衍生品定价库，基于 C++23 核心实现 Black–Scholes–Merton 模型下的香草及奇异衍生品定价。Python 接口通过 nanobind 调用同一核心；C++ 库也可以独立构建和使用。

项目目前处于 Alpha 阶段，API 可能发生不兼容变更。

## 支持的产品与功能

| 产品 | 定价方法 |
| --- | --- |
| 欧式、美式香草期权 | 解析解、美式近似、二叉树、数值积分、有限差分、蒙特卡洛，具体支持取决于行权方式 |
| 现金或无、资产或无数字期权 | 解析解、数值积分、有限差分 |
| 障碍期权、二元障碍与触碰类期权 | 解析方法；普通障碍期权另支持有限差分 |
| 几何平均、算术平均亚式期权 | 几何平均解析解、算术平均近似 |
| 雪球、二元雪球、三元雪球、凤凰及累购产品 | 有限差分、蒙特卡洛 |

此外提供 Greeks、数值敏感度、隐含波动率及隐含票息求解、交易日历和观察日程。蒙特卡洛引擎支持 CPU 及可选 CUDA 后端。选择引擎时，应根据产品类型、合约条件和所需指标确定。

## 安装 Python 包

需要 Python 3.11 或更新版本。从 PyPI 安装：

```sh
python -m pip install kiyosi
```

安装适配当前环境的预编译 wheel 时，无需本地 C++ 编译工具链。

### 从源码安装

从源码安装需要支持 C++23 的编译器；构建使用 CMake 3.28 或更新版本，Python 构建依赖由 pip 的隔离构建环境安装。

Windows 用户请先打开 Visual Studio 的 **Developer PowerShell** 或 **x64 Native Tools Command Prompt**，再运行安装命令。

```sh
git clone https://github.com/lilkui/kiyosi.git
cd kiyosi
python -m pip install .
```

默认源码构建使用 CPU，不需要 CUDA 或 QuantLib。启用 CUDA 时，需要可用的 CUDA Toolkit、兼容的编译器、NVIDIA GPU 及驱动：

```sh
python -m pip install . -Ccmake.define.KIYOSI_ENABLE_CUDA=ON
```

安装后，在支持的蒙特卡洛引擎上指定 `backend="cuda"`。后端不可用时抛出 `BACKEND_UNAVAILABLE` 类别的错误。

## Python 快速上手

计算一年期欧式看涨期权的价格与 Greeks，并反解隐含波动率：

```python
from datetime import date

from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine, implied_volatility

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

engine = AnalyticVanillaEngine()
price = engine.price(option, context)
result = engine.price_with_greeks(option, context, greeks=["delta", "gamma", "vega"])
volatility = implied_volatility(engine, option, context, observed_price=price)

print(f"Price: {price:.6f}")  # 8.827321
print(f"Delta: {result.delta:.6f}")  # 0.573496
print(f"Implied volatility: {volatility:.2%}")  # 20.00%
```

接口按用途分为三个模块：

- `kiyosi.instruments`：合约及产品预设，例如 `EuropeanOption`、`BarrierOption`、`standard_snowball`。
- `kiyosi.market`：模型参数、估值上下文、交易日历和观察日程。
- `kiyosi.pricing`：定价引擎、Greeks 及隐含参数求解。

### 数值与接口约定

- 利率、股息率和波动率使用小数，例如 `0.20` 表示 20%；利率和股息率采用连续复利。
- `price()` 返回浮点数；`price_with_greeks()` 返回 `PricingResult`。未请求、未定义或不支持的 Greek 为 `None`。
- Vega 和 rho 分别表示波动率、利率变动一个百分点时的价格敏感度；theta 按估值时间向前移动一个日历日计量。
- 选项使用字符串，例如 `"call"`、`"put"`、`"cpu"`、`"cuda"`。
- 默认交易日历为 `weekdays_calendar()`；使用上交所日历时显式传入 `calendar=sse_calendar()`。其未来节假日包含估算，详见[日历数据说明](docs/sse-calendar.md)。
- 领域校验失败抛出 `kiyosi.KiyosiError`，通过 `error.category` 与 `kiyosi.ErrorCategory` 判断原因，无需解析错误消息。类型、签名及表示转换错误使用 Python 内置异常，内存耗尽使用 `MemoryError`。

## 定价结果检验

项目使用 **QuantLib** 生成独立参考价格及 Greeks，并将结果保存为[定价参考数据](tests/fixtures/pricing_reference.tsv)，供回归测试按各场景的误差容限检验。参考数据生成环境固定 QuantLib 版本；详见[生成与校验说明](tools/quantlib-oracle/GENERATION.md)。QuantLib 不是 kiyosi 的运行依赖，C++ 参考测试也无需安装 Python 或 QuantLib。

雪球、凤凰和累购等产品另有[有限差分与蒙特卡洛交叉检验](docs/fd-mc-validation.md)，并结合独立结算及极限收益检查。每个测试场景分别设置误差容限，覆盖网格误差和采样误差。

安装 Python 包后，可在仓库根目录运行 Python 测试：

```sh
python -m unittest discover -s tests/python -p "test_*.py"
```

## C++ 用法

C++ 核心无需 Python 或绑定框架即可构建、测试和安装。需要 C++23 编译器与 CMake 3.28+；Windows 下同样先激活 Visual Studio Developer 环境。

```sh
cmake -S . -B out/core -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF -DKIYOSI_BUILD_PYTHON=OFF
cmake --build out/core --config Release
cmake --install out/core --config Release --prefix out/install/core
```

在使用方项目中，将安装目录加入 `CMAKE_PREFIX_PATH`，并链接导出的目标：

```cmake
find_package(kiyosi CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE kiyosi::kiyosi)
```

包含 `<kiyosi/kiyosi.hpp>`，通过 `kiyosi::make_bsm_parameters`、`kiyosi::make_pricing_context` 和 `kiyosi::make_european_option` 创建已校验的对象，再调用 `kiyosi::AnalyticVanillaEngine::price`。工厂及定价操作返回 `kiyosi::Result<T>`（基于 `std::expected`），使用值之前须检查结果，失败时读取 `error().category`。

## 许可证

采用 [MIT 许可证](LICENSE.txt)。
