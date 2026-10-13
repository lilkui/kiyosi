#pragma once

#include <array>
#include <cmath>
#include <new>
#include <random>

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

// Cache the two volatility stencils as scalar prices, retaining the shared Greek arithmetic.
template <typename Engine, typename Sample>
struct MonteCarloMixedPricer {
    using Stencil = SpotStencil<3>;
    const Engine* engine{};
    const PricingContext* base{};
    double h{};
    double volatility_shift{};
    bool center{};
    Sample sample;
    mutable std::array<std::optional<std::array<double, 3>>, 2> prices{};

    template <typename Option>
    Result<double> price(const Option& option, const PricingContext& context) const
    {
        const auto& model = context.model_parameters();
        const auto& initial = base->model_parameters();
        const double volatility = initial.volatility();
        const int index = model.volatility() == volatility + volatility_shift ? 0 : 1;
        const double spot = base->spot_price();
        const auto node = context.spot_price() == spot ? Stencil::center : context.spot_price() == spot + h ? Stencil::up
                                                                                                            : Stencil::down;
        if (h == 0.0 || volatility_shift == 0.0 || model.volatility() != volatility + (index == 0 ? volatility_shift : -volatility_shift) ||
            model.risk_free_rate() != initial.risk_free_rate() || model.dividend_yield() != initial.dividend_yield() ||
            context.valuation_time() != base->valuation_time() || (node == Stencil::down && context.spot_price() != spot - h))
            return engine->price(option, context);
        auto& cached = prices[static_cast<std::size_t>(index)];
        if (!cached) {
            const auto shifted = make_pricing_context(model, spot, context.valuation_time(), context.calendar());
            if (!shifted) return std::unexpected(shifted.error());
            const auto values = sample(*shifted, h, center);
            if (!values) return std::unexpected(values.error());
            cached = *values;
        }
        return (*cached)[node];
    }
};

template <typename Engine, typename Option, typename Validate, typename SpotPrices, typename RatePrices>
Result<PricingResult> complete_monte_carlo_greeks(
    const Engine& scalar, const Option& option, const PricingContext& context,
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
        const auto value = scalar.price(option, context);
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
    const bool mixed = (greeks.has(Greek::vanna) || greeks.has(Greek::zomma)) &&
                       stencils.spot_available && stencils.volatility_available;
    const auto sample = [&](const PricingContext& shifted, double shift, bool center) -> Result<std::array<double, 3>> {
        const auto valid = validate(shifted);
        if (!valid) return std::unexpected(valid.error());
        if (center) return spot_prices.template operator()<3>(shifted, shift);
        const auto values = spot_prices.template operator()<2>(shifted, shift);
        if (!values) return std::unexpected(values.error());
        return std::array{0.0, (*values)[SpotStencil<2>::up], (*values)[SpotStencil<2>::down]};
    };
    const MonteCarloMixedPricer prices{&scalar, &context, mixed ? h : 0.0,
                                       stencils.volatility_shift,
                                       greeks.has(Greek::vega) || greeks.has(Greek::zomma), sample};
    return complete_greeks(prices, option, context, greeks, settings, *native);
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

} // namespace kiyosi::detail
