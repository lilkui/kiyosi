"""Market snapshots, calendars, and observation schedules.

Rates use annual decimal units and continuous compounding (0.05 means 5%).
``RateTermStructure.factors`` allows underflow to zero and raises
``ComputationFailureError`` on overflow, matching ``factor()``. Underflowed factors
cannot reconstruct a curve; pickle preserves the original factors and shift.
Model ``flat_risk_free_rate`` and ``flat_dividend_yield`` properties return scalar rates
for dated or undated constant curves and raise ``UnsupportedOperationError``
for nonconstant curves, matching Rust. Use ``risk_free_curve`` and ``dividend_curve``
for owned curves with stable return types and preserved coverage when passing them
back to the ``risk_free_rate`` and ``dividend_yield`` constructor or
``replace()`` arguments. Scalar queries discard coverage bounds. Either curve's ``flat_rate`` returns the
scalar rate when all forwards are exactly constant, or None otherwise. Constant
node curves retain their reference, nodes and upper coverage bound; flat-rate
engines accept them without discarding those bounds.
Volatility is annualized and expressed in decimal units. BSM volatility and local
surface values must be strictly positive; zero is rejected even at expiry.
Heston variance parameters may be zero. For exact deterministic vanilla pricing,
set ``initial_variance=long_run_variance=vol_of_vol=0`` and use a Heston engine.
This is an intentional model-domain distinction, shared by Rust and Python.
Rate curves
use the Actual/365 Fixed day-count convention; a calendar's trading-day basis
applies only to trading-year fractions. ``BusinessDayConvention`` accepts ``"following"``
or ``"preceding"``, including the supplied date when it is open.
Calendar adjustment raises ``ValueError`` if the result is outside Python's
years 1 through 9999; Rust can represent dates beyond those output limits.
Custom calendars compare by base, normalized overrides and annualization basis;
empty overrides compare equal to the base when the basis is unchanged.
Calendar representations identify the base kind and abbreviate open/closed overrides.
Observation schedules accept integer indices, including negative positions,
and reject boolean indices with TypeError. Out-of-range indices raise IndexError.
Construct ``ObservationSchedule(times=..., start_time=..., end_time=..., calendar=...)``
for explicit or irregular observations. Times must increase strictly within the
inclusive bounds and fall on open calendar dates. Empty schedules are valid;
bounds and calendar are validation inputs, while schedules store only timestamps.
LocalVolatilitySurface owns a rectangular table (rows=time, columns=absolute
spot), with bilinear interpolation of local volatility, constant extrapolation
in the spot dimension, and no time extrapolation. Context changes keep these
absolute nodes fixed. The one-dimensional finite-difference engines support
LocalVolatilityParameters for price, delta and gamma; digital
finite-difference pricing requires constant risk-free rates and dividend
yields. Other requested Greeks remain None. Other engine families and
implied-parameter solvers reject this model with UnsupportedOperationError.
PricingContext.calendar is calendar metadata for callers and custom engines;
built-in pricing does not use it to select or validate observations.
Barrier and touch contracts, range accruals, accumulators and autocallables own
their contractual calendars.
Model and context ``replace()`` methods expose named keyword-only fields.
Omitted fields, or the displayed default ``...`` (``Ellipsis``), retain their
values; explicit ``None`` is rejected.

HestonParameters owns constant risk-neutral variance parameters and deterministic
rate curves. Initial variance is at valuation (20% volatility means 0.04).
Variance parameters are finite and nonnegative; correlation includes [-1, 1].
The Feller condition is not required. AnalyticHestonEuropeanEngine prices European
options; FiniteDifferenceHestonVanillaEngine prices European and American options.
Other engines and implied-parameter solvers reject this model; requested Greeks remain None.

Examples
--------
Price a deterministic call with a zero-variance Heston model:

>>> from datetime import datetime
>>> from kiyosi.instruments import EuropeanOption
>>> from kiyosi.pricing import AnalyticHestonEuropeanEngine
>>> option = EuropeanOption(option_type="call", strike=100,
...     effective_time=datetime(2025, 1, 1), expiry_time=datetime(2026, 1, 1))
>>> deterministic = HestonParameters(risk_free_rate=0.05, dividend_yield=0.02,
...     initial_variance=0, long_run_variance=0, mean_reversion_speed=0,
...     vol_of_vol=0, correlation=0)
>>> context = PricingContext(model_parameters=deterministic, spot_price=100,
...     valuation_time=option.effective_time)
>>> round(AnalyticHestonEuropeanEngine().price(option, context), 6)
2.896925

Validate an irregular schedule with exact observation times:

>>> from datetime import datetime
>>> times = [datetime(2025, 1, 3, 15), datetime(2025, 1, 8, 14, 30)]
>>> schedule = ObservationSchedule(
...     times=times, start_time=datetime(2025, 1, 1),
...     end_time=datetime(2025, 1, 10),
... )
>>> list(schedule) == times
True

Read a dated flat rate without discarding its coverage bound:

>>> from datetime import datetime
>>> flat = RateTermStructure.flat(0.05, reference_time=datetime(2025, 1, 1))
>>> flat.flat_rate
0.05
>>> flat.reference_time
datetime.datetime(2025, 1, 1, 0, 0)

Build monthly observations from a month-end anchor. February is clamped
without moving the March anchor:

>>> from datetime import datetime
>>> schedule = monthly_schedule(
...     start_time=datetime(2025, 1, 31), end_time=datetime(2025, 3, 31),
...     first_observation_month=1, calendar=all_days_calendar(),
... )
>>> list(schedule)
[datetime.datetime(2025, 2, 28, 0, 0), datetime.datetime(2025, 3, 31, 0, 0)]

Combine a discount curve with a scalar dividend yield:

>>> curve = RateTermStructure(
...     reference_time=datetime(2025, 1, 1), times=[datetime(2026, 1, 1)],
...     factors=[0.95],
... )
>>> model = BlackScholesMertonParameters(
...     risk_free_rate=curve, dividend_yield=0.02, volatility=0.2,
... )
>>> context = PricingContext(
...     model_parameters=model, spot_price=100, valuation_time=datetime(2025, 1, 1),
... )
>>> round(context.model_parameters.risk_free_curve.factor(
...     datetime(2025, 1, 1), datetime(2026, 1, 1),
... ), 2)
0.95
"""

from typing import Literal

from ._native import (
    BlackScholesMertonParameters,
    HestonParameters,
    LocalVolatilityParameters,
    LocalVolatilitySurface,
    ObservationSchedule,
    PricingContext,
    RateTermStructure,
    TradingCalendar,
    all_days_calendar,
    custom_calendar,
    fixed_interval_schedule,
    monthly_schedule,
    sse_calendar,
    weekdays_calendar,
)

BusinessDayConvention = Literal["following", "preceding"]

__all__ = [
    "BlackScholesMertonParameters",
    "BusinessDayConvention",
    "HestonParameters",
    "LocalVolatilityParameters",
    "LocalVolatilitySurface",
    "ObservationSchedule",
    "PricingContext",
    "RateTermStructure",
    "TradingCalendar",
    "all_days_calendar",
    "custom_calendar",
    "fixed_interval_schedule",
    "monthly_schedule",
    "sse_calendar",
    "weekdays_calendar",
]
