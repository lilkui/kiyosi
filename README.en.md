# Kiyosi

[简体中文](README.md) | **English**

Kiyosi is a Rust derivatives pricing library with a Python API. It supports pricing vanilla and exotic options, computing Greeks and solving implied parameters under the Black–Scholes–Merton model.

> [!WARNING]
> The project is in Alpha; API backward compatibility is not guaranteed.

## Products and pricing algorithms

| Product | Pricing algorithms |
| --- | --- |
| European vanilla options | BSM closed form, CRR, FD, MC |
| American vanilla options | Bjerksund–Stensland 2002 approximation, CRR, FD, least-squares MC |
| Cash-or-nothing and asset-or-nothing digitals | Closed form, CRR, FD, MC |
| Geometric-average Asian options | Continuous-average closed form, MC |
| Arithmetic-average Asian options | Turnbull–Wakeman continuous-average approximation, MC |
| Single-barrier options | Analytic, CRR, quadrature, FD, MC |
| Double-barrier options | Analytic series, FD, MC |
| Binary barriers, one-touch and no-touch options | Analytic, FD |
| Double-touch and double-no-touch options | Analytic series, FD |
| Snowballs, binary and ternary snowballs | Quadrature, FD, MC |
| Phoenix notes | Quadrature, FD, MC |
| Accumulators and decumulators | Quadrature, FD, MC |
| Range accruals | Sum of analytic BSM range probabilities |

## Installation

Requires **Python 3.11+**:

```bash
python -m pip install kiyosi
```

Wheels require no Rust; source builds require Rust/Cargo and native build tools.

## Quick start

Price a standard snowball with the finite-difference (FD) engine:

```python
from datetime import date

from kiyosi.instruments import standard_snowball
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    monthly_schedule,
    weekdays_calendar,
)
from kiyosi.pricing import FiniteDifferenceAutocallableEngine

start, end = date(2025, 1, 2), date(2026, 1, 2)
calendar = weekdays_calendar()
observations = monthly_schedule(
    start=start,
    end=end,
    first_observation_month=1,
    calendar=calendar,
)
snowball = standard_snowball(
    coupon_rate=0.12,
    initial_spot=100.0,
    knock_in_level=80.0,
    knock_out_level=103.0,
    observation_dates=observations,
    effective_date=start,
    expiry_date=end,
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.02,
        dividend_yield=0.00,
        volatility=0.20,
    ),
    spot_price=100.0,
    valuation_time=start,
    calendar=calendar,
)
engine = FiniteDifferenceAutocallableEngine(
    asset_step_count=800,
    time_step_count=2000,
    scheme="crank_nicolson",
)
price = engine.price(snowball, context)
notional = 1_000_000
print(f"NPV / notional: {price:.6f}")
print(f"NPV: {price * notional:,.2f}")
```

## License

[MIT](LICENSE.txt)
