#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <numeric>
#include <span>
#include <vector>

#include "math.hpp"

namespace kiyosi::detail::quadrature {

using Polynomial = std::array<double, 4>;
inline constexpr double infinity = std::numeric_limits<double>::infinity();
// Their product exp(underflow_log_mass) * underflow_mean_limit rounds to zero.
inline constexpr double underflow_mean_limit = 1e20;
inline constexpr double underflow_log_mass = -800.0;

inline double signed_exponential(double value, double exponent)
{
    if (value == 0.0) return 0.0;
    const double weight = std::exp(exponent);
    if (std::isnormal(weight) && std::isfinite(value * weight)) return value * weight;
    return std::copysign(std::exp(std::log(std::abs(value)) + exponent), value);
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
        return mass > 0.0 ? mean * mass : signed_exponential(mean, log_mass);
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

inline constexpr std::size_t transition_lanes = 4;

struct MomentBlock {
    std::array<double, transition_lanes> mass{};
    std::array<double, transition_lanes> log_mass{};
    std::array<std::array<double, transition_lanes>, 3> conditional{};

    IntervalMoments lane(std::size_t index) const
    {
        return {mass[index], log_mass[index], {conditional[0][index], conditional[1][index], conditional[2][index]}};
    }

    void set_lane(std::size_t index, const IntervalMoments& moments)
    {
        mass[index] = moments.mass;
        log_mass[index] = moments.log_mass;
        for (std::size_t power = 0; power < moments.conditional.size(); ++power)
            conditional[power][index] = moments.conditional[power];
    }
};

struct Transition {
    // Four targets share contiguous moment components without duplicating the matrix.
    std::vector<MomentBlock> weights;
    std::vector<std::array<double, 2>> tails;
};

inline Transition transition_weights_scalar(const std::vector<double>& nodes, double drift, double deviation)
{
    Transition transition;
    const std::size_t cells = nodes.size() - 1;
    transition.weights.resize(((nodes.size() + transition_lanes - 1) / transition_lanes) * cells);
    transition.tails.reserve(nodes.size());
    for (std::size_t target = 0; target < nodes.size(); ++target) {
        const double mean = nodes[target] + drift;
        transition.tails.push_back({log_normal_cdf((nodes.front() - mean) / deviation),
                                    log_normal_cdf((mean - nodes.back()) / deviation)});
        for (std::size_t cell = 0; cell < cells; ++cell) {
            const auto moments = cell_moments(nodes[cell], nodes[cell + 1], mean, deviation);
            auto& block = transition.weights[(target / transition_lanes) * cells + cell];
            block.set_lane(target % transition_lanes, moments);
        }
    }
    return transition;
}

KIYOSI_EXPORT Transition transition_weights(const std::vector<double>& nodes, double drift, double deviation);
Transition transition_weights_avx2(const std::vector<double>& nodes, double drift, double deviation);

inline double integrate_curve(const Curve& curve, std::span<const IntervalMoments> weights,
                              const std::array<double, 2>& tails, double log_discount)
{
    Sum sum;
    sum.add(signed_exponential(curve.low, tails[0] + log_discount));
    sum.add(signed_exponential(curve.high, tails[1] + log_discount));
    const double discount = std::exp(log_discount);
    for (std::size_t cell = 0; cell < curve.cells.size(); ++cell)
        sum.add(std::isnormal(discount) ? weights[cell].integrate(curve.cells[cell]) * discount : signed_exponential(weights[cell].integrate(curve.cells[cell]), log_discount));
    return sum.total();
}

inline void integrate_curves_scalar(const Curve& curve, const Transition& transition, double log_discount,
                                    std::span<double> values, std::size_t first_target = 0)
{
    const double discount = std::exp(log_discount);
    for (std::size_t target = first_target; target < values.size(); ++target) {
        Sum sum;
        sum.add(signed_exponential(curve.low, transition.tails[target][0] + log_discount));
        sum.add(signed_exponential(curve.high, transition.tails[target][1] + log_discount));
        for (std::size_t cell = 0; cell < curve.cells.size(); ++cell) {
            const auto weight = transition.weights[(target / transition_lanes) * curve.cells.size() + cell].lane(target % transition_lanes);
            const double term = weight.integrate(curve.cells[cell]);
            sum.add(std::isnormal(discount) ? term * discount : signed_exponential(term, log_discount));
        }
        values[target] = sum.total();
    }
}

KIYOSI_EXPORT void integrate_curves(const Curve& curve, const Transition& transition, double log_discount,
                                    std::span<double> values);

void integrate_curves_avx2(const Curve& curve, const Transition& transition, double log_discount,
                           std::span<double> values);

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
        sum.add(signed_exponential(piece.cash, log_discount + log_interval_mass(lower, upper)));
        if (piece.asset != 0.0)
            sum.add(signed_exponential(piece.asset, std::log(anchor) + mean + 0.5 * deviation * deviation +
                                                        log_discount + log_interval_mass(lower - deviation, upper - deviation)));
    }
    return sum.total();
}

} // namespace kiyosi::detail::quadrature
