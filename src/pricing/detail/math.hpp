#pragma once

#include <cmath>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi::detail {

inline constexpr double percentage_points_per_unit = 100.0;
inline constexpr double inverse_sqrt_two = 0.70710678118654752440;
inline constexpr double inverse_sqrt_two_pi = 0.39894228040143267794;
inline constexpr double solver_derivative_step_fraction = 1e-3;
inline constexpr double solver_bracket_step_fraction = 1e-2;
inline constexpr double solver_minimum_derivative_step = 1e-6;
// Broadie-Glasserman-Kou discrete-barrier shift constant, zeta(1/2)/sqrt(2*pi).
inline constexpr double bgk_beta = 0.5825971579390107;

/// Day-count shim for call sites that have already validated their dates.
inline double actual_365_fixed_year_fraction(Date start, Date end) noexcept
{
    return *year_fraction(start, end);
}

inline double actual_365_fixed_year_fraction(Timestamp start, Timestamp end) noexcept
{
    return *year_fraction(start, end);
}

inline double normal_cdf(double value) noexcept
{
    return 0.5 * std::erfc(-value * inverse_sqrt_two);
}

inline double normal_pdf(double value) noexcept
{
    return inverse_sqrt_two_pi * std::exp(-0.5 * value * value);
}

} // namespace kiyosi::detail
