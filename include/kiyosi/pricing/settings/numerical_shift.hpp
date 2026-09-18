#pragma once

namespace kiyosi {

/// Absolute bump sizes for finite-difference risk measures computed by revaluing any engine.
/// Spot uses asset-price units, volatility and rates use decimal units, and time uses calendar
/// days. Reported vega, vanna, zomma, and rho are still scaled per percentage point.
struct NumericalShiftSettings {
    double spot_shift = 1e-2;
    double volatility_shift = 1e-4;
    double rate_shift = 1e-4;
    int time_shift_days = 1;
};

} // namespace kiyosi
