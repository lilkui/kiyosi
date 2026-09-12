#include <kiyosi/pricing/engines/digital/finite_difference.hpp>
#include "../../detail/common.hpp"
#include "../../detail/finite_difference.hpp"
#include <algorithm>
#include <cmath>
#include <ranges>
#include <vector>

namespace kiyosi {
using namespace detail;

namespace {
template <typename Option>
result<PricingResult> price_digital_fd(const Option& option, const PricingContext& context,
                                       FiniteDifferenceSettings settings, bool asset)
{
    const auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    auto settings_valid = validate_finite_difference_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings.asset_steps > 10'000 || settings.time_steps > 100'000)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});

    const double time = actual_365(context.valuation_time(), option.expiry());
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    const double payout = [&] {
        if constexpr (requires { option.payout(); }) return option.payout();
        else return 1.0;
    }();
    if (time == 0.0) {
        const bool in_the_money = sign * (spot - strike) > 0.0;
        return PricingResult{{risk_measure::price, in_the_money ? (asset ? spot : payout) : 0.0}};
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const int asset_steps = settings.asset_steps;
    const double upper = settings.upper_boundary > 0.0 ? settings.upper_boundary : std::max(4.0 * strike, 4.0 * spot);
    if (!std::isfinite(upper) || upper <= std::max(spot, strike))
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference upper boundary must exceed spot and strike"});
    const double spacing = upper / static_cast<double>(asset_steps);
    const int time_steps = settings.time_steps;
    const double dt = time / static_cast<double>(time_steps);
    if (settings.scheme == finite_difference_scheme::explicit_euler &&
        dt * (volatility * volatility * asset_steps * asset_steps + rate) > 1.0)
        return std::unexpected(Error{error_category::invalid_parameter, "explicit finite-difference grid is unstable"});
    const double theta = settings.scheme == finite_difference_scheme::explicit_euler ? 0.0 :
                         settings.scheme == finite_difference_scheme::implicit_euler ? 1.0 : 0.5;
    auto terminal = [&](double underlying) {
        // Average the discontinuous payoff over each cell to avoid strike-alignment bias.
        const double low = underlying - 0.5 * spacing;
        const double high = underlying + 0.5 * spacing;
        const double left = sign > 0.0 ? std::max(low, strike) : low;
        const double right = sign > 0.0 ? high : std::min(high, strike);
        if (left >= right) return 0.0;
        return (right - left) / spacing * (asset ? 0.5 * (left + right) : payout);
    };
    auto boundary = [&](double tau, bool high) {
        const bool call = option.type() == option_type::call;
        if (asset) return high && call ? upper * std::exp(-dividend * tau) : 0.0;
        const double discounted = payout * std::exp(-rate * tau);
        return high == call ? discounted : 0.0;
    };

    std::vector<double> old(static_cast<std::size_t>(asset_steps) + 1);
    std::vector<double> next(old.size());
    for (int index = 0; index <= asset_steps; ++index) old[static_cast<std::size_t>(index)] = terminal(spacing * index);
    FiniteDifferenceStep stepper(old.size());

    const auto grid = finite_difference_grid(time, time_steps);
    for (int step = 0; step < time_steps; ++step) {
        const double new_tau = grid[static_cast<std::size_t>(step + 1)];
        next.front() = boundary(new_tau, false);
        next.back() = boundary(new_tau, true);
        if (!stepper.advance(old, next, dt, rate, dividend, volatility, theta, next.front(), next.back()))
            return std::unexpected(Error{error_category::invalid_result,
                                         "finite-difference system is numerically unstable"});
        old.swap(next);
    }
    const double grid_position = spot / spacing;
    const int index = std::clamp(static_cast<int>(std::floor(grid_position)), 1, asset_steps - 1);
    const double weight = grid_position - static_cast<double>(index);
    const auto center = static_cast<std::size_t>(index);
    const double value = old[center] + weight * (old[center + 1] - old[center]);
    const auto right = std::min(center + 1, old.size() - 2);
    const auto node_delta = [&](std::size_t node) { return (old[node + 1] - old[node - 1]) / (2.0 * spacing); };
    const auto node_gamma = [&](std::size_t node) { return (old[node + 1] - 2.0 * old[node] + old[node - 1]) / (spacing * spacing); };
    const double delta = std::lerp(node_delta(center), node_delta(right), weight);
    const double gamma = std::lerp(node_gamma(center), node_gamma(right), weight);
    PricingResult output{{risk_measure::price, value}, {risk_measure::delta, delta}, {risk_measure::gamma, gamma}};
    if (!output.all_finite())
        return std::unexpected(Error{error_category::invalid_result, "finite-difference pricing produced a non-finite result"});
    return output;
}

}

result<PricingResult> FiniteDifferenceDigitalEngine::price(const EuropeanCashOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_fd(option, context, settings_, false);
}
result<PricingResult> FiniteDifferenceDigitalEngine::price(const EuropeanAssetOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_fd(option, context, settings_, true);
}

} // namespace kiyosi
