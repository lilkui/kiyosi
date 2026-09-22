#pragma once

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
struct FiniteDifferenceSettings {
    int asset_step_count = 200;                                             ///< Number of spatial grid steps; must be at least three.
    int time_step_count = 200;                                              ///< Number of time steps; must be positive.
    FiniteDifferenceScheme scheme = FiniteDifferenceScheme::crank_nicolson; ///< Time-marching scheme.
    std::optional<double> asset_upper_boundary{};                           ///< Positive upper spot boundary, or automatic when absent.
};

/// Validates finite-difference grid settings.
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

} // namespace kiyosi
