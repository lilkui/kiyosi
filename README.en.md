# Kiyosi

[![PyPI version](https://img.shields.io/pypi/v/kiyosi)](https://pypi.org/project/kiyosi/)
[![CI](https://github.com/lilkui/kiyosi/actions/workflows/ci.yml/badge.svg)](https://github.com/lilkui/kiyosi/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE.txt)

[简体中文](README.md) | **English**

Kiyosi is a derivatives pricing library with core engines built in Rust and an easy-to-use API for Python users.

> [!WARNING]
> The project is in Alpha; API backward compatibility is not guaranteed.

## Features

- **Rust core**: Model in Python and compute in Rust for ease of use and speed
- **Interchangeable engines**: Price the same product using analytic, tree, finite-difference, Monte Carlo and other methods for cross-validation
- **Risk metrics**: Calculate Greeks and solve for implied volatility and implied coupon rates
- **Structured products**: Support for structures common in China, including snowballs, Phoenix notes, accumulators, decumulators and range accruals
- **Multiple models**: Black–Scholes–Merton (BSM), local volatility and Heston

## Installation

**Requirement:** Python ≥ 3.12

Install a prebuilt wheel from PyPI (recommended):

```bash
pip install kiyosi
```

To build from source, install Rust and a native build toolchain:

```bash
git clone https://github.com/lilkui/kiyosi.git
cd kiyosi
pip install .
```

## Quick start

Price a European call option using the BSM analytic solution:

```python
from datetime import datetime

from kiyosi.instruments import EuropeanOption
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import AnalyticEuropeanEngine, implied_volatility

start, end = datetime(2025, 1, 1), datetime(2026, 1, 1)
option = EuropeanOption(
    option_type="call",
    strike=100.0,
    effective_time=start,
    expiry_time=end,
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.05,
        dividend_yield=0.02,
        volatility=0.20,
    ),
    spot_price=100.0,
    valuation_time=start,
)
engine = AnalyticEuropeanEngine()
result = engine.price_with_greeks(option, context, greeks="delta")
price, delta = result.price, result.delta
sigma = implied_volatility(engine, option, context, observed_price=price)
print(f"Price: {price:.6f}")  # Price: 9.227006
print(f"Delta: {delta:.6f}")  # Delta: 0.586851
print(f"Implied volatility: {sigma:.2%}")  # Implied volatility: 20.00%
```

See [`examples/`](examples/) for more detailed usage.

## Supported products and algorithms

| Product | Models | Pricing algorithms |
| --- | --- | --- |
| European vanilla options | BSM, local volatility, Heston | Analytic, quadrature, CRR, FD, MC |
| American vanilla options | BSM, local volatility, Heston | Bjerksund–Stensland 2002 approximation, quadrature, CRR, FD, least-squares MC |
| Digital options | BSM, local volatility | Analytic, quadrature, CRR, FD, MC |
| Geometric-average Asian options | BSM | Analytic (continuous average), MC |
| Arithmetic-average Asian options | BSM | Turnbull–Wakeman continuous-average approximation, MC |
| Single-barrier options | BSM, local volatility | Analytic, quadrature, CRR, FD, MC |
| Double-barrier options | BSM, local volatility | Analytic series, FD, MC |
| Binary barriers, one-touch and no-touch options | BSM, local volatility | Analytic, FD |
| Double-touch and double-no-touch options | BSM, local volatility | Analytic series, FD |
| Snowballs, binary and ternary snowballs | BSM, local volatility | Quadrature, FD, MC |
| Phoenix notes | BSM, local volatility | Quadrature, FD, MC |
| Accumulators and decumulators | BSM, local volatility | Quadrature, FD, MC |
| Range accruals | BSM | Analytic (sum of range probabilities) |

Abbreviations: CRR = Cox–Ross–Rubinstein binomial tree; FD = finite differences; MC = Monte Carlo simulation.

## Disclaimer

This project is for research and educational purposes only. Its results do not constitute investment advice; validate them independently before production use.

## License

[MIT](LICENSE.txt)
