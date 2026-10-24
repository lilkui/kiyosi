#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include "../../detail/numerical_greeks.hpp"
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

template <std::size_t count, typename Steps, typename Generator>
Result<std::array<double, count>> path_payoffs(
    const Accumulator& option, const Steps& steps,
    std::array<PathState, count> states, Generator& generator)
{
    constexpr bool shared_steps = std::same_as<Steps, std::vector<SimulationStep>>;
    const auto simulated = simulate_path_batch<shared_steps>(steps, states, generator,
                                                             [&](auto& state, const auto& step) {
                                                                 state.discount = step.discount;
                                                                 if (observe_accumulator_path(state.spot, option.strike(), option.knock_out_level(), option.daily_quantity(), option.acceleration_factor(), state.quantity))
                                                                     state.payoff = state.discount.apply(state.quantity, state.spot - option.strike());
                                                             });
    if (!simulated) return std::unexpected(simulated.error());
    std::array<double, count> payoffs{};
    for (std::size_t i = 0; i < count; ++i) {
        const auto& state = states[i];
        payoffs[i] = state.payoff ? *state.payoff : state.discount.apply(state.quantity, state.spot - option.strike());
    }
    return payoffs;
}

Result<void> validate_pricing(const Accumulator& option, const PricingContext& context)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    return validate_trading_expiry(context.calendar(), option.expiry_date());
}

template <std::size_t count, bool rates = false>
Result<std::array<double, count + (rates ? 2 : 0)>> price_spot_bumps(
    const Accumulator& option, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double h, [[maybe_unused]] CudaWorkspace* workspace = nullptr, double rate_shift = 0.0)
{
    constexpr auto offsets = SpotStencil<count>::offsets;
    constexpr auto outputs = count + (rates ? 2 : 0);
    std::array<PathState, outputs> initial{};
    for (std::size_t i = 0; i < outputs; ++i) {
        const double spot = context.spot_price() + (i < count ? offsets[i] * h : 0.0);
        const double rate = context.model_parameters().risk_free_rate() + (i < count ? 0.0 : i == count ? rate_shift
                                                                                                        : -rate_shift);
        const auto shifted = shifted_context(context, spot, context.model_parameters().volatility(),
                                             rate, context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        const auto state = accumulator_initial_state(option, *shifted);
        if (state.settlement) {
            const auto value = checked_price(*state.settlement);
            if (!value) return std::unexpected(value.error());
        }
        initial[i] = {spot, state.quantity, {}, state.settlement};
    }
    std::array<double, outputs> prices{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto steps = prepare_simulation(option, context);
        if (!steps) return std::unexpected(steps.error());
        if constexpr (rates) {
            std::array<std::vector<SimulationStep>, 2> rate_steps;
            std::array<std::span<const SimulationStep>, outputs> sequences{};
            std::fill_n(sequences.begin(), count, std::span<const SimulationStep>{*steps});
            for (std::size_t i = 0; i < rate_steps.size(); ++i) {
                if (initial[count + i].payoff) continue;
                const auto shifted = shifted_context(context, context.spot_price(), context.model_parameters().volatility(),
                                                     context.model_parameters().risk_free_rate() + (i == 0 ? rate_shift : -rate_shift), context.valuation_time());
                if (!shifted) return std::unexpected(shifted.error());
                auto prepared = prepare_simulation(option, *shifted);
                if (!prepared) return std::unexpected(prepared.error());
                rate_steps[i] = std::move(*prepared);
                sequences[count + i] = rate_steps[i];
            }
            const auto values = cpu_path_means<outputs>(settings, [&](auto& generator) { return path_payoffs(option, sequences, initial, generator); }, steps->size());
            if (!values) return std::unexpected(values.error());
            prices = *values;
        } else if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
            const auto values = cuda_active_prices<CudaAccumulatorRequest>(initial, [&](const auto& state) { return CudaAccumulatorRequest{settings.path_count, *settings.seed, state.spot,
                                                                                                                                           option.strike(), option.knock_out_level(), option.daily_quantity(), option.acceleration_factor(), state.quantity}; }, [&](auto requests) { return cuda_accumulator_spot_prices(requests, *steps, workspace); });
            if (!values) return std::unexpected(values.error());
            prices = *values;
#else
            return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
        } else {
            const auto values = cpu_path_means<count>(settings, [&](auto& generator) { return path_payoffs(option, *steps, initial, generator); }, steps->size());
            if (!values) return std::unexpected(values.error());
            prices = *values;
        }
    }
    return checked_path_prices(initial, prices);
}

Result<std::array<double, 3>> price_rate_bumps(
    const Accumulator& option, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings, double h, std::optional<double> base_price = std::nullopt)
{
    return cpu_rate_prices<std::vector<SimulationStep>, PathState>(context, settings, h, base_price, [&](const PricingContext& shifted) -> Result<std::pair<std::vector<SimulationStep>, PathState>> {
            const auto state = accumulator_initial_state(option, shifted);
            std::vector<SimulationStep> steps;
            if (state.settlement) {
                const auto value = checked_price(*state.settlement);
                if (!value) return std::unexpected(value.error());
            } else {
                auto prepared = prepare_simulation(option, shifted);
                if (!prepared) return std::unexpected(prepared.error());
                steps = std::move(*prepared);
            }
            return std::pair{std::move(steps), PathState{shifted.spot_price(), state.quantity, {}, state.settlement}}; }, [&](const auto& steps, const auto& initial, auto& generator) { return path_payoffs(option, steps, initial, generator); });
}

template <std::size_t count>
Result<std::array<double, 2 * count>> price_volatility_bumps(
    const Accumulator& option, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    double spot_shift, double volatility_shift)
{
    std::array<std::vector<SimulationStep>, 2> steps;
    std::array<std::span<const SimulationStep>, 2 * count> sequences{};
    std::array<PathState, 2 * count> initial{};
    for (std::size_t market = 0; market < steps.size(); ++market) {
        const double volatility = context.model_parameters().volatility() + (market == 0 ? volatility_shift : -volatility_shift);
        const auto shifted = shifted_context(context, context.spot_price(), volatility, context.model_parameters().risk_free_rate(), context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        const auto valid = validate_pricing(option, *shifted);
        if (!valid) return std::unexpected(valid.error());
        for (std::size_t index = 0; index < count; ++index) {
            const double spot = context.spot_price() + SpotStencil<count>::offsets[index] * spot_shift;
            const auto bumped = shifted_context(*shifted, spot, volatility, context.model_parameters().risk_free_rate(), context.valuation_time());
            if (!bumped) return std::unexpected(bumped.error());
            const auto state = accumulator_initial_state(option, *bumped);
            if (state.settlement) {
                const auto value = checked_price(*state.settlement);
                if (!value) return std::unexpected(value.error());
            }
            initial[market * count + index] = {spot, state.quantity, {}, state.settlement};
        }
        if (std::ranges::any_of(std::span{initial}.subspan(market * count, count), [](const auto& state) { return !state.payoff; })) {
            auto prepared = prepare_simulation(option, *shifted);
            if (!prepared) return std::unexpected(prepared.error());
            steps[market] = std::move(*prepared);
            std::fill_n(sequences.begin() + market * count, count, std::span<const SimulationStep>{steps[market]});
        }
    }
    std::array<double, 2 * count> prices{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto values = cpu_path_means<2 * count>(settings, [&](auto& generator) { return path_payoffs(option, sequences, initial, generator); }, std::max(steps[0].size(), steps[1].size()));
        if (!values) return std::unexpected(values.error());
        prices = *values;
    }
    return checked_path_prices(initial, prices);
}

Result<double> price_accumulator(
    const Accumulator& option, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings, [[maybe_unused]] CudaWorkspace* workspace = nullptr)
{
    const auto valid = validate_pricing(option, context);
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
    const auto prices = cpu_path_means<1>(settings, [&](auto& generator) { return path_payoffs(option, *steps, std::array{PathState{context.spot_price(), initial.quantity, {}}}, generator); }, steps->size());
    if (!prices) return std::unexpected(prices.error());
    return checked_price((*prices)[0]);
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
            CudaWorkspace* workspace = nullptr;
#if KIYOSI_HAS_CUDA
            CudaWorkspace storage;
            if (seeded.settings().backend == MonteCarloBackend::cuda) workspace = &storage;
#endif
            const auto price = [&](const PricingContext& shifted) {
                return price_accumulator(option, shifted, seeded.settings(), workspace);
            };
            return complete_monte_carlo_greeks(price, option, context, greeks, settings, seeded.settings().backend, [&](const PricingContext& shifted) { return validate_pricing(option, shifted); }, [&]<std::size_t count, bool rates = false>(const PricingContext& shifted, double shift, double rate_shift = 0.0) { return price_spot_bumps<count, rates>(option, shifted, seeded.settings(), shift, workspace, rate_shift); }, [&](double shift, std::optional<double> base_price) { return price_rate_bumps(option, context, seeded.settings(), shift, base_price); }, [&]<std::size_t count>(double shift, double volatility_shift) { return price_volatility_bumps<count>(option, context, seeded.settings(), shift, volatility_shift); });
        });
}

} // namespace kiyosi
