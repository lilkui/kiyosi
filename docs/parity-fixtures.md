# Parity fixtures

`tests/fixtures/european_bsm.tsv` is a language-neutral, tab-separated set of
reviewed European option reference results. The values were generated once
from the DerivaSharp C# implementation (the parity oracle), then checked in
so C++ tests do not need the oracle or a network connection.

Each data row contains the instrument (`option_type`, `strike`, `expiry`),
market assumptions (`spot`, rates, dividend yield, volatility, and valuation
date), the observed price used for implied volatility, and an expected value
and absolute tolerance for every output. `theta`, `charm`, and `color` are per
calendar day using Actual/365 Fixed. `vega`, `vanna`, and `zomma` are per
percentage-point volatility move; `rho` is per percentage-point rate move.
Other Greeks use one-unit changes in their named underlying quantity. Prices
and rates follow the continuously compounded Black-Scholes-Merton convention.

The public `date` boundary is midnight-anchored; `timestamp` and
`PricingContext::valuation_time()` preserve intraday valuation moments. Shared
`year_fraction` uses Actual/365 Fixed, while `TradingCalendar::trading_year_fraction`
uses its validated annual trading-day count for calendar-aware calculations.

Lines beginning with `#` and blank lines are ignored. The test-only parser
rejects missing columns, invalid dates/numbers, unknown option types, and
negative tolerances with a row number and field name. Fixture comparison is
absolute and inclusive at the stated tolerance.

`tests/fixtures/cpu_parity.tsv` is the broader CPU parity manifest. Its
language-neutral rows name the instrument, engine, and variant, then carry
semicolon-separated `key=value` inputs, outputs, and per-output tolerances.
Validation expectations use `category|message`; convergence metadata uses
`parameter|resolution1,resolution2|reference|tolerance`; seeded Monte Carlo
metadata uses `seed|paths|steps|tolerance`. A dash means that metadata does
not apply. The Catch2 manifest test validates every row and includes the case,
instrument, and engine in assertion context, so failures remain actionable
without a live DerivaSharp build.
