#pragma once

#include <cmath>

namespace kiyosi {

/// Absolute bump sizes for finite-difference Greeks computed by revaluing any engine.
/// Spot uses asset-price units, volatility and rates use decimal units, and time uses calendar
/// days. Reported vega, vanna, zomma, and rho are scaled per percentage point.
/// Calculations use the representable symmetric spacing after rounding each bump.
/// Collapsed or asymmetric stencils leave the affected Greeks unavailable.
struct NumericalShiftSettings {
    double spot_shift = 1e-2;       ///< Positive absolute spot bump.
    double volatility_shift = 1e-4; ///< Positive absolute volatility bump.
    double rate_shift = 1e-4;       ///< Positive absolute interest-rate bump.
    int time_shift_days = 1;        ///< Positive calendar-day bump.
};

namespace detail {

// Use the rounded upper bump on both sides; a collapsed or asymmetric stencil is unavailable.
inline double symmetric_shift(double value, double requested)
{
    const double high = value + requested;
    const double shift = high - value;
    const double low = value - shift;
    return std::isfinite(high) && std::isfinite(shift) && shift > 0.0 &&
                   std::isfinite(low) && value - low == shift
               ? shift
               : 0.0;
}

} // namespace detail

} // namespace kiyosi
