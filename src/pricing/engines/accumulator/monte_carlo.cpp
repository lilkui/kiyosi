#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include <cmath>
#include <random>
#include <utility>
#include <vector>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct SimulationStep {
    double drift;
    double diffusion;
    double discount;
};

struct SimulationInputs {
    bool observe_valuation;
    std::vector<SimulationStep> steps;
};

SimulationInputs prepare_simulation(const Accumulator& option, const PricingContext& context)
{
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const timestamp valuation = context.valuation_time();
    const bool observe_valuation = valuation == start_of_day(date_of(valuation)) &&
                                   context.calendar().is_trading_day(date_of(valuation));
    const auto dates = trading_dates(context.calendar(), valuation, option.expiry());
    std::vector<SimulationStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    for (const date current : dates) {
        const double dt = actual_365(previous, current);
        steps.push_back({(rate - dividend - 0.5 * sigma * sigma) * dt,
                         sigma * std::sqrt(dt),
                         std::exp(-rate * actual_365(valuation, current))});
        previous = current;
    }
    return {observe_valuation, std::move(steps)};
}

double path_payoff(const Accumulator& option, const PricingContext& context,
                   const SimulationInputs& inputs, std::mt19937_64& generator)
{
    const timestamp valuation = context.valuation_time();

    double value = context.asset_price();
    double quantity = option.accumulated_quantity();
    double terminal = value;

    if (inputs.observe_valuation) {
        if (value >= option.knock_out()) return quantity * (value - option.strike());
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration()
                                            : option.daily_quantity();
    }
    if (valuation == option.expiry()) return quantity * (value - option.strike());

    std::normal_distribution<double> normal;
    double discount = 1.0;
    for (const auto& step : inputs.steps) {
        value *= std::exp(step.drift + step.diffusion * normal(generator));
        discount = step.discount;
        if (value >= option.knock_out()) {
            terminal = value;
            break;
        }
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration()
                                            : option.daily_quantity();
        terminal = value;
    }
    return quantity * (terminal - option.strike()) * discount;
}

} // namespace

result<PricingResult> MonteCarloAccumulatorEngine::price(
    const Accumulator& option, const PricingContext& context) const
{
    auto contract = make_accumulator({option.strike(), option.knock_out(), option.daily_quantity(),
                                      option.acceleration(), option.accumulated_quantity(),
                                      option.effective(), option.expiry()});
    if (!contract) return std::unexpected(contract.error());
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (settings_.path_count <= 0 || settings_.path_count > 10'000'000)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "structured Monte Carlo path count is out of range"});

    std::mt19937_64 generator(settings_.seed ? *settings_.seed : std::random_device{}());
    const auto inputs = prepare_simulation(option, context);
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path)
        sum += path_payoff(option, context, inputs, generator);
    const double value = sum / static_cast<double>(settings_.path_count);
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result,
                                     "structured pricing produced a non-finite result"});
    return make_pricing_result({{risk_measure::price, value}});
}

} // namespace kiyosi
