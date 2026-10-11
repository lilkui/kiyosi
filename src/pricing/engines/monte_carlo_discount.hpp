#pragma once

#include <cfloat>
#include <cmath>

#if defined(__CUDACC__)
#define KIYOSI_DISCOUNT_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_DISCOUNT_HOST_DEVICE
#endif

namespace kiyosi::detail {

struct MonteCarloDiscount {
    double factor = 1.0;
    double exponent = 0.0;

    KIYOSI_DISCOUNT_HOST_DEVICE double apply(double amount) const
    {
        if (factor >= DBL_MIN && factor <= DBL_MAX)
            return amount * factor;
        if (amount == 0.0) return amount;
        // Scale the cashflow before exponentiating a subnormal, zero, or overflowing discount.
        return std::copysign(std::exp(std::log(std::abs(amount)) + exponent), amount);
    }
};

} // namespace kiyosi::detail

#undef KIYOSI_DISCOUNT_HOST_DEVICE
