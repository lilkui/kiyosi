# kiyosi Python bindings

The `kiyosi` Python package provides a small Python API over Kiyosi's native
C++ pricing library. It currently exposes a closed-form Black–Scholes pricer
for European call and put options, including the standard Greeks.

## Requirements

- Python 3.11 or newer

The package contains a native extension. Installing from source therefore also
requires a C++23 compiler and CMake 3.28 or newer.

## Installation

From a checkout of this repository:

```bash
python -m pip install .
```

## Usage

```python
from datetime import date

import kiyosi

result = kiyosi.black_scholes(
    "call",
    spot=100.0,
    strike=100.0,
    valuation_date=date(2025, 1, 1),
    expiry=date(2026, 1, 1),
    risk_free_rate=0.05,
    dividend_yield=0.02,
    volatility=0.20,
)

print(result["price"])
print(result["delta"])
```

The result contains `price`, `delta`, `gamma`, `theta`, `vega`, and `rho`.
Use `EuropeanOption`, `Market`, and `price` when you prefer to construct the
inputs explicitly.

## Development

The Python API lives in [`kiyosi`](kiyosi), its native extension is in
[`kiyosi/_native.cpp`](_native.cpp), and Python tests are in
[`../tests/python`](../tests/python).

For the C++ library and CMake presets, see the repository
[`README.md`](../README.md).

## License

kiyosi is licensed under the MIT License. See [`../LICENSE.txt`](../LICENSE.txt).
