# Kiyosi

Kiyosi is a modern C++23 derivatives-pricing library with Python bindings, offering consistent APIs for vanilla, exotic, and structured products.

[![PyPI](https://img.shields.io/pypi/v/kiyosi.svg)](https://pypi.org/project/kiyosi/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.txt)

> [!IMPORTANT]
> Kiyosi is alpha software. Its API may change without backward-compatibility guarantees.

## Features

- Vanilla, digital, Asian, barrier, accumulator, snowball, and phoenix instruments
- Analytic, tree-based, finite-difference, integral, and Monte Carlo pricing engines
- Prices and Greeks through a consistent result type
- Numerical analytics, implied volatility, and implied coupon solvers
- Trading calendars and observation schedule builders, including SSE holidays
- A native C++ core exposed through a Python-first API

## Quick start with Python

Kiyosi requires Python 3.11 or newer:

```bash
python -m pip install kiyosi
```

PyPI provides prebuilt x64 wheels for Windows and Linux. On other platforms, installation builds from source and requires CMake 3.28 or newer, Ninja, and a C++23 compiler.

Price a European call with the analytic Black-Scholes engine:

```python
from datetime import date

from kiyosi.instruments import EuropeanOption, OptionType
from kiyosi.market import BsmParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine

valuation = date(2025, 1, 1)
option = EuropeanOption(
    type=OptionType.CALL,
    strike=100.0,
    effective=valuation,
    expiry=date(2026, 1, 1),
)
context = PricingContext(
    parameters=BsmParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        volatility=0.20,
    ),
    asset_price=100.0,
    valuation_time=valuation,
)

result = AnalyticVanillaEngine().price(option, context)
print(result.price)
```

The Python API is organized into three modules:

| Module | Contents |
| --- | --- |
| `kiyosi.instruments` | Derivative instruments and structured-product presets |
| `kiyosi.market` | Model parameters, valuation contexts, calendars, and schedules |
| `kiyosi.pricing` | Pricing engines, analytics, scenarios, and implied-value solvers |

## Pricing coverage

| Instrument family | Available engines |
| --- | --- |
| European vanilla | Analytic, CRR binomial, finite difference, integral, Monte Carlo |
| American vanilla | Bjerksund-Stensland, CRR binomial, finite difference, Monte Carlo |
| Cash-or-nothing and asset-or-nothing digital | Analytic, finite difference, integral |
| Barrier | Analytic, finite difference |
| Binary barrier and touch | Analytic |
| Geometric-average Asian | Closed form |
| Arithmetic-average Asian | Turnbull-Wakeman approximation |
| Accumulator | Finite difference, Monte Carlo |
| Phoenix and snowball variants | Finite difference, Monte Carlo |

### Model scope

The current pricing models use a Black-Scholes-Merton market context with spot and flat risk-free rate, dividend yield, and volatility parameters. Volatility surfaces and rate curves are not part of the current API.

## Validation

Kiyosi's pricing tests compare results with reference values generated independently of Kiyosi using [QuantLib](https://www.quantlib.org/). QuantLib is used by the [reference-generation tooling](tools/quantlib-oracle/GENERATION.md) and [SSE calendar maintenance script](tools/calendar_data.py); it is not a build or runtime dependency of the C++ core.

## C++ library

Building the C++ core requires CMake 3.28 or newer, Ninja, and a C++23 compiler. On Linux, configure, build, test, and install with:

```bash
cmake --preset linux-release
cmake --build --preset linux-release
ctest --preset linux-release
cmake --install out/build/linux-release
```

On Windows, run the commands from a Visual Studio Developer PowerShell and replace `linux-release` with `windows-release`.

After installation, consume the exported CMake target:

```cmake
find_package(kiyosi CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE kiyosi::kiyosi)
```

Include the umbrella header with `#include <kiyosi/kiyosi.hpp>`. See [`examples/all_pricing_engines.cpp`](examples/all_pricing_engines.cpp) for a broader example covering the available instrument and engine families.

## License

Kiyosi is available under the [MIT License](LICENSE.txt).
