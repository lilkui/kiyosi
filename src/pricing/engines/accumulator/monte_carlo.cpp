#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include <cmath>
#include <optional>
#include <random>
#include <vector>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_cuda_host.hpp"

namespace kiyosi {
using namespace detail;

namespace {

using SimulationStep = detail::CudaSimulationStep;

struct InitialState {
    double quantity{};
    std::optional<double> settlement;
};

InitialState initial_state(const Accumulator& option, const PricingContext& context)
{
    const Timestamp valuation = context.valuation_time();
    const double value = context.spot_price();
    double quantity = option.accumulated_quantity();
    if (valuation == start_of_day(date_of(valuation)) &&
        context.calendar().is_trading_day(date_of(valuation))) {
        if (value >= option.knock_out_level())
            return {quantity, quantity * (value - option.strike())};
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration_factor()
                                            : option.daily_quantity();
    }
    if (valuation == option.expiry_date()) return {quantity, quantity * (value - option.strike())};
    return {quantity, std::nullopt};
}

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
                   std::mt19937_64& generator)
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

} // namespace

Result<PricingResult> MonteCarloAccumulatorEngine::price_native(
    const Accumulator& option, const PricingContext& context) const
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    const auto settings_valid = validate_monte_carlo_settings(settings_);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), option.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());

    const auto initial = initial_state(option, context);
    if (initial.settlement) return make_pricing_result(*initial.settlement);

    const auto steps = prepare_simulation(option, context);
    if (!steps) return std::unexpected(steps.error());
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto sum = cuda_sum(detail::cuda_accumulator_price(
            {settings_.path_count, settings_.seed ? *settings_.seed : random_seed(),
             context.spot_price(), option.strike(), option.knock_out_level(),
             option.daily_quantity(), option.acceleration_factor(), initial.quantity},
            *steps));
        if (!sum) return std::unexpected(sum.error());
        return make_pricing_result(*sum / static_cast<double>(settings_.path_count));
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }
    std::mt19937_64 generator(settings_.seed ? *settings_.seed : std::random_device{}());
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path) {
        const auto payoff = path_payoff(option, context, *steps, initial.quantity, generator);
        if (!payoff) return std::unexpected(payoff.error());
        sum += *payoff;
    }
    return make_pricing_result(sum / static_cast<double>(settings_.path_count));
}

} // namespace kiyosi
