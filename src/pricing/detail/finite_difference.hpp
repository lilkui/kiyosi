#pragma once

#include <algorithm>
#include <cmath>
#include <ranges>
#include <optional>
#include <vector>

namespace kiyosi::detail {

inline std::vector<double> finite_difference_grid(double maturity, int steps, std::vector<double> events = {})
{
    for (int index = 0; index <= steps; ++index)
        events.push_back(maturity * index / steps);
    std::sort(events.begin(), events.end());
    events.erase(std::unique(events.begin(), events.end()), events.end());
    return events;
}

class FiniteDifferenceStep {
public:
    explicit FiniteDifferenceStep(std::size_t size)
        : lower(size - 2), diagonal(size - 2), upper_diagonal(size - 2), rhs(size - 2) {}

    template <typename Constraint>
    bool advance(
        const std::vector<double>& old, std::vector<double>& next, double dt, double rate,
        double dividend, double volatility, double theta, double lower_boundary, double upper_boundary,
        Constraint constraint)
    {
        next.front() = lower_boundary;
        next.back() = upper_boundary;
        const int asset_steps = static_cast<int>(old.size()) - 1;
        for (int index = 1; index < asset_steps; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
            const double b = -volatility * volatility * i * i - rate;
            const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            if (const auto fixed = constraint(index)) {
                lower[position] = upper_diagonal[position] = 0.0;
                diagonal[position] = 1.0;
                rhs[position] = *fixed;
                continue;
            }
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
        return std::ranges::all_of(next, [](double value) { return std::isfinite(value); });
    }

    bool advance(const std::vector<double>& old, std::vector<double>& next, double dt, double rate,
                 double dividend, double volatility, double theta, double lower_boundary, double upper_boundary)
    {
        return advance(old, next, dt, rate, dividend, volatility, theta, lower_boundary, upper_boundary,
                       [](int) -> std::optional<double> { return std::nullopt; });
    }

private:
    std::vector<double> lower, diagonal, upper_diagonal, rhs;
};

} // namespace kiyosi::detail
