#include <kiyosi/pricing/engines/digital_fd.hpp>
#include "../detail/common.hpp"

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
    const auto valid = validate_life(context.valuation_date(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (settings.asset_steps < 3 || settings.asset_steps > 10'000 ||
        settings.time_steps <= 0 || settings.time_steps > 100'000)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});
    if (settings.upper_boundary != 0.0 &&
        (!std::isfinite(settings.upper_boundary) || settings.upper_boundary <= 0.0))
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference upper boundary must be finite and positive"});
    switch (settings.scheme) {
    case finite_difference_scheme::explicit_euler:
    case finite_difference_scheme::implicit_euler:
    case finite_difference_scheme::crank_nicolson:
        break;
    default:
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference scheme is invalid"});
    }

    const double time = actual_365(context.valuation_date(), option.expiry());
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
        return sign * (underlying - strike) > 0.0 ? (asset ? underlying : payout) : 0.0;
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
    std::vector<double> lower(old.size() - 2), diagonal(lower.size()), upper_diagonal(lower.size()), rhs(lower.size());
    auto solve = [&]() {
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
        const double new_tau = (static_cast<double>(step) + 1.0) * dt;
        next.front() = boundary(new_tau, false);
        next.back() = boundary(new_tau, true);
        for (int index = 1; index < asset_steps; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
            const double b = -volatility * volatility * i * i - rate;
            const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            rhs[position] = old[static_cast<std::size_t>(index)] + (1.0 - theta) * dt *
                (a * old[static_cast<std::size_t>(index - 1)] + b * old[static_cast<std::size_t>(index)] + c * old[static_cast<std::size_t>(index + 1)]);
            if (index == 1) rhs[position] += theta * dt * a * next.front();
            if (index == asset_steps - 1) rhs[position] += theta * dt * c * next.back();
            lower[position] = -theta * dt * a;
            diagonal[position] = 1.0 - theta * dt * b;
            upper_diagonal[position] = -theta * dt * c;
        }
        if (theta == 0.0) {
            for (std::size_t index = 0; index < rhs.size(); ++index) next[index + 1] = rhs[index];
        } else {
            if (!solve()) return std::unexpected(Error{error_category::invalid_result, "finite-difference system is numerically unstable"});
            for (std::size_t index = 0; index < rhs.size(); ++index) next[index + 1] = rhs[index];
        }
        old.swap(next);
    }
    const double grid_position = spot / spacing;
    const int index = std::clamp(static_cast<int>(std::floor(grid_position)), 1, asset_steps - 1);
    const double weight = grid_position - static_cast<double>(index);
    const auto center = static_cast<std::size_t>(index);
    const double value = old[center] + weight * (old[center + 1] - old[center]);
    const double delta = (old[center + 1] - old[center - 1]) / (2.0 * spacing);
    const double gamma = (old[center + 1] - 2.0 * old[center] + old[center - 1]) / (spacing * spacing);
    PricingResult output{{risk_measure::price, value}, {risk_measure::delta, delta}, {risk_measure::gamma, gamma}};
    if (!std::ranges::all_of(output.values, [](const auto& item) { return !item || std::isfinite(*item); }))
        return std::unexpected(Error{error_category::invalid_result, "finite-difference pricing produced a non-finite result"});
    return output;
}

result<PricingResult> price_digital_integral(option_type type, double strike, double payout, bool asset,
                                             date effective, date expiry, const PricingContext& context)
{
    const auto valid = validate_life(context.valuation_date(), effective, expiry);
    if (!valid) return std::unexpected(valid.error());
    const double time = actual_365(context.valuation_date(), expiry);
    const double spot = context.asset_price().value();
    const double sign = type == option_type::call ? 1.0 : -1.0;
    if (time == 0.0)
        return PricingResult{{risk_measure::price, sign * (spot - strike) > 0.0 ? (asset ? spot : payout) : 0.0}};
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const double root = std::sqrt(time);
    const double drift = (rate - dividend - 0.5 * volatility * volatility) * time;
    const double threshold = (std::log(strike / spot) - drift) / (volatility * root);
    const double lower = sign > 0.0 ? std::max(threshold, -12.0) : -12.0;
    const double upper = sign > 0.0 ? 12.0 : std::min(threshold, 12.0);
    if (lower >= upper) return PricingResult{{risk_measure::price, 0.0}};
    constexpr int panels = 2048;
    const double step = (upper - lower) / panels;
    auto integrand = [&](double z) {
        const double terminal = spot * std::exp(drift + volatility * root * z);
        return (sign * (terminal - strike) > 0.0 ? (asset ? terminal : payout) : 0.0) * normal_pdf(z);
    };
    double sum = integrand(lower) + integrand(upper);
    for (int index = 1; index < panels; ++index) sum += (index % 2 == 0 ? 2.0 : 4.0) * integrand(lower + index * step);
    const double value = std::exp(-rate * time) * sum * step / 3.0;
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "integral pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
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
result<PricingResult> IntegralDigitalEngine::price(const EuropeanCashOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_integral(option.type(), option.strike(), option.payout(), false, option.effective(), option.expiry(), context);
}
result<PricingResult> IntegralDigitalEngine::price(const EuropeanAssetOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_integral(option.type(), option.strike(), 1.0, true, option.effective(), option.expiry(), context);
}
} // namespace kiyosi
