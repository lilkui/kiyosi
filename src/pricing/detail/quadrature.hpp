#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <numeric>
#include <span>
#include <vector>

#include <oneapi/tbb/blocked_range.h>
#include <oneapi/tbb/parallel_for.h>
#include <oneapi/tbb/task_group.h>

#include "math.hpp"

namespace kiyosi::detail::quadrature {

using Polynomial = std::array<double, 4>;
inline constexpr double infinity = std::numeric_limits<double>::infinity();
// Their product exp(underflow_log_mass) * underflow_mean_limit rounds to zero.
inline constexpr double underflow_mean_limit = 1e20;
inline constexpr double underflow_log_mass = -800.0;

template <typename Evaluate>
void for_each_target(std::size_t count, const Evaluate& evaluate)
{
    constexpr std::size_t parallel_threshold = 128;
    constexpr std::size_t grain_size = 16;
    if (count < parallel_threshold) {
        for (std::size_t target = 0; target < count; ++target)
            evaluate(target);
        return;
    }
    // Outer task cancellation must not silently omit pricing rows.
    oneapi::tbb::task_group_context context{oneapi::tbb::task_group_context::isolated};
    oneapi::tbb::parallel_for(oneapi::tbb::blocked_range<std::size_t>{0, count, grain_size}, [&](const auto& range) {
                                for (auto target = range.begin(); target != range.end(); ++target) evaluate(target); }, context);
}

// Requires a validated asset step count and finite contract breaks.
inline Result<std::vector<double>> integration_nodes(double lower, double upper, int asset_step_count,
                                                     std::span<const double> breaks)
{
    if (!std::isfinite(lower) || !std::isfinite(upper) || lower >= upper)
        return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature integration domain is not representable"});
    std::vector<double> nodes;
    const auto steps = static_cast<std::size_t>(asset_step_count);
    nodes.reserve(steps + 1 + breaks.size());
    for (std::size_t index = 0; index <= steps; ++index)
        nodes.push_back(std::lerp(lower, upper, static_cast<double>(index) / static_cast<double>(steps)));
    nodes.insert(nodes.end(), breaks.begin(), breaks.end());
    std::ranges::sort(nodes);
    nodes.erase(std::ranges::unique(nodes).begin(), nodes.end());
    if (nodes.size() < 4)
        return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature grid has insufficient representable nodes"});
    return nodes;
}

// Asset prices at the geometric center of each cell in a validated log grid.
inline std::vector<double> cell_assets(const std::vector<double>& nodes, double anchor)
{
    std::vector<double> assets(nodes.size() - 1);
    for (std::size_t cell = 0; cell < assets.size(); ++cell)
        assets[cell] = anchor * std::exp(std::midpoint(nodes[cell], nodes[cell + 1]));
    return assets;
}

inline double log_interval_mass(double lower, double upper)
{
    if (lower >= upper) return -infinity;
    if (lower == -infinity) return log_normal_cdf(upper);
    if (upper == infinity) return log_normal_cdf(-lower);
    const double center = std::midpoint(lower, upper);
    const double half = 0.5 * (upper - lower);
    if (std::abs(center) * half + 0.5 * half * half < 0.25) {
        double integral = 0.0;
        double previous = 0.0;
        double term = 1.0;
        for (int order = 0; order < 24; ++order) {
            if (order % 2 == 0) integral += 2.0 * term / (order + 1.0);
            const double next = (-center * half * term - half * half * previous) / (order + 1.0);
            previous = term;
            term = next;
        }
        return -0.5 * center * center + std::log(inverse_sqrt_two_pi) + std::log(half) + std::log(integral);
    }
    const double high = lower >= 0.0 ? log_normal_cdf(-lower) : log_normal_cdf(upper);
    const double low = lower >= 0.0 ? log_normal_cdf(-upper) : log_normal_cdf(lower);
    return high + std::log(-std::expm1(low - high));
}

struct IntervalMoments {
    double mass{};
    double log_mass = -infinity;
    std::array<double, 3> conditional{};

    double integrate(const Polynomial& polynomial) const
    {
        if (log_mass < -1500.0) return 0.0;
        const double mean = polynomial[0] + polynomial[1] * conditional[0] +
                            polynomial[2] * conditional[1] + polynomial[3] * conditional[2];
        if (mass == 0.0 && std::abs(mean) < underflow_mean_limit && log_mass < underflow_log_mass) return 0.0;
        return mass > 0.0 ? mean * mass : scaled_exponential(mean, log_mass);
    }
};

// Moments of u=(x-left)/(right-left), conditional on x lying in this cell.
inline IntervalMoments cell_moments(double left, double right, double mean, double deviation)
{
    const double width = right - left;
    const double center = (std::midpoint(left, right) - mean) / deviation;
    const double half = (0.5 * width) / deviation;
    IntervalMoments result;
    if (half > 0.0 && std::abs(center) * half + 0.5 * half * half < 0.25) {
        // A centered density series avoids subtracting almost equal CDFs and raw moments.
        std::array<double, 4> integrals{};
        double previous = 0.0;
        double term = 1.0;
        for (int order = 0; order < 24; ++order) {
            for (int power = 0; power < 4; ++power)
                if ((order + power) % 2 == 0)
                    integrals[static_cast<std::size_t>(power)] += 2.0 * term / (order + power + 1.0);
            const double next = (-center * half * term - half * half * previous) / (order + 1.0);
            previous = term;
            term = next;
        }
        const double first = integrals[1] / integrals[0];
        const double second = integrals[2] / integrals[0];
        const double third = integrals[3] / integrals[0];
        result.log_mass = -0.5 * center * center + std::log(inverse_sqrt_two_pi) +
                          std::log(half) + std::log(integrals[0]);
        result.conditional = {0.5 * (1.0 + first), 0.25 * (1.0 + 2.0 * first + second),
                              0.125 * (1.0 + 3.0 * first + 3.0 * second + third)};
    } else {
        const double lower = (left - mean) / deviation;
        const double upper = (right - mean) / deviation;
        result.log_mass = log_interval_mass(lower, upper);
        // Even a largest finite coefficient cannot recover a representable contribution here.
        if (result.log_mass < -1500.0) return result;
        const auto weighted_density = [&](double z) {
            if (!std::isfinite(z)) return std::array<double, 3>{};
            const double density = std::exp(-0.5 * z * z + std::log(inverse_sqrt_two_pi) - result.log_mass);
            return std::array{density, z * density, (z * z + 2.0) * density};
        };
        const auto lower_density = weighted_density(lower);
        const auto upper_density = weighted_density(upper);
        const double first = lower_density[0] - upper_density[0];
        const double second = 1.0 + lower_density[1] - upper_density[1];
        const double third = lower_density[2] - upper_density[2];
        const double offset = (mean - left) / width;
        const double scale = deviation / width;
        result.conditional = {
            offset + scale * first,
            offset * offset + 2.0 * offset * scale * first + scale * scale * second,
            offset * offset * offset + 3.0 * offset * offset * scale * first +
                3.0 * offset * scale * scale * second + scale * scale * scale * third};
    }
    result.mass = std::exp(result.log_mass);
    return result;
}

struct Sum {
    double value{};
    double correction{};
    void add(double term)
    {
        const double next = value + term;
        correction += std::abs(value) >= std::abs(term) ? (value - next) + term : (term - next) + value;
        value = next;
    }
    double total() const { return value + correction; }
};

struct Curve {
    std::vector<Polynomial> cells;
    double low{};
    double high{};
};

inline Curve interpolate(const std::vector<double>& nodes, const std::vector<double>& values, double low, double high)
{
    Curve curve{{}, low, high};
    curve.cells.reserve(nodes.size() - 1);
    for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell) {
        const std::size_t first = std::min(cell == 0 ? 0 : cell - 1, nodes.size() - 4);
        const double width = nodes[cell + 1] - nodes[cell];
        std::array<double, 4> coordinates{}, differences{};
        for (std::size_t k = 0; k < 4; ++k) {
            coordinates[k] = (nodes[first + k] - nodes[cell]) / width;
            differences[k] = values[first + k];
        }
        for (std::size_t order = 1; order < 4; ++order)
            for (std::size_t k = 4; k-- > order;)
                differences[k] = (differences[k] - differences[k - 1]) / (coordinates[k] - coordinates[k - order]);
        Polynomial polynomial{differences[3], 0.0, 0.0, 0.0};
        for (std::size_t k = 3; k-- > 0;) {
            Polynomial next{};
            for (std::size_t power = 0; power < 4; ++power) {
                next[power] -= coordinates[k] * polynomial[power];
                if (power != 0) next[power] += polynomial[power - 1];
            }
            next[0] += differences[k];
            polynomial = next;
        }
        curve.cells.push_back(polynomial);
    }
    return curve;
}

struct Transition {
    struct Row {
        std::size_t first{};
        std::size_t offset{};
        std::size_t count{};
        friend bool operator==(const Row&, const Row&) = default;
    };
    std::vector<IntervalMoments> weights;
    std::vector<std::array<double, 2>> tails;
    std::vector<Row> rows;
};

inline Transition transition_weights(const std::vector<double>& nodes, double drift, double deviation)
{
    Transition transition;
    const std::size_t cells = nodes.size() - 1;
    transition.rows.reserve(nodes.size());
    std::size_t total = 0;
    std::size_t first_offset = 2 * cells, last_offset = 0;
    for (std::size_t target = 0; target < nodes.size(); ++target) {
        const double mean = nodes[target] + drift;
        // Beyond 56 standard deviations, the entire Gaussian tail is below the existing -1500 cutoff.
        const double radius = 56.0 * deviation;
        const auto lower = std::ranges::upper_bound(nodes, mean - radius) - nodes.begin();
        const auto upper = std::ranges::lower_bound(nodes, mean + radius) - nodes.begin();
        const auto first = std::min(cells, static_cast<std::size_t>(lower == 0 ? 0 : lower - 1));
        const auto last = std::min(cells, static_cast<std::size_t>(upper));
        const auto count = last - first;
        transition.rows.push_back({first, total, count});
        total += count;
        if (count != 0) {
            first_offset = std::min(first_offset, cells + first - target);
            last_offset = std::max(last_offset, cells + last - target);
        }
    }
    const auto geometry = [&](std::size_t cell, double mean) {
        return std::array{nodes[cell + 1] - nodes[cell], std::midpoint(nodes[cell], nodes[cell + 1]) - mean,
                          nodes[cell] - mean, nodes[cell + 1] - mean};
    };
    struct CachedMoments {
        std::array<double, 4> geometry{};
        IntervalMoments moments;
    };
    // Reuse only exactly translated cells, including the midpoint's floating-point rounding.
    std::vector<CachedMoments> stencil(last_offset > first_offset ? last_offset - first_offset : 0);
    for_each_target(stencil.size(), [&](std::size_t index) {
        const auto offset = first_offset + index;
        const auto cell = offset >= cells ? offset - cells : 0;
        const auto target = offset < cells ? cells - offset : 0;
        const double mean = nodes[target] + drift;
        stencil[index] = {geometry(cell, mean), cell_moments(nodes[cell], nodes[cell + 1], mean, deviation)};
    });
    transition.weights.resize(total);
    transition.tails.resize(nodes.size());
    for_each_target(nodes.size(), [&](std::size_t target) {
        const double mean = nodes[target] + drift;
        transition.tails[target] = {log_normal_cdf((nodes.front() - mean) / deviation),
                                    log_normal_cdf((mean - nodes.back()) / deviation)};
        const auto row = transition.rows[target];
        for (std::size_t index = 0; index < row.count; ++index) {
            const auto cell = row.first + index;
            const auto& cached = stencil[cells + cell - target - first_offset];
            transition.weights[row.offset + index] = geometry(cell, mean) == cached.geometry
                                                         ? cached.moments
                                                         : cell_moments(nodes[cell], nodes[cell + 1], mean, deviation);
        }
    });
    return transition;
}

inline double integrate_curve(const Curve& curve, std::span<const IntervalMoments> weights,
                              const std::array<double, 2>& tails, double log_discount, std::size_t first_cell = 0)
{
    Sum sum;
    sum.add(scaled_exponential(curve.low, tails[0] + log_discount));
    sum.add(scaled_exponential(curve.high, tails[1] + log_discount));
    const double discount = std::exp(log_discount);
    for (std::size_t cell = 0; cell < weights.size(); ++cell)
        sum.add(std::isnormal(discount) ? weights[cell].integrate(curve.cells[first_cell + cell]) * discount : scaled_exponential(weights[cell].integrate(curve.cells[first_cell + cell]), log_discount));
    return sum.total();
}

inline void integrate_curves(const Curve& curve, const Transition& transition, double log_discount,
                             std::span<double> values)
{
    for_each_target(values.size(), [&](std::size_t target) {
        const auto row = transition.rows[target];
        values[target] = integrate_curve(curve, std::span{transition.weights}.subspan(row.offset, row.count),
                                         transition.tails[target], log_discount, row.first);
    });
}

// Both layers share each loaded weight while retaining their independent summation order.
inline void integrate_curve_pair(const Curve& first, const Curve& second, const Transition& transition,
                                 double log_discount, std::span<double> first_values, std::span<double> second_values)
{
    const double discount = std::exp(log_discount);
    const bool direct_discount = std::isnormal(discount);
    for_each_target(first_values.size(), [&](std::size_t target) {
        const auto row = transition.rows[target];
        const auto weights = std::span{transition.weights}.subspan(row.offset, row.count);
        const auto tails = transition.tails[target];
        Sum first_sum, second_sum;
        first_sum.add(scaled_exponential(first.low, tails[0] + log_discount));
        first_sum.add(scaled_exponential(first.high, tails[1] + log_discount));
        second_sum.add(scaled_exponential(second.low, tails[0] + log_discount));
        second_sum.add(scaled_exponential(second.high, tails[1] + log_discount));
        for (std::size_t cell = 0; cell < weights.size(); ++cell) {
            const double first_value = weights[cell].integrate(first.cells[row.first + cell]);
            const double second_value = weights[cell].integrate(second.cells[row.first + cell]);
            first_sum.add(direct_discount ? first_value * discount : scaled_exponential(first_value, log_discount));
            second_sum.add(direct_discount ? second_value * discount : scaled_exponential(second_value, log_discount));
        }
        first_values[target] = first_sum.total();
        second_values[target] = second_sum.total();
    });
}

// The terminal function is affine in S on each exact contractual interval.
struct TerminalPiece {
    double lower;
    double upper;
    double cash;
    double asset;
};

inline double integrate_terminal(const std::vector<TerminalPiece>& pieces, double log_spot, double anchor,
                                 double drift, double deviation, double log_discount)
{
    Sum sum;
    const double mean = log_spot + drift;
    for (const auto& piece : pieces) {
        const double lower = (piece.lower - mean) / deviation;
        const double upper = (piece.upper - mean) / deviation;
        if (piece.cash != 0.0)
            sum.add(scaled_exponential(piece.cash, log_discount + log_interval_mass(lower, upper)));
        if (piece.asset != 0.0)
            sum.add(scaled_exponential(piece.asset, std::log(anchor) + mean + 0.5 * deviation * deviation +
                                                        log_discount + log_interval_mass(lower - deviation, upper - deviation)));
    }
    return sum.total();
}

} // namespace kiyosi::detail::quadrature
