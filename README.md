[**English**](README.md) | [简体中文](README.zh-CN.md)

# Kiyosi

Kiyosi is an option pricing library with a C++23 core and Python bindings. It
provides validated market and instrument types together with analytic,
tree-based, finite-difference, integral, and Monte Carlo pricing engines.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.txt)

> [!IMPORTANT]
> Kiyosi is alpha software. Its API may change without backward-compatibility
> guarantees.

## Features

- Vanilla, digital, Asian, barrier, accumulator, snowball, and phoenix instruments
- Analytic, binomial, finite-difference, integral, and Monte Carlo engines
- Prices and Greeks through a consistent result type
- Scenario grids, numerical analytics, and implied-value solvers
- Trading calendars and observation schedule builders, including SSE holidays
- Equivalent domain semantics across the Python and C++ APIs

## Quick start with Python

Python 3.11 or newer is required:

```bash
python -m pip install kiyosi
```

Price a European call with the analytic Black-Scholes engine:

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

The Python API is organized into three modules:

| Module | Contents |
| --- | --- |
| `kiyosi.instruments` | Validated derivative instruments and structured-product presets |
| `kiyosi.market` | Model parameters, valuation contexts, calendars, and schedules |
| `kiyosi.pricing` | Pricing engines, analytics, scenarios, and implied-value solvers |

Domain validation failures raise `KiyosiError` with a stable `ErrorCategory`.
Python conversion failures use the corresponding built-in exception, such as
`TypeError` or `OverflowError`.

## C++ library

Building the C++ core from source requires CMake 3.28 or newer, Ninja, and a
C++23 compiler.

Configure, build, and test with the preset for your platform:

```bash
cmake --preset linux-release
cmake --build --preset linux-release
ctest --preset linux-release
```

Use `windows-release` on Windows after opening a Visual Studio Developer
PowerShell. Other debug, CI, and sanitizer presets are listed in
[`CMakePresets.json`](CMakePresets.json).

Install the library and link its exported CMake target:

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

See [`examples/all_pricing_engines.cpp`](examples/all_pricing_engines.cpp) for
an end-to-end C++ example covering the available instrument and engine families.

## License

Kiyosi is available under the [MIT License](LICENSE.txt).
