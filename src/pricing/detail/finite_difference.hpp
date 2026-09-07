#pragma once

#include <algorithm>
#include <cmath>
#include <ranges>
#include <vector>

namespace kiyosi::detail {

inline bool advance_finite_difference(
    const std::vector<double>& old, std::vector<double>& next, double dt, double rate,
    double dividend, double volatility, double theta, double lower_boundary, double upper_boundary)
{
    next.front() = lower_boundary;
    next.back() = upper_boundary;
    const int asset_steps = static_cast<int>(old.size()) - 1;
    std::vector<double> lower(old.size() - 2), diagonal(lower.size());
    std::vector<double> upper_diagonal(lower.size()), rhs(lower.size());
    for (int index = 1; index < asset_steps; ++index) {
        const double i = static_cast<double>(index);
        const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
        const double b = -volatility * volatility * i * i - rate;
        const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
        const auto position = static_cast<std::size_t>(index - 1);
        rhs[position] = old[static_cast<std::size_t>(index)] + (1.0 - theta) * dt *
            (a * old[position] + b * old[static_cast<std::size_t>(index)] +
             c * old[static_cast<std::size_t>(index + 1)]);
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
}

} // namespace kiyosi::detail
