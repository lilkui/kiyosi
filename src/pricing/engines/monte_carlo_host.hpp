#pragma once

#include <array>
#include <cmath>
#include <new>
#include <random>
#include <utility>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/error.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

#include "monte_carlo_cuda.hpp"
#include "monte_carlo_mean.hpp"
#include "monte_carlo_random.hpp"
#include "monte_carlo_stencil.hpp"

// C++23 host adapters stay separate from the C++20 CUDA translation unit.
namespace kiyosi::detail {

template <std::size_t count>
Result<PricingResult> spot_bump_result(const Result<std::array<double, count>>& prices, GreeksRequest greeks, double h)
{
    using Stencil = SpotStencil<count>;
    if (!prices) return std::unexpected(prices.error());
    std::optional<double> speed;
    if constexpr (count == 5)
        speed = ((*prices)[Stencil::up_twice] - 2.0 * (*prices)[Stencil::up] + 2.0 * (*prices)[Stencil::down] - (*prices)[Stencil::down_twice]) / h / h / (2.0 * h);
    return make_pricing_result((*prices)[Stencil::center],
                               {{Greek::delta, greeks.has(Greek::delta) ? std::optional{((*prices)[Stencil::up] - (*prices)[Stencil::down]) / (2.0 * h)} : std::nullopt},
                                {Greek::gamma, greeks.has(Greek::gamma) ? std::optional{((*prices)[Stencil::up] - 2.0 * (*prices)[Stencil::center] + (*prices)[Stencil::down]) / h / h} : std::nullopt},
                                {Greek::speed, speed}});
}

template <typename Price, typename Option, typename Validate, typename SpotPrices, typename RatePrices>
Result<PricingResult> complete_monte_carlo_greeks(
    const Price& scalar, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings, MonteCarloBackend backend,
    const Validate& validate, const SpotPrices& spot_prices, const RatePrices& price_rates)
{
    const auto valid = validate(context);
    if (!valid) return std::unexpected(valid.error());
    const auto stencils = numerical_stencils(option, context, settings);
    const double h = stencils.spot_shift;
    const double rate_shift = stencils.rate_shift;
    std::optional<std::array<double, 3>> rate_prices;
    const bool batch_rates = backend == MonteCarloBackend::cpu && greeks.has(Greek::rho) &&
                             stencils.rate_available && !greeks_unavailable(option, context);
    auto native = [&]() -> Result<PricingResult> {
        if ((greeks.has(Greek::delta) || greeks.has(Greek::gamma) || (greeks.has(Greek::speed) && stencils.speed_available)) &&
            stencils.spot_available && !greeks_unavailable(option, context)) {
            if (greeks.has(Greek::speed) && stencils.speed_available)
                return spot_bump_result(spot_prices.template operator()<5>(context, h), greeks, h);
            return spot_bump_result(spot_prices.template operator()<3>(context, h), greeks, h);
        }
        if (batch_rates) {
            const auto values = price_rates(rate_shift, std::nullopt);
            if (!values) return std::unexpected(values.error());
            rate_prices = *values;
            return make_pricing_result((*values)[0]);
        }
        const auto value = scalar(context);
        if (!value) return std::unexpected(value.error());
        return make_pricing_result(*value);
    }();
    if (!native) return std::unexpected(native.error());
    if (batch_rates && !rate_prices) {
        const auto values = price_rates(rate_shift, native->price());
        if (!values) return std::unexpected(values.error());
        rate_prices = *values;
    }
    if (rate_prices) {
        native = make_pricing_result(native->price(), {{Greek::delta, *native->get(Greek::delta)},
                                                       {Greek::gamma, *native->get(Greek::gamma)},
                                                       {Greek::speed, *native->get(Greek::speed)},
                                                       {Greek::rho, ((*rate_prices)[1] - (*rate_prices)[2]) / (200.0 * rate_shift)}});
        if (!native) return std::unexpected(native.error());
    }
    const auto sample = [&](const PricingContext& shifted, double shift, bool center) -> Result<NumericalSpotPrices> {
        const auto valid = validate(shifted);
        if (!valid) return std::unexpected(valid.error());
        if (center) {
            const auto values = spot_prices.template operator()<3>(shifted, shift);
            if (!values) return std::unexpected(values.error());
            return NumericalSpotPrices{(*values)[SpotStencil<3>::center], (*values)[SpotStencil<3>::up], (*values)[SpotStencil<3>::down]};
        }
        const auto values = spot_prices.template operator()<2>(shifted, shift);
        if (!values) return std::unexpected(values.error());
        return NumericalSpotPrices{std::nullopt, (*values)[SpotStencil<2>::up], (*values)[SpotStencil<2>::down]};
    };
    return complete_greeks(scalar, option, context, greeks, settings, *native, nullptr, sample);
}

template <std::size_t count, typename PathPayoffs>
Result<std::array<double, count>> cpu_path_means(const TradingDayMonteCarloSettings& settings, PathPayoffs path_payoffs)
{
    std::mt19937_64 generator(settings.seed ? *settings.seed : std::random_device{}());
    std::array<MonteCarloMean, count> means{};
    for (int path = 0; path < settings.path_count; ++path) {
        // Early termination must not change the random draws of later paths.
        Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
        const auto payoffs = path_payoffs(path_generator);
        if (!payoffs) return std::unexpected(payoffs.error());
        for (std::size_t i = 0; i < count; ++i)
            means[i].add((*payoffs)[i]);
    }
    std::array<double, count> prices{};
    for (std::size_t i = 0; i < count; ++i)
        prices[i] = means[i].value();
    return prices;
}

template <typename State, std::size_t count>
Result<std::array<double, count>> checked_path_prices(
    const std::array<State, count>& states, std::array<double, count> prices)
{
    for (std::size_t i = 0; i < count; ++i) {
        const auto value = checked_price(states[i].payoff ? *states[i].payoff : prices[i]);
        if (!value) return std::unexpected(value.error());
        prices[i] = *value;
    }
    return prices;
}

template <typename Inputs, typename State, typename Prepare, typename PathPayoffs>
Result<std::array<double, 3>> cpu_rate_prices(
    const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h, std::optional<double> base_price, const Prepare& prepare, const PathPayoffs& path_payoffs)
{
    std::array<Inputs, 3> inputs{};
    std::array<State, 3> initial{};
    initial[0].payoff = base_price;
    for (std::size_t i = base_price ? 1 : 0; i < initial.size(); ++i) {
        const double rate = context.model_parameters().risk_free_rate() + std::array{0.0, h, -h}[i];
        const auto shifted = shifted_context(context, context.spot_price(), context.model_parameters().volatility(), rate, context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        auto prepared = prepare(*shifted);
        if (!prepared) return std::unexpected(prepared.error());
        inputs[i] = std::move(prepared->first);
        initial[i] = std::move(prepared->second);
    }
    std::array<double, 3> prices{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto values = cpu_path_means<3>(settings, [&](auto& generator) {
            return path_payoffs(inputs, initial, generator);
        });
        if (!values) return std::unexpected(values.error());
        prices = *values;
    }
    return checked_path_prices(initial, prices);
}

// One draw advances every active state; scalar calls enter active, batched states may already be settled.
// Keep product observation inlined in the time-step loop.
template <bool shared_steps, typename Steps, typename State, std::size_t count, typename Observe>
#if defined(_MSC_VER)
__forceinline
#else
[[gnu::always_inline]] inline
#endif
Result<void> simulate_path_batch(const Steps& steps, std::array<State, count>& states,
                                 Pcg32& generator, const Observe& observe)
{
    const auto& common = [&]() -> const auto& {
        if constexpr (shared_steps) return steps;
        else return *std::ranges::max_element(steps, {}, [](const auto& sequence) { return sequence.size(); });
    }();
    std::normal_distribution<double> normal;
    std::size_t index = 0;
    for (const auto& common_step : common) {
        const double draw = normal(generator);
        const double shared_growth = [&] {
            if constexpr (shared_steps) {
                const auto& simulation = [&]() -> const auto& {
                    if constexpr (requires { common_step.simulation; }) return common_step.simulation;
                    else return common_step;
                }();
                return std::exp(simulation.drift + simulation.diffusion * draw);
            }
            else return 0.0;
        }();
        for (std::size_t i = 0; i < count; ++i) {
            auto& state = states[i];
            if constexpr (count > 1)
                if (state.payoff) continue;
            const auto& step = [&]() -> const auto& {
                if constexpr (shared_steps) return common_step;
                else return steps[i][index];
            }();
            if constexpr (shared_steps) state.spot *= shared_growth;
            else {
                const auto& simulation = [&]() -> const auto& {
                    if constexpr (requires { step.simulation; }) return step.simulation;
                    else return step;
                }();
                state.spot *= std::exp(simulation.drift + simulation.diffusion * draw);
            }
            if (!std::isfinite(state.spot) || state.spot <= 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "Monte Carlo simulation produced an invalid asset price"});
            observe(state, step);
            if constexpr (count == 1)
                if (state.payoff) return {};
        }
        if constexpr (count > 1)
            if (std::ranges::all_of(states, [](const auto& state) { return state.payoff.has_value(); })) break;
        ++index;
    }
    return {};
}

template <typename PathPayoff>
Result<double> cpu_path_mean(const TradingDayMonteCarloSettings& settings, PathPayoff path_payoff)
{
    const auto prices = cpu_path_means<1>(settings, [&](auto& generator) -> Result<std::array<double, 1>> {
        const auto payoff = path_payoff(generator);
        if (!payoff) return std::unexpected(payoff.error());
        return std::array{*payoff};
    });
    if (!prices) return std::unexpected(prices.error());
    return checked_price((*prices)[0]);
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

// Pack only active states; the caller merges already settled prices afterward.
template <typename Request, typename State, std::size_t count, typename MakeRequest, typename Price>
Result<std::array<double, count>> cuda_active_prices(
    const std::array<State, count>& states, const MakeRequest& make_request, const Price& price)
{
    std::array<Request, count> requests{};
    std::array<std::size_t, count> indices{};
    std::size_t active = 0;
    for (std::size_t i = 0; i < count; ++i) {
        if (states[i].payoff) continue;
        indices[active] = i;
        requests[active++] = make_request(states[i]);
    }
    const auto result = price(std::span{requests}.first(active));
    const auto valid = cuda_mean(result.result);
    if (!valid) return std::unexpected(valid.error());
    std::array<double, count> prices{};
    for (std::size_t i = 0; i < active; ++i)
        prices[indices[i]] = result.payoff_means[i];
    return prices;
}

} // namespace kiyosi::detail
