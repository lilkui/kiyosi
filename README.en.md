# kiyosi

[简体中文](README.md) | English

kiyosi is a derivatives pricing library for Python users, with a C++23 core for vanilla and exotic derivatives under the Black–Scholes–Merton model. The Python API calls the same core through nanobind; the C++ library can also be built and used independently.

The project is in Alpha. Breaking API changes are possible.

## Products and features

| Product | Pricing methods |
| --- | --- |
| European and American vanilla options | Closed-form formulas, American approximations, binomial trees, quadrature, finite differences, and Monte Carlo, depending on exercise style |
| Cash-or-nothing and asset-or-nothing digital options | Analytic formulas, quadrature, and finite differences |
| Barrier, binary barrier, and touch options | Analytic methods; ordinary barrier options also support finite differences |
| Geometric and arithmetic average-price Asian options | Geometric closed-form formulas and arithmetic approximations |
| Snowballs, binary snowballs, ternary snowballs, phoenixes, and accumulators | Finite differences and Monte Carlo |

The library also provides Greeks, numerical sensitivities, implied volatility and coupon solvers, trading calendars, and observation schedules. Monte Carlo engines support CPU execution and an optional CUDA backend. Choose an engine based on the product type, contract terms, and required measures.

## Install the Python package

Python 3.11 or later is required. Install from PyPI:

```sh
python -m pip install kiyosi
```

Installing a prebuilt wheel compatible with your environment does not require a local C++ build toolchain.

### Install from source

Installing from source requires a compiler with C++23 support. The build uses CMake 3.28 or later; pip installs Python build dependencies in an isolated build environment.

On Windows, open Visual Studio's **Developer PowerShell** or **x64 Native Tools Command Prompt** before running the installation commands.

```sh
git clone https://github.com/lilkui/kiyosi.git
cd kiyosi
python -m pip install .
```

The default source build uses the CPU and requires neither CUDA nor QuantLib. To enable CUDA, you need an available CUDA Toolkit, a compatible compiler, and an NVIDIA GPU and driver:

```sh
python -m pip install . -Ccmake.define.KIYOSI_ENABLE_CUDA=ON
```

After installation, select `backend="cuda"` on a supported Monte Carlo engine. An unavailable backend raises an error with category `BACKEND_UNAVAILABLE`.

## Python quick start

Price a one-year European call, request Greeks, and recover its implied volatility:

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

The API is organized into three modules:

- `kiyosi.instruments`: contracts and product presets, such as `EuropeanOption`, `BarrierOption`, and `standard_snowball`.
- `kiyosi.market`: model parameters, pricing contexts, trading calendars, and observation schedules.
- `kiyosi.pricing`: pricing engines, Greeks, and implied-parameter solvers.

### Numerical and API conventions

- Rates, dividend yields, and volatility are decimals: `0.20` means 20%. Rates and dividend yields are continuously compounded.
- `price()` returns a float; `price_with_greeks()` returns a `PricingResult`. Unrequested, undefined, or unsupported Greeks are `None`.
- Vega and rho measure price sensitivity per percentage point of volatility and interest rate, respectively. Theta is measured per calendar day as valuation time moves forward.
- Choices use strings such as `"call"`, `"put"`, `"cpu"`, and `"cuda"`.
- The default trading calendar is `weekdays_calendar()`. Pass `calendar=sse_calendar()` explicitly for the Shanghai Stock Exchange calendar. Its future holidays include estimates; see the [calendar data notes](docs/sse-calendar.md).
- Domain validation failures raise `kiyosi.KiyosiError`. Inspect `error.category` against `kiyosi.ErrorCategory` rather than parsing messages. Type, signature, and representation conversion errors use Python's built-in exceptions; memory exhaustion raises `MemoryError`.

## Pricing validation

The project uses **QuantLib** to generate independent reference prices and Greeks, stored in a [pricing reference fixture](tests/fixtures/pricing_reference.tsv). Regression tests check these values against scenario-specific error tolerances. The generation environment pins the QuantLib version; see the [generation and validation notes](tools/quantlib-oracle/GENERATION.md). QuantLib is not a runtime dependency of kiyosi, and the C++ reference tests require neither Python nor QuantLib.

Snowballs, phoenixes, and accumulators also have [finite-difference/Monte Carlo cross-validation](docs/fd-mc-validation.md), supplemented by independent settlement and limiting-payoff checks. Each test scenario sets separate tolerances for grid error and sampling error.

After installing the Python package, run the Python tests from the repository root:

```sh
python -m unittest discover -s tests/python -p "test_*.py"
```

## C++ usage

The C++ core can be built, tested, and installed without Python or binding frameworks. It requires a C++23 compiler and CMake 3.28+. On Windows, activate the Visual Studio Developer environment first.

```sh
cmake -S . -B out/core -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF -DKIYOSI_BUILD_PYTHON=OFF
cmake --build out/core --config Release
cmake --install out/core --config Release --prefix out/install/core
```

In a consuming project, add the installation directory to `CMAKE_PREFIX_PATH` and link the exported target:

```cmake
find_package(kiyosi CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE kiyosi::kiyosi)
```

Include `<kiyosi/kiyosi.hpp>`, create validated objects with `kiyosi::make_bsm_parameters`, `kiyosi::make_pricing_context`, and `kiyosi::make_european_option`, then call `kiyosi::AnalyticVanillaEngine::price`. Factories and pricing operations return `kiyosi::Result<T>` (based on `std::expected`): check the result before using its value, and inspect `error().category` on failure.

## License

Licensed under the [MIT License](LICENSE.txt).
