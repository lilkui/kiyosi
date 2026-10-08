#pragma once

#include <cmath>
#include <new>
#include <random>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/error.hpp>
#include <kiyosi/market/context.hpp>

#include "monte_carlo_cuda.hpp"

// C++23 host adapters stay separate from the C++20 CUDA translation unit.
namespace kiyosi::detail {

inline Result<CudaSimulationStep> simulation_step(const PricingContext& context, Timestamp previous, Date current)
{
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const double dt = actual_365_fixed_year_fraction(previous, current);
    const CudaSimulationStep step{(rate - dividend - 0.5 * sigma * sigma) * dt,
                                  sigma * std::sqrt(dt),
                                  std::exp(-rate * actual_365_fixed_year_fraction(context.valuation_time(), current))};
    if (!std::isfinite(step.drift) || !std::isfinite(step.diffusion) || !std::isfinite(step.discount))
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "Monte Carlo simulation parameters are non-finite"});
    return step;
}

inline std::uint64_t random_seed()
{
    std::random_device source;
    return (static_cast<std::uint64_t>(source()) << 32U) ^
           static_cast<std::uint64_t>(source());
}

inline Result<double> cuda_sum(CudaPricingResult cuda_result)
{
    switch (cuda_result.status) {
    case CudaPricingStatus::success:
        return cuda_result.payoff_sum;
    case CudaPricingStatus::unavailable:
        return std::unexpected(Error{ErrorCategory::backend_unavailable, cuda_result.message});
    case CudaPricingStatus::failure:
        return std::unexpected(Error{ErrorCategory::backend_failure, cuda_result.message});
    case CudaPricingStatus::out_of_memory:
        throw std::bad_alloc{};
    case CudaPricingStatus::invalid_result:
        return std::unexpected(Error{ErrorCategory::invalid_result, cuda_result.message});
    }
    return std::unexpected(Error{ErrorCategory::backend_failure,
                                 "CUDA Monte Carlo returned an unknown status"});
}

} // namespace kiyosi::detail
