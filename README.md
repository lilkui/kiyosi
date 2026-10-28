# kiyosi

[![PyPI](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![MIT License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

简体中文 | [English](README.en.md)

kiyosi 是一个面向 Python 用户的金融衍生品定价库，支持 Black–Scholes–Merton 模型下的香草及奇异衍生品定价，核心使用 Rust，可脱离 Python 独立使用。

项目目前处于 Alpha 阶段，API 可能发生不兼容变更。

默认构建与 Python 包统一使用 Rust CPU 核心。详见[目录结构与职责](docs/architecture.md)及[测试指南](tests/README.md)。仅交付 CPU 后端，不提供 C++ 或 C ABI。

## 支持的产品与功能

| 产品 | 定价方法 |
| --- | --- |
| 欧式香草期权 | 解析解、二叉树、数值积分、有限差分、蒙特卡洛 |
| 美式香草期权 | Bjerksund–Stensland 近似、二叉树、有限差分、蒙特卡洛 |
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
- CPU 蒙特卡洛并行、伪随机采样、Sobol 准蒙特卡洛采样和对偶变量。

## 安装 Python 包

需要 Python 3.11 或更新版本。从 PyPI 安装：

```sh
python -m pip install kiyosi
```

普通 CPython 3.11–3.15 wheel 覆盖 Linux x86-64 和 Windows AMD64。安装预编译 wheel 无需 Rust 工具链；从源码构建使用 maturin 和 Rust，Windows 还需要 Visual Studio C++ 链接工具。

Rust 消费方可在 Cargo 依赖中加入 `kiyosi-core = { path = "crates/kiyosi-core" }`，并按实际仓库位置调整路径。核心的构建、测试、文档与打包只需要 Cargo，workspace 默认仅构建核心。

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
