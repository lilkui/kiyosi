#include <kiyosi/pricing/engines/structured.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <cmath>
#include <random>
#include <type_traits>

namespace kiyosi {
using namespace detail;
namespace {
std::vector<date> trading_dates(const TradingCalendar& calendar, date start, date end)
{
    std::vector<date> dates;
    for (auto value = start + std::chrono::days{1}; value <= end; value += std::chrono::days{1})
        if (calendar.is_trading_day(value)) dates.push_back(value);
    return dates;
}

template <typename Option>
double terminal_settlement(const Option& option, double spot, bool knocked_in)
{
    if constexpr (std::is_same_v<Option, PhoenixOption>) {
        const double loss = std::clamp(spot - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) /
                            option.initial_price();
        return option.principal_ratio() + (knocked_in ? loss : 0.0);
    } else if constexpr (std::is_same_v<Option, SnowballOption>) {
        const double loss = std::clamp(spot - option.upper_strike(), option.lower_strike() - option.upper_strike(), 0.0) /
                            option.initial_price();
        const double coupon = knocked_in ? loss : option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry());
        return option.principal_ratio() + coupon;
    } else if constexpr (std::is_same_v<Option, TernarySnowballOption>) {
        const double rate = knocked_in ? option.minimal_coupon_rate() : option.maturity_coupon_rate();
        return option.principal_ratio() + rate * actual_365(option.effective(), option.expiry());
    } else {
        return option.principal_ratio() + option.maturity_coupon_rate() * actual_365(option.effective(), option.expiry());
    }
}

template <typename Option>
double observation_coupon(const Option& option, std::size_t index, double spot)
{
    if constexpr (std::is_same_v<Option, PhoenixOption>)
        return spot >= option.coupon_barriers()[index] ? option.initial_price() * option.coupon_rate() : 0.0;
    else
        return option.knock_out_coupon_rates()[index] * actual_365(option.effective(), option.observation_dates()[index]);
}

template <typename Option>
bool is_knocked_in(const Option& option, double spot, bool knocked_in, bool expiry)
{
    if constexpr (requires { option.knock_in_price(); }) {
        if (option.knock_in_frequency() == observation_frequency::daily || expiry)
            return knocked_in || spot < option.knock_in_price();
    }
    return knocked_in;
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
    if constexpr (requires { option.touch_status(); })
        if (option.touch_status() == barrier_touch_status::up) return 0.0;
    if constexpr (std::is_same_v<Option, Accumulator>) {
        if (valuation == option.expiry())
            return option.accumulated_quantity() * (spot - option.strike());
        double value = spot;
        double quantity = option.accumulated_quantity();
        double terminal = spot;
        std::normal_distribution<double> normal;
        auto previous = valuation;
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
        double value = spot;
        double coupons = 0.0;
        bool knocked_in = option.touch_status() == barrier_touch_status::down;
        if (valuation == option.expiry()) {
            knocked_in = is_knocked_in(option, value, knocked_in, true);
            const auto expiry = std::find(dates.begin(), dates.end(), option.expiry());
            if (expiry != dates.end()) {
                const auto expiry_index = static_cast<std::size_t>(expiry - dates.begin());
                if (value >= option.knock_out_prices()[expiry_index])
                    return option.principal_ratio() + observation_coupon(option, expiry_index, value);
                if constexpr (std::is_same_v<Option, PhoenixOption>)
                    coupons = observation_coupon(option, expiry_index, value);
            }
            return coupons + terminal_settlement(option, value, knocked_in);
        }
        std::normal_distribution<double> normal;
        std::size_t index = 0;
        while (index < dates.size() && dates[index] <= valuation) ++index;
        auto previous = valuation;
        for (const auto current : trading_dates(calendar, valuation, option.expiry())) {
            const double dt = actual_365(previous, current);
            value *= std::exp((rate - dividend - 0.5 * sigma * sigma) * dt + sigma * std::sqrt(dt) * normal(generator));
            previous = current;
            knocked_in = is_knocked_in(option, value, knocked_in, false);
            if (index >= dates.size() || dates[index] != current) continue;
            const double time = actual_365(valuation, current);
            const double coupon = observation_coupon(option, index, value);
            if (value >= option.knock_out_prices()[index]) {
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
    auto valid = validate_life(context.valuation_date(), option.effective(), option.expiry());
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

template <typename Option>
result<PricingResult> price_finite_difference_structured(
    const Option& option, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_life(context.valuation_date(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (settings.asset_steps < 3 || settings.time_steps <= 0 || settings.asset_steps > 2000 || settings.time_steps > 2000)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});
    if (settings.upper_boundary != 0.0 && (!std::isfinite(settings.upper_boundary) || settings.upper_boundary <= 0.0))
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must be finite and positive"});
    if (settings.scheme != finite_difference_scheme::explicit_euler &&
        settings.scheme != finite_difference_scheme::implicit_euler &&
        settings.scheme != finite_difference_scheme::crank_nicolson)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference scheme is invalid"});
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
    const double spot = context.asset_price().value();
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
    const double maturity = actual_365(context.valuation_date(), option.expiry());
    if (maturity == 0.0) {
        if constexpr (std::is_same_v<Option, Accumulator>)
            return PricingResult{{risk_measure::price, option.accumulated_quantity() *
                                  (context.asset_price().value() - option.strike())}};
        else {
            bool knocked_in = option.touch_status() == barrier_touch_status::down;
            knocked_in = is_knocked_in(option, context.asset_price().value(), knocked_in, true);
            const auto& dates = option.observation_dates();
            std::size_t expiry_index = 0;
            while (expiry_index < dates.size() && dates[expiry_index] < option.expiry()) ++expiry_index;
            if (expiry_index < dates.size() && dates[expiry_index] == option.expiry() &&
                context.asset_price().value() >= option.knock_out_prices()[expiry_index])
                return PricingResult{{risk_measure::price, option.principal_ratio() + observation_coupon(option, expiry_index,
                                                                                                          context.asset_price().value())}};
            const double coupon = expiry_index < dates.size() && dates[expiry_index] == option.expiry() &&
                                  std::is_same_v<Option, PhoenixOption>
                                      ? observation_coupon(option, expiry_index, context.asset_price().value()) : 0.0;
            return PricingResult{{risk_measure::price, terminal_settlement(option, context.asset_price().value(), knocked_in) + coupon}};
        }
    }
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const int asset_steps = settings.asset_steps;
    const double upper = settings.upper_boundary > 0.0 ? settings.upper_boundary : std::max(4.0 * relevant, relevant + 1.0);
    const double spacing = upper / static_cast<double>(asset_steps);
    const double max_dt = maturity / static_cast<double>(settings.time_steps);
    std::vector<double> anchors{0.0, maturity};
    auto add_anchor = [&](date value) {
        const double time = actual_365(context.valuation_date(), value);
        if (time > 0.0 && time < maturity) anchors.push_back(time);
    };
    const auto future_trading_dates = trading_dates(context.calendar(), context.valuation_date(), option.expiry());
    std::vector<double> trading_times;
    trading_times.reserve(future_trading_dates.size());
    for (const date value : future_trading_dates)
        trading_times.push_back(actual_365(context.valuation_date(), value));
    if constexpr (std::is_same_v<Option, Accumulator>) {
        for (const date value : future_trading_dates) add_anchor(value);
    } else {
        for (const date value : option.observation_dates()) add_anchor(value);
        if constexpr (requires { option.knock_in_frequency(); })
            if (option.knock_in_frequency() == observation_frequency::daily)
                for (const date value : future_trading_dates) add_anchor(value);
    }
    std::sort(anchors.begin(), anchors.end());
    anchors.erase(std::unique(anchors.begin(), anchors.end()), anchors.end());
    std::vector<double> grid{0.0};
    for (std::size_t index = 1; index < anchors.size(); ++index) {
        const double width = anchors[index] - anchors[index - 1];
        const int pieces = std::max(1, static_cast<int>(std::ceil(width / max_dt)));
        for (int piece = 1; piece <= pieces; ++piece)
            grid.push_back(piece == pieces ? anchors[index] : anchors[index - 1] +
                           width * static_cast<double>(piece) / static_cast<double>(pieces));
    }
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
    auto advance = [&](const std::vector<double>& old, std::vector<double>& next, double dt) -> bool {
        next.front() = old.front() * std::exp(-rate * dt);
        const double high_slope = (old.back() - old[old.size() - 2]) / spacing;
        const double high_intercept = old.back() - high_slope * upper;
        next.back() = high_slope * upper * std::exp(-dividend * dt) +
                      high_intercept * std::exp(-rate * dt);
        std::vector<double> lower(asset_steps - 1), diagonal(asset_steps - 1), upper_diagonal(asset_steps - 1), rhs(asset_steps - 1);
        for (int index = 1; index < asset_steps; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * sigma * sigma * i * i - 0.5 * (rate - dividend) * i;
            const double b = -sigma * sigma * i * i - rate;
            const double c = 0.5 * sigma * sigma * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            rhs[position] = old[static_cast<std::size_t>(index)] + (1.0 - theta) * dt *
                (a * old[position] + b * old[static_cast<std::size_t>(index)] + c * old[static_cast<std::size_t>(index + 1)]);
            if (index == 1) rhs[position] += theta * dt * a * next.front();
            if (index == asset_steps - 1) rhs[position] += theta * dt * c * next.back();
            lower[position] = -theta * dt * a;
            diagonal[position] = 1.0 - theta * dt * b;
            upper_diagonal[position] = -theta * dt * c;
        }
        if (theta == 0.0) {
            std::copy(rhs.begin(), rhs.end(), next.begin() + 1);
            return std::ranges::all_of(next, [](double value) { return std::isfinite(value); });
        }
        for (std::size_t index = 1; index < diagonal.size(); ++index) {
            if (!std::isfinite(diagonal[index - 1]) || diagonal[index - 1] == 0.0) return false;
            const double factor = lower[index] / diagonal[index - 1];
            diagonal[index] -= factor * upper_diagonal[index - 1];
            rhs[index] -= factor * rhs[index - 1];
        }
        if (diagonal.empty() || !std::isfinite(diagonal.back()) || diagonal.back() == 0.0) return false;
        rhs.back() /= diagonal.back();
        for (std::size_t index = diagonal.size() - 1; index-- > 0;)
            rhs[index] = (rhs[index] - upper_diagonal[index] * rhs[index + 1]) / diagonal[index];
        if (!std::ranges::all_of(rhs, [](double value) { return std::isfinite(value); })) return false;
        std::copy(rhs.begin(), rhs.end(), next.begin() + 1);
        return true;
    };
    auto event_index = [&](double time) -> std::optional<std::size_t> {
        if constexpr (std::is_same_v<Option, Accumulator>) {
            (void)time;
            return std::nullopt;
        } else {
            const auto found = std::find_if(option.observation_dates().begin(), option.observation_dates().end(), [&](date value) {
                const double event_time = actual_365(context.valuation_date(), value);
                return event_time > 0.0 && event_time == time;
            });
            if (found == option.observation_dates().end()) return std::nullopt;
            return static_cast<std::size_t>(found - option.observation_dates().begin());
        }
    };
    auto daily_event = [&](double time) {
        if constexpr (std::is_same_v<Option, Accumulator>) {
            (void)time;
            return false;
        } else if constexpr (requires { option.knock_in_frequency(); }) {
            return option.knock_in_frequency() == observation_frequency::daily &&
                   std::any_of(future_trading_dates.begin(), future_trading_dates.end(), [&](date value) {
                                   return actual_365(context.valuation_date(), value) == time;
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
                const double coupon = expiry_observation && std::is_same_v<Option, PhoenixOption>
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
                    const double coupon = std::is_same_v<Option, PhoenixOption> ? observation_coupon(option, *observation, value) : 0.0;
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
