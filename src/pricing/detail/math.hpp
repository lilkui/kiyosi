#pragma once

#include <cmath>
#include <numbers>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi::detail {

inline constexpr double percentage_points_per_unit = 100.0;
inline constexpr double inverse_sqrt_two = 1.0 / std::numbers::sqrt2;
inline constexpr double inverse_sqrt_two_pi = std::numbers::inv_sqrtpi * inverse_sqrt_two;
// Broadie-Glasserman-Kou discrete-barrier shift constant, -zeta(1/2)/sqrt(2*pi).
inline constexpr double bgk_beta = 0.5825971579390107;

inline double normal_cdf(double value) noexcept
{
    return 0.5 * std::erfc(-value * inverse_sqrt_two);
}

inline double normal_pdf(double value) noexcept
{
    return inverse_sqrt_two_pi * std::exp(-0.5 * value * value);
}

inline double log_normal_cdf(double value) noexcept
{
    if (value >= -10.0) return std::log(normal_cdf(value));
    // The erfc tail expansion avoids underflow before a large exponential weight cancels it (DLMF 7.12.1).
    const double inverse_square = (1.0 / value) / value;
    double term = 1.0;
    double sum = 1.0;
    for (int index = 1; index <= 20; ++index) {
        term *= -(2.0 * index - 1.0) * inverse_square;
        sum += term;
    }
    return -0.5 * value * value - std::log(-value) + std::log(inverse_sqrt_two_pi * sum);
}

inline double exponential_normal_cdf(double log_weight, double value) noexcept
{
    return std::exp(log_weight + log_normal_cdf(value));
}

/// Simpson integration of a centered normal tail; reflect negative thresholds to avoid long intervals.
inline double normal_tail_integral(double threshold) noexcept
{
    if (std::isnan(threshold)) return threshold;
    const double lower = std::abs(threshold);
    const double density = normal_pdf(lower);
    if (density == 0.0) return threshold < 0.0 ? 1.0 : 0.0;
    // Extend until the endpoint density is exp(-72) times the starting density.
    const double upper = std::hypot(lower, 12.0);
    constexpr int panels = 2048;
    const double step = (upper - lower) / panels;
    double sum = density + normal_pdf(upper);
    double correction = 0.0;
    for (int index = 1; index < panels; ++index) {
        // Compensated summation keeps price roundoff from dominating third-order Greeks.
        const double term = (index % 2 == 0 ? 2.0 : 4.0) * normal_pdf(lower + index * step) - correction;
        const double next = sum + term;
        correction = (next - sum) - term;
        sum = next;
    }
    const double tail = sum * step / 3.0;
    return threshold < 0.0 ? 1.0 - tail : tail;
}

} // namespace kiyosi::detail
