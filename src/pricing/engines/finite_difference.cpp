#include <kiyosi/pricing/engines/finite_difference.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <ranges>
#include <vector>

namespace kiyosi {
using namespace detail;

template <typename Option>
result<PricingResult> price_finite_difference(
    const Option& option, const PricingContext& context,
    FiniteDifferenceSettings settings, bool american)
{
    const auto valid_expiry = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    if (settings.asset_steps < 3 || settings.asset_steps > 10'000 ||
        settings.time_steps <= 0 || settings.time_steps > 100'000) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    }
    if (settings.upper_boundary != 0.0 &&
        (!std::isfinite(settings.upper_boundary) || settings.upper_boundary <= 0.0)) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must be finite and positive"});
    }
    switch (settings.scheme) {
    case finite_difference_scheme::explicit_euler:
    case finite_difference_scheme::implicit_euler:
    case finite_difference_scheme::crank_nicolson:
        break;
    default:
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference scheme is invalid"});
    }

    const double time = actual_365(context.valuation_date(), option.expiry());
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    if (time == 0.0) {
        auto output = PricingResult{{risk_measure::price, std::max(sign * (spot - strike), 0.0)}};
        return output;
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const int asset_steps = settings.asset_steps;
    const double upper = settings.upper_boundary > 0.0
                             ? settings.upper_boundary
                             : std::max(4.0 * strike, 4.0 * spot);
    if (!std::isfinite(upper) || upper <= std::max(spot, strike)) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must exceed spot and strike"});
    }
    const double spacing = upper / static_cast<double>(asset_steps);
    int time_steps = settings.time_steps;
    if (settings.scheme == finite_difference_scheme::explicit_euler) {
        const double required = time * volatility * volatility * asset_steps * asset_steps * 1.1;
        if (!std::isfinite(required) || required > 100'000.0)
            return std::unexpected(Error{error_category::invalid_parameter,
                                         "explicit finite-difference grid requires too many time steps"});
        if (required > static_cast<double>(time_steps))
            time_steps = static_cast<int>(std::ceil(required));
    }
    const double dt = time / static_cast<double>(time_steps);
    const double theta = settings.scheme == finite_difference_scheme::explicit_euler   ? 0.0
                         : settings.scheme == finite_difference_scheme::implicit_euler ? 1.0
                                                                                       : 0.5;

    auto boundary = [&](double tau, bool high) {
        if (high) {
            if (option.type() == option_type::call)
                return american ? upper - strike : upper * std::exp(-dividend * tau) - strike * std::exp(-rate * tau);
            return 0.0;
        }
        if (option.type() == option_type::put)
            return american ? strike : strike * std::exp(-rate * tau);
        return 0.0;
    };
    auto intrinsic = [&](double underlying) {
        return std::max(sign * (underlying - strike), 0.0);
    };

    std::vector<double> old(static_cast<std::size_t>(asset_steps) + 1);
    std::vector<double> next(old.size());
    for (int index = 0; index <= asset_steps; ++index)
        old[static_cast<std::size_t>(index)] = intrinsic(spacing * index);

    std::vector<double> lower(static_cast<std::size_t>(asset_steps) - 1);
    std::vector<double> diagonal(lower.size());
    std::vector<double> upper_diagonal(lower.size());
    std::vector<double> rhs(lower.size());
    auto solve = [&]() -> bool {
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
        return std::ranges::all_of(rhs, [](double value) { return std::isfinite(value); });
    };

    for (int step = 0; step < time_steps; ++step) {
        const double old_tau = static_cast<double>(step) * dt;
        const double new_tau = old_tau + dt;
        next.front() = boundary(new_tau, false);
        next.back() = boundary(new_tau, true);
        for (int index = 1; index < asset_steps; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
            const double b = -volatility * volatility * i * i - rate;
            const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            rhs[position] = old[static_cast<std::size_t>(index)] + (1.0 - theta) * dt *
                                                                       (a * old[static_cast<std::size_t>(index - 1)] +
                                                                        b * old[static_cast<std::size_t>(index)] +
                                                                        c * old[static_cast<std::size_t>(index + 1)]);
            if (index == 1) rhs[position] += theta * dt * a * next.front();
            if (index == asset_steps - 1)
                rhs[position] += theta * dt * c * next.back();
            lower[position] = -theta * dt * a;
            diagonal[position] = 1.0 - theta * dt * b;
            upper_diagonal[position] = -theta * dt * c;
        }
        if (theta == 0.0) {
            for (std::size_t index = 0; index < rhs.size(); ++index)
                next[index + 1] = rhs[index];
        } else {
            const bool solved = solve();
            if (!solved) {
                return std::unexpected(Error{error_category::invalid_result,
                                             "finite-difference system is numerically unstable"});
            }
            for (std::size_t index = 0; index < rhs.size(); ++index)
                next[index + 1] = rhs[index];
        }
        if (american) {
            for (int index = 1; index < asset_steps; ++index)
                next[static_cast<std::size_t>(index)] =
                    std::max(next[static_cast<std::size_t>(index)], intrinsic(spacing * index));
        }
        old.swap(next);
    }

    const double grid_position = spot / spacing;
    const int index = std::clamp(static_cast<int>(std::floor(grid_position)), 1, asset_steps - 1);
    const double weight = grid_position - static_cast<double>(index);
    const auto center = static_cast<std::size_t>(index);
    const double value = old[center] + weight * (old[center + 1] - old[center]);
    const double delta = (old[center + 1] - old[center - 1]) / (2.0 * spacing);
    const double gamma = (old[center + 1] - 2.0 * old[center] + old[center - 1]) /
                         (spacing * spacing);
    const auto output = PricingResult{{risk_measure::price, value}, {risk_measure::delta, delta},
                                      {risk_measure::gamma, gamma}};
    if (!std::ranges::all_of(output.values, [](const auto& item) {
            return !item || std::isfinite(*item);
        }))
        return std::unexpected(Error{error_category::invalid_result,
                                     "finite-difference pricing produced a non-finite result"});
    return output;
}

result<PricingResult> FiniteDifferenceEuropeanEngine::price_impl(
    const EuropeanOption& option, const PricingContext& context, PricingRequest request) const
{
    return select_outputs(price_finite_difference(option, context, settings_, false),
                          request, supported_risk_measures);
}

result<PricingResult> FiniteDifferenceAmericanEngine::price_impl(
    const AmericanOption& option, const PricingContext& context, PricingRequest request) const
{
    return select_outputs(price_finite_difference(option, context, settings_, true),
                          request, supported_risk_measures);
}

} // namespace kiyosi
