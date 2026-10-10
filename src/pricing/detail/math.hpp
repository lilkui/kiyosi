#pragma once

#include <cmath>
#include <limits>
#include <numbers>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi::detail {

inline Result<double> time_to_expiry(Timestamp valuation_time, Date effective_date, Date expiry_date)
{
    const auto valid = validate_valuation_within_instrument_life(valuation_time, effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    return actual_365_fixed_year_fraction(valuation_time, expiry_date);
}

inline constexpr double percentage_points_per_unit = 100.0;
inline constexpr double inverse_sqrt_two = 1.0 / std::numbers::sqrt2;
inline constexpr double inverse_sqrt_two_pi = std::numbers::inv_sqrtpi * inverse_sqrt_two;
// Broadie-Glasserman-Kou discrete-barrier shift constant, -zeta(1/2)/sqrt(2*pi).
inline constexpr double bgk_beta = 0.5825971579390107;

inline double log_price_ratio(double numerator, double denominator) noexcept
{
    const double relative_difference = (numerator - denominator) / denominator;
    return std::abs(relative_difference) < 0.5
               ? std::log1p(relative_difference)
               : std::log(numerator) - std::log(denominator);
}

inline double standardize_forward(double forward, double volatility, double root_time) noexcept
{
    const double width = volatility * root_time;
    // Divide separately if positive volatility's time-scaled width rounds to zero.
    return width == 0.0 ? (forward / volatility) / root_time : forward / width;
}

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

inline double scaled_exponential(double amount, double exponent) noexcept
{
    if (amount == 0.0) return 0.0;
    const double weight = std::exp(exponent);
    const double result = amount * weight;
    return std::isnormal(weight) && std::isfinite(result)
               ? result
               : std::exp(std::log(amount) + exponent);
}

inline double scaled_normal_cdf(double amount, double log_weight, double value) noexcept
{
    if (amount == 0.0) return 0.0;
    // Include the payoff before exponentiating a probability or weight that could underflow.
    return exponential_normal_cdf(std::log(amount) + log_weight, value);
}

inline double barrier_hit_discount(double distance, bool upper, double drift, double volatility, double t, double rate) noexcept
{
    if (t == 0.0) return 1.0;
    const double variance = volatility * volatility;
    const double signed_drift = upper ? -drift : drift;
    if (variance == 0.0) {
        if (signed_drift >= 0.0) return 0.0;
        // Keep the normal boundary at expiry even when squaring volatility underflows.
        return exponential_normal_cdf(rate * (distance / signed_drift),
                                      standardize_forward(-signed_drift * t - distance, volatility, std::sqrt(t)));
    }
    const double discriminant = signed_drift * signed_drift + 2.0 * rate * variance;
    if (discriminant < 0.0)
        return std::numeric_limits<double>::quiet_NaN();
    const double root = std::sqrt(discriminant);
    const double root_time = volatility * std::sqrt(t);
    // Rationalize the small root difference to retain discounting as variance approaches zero.
    const double first_exponent = signed_drift < 0.0 ? -2.0 * rate / (root - signed_drift) : (-signed_drift - root) / variance;
    const double second_exponent = signed_drift > 0.0 ? 2.0 * rate / (root + signed_drift) : (-signed_drift + root) / variance;
    const double first = exponential_normal_cdf(first_exponent * distance,
                                                (root * t - distance) / root_time);
    const double second = exponential_normal_cdf(second_exponent * distance,
                                                 (-root * t - distance) / root_time);
    const double result = first + second;
    return std::isfinite(result) ? result : std::numeric_limits<double>::quiet_NaN();
}

/// Simpson integration of a scaled normal tail; reflect negative thresholds to avoid long intervals.
inline double normal_tail_integral(double threshold, double amount = 1.0, double log_discount = 0.0) noexcept
{
    if (std::isnan(threshold)) return threshold;
    const double lower = std::abs(threshold);
    const bool scaled_tail = threshold > 10.0;
    const double discount = scaled_tail ? 1.0 : std::exp(log_discount);
    const double scale = scaled_tail ? 1.0 : amount * discount;
    const bool direct_scale = std::isnormal(discount) && std::isfinite(scale);
    const double log_weight = scaled_tail ? std::log(amount) + log_discount : 0.0;
    // Scale before exponentiating so representable prices survive probability underflow.
    const auto density_at = [&](double value) {
        return scaled_tail ? inverse_sqrt_two_pi * std::exp(log_weight - 0.5 * value * value) : normal_pdf(value);
    };
    const double density = density_at(lower);
    if (density == 0.0) {
        if (threshold >= 0.0) return 0.0;
        return direct_scale ? scale : scaled_normal_cdf(amount, log_discount, -threshold);
    }
    // Extend until the endpoint density is exp(-72) times the starting density.
    const double upper = std::hypot(lower, 12.0);
    constexpr int panels = 2048;
    const double step = (upper - lower) / panels;
    double sum = density + density_at(upper);
    double correction = 0.0;
    for (int index = 1; index < panels; ++index) {
        // Compensated summation keeps price roundoff from dominating third-order Greeks.
        const double term = (index % 2 == 0 ? 2.0 : 4.0) * density_at(lower + index * step) - correction;
        const double next = sum + term;
        correction = (next - sum) - term;
        sum = next;
    }
    const double tail = sum * step / 3.0;
    const double probability = threshold < 0.0 ? 1.0 - tail : tail;
    return direct_scale ? scale * probability
                        : std::exp(std::log(amount) + log_discount + std::log(probability));
}

} // namespace kiyosi::detail
