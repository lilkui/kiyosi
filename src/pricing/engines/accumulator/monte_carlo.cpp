#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <optional>
#include <random>
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

Result<double> path_payoff(const Accumulator& option, const PricingContext& context,
                           const std::vector<SimulationStep>& steps, double quantity,
                           Pcg32& generator)
{
    double value = context.spot_price();

    std::normal_distribution<double> normal;
    double discount = 1.0;
    for (const auto& step : steps) {
        value *= std::exp(step.drift + step.diffusion * normal(generator));
        if (!std::isfinite(value) || value <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "Monte Carlo simulation produced an invalid asset price"});
        discount = step.discount;
        if (value >= option.knock_out_level()) break;
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                            : option.daily_quantity();
    }
    return quantity * (value - option.strike()) * discount;
}

struct PathState {
    double spot;
    double quantity;
    double discount = 1.0;
    std::optional<double> payoff{};
};

template <std::size_t count>
Result<std::array<double, count>> path_payoffs(
    const Accumulator& option, const std::vector<SimulationStep>& steps,
    std::array<PathState, count> states, Pcg32& generator)
{
    std::normal_distribution<double> normal;
    for (const auto& step : steps) {
        const double growth = std::exp(step.drift + step.diffusion * normal(generator));
        for (auto& state : states) {
            if (state.payoff) continue;
            state.spot *= growth;
            if (!std::isfinite(state.spot) || state.spot <= 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "Monte Carlo simulation produced an invalid asset price"});
            state.discount = step.discount;
            if (state.spot >= option.knock_out_level())
                state.payoff = state.quantity * (state.spot - option.strike()) * state.discount;
            else
                state.quantity += state.spot < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                                               : option.daily_quantity();
        }
        if (std::ranges::all_of(states, [](const auto& state) { return state.payoff.has_value(); })) break;
    }
    std::array<double, count> payoffs{};
    for (std::size_t i = 0; i < count; ++i)
        payoffs[i] = states[i].payoff ? *states[i].payoff : states[i].quantity * (states[i].spot - option.strike()) * states[i].discount;
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
Result<PricingResult> price_spot_bumps(
    const Accumulator& option, const PricingContext& context, const TradingDayMonteCarloSettings& settings,
    GreeksRequest greeks, double h)
{
    constexpr std::array offsets{0.0, 1.0, -1.0, 2.0, -2.0};
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
        initial[i] = {spot, state.quantity, 1.0, state.settlement};
    }
    std::array<MonteCarloMean, count> means{};
    if (std::ranges::any_of(initial, [](const auto& state) { return !state.payoff; })) {
        const auto steps = prepare_simulation(option, context);
        if (!steps) return std::unexpected(steps.error());
        std::mt19937_64 generator{*settings.seed};
        for (int path = 0; path < settings.path_count; ++path) {
            // Separate path streams preserve scalar draws when bumps knock out at different dates.
            Pcg32 path_generator{generator(), static_cast<std::uint64_t>(path)};
            const auto payoffs = path_payoffs(option, *steps, initial, path_generator);
            if (!payoffs) return std::unexpected(payoffs.error());
            for (std::size_t i = 0; i < count; ++i)
                if (!initial[i].payoff) means[i].add((*payoffs)[i]);
        }
    }
    std::array<double, count> prices{};
    for (std::size_t i = 0; i < count; ++i) {
        const auto value = checked_price(initial[i].payoff ? *initial[i].payoff : means[i].value());
        if (!value) return std::unexpected(value.error());
        prices[i] = *value;
    }
    std::optional<double> speed;
    if constexpr (count == 5)
        speed = (prices[3] - 2.0 * prices[1] + 2.0 * prices[2] - prices[4]) / h / h / (2.0 * h);
    return make_pricing_result(prices[0],
                               {{Greek::delta, greeks.has(Greek::delta) ? std::optional{(prices[1] - prices[2]) / (2.0 * h)} : std::nullopt},
                                {Greek::gamma, greeks.has(Greek::gamma) ? std::optional{(prices[1] - 2.0 * prices[0] + prices[2]) / h / h} : std::nullopt},
                                {Greek::speed, speed}});
}

} // namespace

Result<double> MonteCarloAccumulatorEngine::price(
    const Accumulator& option, const PricingContext& context) const
{
    const auto valid = validate_pricing(option, context, settings_);
    if (!valid) return std::unexpected(valid.error());

    const auto initial = accumulator_initial_state(option, context);
    if (initial.settlement) return checked_price(*initial.settlement);

    const auto steps = prepare_simulation(option, context);
    if (!steps) return std::unexpected(steps.error());
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto mean = cuda_mean(detail::cuda_accumulator_price(
            {settings_.path_count, settings_.seed ? *settings_.seed : random_seed(),
             context.spot_price(), option.strike(), option.knock_out_level(),
             option.daily_quantity(), option.acceleration_factor(), initial.quantity},
            *steps));
        if (!mean) return std::unexpected(mean.error());
        return checked_price(*mean);
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }
    return cpu_path_mean(settings_, [&](auto& generator) {
        return path_payoff(option, context, *steps, initial.quantity, generator);
    });
}

Result<PricingResult> MonteCarloAccumulatorEngine::price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                                     GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& seeded) -> Result<PricingResult> {
                                         const auto valid = validate_pricing(option, context, seeded.settings());
                                         if (!valid) return std::unexpected(valid.error());
                                         const double spot = context.spot_price();
                                         const double h = symmetric_shift(spot, settings.spot_shift);
                                         const double two_h = 2.0 * h;
                                         const bool speed_available = std::isfinite(two_h) && spot > two_h &&
                                                                      symmetric_shift(spot, two_h) == two_h &&
                                                                      !at_spot_discontinuity(option, context, two_h);
                                         if (seeded.settings().backend == MonteCarloBackend::cpu &&
                                             (greeks.has(Greek::delta) || greeks.has(Greek::gamma) || greeks.has(Greek::speed)) &&
                                             h > 0.0 && spot > h && !greeks_unavailable(option, context) &&
                                             !at_spot_discontinuity(option, context, h) &&
                                             (!greeks.has(Greek::speed) || speed_available)) {
                                             if (greeks.has(Greek::speed))
                                                 return price_spot_bumps<5>(option, context, seeded.settings(), greeks, h);
                                             return price_spot_bumps<3>(option, context, seeded.settings(), greeks, h);
                                         }
                                         const auto value = seeded.price(option, context);
                                         if (!value) return std::unexpected(value.error());
                                         return make_pricing_result(*value);
                                     });
}

} // namespace kiyosi
