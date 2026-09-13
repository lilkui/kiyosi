#pragma once

#include <algorithm>
#include <cmath>
#include <ranges>
#include <optional>
#include <span>
#include <utility>
#include <vector>

#include <kiyosi/pricing/engines/settings/finite_difference.hpp>

namespace kiyosi::detail {

inline std::vector<double> finite_difference_grid(double maturity, int steps, std::vector<double> events = {})
{
    for (int index = 0; index <= steps; ++index)
        events.push_back(maturity * index / steps);
    std::sort(events.begin(), events.end());
    events.erase(std::unique(events.begin(), events.end()), events.end());
    return events;
}

[[nodiscard]] inline double scheme_theta(finite_difference_scheme scheme) noexcept
{
    return scheme == finite_difference_scheme::explicit_euler ? 0.0
         : scheme == finite_difference_scheme::implicit_euler ? 1.0 : 0.5;
}

/// Rejects explicit-Euler grids whose largest step breaks positivity of the update at the top node.
[[nodiscard]] inline result<void> check_explicit_stability(
    finite_difference_scheme scheme, std::span<const double> grid, double volatility, double rate,
    int asset_steps)
{
    if (scheme != finite_difference_scheme::explicit_euler) return {};
    const auto steps = grid | std::views::adjacent_transform<2>(
                                  [](double start, double end) { return end - start; });
    const double nodes = static_cast<double>(asset_steps);
    if (std::ranges::max(steps) * (volatility * volatility * nodes * nodes + rate) > 1.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "explicit finite-difference grid is unstable"});
    return {};
}

/// Uniform asset grid over [0, upper] with spot lookup and node-difference sensitivities.
struct SpatialGrid {
    double upper;
    double spacing;
    int asset_steps;

    [[nodiscard]] std::size_t size() const noexcept { return static_cast<std::size_t>(asset_steps) + 1; }

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
        const int index = std::clamp(static_cast<int>(std::floor(position)), 0, asset_steps - 1);
        return {static_cast<std::size_t>(index), position - static_cast<double>(index)};
    }

    // Central differences need an interior stencil, so the bracketing nodes are pulled inside the edges.
    template <typename NodeValue>
    [[nodiscard]] double blend(double spot, NodeValue value) const
    {
        const auto [index, weight] = locate(spot);
        const auto interior = [&](std::size_t node) {
            return std::clamp(node, std::size_t{1}, static_cast<std::size_t>(asset_steps) - 1);
        };
        return std::lerp(value(interior(index)), value(interior(index + 1)), weight);
    }
};

/// Resolves the upper boundary from settings or the product default, rejecting grids that clip a product level.
[[nodiscard]] inline result<SpatialGrid> make_spatial_grid(
    const FiniteDifferenceSettings& settings, double default_upper,
    std::initializer_list<double> must_exceed)
{
    const double upper = settings.upper_boundary > 0.0 ? settings.upper_boundary : default_upper;
    if (!std::isfinite(upper) || upper <= std::ranges::max(must_exceed))
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must exceed every product level"});
    return SpatialGrid{upper, upper / static_cast<double>(settings.asset_steps), settings.asset_steps};
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

struct DiffusionParameters {
    double rate;
    double dividend;
    double volatility;
    double theta;
};

struct Boundaries {
    double lower;
    double upper;
};

/// Marches one value layer from expiry back to valuation; `tau` is the time remaining to expiry.
template <typename BoundaryValues,
          typename Event = decltype([](std::vector<double>&, double, double) {}),
          typename Constraint = decltype([](int, double) { return std::optional<double>{}; })>
[[nodiscard]] inline result<void> march_backward(
    std::span<const double> grid, const DiffusionParameters& parameters, std::vector<double>& layer,
    BoundaryValues boundaries, Event event = {}, Constraint constraint = {})
{
    const double maturity = grid.back();
    std::vector<double> next(layer.size());
    FiniteDifferenceStep stepper(layer.size());
    for (std::size_t step = grid.size() - 1; step-- > 0;) {
        const double dt = grid[step + 1] - grid[step];
        const double tau = maturity - grid[step];
        const Boundaries edges = boundaries(tau);
        if (!stepper.advance(layer, next, dt, parameters.rate, parameters.dividend,
                             parameters.volatility, parameters.theta, edges.lower, edges.upper,
                             [&](int index) { return constraint(index, tau); }))
            return std::unexpected(Error{error_category::invalid_result,
                                         "finite-difference system is numerically unstable"});
        event(next, tau, grid[step]);
        layer.swap(next);
    }
    return {};
}

} // namespace kiyosi::detail
