#include <kiyosi/pricing/engines/accumulator/monte_carlo.hpp>

#include <cmath>
#include <random>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

double path_payoff(const Accumulator& option, const PricingContext& context, std::mt19937_64& generator)
{
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const timestamp valuation = context.valuation_time();
    const auto& calendar = context.calendar();

    double value = context.asset_price();
    double quantity = option.accumulated_quantity();
    double terminal = value;

    if (valuation == start_of_day(date_of(valuation)) && calendar.is_trading_day(date_of(valuation))) {
        if (value >= option.knock_out()) return quantity * (value - option.strike());
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration()
                                            : option.daily_quantity();
    }
    if (valuation == option.expiry()) return quantity * (value - option.strike());

    std::normal_distribution<double> normal;
    auto previous = valuation;
    for (const auto current : trading_dates(calendar, valuation, option.expiry())) {
        const double dt = actual_365(previous, current);
        value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt +
                          sigma * std::sqrt(dt) * normal(generator));
        previous = current;
        if (value >= option.knock_out()) {
            terminal = value;
            break;
        }
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration()
                                            : option.daily_quantity();
        terminal = value;
    }
    return quantity * (terminal - option.strike()) * std::exp(-rate * actual_365(valuation, previous));
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

    std::mt19937_64 generator(settings_.seed.value_or(std::random_device{}()));
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path)
        sum += path_payoff(option, context, generator);
    const double value = sum / static_cast<double>(settings_.path_count);
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result,
                                     "structured pricing produced a non-finite result"});
    return make_pricing_result({{risk_measure::price, value}});
}

} // namespace kiyosi
