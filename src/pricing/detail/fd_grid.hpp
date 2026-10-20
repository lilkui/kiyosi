#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <initializer_list>
#include <limits>
#include <numbers>
#include <optional>
#include <ranges>
#include <span>
#include <utility>
#include <vector>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/day_count.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi::detail {

template <typename Price, typename Gamma>
Result<std::optional<double>> finite_difference_speed(
    double spot, double h, double upper, const Price& price_at, const Gamma& gamma_at)
{
    // Unavailable stencils are left to the shared Greek completion path.
    const bool stencil_available = h > 0.0 && spot > h && spot + h < upper;
    if (!stencil_available) return std::nullopt;
    for (const double shifted_spot : {spot + h, spot - h}) {
        const auto price = price_at(shifted_spot);
        if (!price) return std::unexpected(price.error());
    }
    return std::optional{(gamma_at(spot + h) - gamma_at(spot - h)) / (2.0 * h)};
}

template <typename Price, typename Delta, typename Gamma>
Result<PricingResult> finite_difference_result(
    double value, double spot, double upper, GreeksRequest output, double spot_shift,
    const Price& price_at, const Delta& delta_at, const Gamma& gamma_at)
{
    const double h = output.has(Greek::speed) ? symmetric_shift(spot, spot_shift) : 0.0;
    const auto speed = finite_difference_speed(spot, h, upper, price_at, gamma_at);
    if (!speed) return std::unexpected(speed.error());
    return make_pricing_result(value,
                               {{Greek::delta, output.has(Greek::delta) ? std::optional{delta_at(spot)} : std::nullopt},
                                {Greek::gamma, output.has(Greek::gamma) ? std::optional{gamma_at(spot)} : std::nullopt},
                                {Greek::speed, *speed}});
}

struct FiniteDifferenceTimeGrid {
    std::vector<double> times;
    std::vector<double> time_steps;
};

/// Exact event times with canonical widths for unsplit uniform intervals.
inline FiniteDifferenceTimeGrid make_finite_difference_time_grid(double maturity, int steps, std::vector<double> events = {})
{
    const double uniform_dt = maturity / steps;
    // Zero or subnormal widths can collapse distinct uniform nodes and still need deduplication.
    if (events.empty() && maturity > 0.0 && std::isnormal(uniform_dt)) {
        FiniteDifferenceTimeGrid grid;
        grid.times.resize(static_cast<std::size_t>(steps) + 1);
        grid.time_steps.assign(static_cast<std::size_t>(steps), uniform_dt);
        for (int index = 1; index < steps; ++index)
            grid.times[static_cast<std::size_t>(index)] = maturity * index / steps;
        grid.times.back() = maturity;
        return grid;
    }
    // Preserve exact endpoints: rounding maturity * steps / steps can add a node
    // beyond expiry_date and cause event-driven engines to apply terminal cashflows twice.
    events.push_back(0.0);
    events.push_back(maturity);
    std::ranges::sort(events);
    const std::size_t event_count = events.size();
    events.reserve(event_count + static_cast<std::size_t>(steps) - 1);
    const double tolerance = 4.0 * std::numeric_limits<double>::epsilon() * maturity;
    std::size_t next_event = 0;
    for (int index = 1; index < steps; ++index) {
        const double time = maturity * index / steps;
        while (next_event < event_count && events[next_event] < time)
            ++next_event;
        // Preserve every event; omit only uniform nodes within rounding error of an event.
        if ((next_event < event_count && std::abs(events[next_event] - time) <= tolerance) ||
            (next_event > 0 && std::abs(events[next_event - 1] - time) <= tolerance)) continue;
        events.push_back(time);
    }
    std::ranges::sort(events);
    events.erase(std::ranges::unique(events).begin(), events.end());
    FiniteDifferenceTimeGrid grid{std::move(events), {}};
    grid.time_steps.reserve(grid.times.size() - 1);
    int uniform_index = 1;
    double uniform_start = 0.0;
    for (std::size_t index = 0; index + 1 < grid.times.size(); ++index) {
        const double start = grid.times[index], end = grid.times[index + 1];
        double uniform_end = uniform_index == steps ? maturity : maturity * uniform_index / steps;
        while (uniform_index < steps && uniform_end < end) {
            uniform_start = uniform_end;
            ++uniform_index;
            uniform_end = uniform_index == steps ? maturity : maturity * uniform_index / steps;
        }
        // Subtraction rounds equal uniform intervals differently and defeats factor reuse.
        grid.time_steps.push_back(start == uniform_start && end == uniform_end ? uniform_dt : end - start);
        if (end == uniform_end && uniform_index < steps) {
            uniform_start = uniform_end;
            ++uniform_index;
        }
    }
    return grid;
}

/// Calendar anchors retain their exact durations instead of subtracting rounded year fractions.
inline FiniteDifferenceTimeGrid make_finite_difference_time_grid(
    Timestamp valuation, Date expiry, int steps, std::vector<Date> events)
{
    events.push_back(expiry);
    std::ranges::sort(events);
    events.erase(std::ranges::unique(events).begin(), events.end());
    std::vector<double> times{0.0};
    std::vector<Timestamp> anchors{valuation};
    for (const Date event : events) {
        if (event <= valuation) continue;
        times.push_back(actual_365_fixed_year_fraction(valuation, event));
        anchors.push_back(start_of_day(event));
    }
    auto grid = make_finite_difference_time_grid(
        actual_365_fixed_year_fraction(valuation, expiry), steps, times);
    std::size_t event = 0;
    for (std::size_t index = 0; index < grid.time_steps.size(); ++index) {
        while (event + 1 < times.size() && times[event] < grid.times[index])
            ++event;
        if (event + 1 < times.size() && times[event] == grid.times[index] &&
            times[event + 1] == grid.times[index + 1])
            grid.time_steps[index] = actual_365_fixed_year_fraction(anchors[event], anchors[event + 1]);
    }
    return grid;
}

/// Uniformly replaces [0, end] when it has fewer than minimum_steps intervals, preserving the suffix exactly.
/// Requires a positive existing end, no required events before end, and positive minimum_steps.
inline void refine_initial_finite_difference_time_grid(
    FiniteDifferenceTimeGrid& grid, double end, int minimum_steps)
{
    const auto first_event = std::ranges::lower_bound(grid.times, end);
    const auto intervals = first_event - grid.times.begin();
    if (intervals >= minimum_steps) return;
    FiniteDifferenceTimeGrid refined;
    const double width = end / minimum_steps;
    refined.times.reserve(grid.times.size() + minimum_steps);
    refined.time_steps.assign(minimum_steps, width);
    for (int step = 0; step < minimum_steps; ++step)
        refined.times.push_back(end * step / minimum_steps);
    refined.times.insert(refined.times.end(), first_event, grid.times.end());
    refined.time_steps.insert(refined.time_steps.end(), grid.time_steps.begin() + intervals, grid.time_steps.end());
    grid = std::move(refined);
}

[[nodiscard]] inline double scheme_theta(FiniteDifferenceScheme scheme) noexcept
{
    return scheme == FiniteDifferenceScheme::explicit_euler   ? 0.0
           : scheme == FiniteDifferenceScheme::implicit_euler ? 1.0
                                                              : 0.5;
}

/// Rejects explicit-Euler grids whose largest step breaks positivity of the update at the top node.
[[nodiscard]] inline Result<void> check_explicit_stability(
    FiniteDifferenceScheme scheme, const FiniteDifferenceTimeGrid& grid, double volatility, double rate, double dividend,
    int asset_step_count)
{
    if (scheme != FiniteDifferenceScheme::explicit_euler) return {};
    const double nodes = static_cast<double>(asset_step_count);
    const double diffusion = std::max(volatility * volatility * nodes * nodes, std::abs(rate - dividend) * nodes);
    if (std::ranges::max(grid.time_steps) * (diffusion + rate) > 1.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "explicit finite-difference grid is unstable"});
    return {};
}

/// Uniform S or asinh(S / sinh_scale) grid with sensitivities in asset-price units.
struct SpatialGrid {
    double upper;
    double spacing;
    int asset_step_count;
    double sinh_scale = 0.0;
    std::vector<double> nodes{};

    [[nodiscard]] std::size_t size() const noexcept { return static_cast<std::size_t>(asset_step_count) + 1; }

    [[nodiscard]] double coordinate_of(double asset) const
    {
        if (sinh_scale == 0.0) return asset;
        const double ratio = asset / sinh_scale;
        return std::isfinite(ratio) ? std::asinh(ratio) : std::log(asset) - std::log(sinh_scale) + std::numbers::ln2;
    }

    [[nodiscard]] double asset_at(std::size_t index) const
    {
        if (!nodes.empty()) return nodes[index];
        if (std::cmp_equal(index, asset_step_count)) return upper;
        return asset_of_coordinate(spacing * static_cast<double>(index));
    }

    [[nodiscard]] double asset_of_coordinate(double x) const
    {
        if (sinh_scale == 0.0) return x;
        return x > 700.0 ? std::exp(std::log(sinh_scale) + x - std::numbers::ln2) : sinh_scale * std::sinh(x);
    }

    [[nodiscard]] double coordinate_at(std::size_t index) const
    {
        return nodes.empty() ? spacing * static_cast<double>(index) : nodes[index];
    }

    [[nodiscard]] double linear_interpolate(std::span<const double> values, double spot) const
    {
        const auto [index, weight] = locate(spot);
        return std::lerp(values[index], values[index + 1], weight);
    }

    [[nodiscard]] std::size_t nearest_node(double spot) const noexcept
    {
        const auto [index, weight] = locate(spot);
        return index + (weight > 0.5 ? 1 : 0);
    }

    [[nodiscard]] double interpolate(std::span<const double> values, double spot) const
    {
        std::vector<double> curvature;
        return interpolate(values, spot, curvature);
    }

    /// Reuse curvature only for the same grid and unchanged values; start with an empty buffer.
    [[nodiscard]] double interpolate(std::span<const double> values, double spot, std::vector<double>& curvature) const
    {
        if (!nodes.empty()) return sample(values, spot, curvature)[0];
        const auto [index, weight] = locate(spot);
        if (weight == 0.0) return values[index];
        if (weight == 1.0) return values[index + 1];
        // A C2 spline preserves curvature when numerical Greek bumps stay inside one cell.
        if (curvature.empty()) {
            curvature.resize(values.size());
            std::vector<double> factors(values.size());
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
        }
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
        if (!nodes.empty()) {
            std::vector<double> curvature;
            return sample(values, spot, curvature)[1];
        }
        const double derivative = coordinate_delta(values, spot);
        return sinh_scale == 0.0 ? derivative : derivative / std::hypot(sinh_scale, spot);
    }

    [[nodiscard]] double gamma(std::span<const double> values, double spot) const
    {
        if (!nodes.empty()) {
            std::vector<double> curvature;
            return sample(values, spot, curvature)[2];
        }
        const double curvature = blend(spot, [&](std::size_t index) {
            // Divide separately so squared spacing cannot overflow or underflow.
            return ((values[index + 1] - values[index]) - (values[index] - values[index - 1])) / spacing / spacing;
        });
        if (sinh_scale == 0.0) return curvature;
        const double jacobian = std::hypot(sinh_scale, spot);
        return (curvature - coordinate_delta(values, spot) * std::tanh(coordinate_of(spot))) / jacobian / jacobian;
    }

    [[nodiscard]] double delta(std::span<const double> values, double spot, std::vector<double>& curvature) const
    {
        return nodes.empty() ? delta(values, spot) : sample(values, spot, curvature)[1];
    }

    [[nodiscard]] double gamma(std::span<const double> values, double spot, std::vector<double>& curvature) const
    {
        return nodes.empty() ? gamma(values, spot) : sample(values, spot, curvature)[2];
    }

private:
    [[nodiscard]] std::array<double, 3> sample(std::span<const double> values, double spot,
                                               std::vector<double>& curvature) const
    {
        if (curvature.empty()) {
            curvature.assign(values.size(), 0.0);
            std::vector<double> factors(values.size());
            for (std::size_t node = 1; node + 1 < values.size(); ++node) {
                const double left = nodes[node] - nodes[node - 1];
                const double right = nodes[node + 1] - nodes[node];
                const double total = nodes[node + 1] - nodes[node - 1];
                const double a = left / total;
                const double diagonal = 2.0 - a * factors[node - 1];
                factors[node] = (right / total) / diagonal;
                curvature[node] = (6.0 * (((values[node + 1] - values[node]) / right -
                                           (values[node] - values[node - 1]) / left) /
                                          total) -
                                   a * curvature[node - 1]) /
                                  diagonal;
            }
            for (std::size_t node = values.size() - 2; node > 0; --node)
                curvature[node] -= factors[node] * curvature[node + 1];
        }
        const auto [index, weight] = locate(spot);
        const double h = nodes[index + 1] - nodes[index];
        const double left = 1.0 - weight;
        const double correction = ((left * left * left - left) * curvature[index] +
                                   (weight * weight * weight - weight) * curvature[index + 1]) /
                                  6.0;
        const double linear = std::lerp(values[index], values[index + 1], weight);
        const double first_control = std::lerp(values[index], values[index + 1], 1.0 / 3.0) -
                                     ((2.0 * curvature[index] + curvature[index + 1]) * h) * h / 18.0;
        const double second_control = std::lerp(values[index], values[index + 1], 2.0 / 3.0) -
                                      ((curvature[index] + 2.0 * curvature[index + 1]) * h) * h / 18.0;
        const auto [control_lower, control_upper] = std::minmax(first_control, second_control);
        const auto [minimum, maximum] = std::minmax(values[index], values[index + 1]);
        if (control_lower < minimum || control_upper > maximum) {
            const auto [lower, upper] = std::ranges::minmax(values);
            // Bezier control points bound the whole cell; keep unsafe cells linear and continuous.
            if (control_lower < lower || control_upper > upper)
                return {linear, (values[index + 1] - values[index]) / h, 0.0};
        }
        return {linear + (correction * h) * h,
                (values[index + 1] - values[index]) / h +
                    h * ((1.0 - 3.0 * left * left) * curvature[index] + (3.0 * weight * weight - 1.0) * curvature[index + 1]) / 6.0,
                std::lerp(curvature[index], curvature[index + 1], weight)};
    }

    [[nodiscard]] double coordinate_delta(std::span<const double> values, double spot) const
    {
        return blend(spot, [&](std::size_t index) {
            return (values[index + 1] - values[index - 1]) / (2.0 * spacing);
        });
    }

    [[nodiscard]] std::pair<std::size_t, double> locate(double spot) const noexcept
    {
        if (!nodes.empty()) {
            const auto found = std::ranges::upper_bound(nodes, spot);
            const auto index = static_cast<std::size_t>(std::clamp(found - nodes.begin() - 1,
                                                                   std::ptrdiff_t{0}, static_cast<std::ptrdiff_t>(nodes.size() - 2)));
            return {index, (spot - nodes[index]) / (nodes[index + 1] - nodes[index])};
        }
        const double position = coordinate_of(spot) / spacing;
        const auto index = static_cast<int>(std::clamp(std::floor(position), 0.0, static_cast<double>(asset_step_count - 1)));
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
    std::initializer_list<double> must_exceed, double sinh_scale = 0.0)
{
    const double upper = settings.asset_upper_boundary.value_or(default_upper);
    if (!std::isfinite(upper) || upper <= std::ranges::max(must_exceed))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference upper boundary must exceed every product level"});
    SpatialGrid grid{upper, 0.0, settings.asset_step_count, sinh_scale};
    const double spacing = grid.coordinate_of(upper) / static_cast<double>(settings.asset_step_count);
    if (!std::isfinite(spacing) || spacing <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference spacing must be finite and positive"});
    grid.spacing = spacing;
    return grid;
}

} // namespace kiyosi::detail
