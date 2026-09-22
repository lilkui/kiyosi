#pragma once

namespace kiyosi {

/// Determines whether a Snowball maturity coupon moves with its quoted knock-out coupon.
enum class CouponQuoteConvention {
    shift_maturity_coupon,    ///< Shift the maturity coupon with the quoted knock-out coupon.
    preserve_maturity_coupon, ///< Keep the maturity coupon fixed while solving.
};

/// Bracketing bounds and convergence controls for the implied-volatility solver.
struct ImpliedVolatilitySettings {
    double lower_bound = 0.0001; ///< Positive lower volatility bound.
    double upper_bound = 4.0;    ///< Upper volatility bound, greater than the lower bound.
    double tolerance = 1e-8;     ///< Positive price and interval convergence tolerance.
    int max_iterations = 100;    ///< Positive maximum bisection iteration count.
};

/// Bracketing bounds and convergence controls for the implied-coupon solver.
struct ImpliedCouponSettings {
    double lower_bound = 0.0; ///< Non-negative lower coupon bound.
    double upper_bound = 2.0; ///< Upper coupon bound, greater than the lower bound.
    double tolerance = 1e-8;  ///< Positive price and interval convergence tolerance.
    int max_iterations = 100; ///< Positive maximum bisection iteration count.
};

} // namespace kiyosi
