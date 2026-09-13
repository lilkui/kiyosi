#pragma once

namespace kiyosi {

/// Bump sizes for the finite-difference risk measures computed by revaluing any engine.
struct NumericalShiftSettings {
    double spot_shift = 1e-2;
    double volatility_shift = 1e-4;
    double rate_shift = 1e-4;
    int time_shift_days = 1;
};

} // namespace kiyosi
