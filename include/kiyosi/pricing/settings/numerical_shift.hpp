#pragma once

namespace kiyosi {

/// Absolute bump sizes for finite-difference Greeks computed by revaluing any engine.
/// Spot uses asset-price units, volatility and rates use decimal units, and time uses calendar
/// days. Reported vega, vanna, zomma, and rho are still scaled per percentage point.
/// Calculations use the representable symmetric spacing after rounding each bump.
/// Collapsed or asymmetric stencils leave the affected Greeks unavailable.
struct NumericalShiftSettings {
    double spot_shift = 1e-2;       ///< Positive absolute spot bump.
    double volatility_shift = 1e-4; ///< Positive absolute volatility bump.
    double rate_shift = 1e-4;       ///< Positive absolute interest-rate bump.
    int time_shift_days = 1;        ///< Positive calendar-day bump.
};

} // namespace kiyosi
