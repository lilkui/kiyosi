# Deliver the first slice in this repository

The C++ redesign is developed in this repository while the C# implementation remains an external parity oracle. The first vertical slice is European call/put valuation with full deterministic Greeks, implied volatility, BSM assumptions, dates, calendars, and language-neutral golden vectors. Built-in instruments remain a closed set initially, custom calendars are supported, invalid values are created through `std::expected` factories, and Monte Carlo/CUDA extension seams wait until those phases are implemented.
