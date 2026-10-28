# kiyosi

[![PyPI](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![MIT License](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

[简体中文](README.md) | English

kiyosi is a derivatives pricing library for Python users, supporting vanilla and exotic derivatives under the Black–Scholes–Merton model. Its Rust core also works independently of Python.

The project is in Alpha. Breaking API changes are possible.

The default build and Python package use the Rust CPU core. See the
[repository layout and responsibilities](docs/architecture.md) and
[test guide](tests/README.md). Delivery is CPU only, with no public C++ or C ABI.

## Products and features

| Product | Pricing methods |
| --- | --- |
| European vanilla options | Closed-form formulas, binomial trees, quadrature, finite differences, and Monte Carlo |
| American vanilla options | Bjerksund–Stensland approximation, binomial trees, finite differences, and Monte Carlo |
| Cash-or-nothing and asset-or-nothing digital options | Analytic formulas, quadrature, finite differences, CRR binomial trees, and Monte Carlo |
| Single-barrier vanilla options | Analytic methods, finite differences, Monte Carlo, CRR binomial trees, and quadrature |
| Binary barrier options and single-barrier touch/no-touch options | Analytic methods and finite differences |
| Double-barrier options | Continuous analytic valuation, finite differences, and Monte Carlo |
| Cash double one-touch and double no-touch options | Finite differences |
| Geometric and arithmetic average-price Asian options | Geometric closed-form formulas, arithmetic approximations, and Monte Carlo |
| Snowballs and phoenixes | Quadrature, finite differences, and Monte Carlo |
| Accumulators | Quadrature, finite differences and Monte Carlo |
| Range accruals | Analytic marginal probabilities |

The library also provides:

- Greeks, numerical sensitivities, and implied volatility and coupon solvers.
- Risk-free rate and dividend yield term structures, trading calendars, and observation schedules.
- Parallel CPU Monte Carlo execution with pseudorandom sampling, Sobol quasi-Monte Carlo sampling, and antithetic variates.

## Install the Python package

Python 3.11 or later is required. Install from PyPI:

```sh
python -m pip install kiyosi
```

Ordinary CPython 3.11–3.15 wheels target Linux x86-64 and Windows AMD64.
Prebuilt wheels require no Rust toolchain. Source builds use maturin and Rust;
Windows also needs the Visual Studio C++ linker tools.

For the standalone Rust core, add `kiyosi-core = { path = "crates/kiyosi-core" }` to a
consumer's Cargo dependencies, adjusting the path to your checkout. Core build,
test, documentation and packaging need only Cargo; the workspace defaults to it.

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
