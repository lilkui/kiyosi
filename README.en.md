# kiyosi

[![PyPI](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![MIT License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

[简体中文](README.md) | English

kiyosi is a derivatives pricing library for Python users, supporting vanilla and exotic derivatives under the Black–Scholes–Merton model. It also provides a standalone C++ library.

The project is in Alpha. Breaking API changes are possible.

## Products and features

| Product | Pricing methods |
| --- | --- |
| European and American vanilla options | Closed-form formulas, American approximations, binomial trees, quadrature, finite differences, and Monte Carlo |
| Cash-or-nothing and asset-or-nothing digital options | Analytic formulas, quadrature, finite differences, CRR binomial trees, and Monte Carlo |
| Single-barrier vanilla options | Analytic methods, finite differences, Monte Carlo, CRR binomial trees, and quadrature |
| Binary barrier options and single-barrier touch/no-touch options | Analytic methods and finite differences |
| Double-barrier options | Continuous analytic valuation, finite differences, and Monte Carlo |
| Cash double one-touch and double no-touch options | Finite differences |
| Geometric and arithmetic average-price Asian options | Geometric closed-form formulas, arithmetic approximations, and Monte Carlo |
| Snowballs and phoenixes | Quadrature, finite differences, and Monte Carlo |
| Accumulators | Quadrature, finite differences and Monte Carlo |
| Range accruals | Monte Carlo |

The library also provides:

- Greeks, numerical sensitivities, and implied volatility and coupon solvers.
- Risk-free rate and dividend yield term structures, trading calendars, and observation schedules.
- Parallel CPU execution and an optional CUDA backend for Monte Carlo, with pseudorandom sampling, Sobol quasi-Monte Carlo sampling, and antithetic variates.

## Install the Python package

Python 3.11 or later is required. Install from PyPI:

```sh
python -m pip install kiyosi
```

Prebuilt wheels require no local C++ toolchain. Source builds require a C++23 compiler, CMake 3.28+, and oneTBB 2023.1+ (downloaded automatically if not installed).

Windows source builds and CMake presets use clang-cl. Install the Visual Studio C++ tools and C++ Clang compiler component, and run build commands in a Visual Studio Developer PowerShell.

## Python quick start

Price a one-year European call. Rates, dividend yields, and volatility are decimals; rates and dividend yields are continuously compounded.

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

## License

Licensed under the [MIT License](LICENSE.txt). See [third-party notices](THIRD_PARTY_NOTICES.txt) for third-party copyrights and license terms.
