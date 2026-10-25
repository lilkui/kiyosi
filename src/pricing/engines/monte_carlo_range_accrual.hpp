#pragma once

#include <cfloat>
#include <cmath>

#include "monte_carlo_simulation.hpp"

#if defined(__CUDACC__)
#define KIYOSI_RANGE_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_RANGE_HOST_DEVICE
#endif

namespace kiyosi::detail {

struct RangeAccrualMonteCarloParameters {
    double log_spot{}, log_lower{}, log_upper{}, initial_hits{};
    MonteCarloDiscount payment{}; ///< Discounted coupon per observation, including full-contract scaling.
};

KIYOSI_RANGE_HOST_DEVICE inline double range_accrual_probability(
    double log_spot, const SimulationStep& step, const RangeAccrualMonteCarloParameters& parameters)
{
    const double mean = log_spot + step.drift;
    if (step.diffusion == 0.0)
        return parameters.log_lower <= mean && mean <= parameters.log_upper ? 1.0 : 0.0;
    constexpr double inverse_sqrt_two = 0.70710678118654752440;
    const double lower = (parameters.log_lower - mean) / step.diffusion * inverse_sqrt_two;
    const double upper = (parameters.log_upper - mean) / step.diffusion * inverse_sqrt_two;
    // Subtract survival probabilities in the upper tail to retain small range probabilities.
    return lower > 0.0 ? 0.5 * (std::erfc(lower) - std::erfc(upper))
                       : 0.5 * (std::erfc(-upper) - std::erfc(-lower));
}

KIYOSI_RANGE_HOST_DEVICE inline double range_accrual_log_tail(double value)
{
    if (value < 10.0) return std::log(0.5 * std::erfc(value * 0.70710678118654752440));
    const double inverse_square = (1.0 / value) / value;
    double term = 1.0;
    double sum = 1.0;
    for (int index = 1; index <= 20; ++index) {
        term *= -(2.0 * index - 1.0) * inverse_square;
        sum += term;
    }
    return -0.5 * value * value - std::log(value) - 0.91893853320467274178 + std::log(sum);
}

KIYOSI_RANGE_HOST_DEVICE inline double range_accrual_value(
    double log_spot, const SimulationStep& step, const RangeAccrualMonteCarloParameters& parameters)
{
    const double probability = range_accrual_probability(log_spot, step, parameters);
    if (probability >= DBL_MIN || step.diffusion == 0.0) return parameters.payment.apply(probability);
    // Combine an underflowing tail with the coupon before exponentiating either factor.
    const double lower = (parameters.log_lower - log_spot - step.drift) / step.diffusion;
    const double upper = (parameters.log_upper - log_spot - step.drift) / step.diffusion;
    const double near = range_accrual_log_tail(lower > 0.0 ? lower : -upper);
    const double far = range_accrual_log_tail(lower > 0.0 ? upper : -lower);
    if (!(near >= -DBL_MAX)) return 0.0;
    return std::exp(parameters.payment.exponent + near + std::log(-std::expm1(far - near)));
}

} // namespace kiyosi::detail

#undef KIYOSI_RANGE_HOST_DEVICE
