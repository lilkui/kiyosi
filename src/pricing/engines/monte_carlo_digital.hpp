#pragma once

#include <cmath>

#include "monte_carlo_discount.hpp"

#if defined(__CUDACC__)
#define KIYOSI_DIGITAL_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_DIGITAL_HOST_DEVICE
#endif

namespace kiyosi::detail {

struct DigitalMonteCarloSample {
    double price{};
    double delta{};
    double gamma{};
};

// Validated lognormal inputs shared by the CPU and CUDA terminal estimators.
struct DigitalMonteCarloParameters {
    double standardized_moneyness{};
    double diffusion{};
    double amount{};
    double log_amount{};
    double payoff_drift{};
    double log_spot{};
    double log_width{};
    bool asset{};
    bool call{};

    KIYOSI_DIGITAL_HOST_DEVICE DigitalMonteCarloSample sample(double normal, bool delta, bool gamma) const
    {
        // Standardization preserves the exercise test even when a terminal spot rounds to strike.
        const double distance = standardized_moneyness + normal;
        if (!(call ? distance > 0.0 : distance < 0.0)) return {};
        const double exponent = payoff_drift + (asset ? diffusion * normal : 0.0);
        const MonteCarloDiscount discount{std::exp(exponent), exponent};
        DigitalMonteCarloSample result{discount.apply(amount)};
        // Log scaling avoids overflowing a likelihood weight before multiplication by a tiny payoff.
        const double log_payoff = log_amount + exponent;
        if (delta && normal != 0.0)
            result.delta = std::copysign(std::exp(log_payoff + std::log(std::abs(normal)) - log_spot - log_width), normal);
        if (gamma) {
            const double score = std::fma(-diffusion, normal, normal * normal - 1.0);
            if (score != 0.0)
                result.gamma = std::copysign(std::exp(log_payoff + std::log(std::abs(score)) - 2.0 * log_spot - 2.0 * log_width), score);
        }
        return result;
    }
};

} // namespace kiyosi::detail

#undef KIYOSI_DIGITAL_HOST_DEVICE
