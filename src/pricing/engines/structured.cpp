#include <kiyosi/pricing/engines/structured.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <cmath>
#include <random>
#include <map>
#include <tuple>
#include <type_traits>
#include <functional>

namespace kiyosi {
using namespace detail;
namespace {
double trading_step(const TradingCalendar& calendar, date previous, date current)
{
    auto days = calendar.trading_days_between(previous, current);
    if (calendar.is_trading_day(current) && !calendar.is_trading_day(previous)) ++days;
    return static_cast<double>(days) / static_cast<double>(calendar.annual_trading_days());
}

std::vector<date> trading_dates(const TradingCalendar& calendar, date start, date end)
{
    std::vector<date> dates;
    for (auto value = start + std::chrono::days{1}; value <= end; value += std::chrono::days{1})
        if (calendar.is_trading_day(value)) dates.push_back(value);
    return dates;
}

template <typename Option>
double path_payoff(const Option& option, const PricingContext& context, std::mt19937_64& generator)
{
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const double spot = context.asset_price().value();
    const date valuation = context.valuation_date();
    const auto& calendar = context.calendar();
    if constexpr (std::is_same_v<Option, Accumulator>) {
        double value = spot;
        double quantity = option.accumulated_quantity();
        double terminal = spot;
        double elapsed = 0.0;
        std::normal_distribution<double> normal;
        auto previous = valuation;
        for (const auto current : trading_dates(calendar, valuation, option.expiry())) {
            const double dt = trading_step(calendar, previous, current);
            elapsed += dt;
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt + sigma * std::sqrt(dt) * normal(generator));
            if (value >= option.knock_out()) {
                terminal = value;
                break;
            }
            quantity += value < option.strike() ? option.daily_quantity() * option.acceleration() : option.daily_quantity();
            terminal = value;
            previous = current;
        }
        return quantity * (terminal - option.strike()) * std::exp(-rate * elapsed);
    } else {
        const auto& dates = option.observation_dates();
        double value = spot;
        double elapsed = 0.0;
        double coupons = 0.0;
        bool knocked_in = option.touch_status() == barrier_touch_status::down;
        std::normal_distribution<double> normal;
        std::size_t index = 0;
        auto previous = valuation;
        for (const auto current : trading_dates(calendar, valuation, option.expiry())) {
            const double dt = trading_step(calendar, previous, current);
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt + sigma * std::sqrt(dt) * normal(generator));
            elapsed += dt;
            previous = current;
            if constexpr (std::is_base_of_v<KiAutocallableNote, Option>) {
                if (option.knock_in_frequency() == observation_frequency::daily && value <= option.knock_in_price())
                    knocked_in = true;
            }
            while (index < dates.size() && dates[index] < current) ++index;
            if (index >= dates.size() || dates[index] != current) continue;
            const double time = elapsed;
            if constexpr (std::is_same_v<Option, PhoenixOption>) {
                if (value >= option.coupon_barriers()[index]) coupons += option.initial_price() * option.coupon_rate() * std::exp(-rate * time);
            }
            if (value >= option.knock_out_prices()[index]) {
                double coupon = 0.0;
                if constexpr (requires { option.knock_out_coupon_rates(); })
                    coupon = option.knock_out_coupon_rates()[index] * calendar.trading_year_fraction(option.effective(), dates[index]);
                return (option.principal_ratio() + coupon) * std::exp(-rate * time) + coupons;
            }
            ++index;
        }
        const double final_time = elapsed;
        if constexpr (std::is_same_v<Option, PhoenixOption>) {
            if (option.knock_in_frequency() == observation_frequency::at_expiry && value <= option.knock_in_price()) knocked_in = true;
            const double loss = std::clamp(value - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) / option.initial_price();
            return coupons + std::exp(-rate * final_time) * (option.principal_ratio() + (knocked_in ? loss : 0.0));
        } else if constexpr (std::is_same_v<Option, TernarySnowballOption>) {
            if (option.knock_in_frequency() == observation_frequency::at_expiry && value <= option.knock_in_price()) knocked_in = true;
            const double coupon = knocked_in ? option.minimal_coupon_rate() : option.maturity_coupon_rate();
            return std::exp(-rate * final_time) * (option.principal_ratio() + coupon * calendar.trading_year_fraction(option.effective(), option.expiry()));
        } else if constexpr (std::is_same_v<Option, SnowballOption>) {
            if (option.knock_in_frequency() == observation_frequency::at_expiry && value <= option.knock_in_price()) knocked_in = true;
            const double loss = std::clamp(value - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) / option.initial_price();
            const double coupon = knocked_in ? loss : option.maturity_coupon_rate() * calendar.trading_year_fraction(option.effective(), option.expiry());
            return std::exp(-rate * final_time) * (option.principal_ratio() + coupon);
        } else {
            return std::exp(-rate * final_time) * (option.principal_ratio() + option.maturity_coupon_rate() * calendar.trading_year_fraction(option.effective(), option.expiry()));
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

template <typename Option>
result<PricingResult> price_finite_difference_structured(
    const Option& option, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (settings.asset_steps < 3 || settings.time_steps <= 0 || settings.asset_steps > 2000 || settings.time_steps > 2000)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});
    if constexpr (requires { option.initial_price(); }) {
        auto contract = validate_note(option);
        if (!contract) return std::unexpected(contract.error());
        if (context.valuation_date() < option.effective())
            return std::unexpected(Error{error_category::invalid_schedule, "valuation precedes contract effective date"});
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const double maturity = actual_365(context.valuation_date(), option.expiry());
    if (maturity == 0.0) return PricingResult{{risk_measure::price, 0.0}};
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const int steps = settings.time_steps;
    const double dt = maturity / static_cast<double>(steps);
    const double up = std::exp(sigma * std::sqrt(dt));
    const double down = 1.0 / up;
    const double growth = std::exp((rate - dividend) * dt);
    const double probability = std::clamp((growth - down) / (up - down), 0.0, 1.0);
    const double discount = std::exp(-rate * dt);
    const double spot = context.asset_price().value();
    auto underlying = [&](int step, int index) { return spot * std::pow(up, index) * std::pow(down, step - index); };

    if constexpr (std::is_same_v<Option, Accumulator>) {
        using key = std::tuple<int, int, int, int>;
        std::map<key, double> memo;
        std::function<double(int, int, int, int)> value = [&](int step, int index, int normal, int accelerated) -> double {
            const key state{step, index, normal, accelerated};
            if (auto found = memo.find(state); found != memo.end()) return found->second;
            const double asset = underlying(step, index);
            const double quantity = option.accumulated_quantity() + option.daily_quantity() * normal + option.daily_quantity() * option.acceleration() * accelerated;
            if (asset >= option.knock_out()) return memo[state] = quantity * (asset - option.strike());
            if (step == steps) return memo[state] = quantity * (asset - option.strike());
            const bool below = asset < option.strike();
            const double continuation = probability * value(step + 1, index + 1, normal + (below ? 0 : 1), accelerated + (below ? 1 : 0)) +
                                        (1.0 - probability) * value(step + 1, index, normal + (below ? 0 : 1), accelerated + (below ? 1 : 0));
            return memo[state] = discount * continuation;
        };
        return PricingResult{{risk_measure::price, value(0, 0, 0, 0)}};
    } else {
        std::vector<int> observation_steps;
        for (const auto date_value : option.observation_dates()) {
            const double fraction = actual_365(context.valuation_date(), date_value) / maturity;
            observation_steps.push_back(std::clamp(static_cast<int>(std::lround(fraction * steps)), 1, steps));
        }
        using key = std::tuple<int, int, bool>;
        std::map<key, double> memo;
        std::function<double(int, int, bool)> value = [&](int step, int index, bool knocked_in) -> double {
            const key state{step, index, knocked_in};
            if (auto found = memo.find(state); found != memo.end()) return found->second;
            const double asset = underlying(step, index);
            bool ki = knocked_in;
            if constexpr (requires { option.knock_in_price(); })
                if (option.knock_in_frequency() == observation_frequency::daily && asset <= option.knock_in_price()) ki = true;
            if (step == steps) {
                if constexpr (requires { option.knock_in_price(); })
                    if (option.knock_in_frequency() == observation_frequency::at_expiry && asset <= option.knock_in_price()) ki = true;
                if constexpr (std::is_same_v<Option, PhoenixOption>) {
                    const double loss = std::clamp(asset - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) / option.initial_price();
                    return memo[state] = discount * 0.0 + option.principal_ratio() + (ki ? loss : 0.0);
                } else if constexpr (std::is_same_v<Option, SnowballOption>) {
                    const double loss = std::clamp(asset - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) / option.initial_price();
                    const double coupon = ki ? loss : option.maturity_coupon_rate() * context.calendar().trading_year_fraction(option.effective(), option.expiry());
                    return memo[state] = option.principal_ratio() + coupon;
                } else if constexpr (std::is_same_v<Option, TernarySnowballOption>) {
                    const double coupon = (ki ? option.minimal_coupon_rate() : option.maturity_coupon_rate()) * context.calendar().trading_year_fraction(option.effective(), option.expiry());
                    return memo[state] = option.principal_ratio() + coupon;
                } else {
                    return memo[state] = option.principal_ratio() + option.maturity_coupon_rate() * context.calendar().trading_year_fraction(option.effective(), option.expiry());
                }
            }
            const auto obs = std::find(observation_steps.begin(), observation_steps.end(), step + 1);
            const std::size_t obs_index = obs == observation_steps.end() ? 0 : static_cast<std::size_t>(obs - observation_steps.begin());
            const bool is_observation = obs != observation_steps.end();
            if (is_observation && asset >= option.knock_out_prices()[obs_index]) {
                double coupon = 0.0;
                if constexpr (requires { option.knock_out_coupon_rates(); })
                    coupon = option.knock_out_coupon_rates()[obs_index] * context.calendar().trading_year_fraction(option.effective(), option.observation_dates()[obs_index]);
                if constexpr (std::is_same_v<Option, PhoenixOption>)
                    if (asset >= option.coupon_barriers()[obs_index]) coupon = option.coupon_rate() * context.calendar().trading_year_fraction(option.effective(), option.observation_dates()[obs_index]);
                return memo[state] = discount * (option.principal_ratio() + coupon);
            }
            double cash = 0.0;
            if (is_observation) {
                if constexpr (std::is_same_v<Option, PhoenixOption>)
                    if (asset >= option.coupon_barriers()[obs_index]) cash = option.initial_price() * option.coupon_rate() * context.calendar().trading_year_fraction(option.effective(), option.observation_dates()[obs_index]);
            }
            const double continuation = probability * value(step + 1, index + 1, ki) + (1.0 - probability) * value(step + 1, index, ki);
            return memo[state] = cash * discount + discount * continuation;
        };
        bool knocked = option.touch_status() == barrier_touch_status::down;
        return PricingResult{{risk_measure::price, value(0, 0, knocked)}};
    }
}

template result<PricingResult> price_finite_difference_structured(const Accumulator&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const PhoenixOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const SnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const BinarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const TernarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);
} // namespace kiyosi
