# kiyosi Python bindings

`kiyosi` exposes validated derivative instruments and pricing engines from the C++ core.
The Python layer adapts Python values and exceptions; domain rules remain authoritative in C++.

## Installation

```bash
python -m pip install .
```

## Usage

```python
from datetime import date

from kiyosi.instruments import EuropeanOption, OptionType
from kiyosi.market import BsmParameters, PricingContext
from kiyosi.pricing import AnalyticVanillaEngine

parameters = BsmParameters(risk_free_rate=0.05, dividend_yield=0.02, volatility=0.20)
context = PricingContext(parameters=parameters, asset_price=100.0, valuation_time=date(2025, 1, 1))
option = EuropeanOption(
    type=OptionType.CALL,
    strike=100.0,
    effective=date(2025, 1, 1),
    expiry=date(2026, 1, 1),
)

result = AnalyticVanillaEngine().price(option, context)
print(result.price, result.delta, result["vega"])
```

Use `kiyosi.instruments` for European/American, digital, Asian, barrier, accumulator,
snowball, and Phoenix instruments. Use `kiyosi.market` for model parameters, valuation
contexts, calendars, and schedules. Use `kiyosi.pricing` for explicit engines, numerical
analytics, scenario grids, and implied quantities.

`PricingResult` is a read-only mapping with the stable keys `price`, `delta`, `gamma`,
`speed`, `theta`, `charm`, `color`, `vega`, `vanna`, `zomma`, and `rho`; unavailable
measures are `None`. Domain validation failures raise `KiyosiError` with an
`ErrorCategory`. Calendar-date accessors return `date`. `PricingContext.valuation_time`
returns a timezone-aware UTC `datetime`; `date` inputs become midnight UTC. Naive datetimes,
booleans, strings, and `Decimal` values are rejected.

## Development

Run the Python checks with:

```bash
python -m unittest discover -s tests/python
```

The native extension is implemented in [`kiyosi/_native.cpp`](_native.cpp) and split
binding units beside it. The C++ library and CMake presets are documented in the repository
[`README.md`](../README.md).
