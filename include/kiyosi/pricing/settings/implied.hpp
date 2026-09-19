#pragma once

namespace kiyosi {

/// Determines whether a Snowball maturity coupon moves with its quoted knock-out coupon.
enum class CouponQuoteConvention {
    shift_maturity_coupon,
    preserve_maturity_coupon,
};

/// Bracketing bounds and convergence controls for the implied-volatility solver.
struct ImpliedVolatilitySettings {
    double lower_bound = 0.0001;
    double upper_bound = 4.0;
    double tolerance = 1e-8;
    int max_iterations = 100;
};

/// Bracketing bounds and convergence controls for the implied-coupon solver.
struct ImpliedCouponSettings {
    double lower_bound = 0.0;
    double upper_bound = 2.0;
    double tolerance = 1e-8;
    int max_iterations = 100;
};

} // namespace kiyosi
