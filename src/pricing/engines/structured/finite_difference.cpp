#include <kiyosi/pricing/engines/structured/finite_difference.hpp>
#include <kiyosi/market/observation_schedule.hpp>
#include "../../detail/common.hpp"
#include "../../detail/structured.hpp"
#include "../../detail/finite_difference.hpp"
#include <cmath>

namespace kiyosi {
using namespace detail;

template <typename Option>
result<PricingResult> price_finite_difference_structured(
    const Option& option, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    auto settings_valid = validate_finite_difference_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings.asset_steps > 2000 || settings.time_steps > 2000)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});
    if constexpr (std::is_same_v<Option, Accumulator>) {
        auto contract = make_accumulator(option.strike(), option.knock_out(), option.daily_quantity(), option.acceleration(),
                                         option.accumulated_quantity(), option.effective(), option.expiry());
        if (!contract) return std::unexpected(contract.error());
    } else {
        auto contract = validate_note(option);
        if (!contract) return std::unexpected(contract.error());
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    if constexpr (requires { option.touch_status(); })
        if (option.touch_status() == barrier_touch_status::up && settings.upper_boundary == 0.0)
            return PricingResult{{risk_measure::price, 0.0}};
    const double spot = context.asset_price();
    double relevant = spot;
    if constexpr (std::is_same_v<Option, Accumulator>) {
        relevant = std::max({relevant, option.strike(), option.knock_out()});
    } else {
        relevant = std::max({relevant, option.initial_price(), option.upper_strike(), option.lower_strike()});
        for (const double level : option.knock_out_prices()) relevant = std::max(relevant, level);
        if constexpr (requires { option.knock_in_price(); }) relevant = std::max(relevant, option.knock_in_price());
        if constexpr (requires { option.coupon_barriers(); })
            for (const double level : option.coupon_barriers()) relevant = std::max(relevant, level);
    }
    if (settings.upper_boundary != 0.0 && settings.upper_boundary <= relevant)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must exceed every product level"});
    if constexpr (requires { option.touch_status(); })
        if (option.touch_status() == barrier_touch_status::up)
            return PricingResult{{risk_measure::price, 0.0}};
    const double maturity = actual_365(context.valuation_time(), option.expiry());
    if (maturity == 0.0) {
        if constexpr (std::is_same_v<Option, Accumulator>) {
            double quantity = option.accumulated_quantity();
            const double value = context.asset_price();
            if (context.calendar().is_trading_day(option.expiry()) && value < option.knock_out())
                quantity += value < option.strike() ? option.daily_quantity() * option.acceleration() : option.daily_quantity();
            return PricingResult{{risk_measure::price, quantity * (value - option.strike())}};
        }
        else {
            bool knocked_in = option.touch_status() == barrier_touch_status::down;
            knocked_in = is_knocked_in(option, context.asset_price(), knocked_in, true);
            const auto& dates = option.observation_dates();
            std::size_t expiry_index = 0;
            while (expiry_index < dates.size() && dates[expiry_index] < option.expiry()) ++expiry_index;
            if (expiry_index < dates.size() && dates[expiry_index] == option.expiry() &&
                context.asset_price() >= option.knock_out_prices()[expiry_index])
                return PricingResult{{risk_measure::price, option.principal_ratio() + observation_coupon(option, expiry_index,
                                                                                                          context.asset_price())}};
            const double coupon = expiry_index < dates.size() && dates[expiry_index] == option.expiry() &&
                                   carries_observation_coupon<Option>
                                      ? observation_coupon(option, expiry_index, context.asset_price()) : 0.0;
            return PricingResult{{risk_measure::price, terminal_settlement(option, context.asset_price(), knocked_in) + coupon}};
        }
    }
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const int asset_steps = settings.asset_steps;
    const double upper = settings.upper_boundary > 0.0 ? settings.upper_boundary : std::max(4.0 * relevant, relevant + 1.0);
    const double spacing = upper / static_cast<double>(asset_steps);
    std::vector<double> anchors{0.0, maturity};
    auto add_anchor = [&](date value) {
        if (value <= context.valuation_time()) return;
        const double time = actual_365(context.valuation_time(), value);
        if (time > 0.0 && time < maturity) anchors.push_back(time);
    };
    const auto future_trading_dates = trading_dates(context.calendar(), context.valuation_time(), option.expiry(), true);
    std::vector<double> trading_times;
    trading_times.reserve(future_trading_dates.size());
    for (const date value : future_trading_dates)
        trading_times.push_back(actual_365(context.valuation_time(), value));
    if constexpr (std::is_same_v<Option, Accumulator>) {
        for (const date value : future_trading_dates) add_anchor(value);
    } else {
        for (const date value : option.observation_dates()) add_anchor(value);
        if constexpr (requires { option.knock_in_frequency(); })
            if (option.knock_in_frequency() == observation_frequency::daily)
                for (const date value : future_trading_dates) add_anchor(value);
    }
    const auto grid = finite_difference_grid(maturity, settings.time_steps, std::move(anchors));
    if (settings.scheme == finite_difference_scheme::explicit_euler) {
        double largest_dt = 0.0;
        for (std::size_t index = 1; index < grid.size(); ++index)
            largest_dt = std::max(largest_dt, grid[index] - grid[index - 1]);
        if (largest_dt * (sigma * sigma * asset_steps * asset_steps + std::abs(rate)) > 1.0)
            return std::unexpected(Error{error_category::invalid_parameter,
                                         "explicit finite-difference grid is unstable"});
    }
    const double theta = settings.scheme == finite_difference_scheme::explicit_euler ? 0.0 :
                         settings.scheme == finite_difference_scheme::implicit_euler ? 1.0 : 0.5;
    const std::size_t size = static_cast<std::size_t>(asset_steps) + 1;
    const auto asset = [&](std::size_t index) { return spacing * static_cast<double>(index); };
    const auto interpolate = [&](const std::vector<double>& values) {
        const double position = spot / spacing;
        const int index = std::clamp(static_cast<int>(std::floor(position)), 0, asset_steps - 1);
        const double weight = position - static_cast<double>(index);
        return values[static_cast<std::size_t>(index)] + weight *
               (values[static_cast<std::size_t>(index + 1)] - values[static_cast<std::size_t>(index)]);
    };
    FiniteDifferenceStep stepper(size);
    auto advance = [&](const std::vector<double>& old, std::vector<double>& next, double dt) -> bool {
        const double high_slope = (old.back() - old[old.size() - 2]) / spacing;
        const double high_intercept = old.back() - high_slope * upper;
        const double high_boundary = high_slope * upper * std::exp(-dividend * dt) +
                                     high_intercept * std::exp(-rate * dt);
        return stepper.advance(old, next, dt, rate, dividend, sigma, theta,
                                         old.front() * std::exp(-rate * dt), high_boundary);
    };
    const auto observation_events = observation_schedule(option, context.valuation_time());
    auto event_index = [&](double time) -> std::optional<std::size_t> {
        if constexpr (std::is_same_v<Option, Accumulator>) {
            (void)time;
            return std::nullopt;
        } else {
            const auto found = std::find_if(observation_events.begin(), observation_events.end(), [&](std::size_t index) {
                return actual_365(context.valuation_time(), option.observation_dates()[index]) == time;
            });
            return found == observation_events.end() ? std::nullopt : std::optional<std::size_t>{*found};
        }
    };
    auto daily_event = [&](double time) {
        if constexpr (std::is_same_v<Option, Accumulator>) {
            (void)time;
            return false;
        } else if constexpr (requires { option.knock_in_frequency(); }) {
            return option.knock_in_frequency() == observation_frequency::daily &&
                   std::any_of(future_trading_dates.begin(), future_trading_dates.end(), [&](date value) {
                                   return actual_365(context.valuation_time(), value) == time;
                               });
        } else {
            (void)time;
            return false;
        }
    };
    if constexpr (std::is_same_v<Option, Accumulator>) {
        std::vector<double> slope(size), intercept(size), next_slope(size), next_intercept(size);
        const bool expiry_trading = context.calendar().is_trading_day(option.expiry());
        for (std::size_t index = 0; index < size; ++index) {
            slope[index] = asset(index) - option.strike();
            intercept[index] = expiry_trading ? slope[index] * (asset(index) < option.strike() ? option.daily_quantity() * option.acceleration() : option.daily_quantity()) : 0.0;
            if (!expiry_trading) continue;
            if (asset(index) >= option.knock_out()) intercept[index] = 0.0;
        }
        for (std::size_t step = grid.size() - 1; step-- > 0;) {
            const double dt = grid[step + 1] - grid[step];
            if (!advance(slope, next_slope, dt) || !advance(intercept, next_intercept, dt))
                return std::unexpected(Error{error_category::invalid_result, "finite-difference system is numerically unstable"});
            const bool trading = std::binary_search(trading_times.begin(), trading_times.end(), grid[step]);
            if (trading) for (std::size_t index = 0; index < size; ++index) {
                if (asset(index) >= option.knock_out()) {
                    next_intercept[index] += next_slope[index] * option.accumulated_quantity();
                    next_slope[index] = 0.0;
                } else {
                    next_intercept[index] += next_slope[index] * (asset(index) < option.strike() ? option.daily_quantity() * option.acceleration() : option.daily_quantity());
                }
            }
            slope.swap(next_slope);
            intercept.swap(next_intercept);
        }
        return PricingResult{{risk_measure::price, interpolate(slope) * option.accumulated_quantity() + interpolate(intercept)}};
    } else {
        std::vector<double> knocked_in_values(size), not_knocked_in_values(size);
        std::vector<double> next_knocked_in_values(size), next_not_knocked_in_values(size);
        const auto expiry_observation = event_index(maturity);
        for (std::size_t index = 0; index < size; ++index) {
            const double value = asset(index);
            bool ki = option.touch_status() == barrier_touch_status::down;
            if constexpr (requires { option.knock_in_frequency(); })
                ki = ki || value < option.knock_in_price();
            if (expiry_observation && value >= option.knock_out_prices()[*expiry_observation]) {
                knocked_in_values[index] = not_knocked_in_values[index] =
                    option.principal_ratio() + observation_coupon(option, *expiry_observation, value);
            } else {
                const double coupon = expiry_observation && carries_observation_coupon<Option>
                                          ? observation_coupon(option, *expiry_observation, value) : 0.0;
                knocked_in_values[index] = terminal_settlement(option, value, true) + coupon;
                not_knocked_in_values[index] = terminal_settlement(option, value, ki) + coupon;
            }
        }
        for (std::size_t step = grid.size() - 1; step-- > 0;) {
            const double dt = grid[step + 1] - grid[step];
            if (!advance(knocked_in_values, next_knocked_in_values, dt) ||
                !advance(not_knocked_in_values, next_not_knocked_in_values, dt))
                return std::unexpected(Error{error_category::invalid_result, "finite-difference system is numerically unstable"});
            const auto observation = event_index(grid[step]);
            const bool daily = daily_event(grid[step]);
            for (std::size_t index = 0; index < size; ++index) {
                const double value = asset(index);
                bool transitioned = false;
                if constexpr (requires { option.knock_in_price(); })
                    transitioned = daily && value < option.knock_in_price();
                if (observation && value >= option.knock_out_prices()[*observation]) {
                    next_knocked_in_values[index] = next_not_knocked_in_values[index] =
                        option.principal_ratio() + observation_coupon(option, *observation, value);
                } else if (observation) {
                    const double coupon = carries_observation_coupon<Option> ? observation_coupon(option, *observation, value) : 0.0;
                    const double continuation_in = next_knocked_in_values[index];
                    const double continuation_out = transitioned ? continuation_in : next_not_knocked_in_values[index];
                    next_knocked_in_values[index] = continuation_in + coupon;
                    next_not_knocked_in_values[index] = continuation_out + coupon;
                } else if (transitioned) {
                    next_not_knocked_in_values[index] = next_knocked_in_values[index];
                }
            }
            knocked_in_values.swap(next_knocked_in_values);
            not_knocked_in_values.swap(next_not_knocked_in_values);
        }
        const double value = interpolate(option.touch_status() == barrier_touch_status::down
                                             ? knocked_in_values : not_knocked_in_values);
        return PricingResult{{risk_measure::price, value}};
    }
}

template result<PricingResult> price_finite_difference_structured(const Accumulator&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const PhoenixOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const SnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const BinarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_finite_difference_structured(const TernarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);

} // namespace kiyosi
