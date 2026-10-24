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
    std::vector<IntervalMoments> weights;
    std::vector<std::array<double, 2>> tails;
};

inline Transition transition_weights(const std::vector<double>& nodes, double drift, double deviation)
{
    Transition transition;
    const std::size_t cells = nodes.size() - 1;
    transition.weights.resize(nodes.size() * cells);
    transition.tails.resize(nodes.size());
    for_each_target(nodes.size(), [&](std::size_t target) {
        const double mean = nodes[target] + drift;
        transition.tails[target] = {log_normal_cdf((nodes.front() - mean) / deviation),
                                    log_normal_cdf((mean - nodes.back()) / deviation)};
        for (std::size_t cell = 0; cell < cells; ++cell)
            transition.weights[target * cells + cell] = cell_moments(nodes[cell], nodes[cell + 1], mean, deviation);
    });
    return transition;
}

inline double integrate_curve(const Curve& curve, std::span<const IntervalMoments> weights,
                              const std::array<double, 2>& tails, double log_discount)
{
    Sum sum;
    sum.add(scaled_exponential(curve.low, tails[0] + log_discount));
    sum.add(scaled_exponential(curve.high, tails[1] + log_discount));
    const double discount = std::exp(log_discount);
    for (std::size_t cell = 0; cell < curve.cells.size(); ++cell)
        sum.add(std::isnormal(discount) ? weights[cell].integrate(curve.cells[cell]) * discount : scaled_exponential(weights[cell].integrate(curve.cells[cell]), log_discount));
    return sum.total();
}

inline void integrate_curves(const Curve& curve, const Transition& transition, double log_discount,
                             std::span<double> values)
{
    for_each_target(values.size(), [&](std::size_t target) {
        values[target] = integrate_curve(curve, std::span{transition.weights}.subspan(target * curve.cells.size(), curve.cells.size()),
                                         transition.tails[target], log_discount);
    });
}

// Both layers use the same grid; keep each weight row hot for the second integral.
inline void integrate_curve_pair(const Curve& first, const Curve& second, const Transition& transition,
                                 double log_discount, std::span<double> first_values, std::span<double> second_values)
{
    for_each_target(first_values.size(), [&](std::size_t target) {
        const auto weights = std::span{transition.weights}.subspan(target * first.cells.size(), first.cells.size());
        first_values[target] = integrate_curve(first, weights, transition.tails[target], log_discount);
        second_values[target] = integrate_curve(second, weights, transition.tails[target], log_discount);
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
        sum.add(scaled_exponential(piece.cash, log_discount + log_interval_mass(lower, upper)));
        if (piece.asset != 0.0)
            sum.add(scaled_exponential(piece.asset, std::log(anchor) + mean + 0.5 * deviation * deviation +
                                                        log_discount + log_interval_mass(lower - deviation, upper - deviation)));
    }
    return sum.total();
}

} // namespace kiyosi::detail::quadrature
