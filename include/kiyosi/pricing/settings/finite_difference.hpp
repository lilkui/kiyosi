#pragma once

#include <cmath>
#include <optional>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

enum class FiniteDifferenceScheme : unsigned char {
    explicit_euler,
    implicit_euler,
    crank_nicolson,
};

/// Aggregate configuration validated by finite-difference engines when price() is called.
struct FiniteDifferenceSettings {
    int asset_step_count = 200;
    int time_step_count = 200;
    FiniteDifferenceScheme scheme = FiniteDifferenceScheme::crank_nicolson;
    std::optional<double> asset_upper_boundary;
};

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
