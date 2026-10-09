#pragma once

#include <algorithm>
#include <cmath>
#include <initializer_list>
#include <ranges>
#include <span>
#include <utility>
#include <vector>

#include <kiyosi/core/error.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi::detail {

/// Uniform time grid merged with the supplied event times, deduplicated and sorted.
inline std::vector<double> make_finite_difference_time_grid(double maturity, int steps, std::vector<double> events = {})
{
    // Preserve exact endpoints: rounding maturity * steps / steps can add a node
    // beyond expiry_date and cause event-driven engines to apply terminal cashflows twice.
    events.push_back(0.0);
    events.push_back(maturity);
    for (int index = 1; index < steps; ++index)
        events.push_back(maturity * index / steps);
    std::ranges::sort(events);
    events.erase(std::ranges::unique(events).begin(), events.end());
    return events;
}

[[nodiscard]] inline double scheme_theta(FiniteDifferenceScheme scheme) noexcept
{
    return scheme == FiniteDifferenceScheme::explicit_euler   ? 0.0
           : scheme == FiniteDifferenceScheme::implicit_euler ? 1.0
                                                              : 0.5;
}

/// Rejects explicit-Euler grids whose largest step breaks positivity of the update at the top node.
[[nodiscard]] inline Result<void> check_explicit_stability(
    FiniteDifferenceScheme scheme, std::span<const double> grid, double volatility, double rate, double dividend,
    int asset_step_count)
{
    if (scheme != FiniteDifferenceScheme::explicit_euler) return {};
    const auto steps = grid | std::views::adjacent_transform<2>(
                                  [](double start, double end) { return end - start; });
    const double nodes = static_cast<double>(asset_step_count);
    const double diffusion = std::max(volatility * volatility * nodes * nodes, std::abs(rate - dividend) * nodes);
    if (std::ranges::max(steps) * (diffusion + rate) > 1.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "explicit finite-difference grid is unstable"});
    return {};
}

/// Uniform asset grid over [0, upper] with spot lookup and node-difference sensitivities.
struct SpatialGrid {
    double upper;
    double spacing;
    int asset_step_count;

    [[nodiscard]] std::size_t size() const noexcept { return static_cast<std::size_t>(asset_step_count) + 1; }

    [[nodiscard]] double interpolate(std::span<const double> values, double spot) const
    {
        const auto [index, weight] = locate(spot);
        // A C2 spline preserves curvature when numerical Greek bumps stay inside one cell.
        std::vector<double> curvature(values.size()), factors(values.size());
        for (std::size_t node = 1; node + 1 < values.size(); ++node) {
            const double diagonal = 4.0 - factors[node - 1];
            factors[node] = 1.0 / diagonal;
            curvature[node] = (6.0 * ((values[node + 1] - values[node]) -
                                      (values[node] - values[node - 1])) -
                               curvature[node - 1]) /
                              diagonal;
        }
        for (std::size_t node = values.size() - 2; node > 0; --node)
            curvature[node] -= factors[node] * curvature[node + 1];
        const double left = 1.0 - weight;
        const double interpolated = std::lerp(values[index], values[index + 1], weight) +
                                    ((left * left * left - left) * curvature[index] +
                                     (weight * weight * weight - weight) * curvature[index + 1]) /
                                        6.0;
        // Bound spline overshoot near payoff kinks without losing smooth-region curvature.
        return std::clamp(interpolated, std::min(values[index], values[index + 1]),
                          std::max(values[index], values[index + 1]));
    }

    [[nodiscard]] double delta(std::span<const double> values, double spot) const
    {
        return blend(spot, [&](std::size_t index) {
            return (values[index + 1] - values[index - 1]) / (2.0 * spacing);
        });
    }

    [[nodiscard]] double gamma(std::span<const double> values, double spot) const
    {
        return blend(spot, [&](std::size_t index) {
            // Divide separately so squared spacing cannot overflow or underflow.
            return (values[index + 1] - 2.0 * values[index] + values[index - 1]) / spacing / spacing;
        });
    }

private:
    [[nodiscard]] std::pair<std::size_t, double> locate(double spot) const noexcept
    {
        const double position = spot / spacing;
        const int index = std::clamp(static_cast<int>(std::floor(position)), 0, asset_step_count - 1);
        return {static_cast<std::size_t>(index), position - static_cast<double>(index)};
    }

    // Central differences need an interior stencil, so the bracketing nodes are pulled inside the edges.
    template <typename NodeValue>
    [[nodiscard]] double blend(double spot, NodeValue value) const
    {
        const auto [index, weight] = locate(spot);
        const auto interior = [&](std::size_t node) {
            return std::clamp(node, std::size_t{1}, static_cast<std::size_t>(asset_step_count) - 1);
        };
        return std::lerp(value(interior(index)), value(interior(index + 1)), weight);
    }
};

/// Resolves the upper boundary from settings or the product default, rejecting grids that clip a product level.
[[nodiscard]] inline Result<SpatialGrid> make_spatial_grid(
    const FiniteDifferenceSettings& settings, double default_upper,
    std::initializer_list<double> must_exceed)
{
    const double upper = settings.asset_upper_boundary.value_or(default_upper);
    if (!std::isfinite(upper) || upper <= std::ranges::max(must_exceed))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference upper boundary must exceed every product level"});
    const double spacing = upper / static_cast<double>(settings.asset_step_count);
    if (!std::isfinite(spacing) || spacing <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference spacing must be finite and positive"});
    return SpatialGrid{upper, spacing, settings.asset_step_count};
}

} // namespace kiyosi::detail
