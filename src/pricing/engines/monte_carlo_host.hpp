#pragma once

#include <cmath>
#include <new>
#include <random>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/error.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

#include "monte_carlo_cuda.hpp"
#include "monte_carlo_mean.hpp"

// C++23 host adapters stay separate from the C++20 CUDA translation unit.
namespace kiyosi::detail {

template <typename PathPayoff>
Result<double> cpu_path_mean(const TradingDayMonteCarloSettings& settings, PathPayoff path_payoff)
{
    std::mt19937_64 generator(settings.seed ? *settings.seed : std::random_device{}());
    MonteCarloMean mean{};
    for (int path = 0; path < settings.path_count; ++path) {
        // Early termination must not change the random draws of later paths.
        std::mt19937_64 path_generator{generator()};
        const auto payoff = path_payoff(path_generator);
        if (!payoff) return std::unexpected(payoff.error());
        mean.add(*payoff);
    }
    return checked_price(mean.value());
}

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

inline Result<double> cuda_mean(CudaPricingResult cuda_result)
{
    switch (cuda_result.status) {
    case CudaPricingStatus::success:
        return cuda_result.payoff_mean;
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
