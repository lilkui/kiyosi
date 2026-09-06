#include <kiyosi/pricing/engines/structured.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <cmath>
#include <random>
#include <type_traits>

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
    const date valuation = context.valuation_date();
    const double maturity = actual_365(valuation, option.expiry());
    if constexpr (std::is_same_v<Option, Accumulator>) {
        const int steps = std::max(1, static_cast<int>((option.expiry() - valuation).count()));
        const double dt = maturity / steps;
        double value = spot;
        double quantity = option.accumulated_quantity();
        double terminal = spot;
        int terminal_step = steps;
        std::normal_distribution<double> normal;
        for (int step = 1; step <= steps; ++step) {
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt + sigma * std::sqrt(dt) * normal(generator));
            if (value >= option.knock_out()) {
                terminal = value;
                terminal_step = step;
                break;
            }
            quantity += value < option.strike() ? option.daily_quantity() * option.acceleration() : option.daily_quantity();
            terminal = value;
        }
        return quantity * (terminal - option.strike()) * std::exp(-rate * dt * terminal_step);
    } else {
        const auto& dates = option.observation_dates();
        if (dates.empty()) return option.principal_ratio() * std::exp(-rate * maturity);
        double value = spot;
        double previous_time = 0.0;
        double coupons = 0.0;
        bool knocked_in = option.touch_status() == barrier_touch_status::down;
        std::normal_distribution<double> normal;
        for (std::size_t index = 0; index < dates.size(); ++index) {
            if (dates[index] < valuation) continue;
            const double time = actual_365(valuation, dates[index]);
            const double dt = std::max(0.0, time - previous_time);
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt + sigma * std::sqrt(dt) * normal(generator));
            previous_time = time;
            if constexpr (std::is_base_of_v<KiAutocallableNote, Option>) {
                if (option.knock_in_frequency() != observation_frequency::at_expiry && value <= option.knock_in_price()) knocked_in = true;
            }
            if constexpr (std::is_same_v<Option, PhoenixOption>) {
                if (value >= option.coupon_barriers()[index]) coupons += option.initial_price() * option.coupon_rate() * std::exp(-rate * time);
            }
            if (value >= option.knock_out_prices()[index]) {
                double coupon = 0.0;
                if constexpr (requires { option.knock_out_coupon_rates(); }) coupon = option.knock_out_coupon_rates()[index] * actual_365(option.effective(), dates[index]);
                return (option.principal_ratio() + coupon) * std::exp(-rate * time) + coupons;
            }
        }
        const double final_time = maturity;
        if constexpr (std::is_same_v<Option, PhoenixOption>) {
            if (option.knock_in_frequency() == observation_frequency::at_expiry && value <= option.knock_in_price()) knocked_in = true;
            const double loss = std::clamp(value - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) / option.initial_price();
            return coupons + std::exp(-rate * final_time) * (option.principal_ratio() + (knocked_in ? loss : 0.0));
        } else if constexpr (std::is_same_v<Option, TernarySnowballOption>) {
            if (option.knock_in_frequency() == observation_frequency::at_expiry && value <= option.knock_in_price()) knocked_in = true;
            const double coupon = knocked_in ? option.minimal_coupon_rate() : option.maturity_coupon_rate();
            return std::exp(-rate * final_time) * (option.principal_ratio() + coupon * actual_365(option.effective(), option.expiry()));
        } else if constexpr (std::is_same_v<Option, SnowballOption>) {
            if (option.knock_in_frequency() == observation_frequency::at_expiry && value <= option.knock_in_price()) knocked_in = true;
            const double loss = std::clamp(value - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) / option.initial_price();
            const double coupon = knocked_in ? loss : option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry());
            return std::exp(-rate * final_time) * (option.principal_ratio() + coupon);
        } else {
            return std::exp(-rate * final_time) * (option.principal_ratio() + option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry()));
        }
    }
}

template <typename Option>
result<PricingResult> price_structured(const Option& option, const PricingContext& context, StructuredMonteCarloSettings settings)
{
    if constexpr (requires { option.initial_price(); }) {
        auto contract = validate_note(option);
        if (!contract) return std::unexpected(contract.error());
    }
    auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (context.valuation_date() < option.effective())
        return std::unexpected(Error{error_category::invalid_schedule, "valuation precedes contract effective date"});
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
