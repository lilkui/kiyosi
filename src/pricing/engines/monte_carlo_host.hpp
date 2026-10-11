#pragma once

#include <array>
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
#include "monte_carlo_random.hpp"

// C++23 host adapters stay separate from the C++20 CUDA translation unit.
namespace kiyosi::detail {

#if KIYOSI_HAS_CUDA
template <typename Price>
struct CudaMonteCarloPricer {
    Price evaluate;
    mutable CudaWorkspace workspace{};

    template <typename Option>
    Result<double> price(const Option& option, const PricingContext& context) const
    {
        return evaluate(option, context, &workspace);
    }
};
#endif

template <std::size_t count>
Result<PricingResult> spot_bump_result(const Result<std::array<double, count>>& prices, GreeksRequest greeks, double h)
{
    if (!prices) return std::unexpected(prices.error());
    std::optional<double> speed;
    if constexpr (count == 5)
        speed = ((*prices)[3] - 2.0 * (*prices)[1] + 2.0 * (*prices)[2] - (*prices)[4]) / h / h / (2.0 * h);
    return make_pricing_result((*prices)[0],
                               {{Greek::delta, greeks.has(Greek::delta) ? std::optional{((*prices)[1] - (*prices)[2]) / (2.0 * h)} : std::nullopt},
                                {Greek::gamma, greeks.has(Greek::gamma) ? std::optional{((*prices)[1] - 2.0 * (*prices)[0] + (*prices)[2]) / h / h} : std::nullopt},
                                {Greek::speed, speed}});
}

// Cache the two volatility stencils as scalar prices, retaining the shared Greek arithmetic.
template <typename Engine, typename Sample>
struct MonteCarloMixedPricer {
    const Engine& engine;
    const PricingContext& base;
    double h;
    double volatility_shift;
    bool center;
    Sample sample;
    mutable std::array<std::optional<std::array<double, 3>>, 2> prices{};

    template <typename Option>
    Result<double> price(const Option& option, const PricingContext& context) const
    {
        const auto& model = context.model_parameters();
        const auto& initial = base.model_parameters();
        const double volatility = initial.volatility();
        const int index = model.volatility() == volatility + volatility_shift ? 0 : 1;
        const double spot = base.spot_price();
        const int node = context.spot_price() == spot ? 0 : context.spot_price() == spot + h ? 1
                                                                                             : 2;
        if (h == 0.0 || volatility_shift == 0.0 || model.volatility() != volatility + (index == 0 ? volatility_shift : -volatility_shift) ||
            model.risk_free_rate() != initial.risk_free_rate() || model.dividend_yield() != initial.dividend_yield() ||
            context.valuation_time() != base.valuation_time() || (node == 2 && context.spot_price() != spot - h))
            return engine.price(option, context);
        auto& cached = prices[static_cast<std::size_t>(index)];
        if (!cached) {
            const auto shifted = make_pricing_context(model, spot, context.valuation_time(), context.calendar());
            if (!shifted) return std::unexpected(shifted.error());
            const auto values = sample(*shifted, h, center);
            if (!values) return std::unexpected(values.error());
            cached = *values;
        }
        return (*cached)[static_cast<std::size_t>(node)];
    }
};

template <typename PathPayoff>
Result<double> cpu_path_mean(const TradingDayMonteCarloSettings& settings, PathPayoff path_payoff)
{
    std::mt19937_64 generator(settings.seed ? *settings.seed : std::random_device{}());
    MonteCarloMean mean{};
    for (int path = 0; path < settings.path_count; ++path) {
        // Early termination must not change the random draws of later paths.
        Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
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
    const double exponent = -rate * actual_365_fixed_year_fraction(context.valuation_time(), current);
    const CudaSimulationStep step{(rate - dividend - 0.5 * sigma * sigma) * dt,
                                  sigma * std::sqrt(dt),
                                  {std::exp(exponent), exponent}};
    if (!std::isfinite(step.drift) || !std::isfinite(step.diffusion) || !std::isfinite(exponent))
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
