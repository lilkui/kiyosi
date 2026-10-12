#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <concepts>
#include <optional>
#include <random>
#include <utility>
#include <vector>

#include "../../detail/accumulator_state.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_host.hpp"

namespace kiyosi {
using namespace detail;

namespace {

using SimulationStep = detail::CudaSimulationStep;

Result<std::vector<SimulationStep>> prepare_simulation(const Accumulator& option,
                                                       const PricingContext& context)
{
    const Timestamp valuation = context.valuation_time();
    const auto dates = trading_dates(context.calendar(), valuation, option.expiry_date());
    std::vector<SimulationStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    for (const Date current : dates) {
        const auto step = simulation_step(context, previous, current);
        if (!step) return std::unexpected(step.error());
        steps.push_back(*step);
        previous = current;
    }
    return steps;
}

struct PathState {
    double spot;
    double quantity;
    MonteCarloDiscount discount;
    std::optional<double> payoff{};
};

template <std::size_t count, typename Steps>
Result<std::array<double, count>> path_payoffs(
    const Accumulator& option, const Steps& steps,
    std::array<PathState, count> states, Pcg32& generator)
{
    constexpr bool shared_steps = std::same_as<Steps, std::vector<SimulationStep>>;
    const auto& common_steps = [&]() -> const auto& {
        if constexpr (shared_steps) return steps;
        else return *std::ranges::max_element(steps, {}, [](const auto& sequence) { return sequence.size(); });
    }();
    std::normal_distribution<double> normal;
    for (std::size_t index = 0; index < common_steps.size(); ++index) {
        const double draw = normal(generator);
        const double growth = [&] {
            if constexpr (shared_steps) return std::exp(common_steps[index].drift + common_steps[index].diffusion * draw);
            else return 0.0;
        }();
        for (std::size_t i = 0; i < count; ++i) {
            auto& state = states[i];
            if (state.payoff) continue;
            const auto& step = [&]() -> const auto& {
                if constexpr (shared_steps) return common_steps[index];
                else return steps[i][index];
            }();
            if constexpr (shared_steps) state.spot *= growth;
            else state.spot *= std::exp(step.drift + step.diffusion * draw);
            if (!std::isfinite(state.spot) || state.spot <= 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "Monte Carlo simulation produced an invalid asset price"});
            state.discount = step.discount;
            if (state.spot >= option.knock_out_level())
                state.payoff = state.discount.apply(state.quantity, state.spot - option.strike());
            else
                state.quantity += state.spot < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                                               : option.daily_quantity();
        }
        if (std::ranges::all_of(states, [](const auto& state) { return state.payoff.has_value(); })) break;
    }
    std::array<double, count> payoffs{};
    for (std::size_t i = 0; i < count; ++i)
        payoffs[i] = states[i].payoff ? *states[i].payoff : states[i].discount.apply(states[i].quantity, states[i].spot - option.strike());
    return payoffs;
}

Result<void> validate_pricing(const Accumulator& option, const PricingContext& context,
                              const TradingDayMonteCarloSettings& settings)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    const auto settings_valid = validate_monte_carlo_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    return validate_trading_expiry(context.calendar(), option.expiry_date());
}

template <std::size_t count>
Result<std::array<double, count>> price_spot_bumps(
    const Accumulator& option, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h, [[maybe_unused]] CudaWorkspace* workspace = nullptr)
{
    constexpr std::array offsets = count == 2 ? std::array{1.0, -1.0, 0.0, 0.0, 0.0} : std::array{0.0, 1.0, -1.0, 2.0, -2.0};
    std::array<PathState, count> initial{};
    for (std::size_t i = 0; i < count; ++i) {
        const double spot = context.spot_price() + offsets[i] * h;
        const auto shifted = shifted_context(context, spot, context.model_parameters().volatility(),
                                             context.model_parameters().risk_free_rate(), context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        const auto state = accumulator_initial_state(option, *shifted);
        if (state.settlement) {
            const auto value = checked_price(*state.settlement);
            if (!value) return std::unexpected(value.error());
        }
        initial[i] = {spot, state.quantity, {}, state.settlement};
    }
    std::array<double, count> prices{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto steps = prepare_simulation(option, context);
        if (!steps) return std::unexpected(steps.error());
        if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
            std::array<CudaAccumulatorRequest, count> requests{};
            std::array<std::size_t, count> indices{};
            std::size_t active = 0;
            for (std::size_t i = 0; i < count; ++i) {
                if (initial[i].payoff) continue;
                indices[active] = i;
                requests[active++] = {settings.path_count, *settings.seed, initial[i].spot,
                                      option.strike(), option.knock_out_level(), option.daily_quantity(),
                                      option.acceleration_factor(), initial[i].quantity};
            }
            const auto result = cuda_accumulator_spot_prices(std::span{requests}.first(active), *steps, workspace);
            const auto valid = cuda_mean(result.result);
            if (!valid) return std::unexpected(valid.error());
            for (std::size_t i = 0; i < active; ++i)
                prices[indices[i]] = result.payoff_means[i];
#else
            return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
        } else {
            const auto values = cpu_path_means<count>(settings, [&](auto& generator) {
                return path_payoffs(option, *steps, initial, generator);
            });
            if (!values) return std::unexpected(values.error());
            prices = *values;
        }
    }
    for (std::size_t i = 0; i < count; ++i) {
        const auto value = checked_price(initial[i].payoff ? *initial[i].payoff : prices[i]);
        if (!value) return std::unexpected(value.error());
        prices[i] = *value;
    }
    return prices;
}

Result<std::array<double, 3>> price_rate_bumps(
    const Accumulator& option, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings, double h, std::optional<double> base_price = std::nullopt)
{
    std::array<std::vector<SimulationStep>, 3> steps;
    std::array<PathState, 3> initial{};
    initial[0].payoff = base_price;
    for (std::size_t i = base_price ? 1 : 0; i < initial.size(); ++i) {
        const double rate = context.model_parameters().risk_free_rate() + std::array{0.0, h, -h}[i];
        const auto shifted = shifted_context(context, context.spot_price(), context.model_parameters().volatility(),
                                             rate, context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        const auto state = accumulator_initial_state(option, *shifted);
        if (state.settlement) {
            const auto value = checked_price(*state.settlement);
            if (!value) return std::unexpected(value.error());
        } else {
            auto prepared = prepare_simulation(option, *shifted);
            if (!prepared) return std::unexpected(prepared.error());
            steps[i] = std::move(*prepared);
        }
        initial[i] = {shifted->spot_price(), state.quantity, {}, state.settlement};
    }
    std::array<double, 3> prices{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto values = cpu_path_means<3>(settings, [&](auto& generator) {
            return path_payoffs(option, steps, initial, generator);
        });
        if (!values) return std::unexpected(values.error());
        prices = *values;
    }
    for (std::size_t i = 0; i < prices.size(); ++i) {
        const auto value = checked_price(initial[i].payoff ? *initial[i].payoff : prices[i]);
        if (!value) return std::unexpected(value.error());
        prices[i] = *value;
    }
    return prices;
}

Result<double> price_accumulator(
    const Accumulator& option, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings, [[maybe_unused]] CudaWorkspace* workspace = nullptr)
{
    const auto valid = validate_pricing(option, context, settings);
    if (!valid) return std::unexpected(valid.error());

    const auto initial = accumulator_initial_state(option, context);
    if (initial.settlement) return checked_price(*initial.settlement);

    const auto steps = prepare_simulation(option, context);
    if (!steps) return std::unexpected(steps.error());
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto mean = cuda_mean(detail::cuda_accumulator_price(
            {settings.path_count, settings.seed ? *settings.seed : random_seed(),
             context.spot_price(), option.strike(), option.knock_out_level(),
             option.daily_quantity(), option.acceleration_factor(), initial.quantity},
            *steps, workspace));
        if (!mean) return std::unexpected(mean.error());
        return checked_price(*mean);
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }
    return cpu_path_mean(settings, [&](auto& generator) -> Result<double> {
        const auto payoffs = path_payoffs(option, *steps, std::array{PathState{context.spot_price(), initial.quantity, {}}}, generator);
        if (!payoffs) return std::unexpected(payoffs.error());
        return (*payoffs)[0];
    });
}

} // namespace

Result<double> MonteCarloAccumulatorEngine::price(const Accumulator& option, const PricingContext& context) const
{
    return price_accumulator(option, context, settings_);
}

template <>
Result<double> implied_volatility<MonteCarloAccumulatorEngine, Accumulator>(
    const MonteCarloAccumulatorEngine& engine, const Accumulator& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    CudaWorkspace* workspace = nullptr;
#if KIYOSI_HAS_CUDA
    CudaWorkspace storage;
    workspace = &storage;
#endif
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& seeded, const auto& instrument, const auto& shifted) {
                                                return price_accumulator(instrument, shifted, seeded.settings(), workspace);
                                            });
}

Result<PricingResult> MonteCarloAccumulatorEngine::price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                                     GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(
        *this, option, context, greeks, settings,
        [&](const auto& seeded) -> Result<PricingResult> {
            const auto calculate = [&](const auto& scalar, CudaWorkspace* workspace) -> Result<PricingResult> {
                return complete_monte_carlo_greeks(scalar, option, context, greeks, settings, seeded.settings().backend, [&](const PricingContext& shifted) { return validate_pricing(option, shifted, seeded.settings()); }, [&]<std::size_t count>(const PricingContext& shifted, double shift) { return price_spot_bumps<count>(option, shifted, seeded.settings(), shift, workspace); }, [&](double shift, std::optional<double> base_price) { return price_rate_bumps(option, context, seeded.settings(), shift, base_price); });
            };
#if KIYOSI_HAS_CUDA
            if (seeded.settings().backend == MonteCarloBackend::cuda) {
                const CudaMonteCarloPricer prices{[&](const Accumulator& instrument, const PricingContext& shifted, CudaWorkspace* workspace) {
                    return price_accumulator(instrument, shifted, seeded.settings(), workspace);
                }};
                return calculate(prices, &prices.workspace);
            }
#endif
            return calculate(seeded, nullptr);
        });
}

} // namespace kiyosi
