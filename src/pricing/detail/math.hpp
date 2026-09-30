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

} // namespace kiyosi::detail
