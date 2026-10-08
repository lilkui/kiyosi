#pragma once

#include <algorithm>
#include <cmath>
#include <optional>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Time-marching schemes supported by finite-difference engines.
enum class FiniteDifferenceScheme : unsigned char {
    explicit_euler, ///< First-order explicit Euler scheme.
    implicit_euler, ///< First-order implicit Euler scheme.
    crank_nicolson, ///< Second-order Crank-Nicolson scheme.
};

/// Aggregate configuration validated by finite-difference engines when price() is called.
/// The shared validator checks asset steps >= 3 and time steps >= 1. Vanilla, digital, and
/// barrier engines accept at most 10,000 asset and 100,000 time steps; accumulator and
/// autocallable engines accept at most 2,000 of each. An explicit scheme may impose a
/// further stability constraint for the chosen market and grid.
struct FiniteDifferenceSettings {
    int asset_step_count = 200;                                             ///< Number of spatial grid steps; must be at least three.
    int time_step_count = 200;                                              ///< Number of time steps; must be positive.
    FiniteDifferenceScheme scheme = FiniteDifferenceScheme::crank_nicolson; ///< Time-marching scheme.
    std::optional<double> asset_upper_boundary{};                           ///< Positive upper spot boundary, or automatic when absent.
};

namespace detail {
inline constexpr int general_fd_max_asset_steps = 10'000;
inline constexpr int general_fd_max_time_steps = 100'000;
inline constexpr int trading_fd_max_steps = 2'000;

template <typename Option, typename Context>
double highest_finite_difference_level(const Option& option, const Context& context)
{
    double relevant = context.spot_price();
    if constexpr (requires { option.strike(); }) relevant = std::max(relevant, option.strike());
    if constexpr (requires { option.initial_spot(); option.upper_strike(); option.lower_strike(); })
        relevant = std::max({relevant, option.initial_spot(), option.upper_strike(), option.lower_strike()});
    if constexpr (requires { option.knock_out_level(); }) relevant = std::max(relevant, option.knock_out_level());
    if constexpr (requires { option.knock_out_levels(); })
        for (const double level : option.knock_out_levels()) relevant = std::max(relevant, level);
    if constexpr (requires { option.knock_in_level(); }) relevant = std::max(relevant, option.knock_in_level());
    if constexpr (requires { option.coupon_barrier_levels(); })
        for (const double level : option.coupon_barrier_levels()) relevant = std::max(relevant, level);
    return relevant;
}

template <typename Option, typename Context>
double default_finite_difference_upper_boundary(const Option& option, const Context& context)
{
    const double relevant = highest_finite_difference_level(option, context);
    if constexpr (requires { option.barrier_state(); } || requires { option.accumulated_quantity(); })
        return std::max(4.0 * relevant, relevant + 1.0);
    return 4.0 * relevant;
}
} // namespace detail

/// Validates shared finite-difference grid settings; engine-specific limits are checked by price().
/// @param settings Settings to validate.
/// @return Success, or an `invalid_parameter` error.
[[nodiscard]] inline Result<void> validate_finite_difference_settings(
    const FiniteDifferenceSettings& settings)
{
    if (settings.asset_step_count < 3 || settings.time_step_count <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    if (settings.asset_upper_boundary &&
        (!std::isfinite(*settings.asset_upper_boundary) || *settings.asset_upper_boundary <= 0.0))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference upper boundary must be finite and positive"});
    if (settings.scheme != FiniteDifferenceScheme::explicit_euler &&
        settings.scheme != FiniteDifferenceScheme::implicit_euler &&
        settings.scheme != FiniteDifferenceScheme::crank_nicolson)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference scheme is invalid"});
    return {};
}

namespace detail {

/// Validates shared settings before applying the engine-specific grid limits.
[[nodiscard]] inline Result<void> validate_finite_difference_settings(
    const FiniteDifferenceSettings& settings, int max_asset_steps, int max_time_steps)
{
    auto valid = kiyosi::validate_finite_difference_settings(settings);
    if (!valid) return valid;
    if (settings.asset_step_count > max_asset_steps || settings.time_step_count > max_time_steps)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    return {};
}

} // namespace detail

} // namespace kiyosi
