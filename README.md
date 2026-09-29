# Kiyosi

Kiyosi is a C++23 library for pricing vanilla, exotic, and structured derivatives, with Python bindings.

[![PyPI](https://img.shields.io/pypi/v/kiyosi.svg)](https://pypi.org/project/kiyosi/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.txt)

> [!IMPORTANT]
> Kiyosi is in alpha. Its APIs may change without backward-compatibility guarantees.

## Features

- Vanilla, digital, Asian, barrier, accumulator, snowball, and phoenix instruments
- Analytic, tree-based, finite-difference, integral, and Monte Carlo pricing engines, with optional CUDA support for Monte Carlo
- Price-only valuation or on-demand calculation of selected Greeks
- Numerical analytics and solvers for implied volatility and coupons
- Trading calendars and observation schedule builders, including the SSE calendar
- A C++ core that builds independently of the Python bindings

## Quick start with Python

Install Kiyosi from PyPI with Python 3.11 or newer:

```bash
python -m pip install kiyosi
```

Prebuilt x64 wheels are available for Windows and Linux. Installation on other platforms builds from source and requires CMake 3.28 or newer, Ninja, and a C++23 compiler.

Calculate the price of a European call with the analytic Black-Scholes engine:

```python
from datetime import date

from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine

valuation = date(2025, 1, 1)
option = EuropeanOption(
    option_type="call",
    strike=100.0,
    effective_date=valuation,
    expiry_date=date(2026, 1, 1),
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        volatility=0.20,
    ),
    spot_price=100.0,
    valuation_time=valuation,
)

engine = AnalyticVanillaEngine()
print(engine.price(option, context))
```

## C++ library

The C++ core requires CMake 3.28 or newer, Ninja, and a C++23 compiler. On Linux, build, test, and install it with:

```bash
cmake --workflow --preset linux-release
cmake --install out/build/linux-release
```

On Windows, run the commands in a Visual Studio Developer PowerShell, replacing `linux-release` with `windows-release`.

After installation, link the exported CMake target:

```cmake
find_package(kiyosi CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE kiyosi::kiyosi)
```

Include the umbrella header with `#include <kiyosi/kiyosi.hpp>`. For examples of additional instruments and pricing engines, see [`examples/all_pricing_engines.cpp`](examples/all_pricing_engines.cpp).

## Validation

Pricing tests compare Kiyosi's results with reference values generated independently using [QuantLib](https://www.quantlib.org/). QuantLib is used by the [reference-generation tooling](tools/quantlib-oracle/GENERATION.md) and is not required to build or run the C++ core. See the [pricing benchmark matrix](benchmarks/README.md) for performance comparisons.

## License

Kiyosi is available under the [MIT License](LICENSE.txt).
