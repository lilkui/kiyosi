#pragma once

#include <cstdint>

namespace kiyosi {

/// Determines whether a Snowball maturity coupon moves with its quoted knock-out coupon.
enum class CouponQuoteConvention : std::uint8_t {
    shift_maturity_coupon,    ///< Shift the maturity coupon with the quoted knock-out coupon.
    preserve_maturity_coupon, ///< Keep the maturity coupon fixed while solving.
};

/// Bracketing bounds and convergence controls for the implied-volatility solver.
/// Stops when either the price error or the parameter interval meets its own tolerance.
struct ImpliedVolatilitySettings {
    double lower_bound = 0.0001; ///< Positive lower volatility bound.
    double upper_bound = 4.0;    ///< Upper volatility bound, greater than the lower bound.
    double price_tolerance = 1e-8;     ///< Positive absolute price-error tolerance.
    double parameter_tolerance = 1e-8; ///< Positive absolute volatility-interval tolerance.
    int max_iterations = 100;    ///< Positive maximum bisection iteration count.
};

/// Bracketing bounds and convergence controls for the implied-coupon solver.
/// Stops when either the price error or the parameter interval meets its own tolerance.
struct ImpliedCouponSettings {
    double lower_bound = 0.0; ///< Finite lower coupon bound.
    double upper_bound = 2.0; ///< Finite upper coupon bound, greater than the lower bound.
    double price_tolerance = 1e-8;     ///< Positive absolute price-error tolerance.
    double parameter_tolerance = 1e-8; ///< Positive absolute coupon-interval tolerance.
    int max_iterations = 100; ///< Positive maximum bisection iteration count.
};

} // namespace kiyosi
