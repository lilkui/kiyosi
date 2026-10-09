#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include <cmath>
#include <random>
#include <vector>

#include "../monte_carlo_mean.hpp"
#include "../../detail/accumulator_state.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_cuda_host.hpp"

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

    const auto initial = accumulator_initial_state(option, context);
    if (initial.settlement) return make_pricing_result(*initial.settlement);

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
        return make_pricing_result(*mean);
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }
    std::mt19937_64 generator(settings_.seed ? *settings_.seed : std::random_device{}());
    MonteCarloMean mean{};
    for (int path = 0; path < settings_.path_count; ++path) {
        // Early termination must not change the random draws of later paths.
        std::mt19937_64 path_generator{generator()};
        const auto payoff = path_payoff(option, context, *steps, initial.quantity, path_generator);
        if (!payoff) return std::unexpected(payoff.error());
        mean.add(*payoff);
    }
    return make_pricing_result(mean.value());
}

} // namespace kiyosi
