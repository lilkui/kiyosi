#include <kiyosi/pricing/engines/structured/monte_carlo.hpp>
#include <kiyosi/market/observation_schedule.hpp>
#include "../../detail/common.hpp"
#include "../../detail/structured.hpp"
#include <cmath>
#include <random>

namespace kiyosi {
using namespace detail;

namespace {
template <typename Option>
double path_payoff(const Option& option, const PricingContext& context, std::mt19937_64& generator)
{
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const double spot = context.asset_price().value();
    const timestamp valuation = context.valuation_time();
    const auto& calendar = context.calendar();
    if constexpr (requires { option.touch_status(); })
        if (option.touch_status() == barrier_touch_status::up) return 0.0;
    if constexpr (std::is_same_v<Option, Accumulator>) {
        double value = spot;
        double quantity = option.accumulated_quantity();
        double terminal = spot;
        std::normal_distribution<double> normal;
        auto previous = valuation;
        if (valuation == start_of_day(date_of(valuation)) && calendar.is_trading_day(date_of(valuation))) {
            if (spot >= option.knock_out())
                return quantity * (spot - option.strike());
            quantity += spot < option.strike() ? option.daily_quantity() * option.acceleration() : option.daily_quantity();
        }
        if (valuation == option.expiry())
            return quantity * (spot - option.strike());
        for (const auto current : trading_dates(calendar, valuation, option.expiry())) {
            const double dt = actual_365(previous, current);
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt + sigma * std::sqrt(dt) * normal(generator));
            previous = current;
            if (value >= option.knock_out()) {
                terminal = value;
                break;
            }
            quantity += value < option.strike() ? option.daily_quantity() * option.acceleration() : option.daily_quantity();
            terminal = value;
        }
        return quantity * (terminal - option.strike()) * std::exp(-rate * actual_365(valuation, previous));
    } else {
        const auto& dates = option.observation_dates();
        const auto schedule = observation_schedule(option, valuation);
        double value = spot;
        double coupons = 0.0;
        bool knocked_in = option.touch_status() == barrier_touch_status::down;
        if (valuation == start_of_day(date_of(valuation)))
            knocked_in = is_knocked_in(option, value, knocked_in, valuation == option.expiry());
        std::size_t index = 0;
        if (!schedule.empty() && dates[schedule.front()] == valuation) {
            const auto event = schedule.front();
            const double coupon = observation_coupon(option, event, value);
            if (value >= option.knock_out_prices()[event])
                return option.principal_ratio() + coupon;
            if constexpr (std::is_same_v<Option, PhoenixOption>) coupons = coupon;
            index = 1;
        }
        if (valuation == option.expiry()) {
            knocked_in = is_knocked_in(option, value, knocked_in, true);
            return coupons + terminal_settlement(option, value, knocked_in);
        }
        std::normal_distribution<double> normal;
        auto previous = valuation;
        for (const auto current : trading_dates(calendar, valuation, option.expiry())) {
            const double dt = actual_365(previous, current);
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt + sigma * std::sqrt(dt) * normal(generator));
            previous = current;
            knocked_in = is_knocked_in(option, value, knocked_in, false);
            if (index >= schedule.size() || dates[schedule[index]] != current) continue;
            const auto event = schedule[index];
            const double time = actual_365(valuation, current);
            const double coupon = observation_coupon(option, event, value);
            if (value >= option.knock_out_prices()[event]) {
                return (option.principal_ratio() + coupon) * std::exp(-rate * time) + coupons;
            }
            if constexpr (std::is_same_v<Option, PhoenixOption>)
                coupons += coupon * std::exp(-rate * time);
            ++index;
        }
        knocked_in = is_knocked_in(option, value, knocked_in, true);
        return coupons + std::exp(-rate * actual_365(valuation, option.expiry())) * terminal_settlement(option, value, knocked_in);
    }
}

template <typename Option>
result<PricingResult> price_structured(const Option& option, const PricingContext& context, StructuredMonteCarloSettings settings)
{
    if constexpr (std::is_same_v<Option, Accumulator>) {
        auto contract = make_accumulator(option.strike(), option.knock_out(), option.daily_quantity(), option.acceleration(),
                                         option.accumulated_quantity(), option.effective(), option.expiry());
        if (!contract) return std::unexpected(contract.error());
    } else {
        auto contract = validate_note(option);
        if (!contract) return std::unexpected(contract.error());
    }
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if constexpr (requires { option.observation_dates(); }) {
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    if (settings.path_count <= 0 || settings.path_count > 10'000'000)
        return std::unexpected(Error{error_category::invalid_parameter, "structured Monte Carlo path count is out of range"});
    std::mt19937_64 generator(settings.seed.value_or(std::random_device{}()));
    double sum = 0.0;
    for (int path = 0; path < settings.path_count; ++path)
        sum += path_payoff(option, context, generator);
    const double value = sum / static_cast<double>(settings.path_count);
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "structured pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
}
} // namespace

template <typename Option>
result<PricingResult> MonteCarloStructuredEngine<Option>::price(const Option& option, const PricingContext& context) const
{
    return price_structured(option, context, settings_);
}

template class MonteCarloStructuredEngine<Accumulator>;
template class MonteCarloStructuredEngine<PhoenixOption>;
template class MonteCarloStructuredEngine<SnowballOption>;
template class MonteCarloStructuredEngine<BinarySnowballOption>;
template class MonteCarloStructuredEngine<TernarySnowballOption>;

} // namespace kiyosi
