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
    std::sort(events.begin(), events.end());
    events.erase(std::unique(events.begin(), events.end()), events.end());
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
    FiniteDifferenceScheme scheme, std::span<const double> grid, double volatility, double rate,
    int asset_step_count)
{
    if (scheme != FiniteDifferenceScheme::explicit_euler) return {};
    const auto steps = grid | std::views::adjacent_transform<2>(
                                  [](double start, double end) { return end - start; });
    const double nodes = static_cast<double>(asset_step_count);
    if (std::ranges::max(steps) * (volatility * volatility * nodes * nodes + rate) > 1.0)
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
        return std::lerp(values[index], values[index + 1], weight);
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
            return (values[index + 1] - 2.0 * values[index] + values[index - 1]) / (spacing * spacing);
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
    return SpatialGrid{upper, upper / static_cast<double>(settings.asset_step_count), settings.asset_step_count};
}

} // namespace kiyosi::detail
