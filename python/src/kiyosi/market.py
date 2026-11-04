"""Market snapshots, calendars, and observation schedules.

``RateTermStructure.is_explicitly_flat`` identifies the flat-rate representation;
node-based curves return False even when their forward rates are constant.
Rates use annual decimal units and continuous compounding (0.05 means 5%).
``RateTermStructure.factors`` allows underflow to zero and raises
``ComputationFailureError`` on overflow, matching ``factor()``. Underflowed factors
cannot reconstruct a curve; pickle preserves the original factors and shift.
Model rate properties return scalars only for undated flat curves. Dated flat
and node-based curves remain owned RateTermStructure values, preserving coverage
when passed back to constructors or ``replace()``.
Use ``model.risk_free_curve.flat_rate`` or ``model.dividend_curve.flat_rate``
for scalar arithmetic: either dated or undated flat curves return a float,
while node-based curves return None. This matches Rust ``RateTermStructure::flat_rate``.
Volatility is annualized and expressed in decimal units. Rate curves
use the Actual/365 Fixed day-count convention; a calendar's trading-day basis
applies only to trading-year fractions. ``BusinessDayConvention`` accepts ``"following"``
or ``"preceding"``, including the supplied date when it is open.
Custom calendars compare by base, normalized overrides and annualization basis;
empty overrides compare equal to the base when the basis is unchanged.
Observation schedules accept integer indices, including negative positions,
and reject boolean indices with TypeError. Out-of-range indices raise IndexError.
LocalVolatilitySurface owns a rectangular table (rows=time, columns=absolute
spot), with bilinear interpolation of local volatility, constant extrapolation
in the spot dimension, and no time extrapolation. Context changes keep these
absolute nodes fixed. The one-dimensional finite-difference engines support
LocalVolatilityParameters for price, delta and gamma; digital
finite-difference pricing requires explicitly flat risk-free rates and dividend
yields. Other requested Greeks remain None. Other engine families and
implied-parameter solvers reject this model with UnsupportedOperationError.
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
