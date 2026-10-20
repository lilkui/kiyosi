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

/// Aggregate configuration validated when creating a finite-difference engine.
/// Asset steps must be >= 3 and time steps >= 1, within the int representation.
/// Pricing remains subject to available memory and runtime;
/// allocation failures propagate as std::bad_alloc. An explicit scheme may impose a
/// stability constraint for the chosen market and grid.
/// American vanilla and autocallable engines widen automatic domains with volatility and maturity.
/// American grids and wide autocallable grids use sinh coordinates to retain asset resolution.
/// Autocallable implicit grids concentrate around contract levels; their engine's default
/// configuration is 800 asset and 2,000 time steps, overriding this shared aggregate's defaults.
/// Explicit upper boundaries still require a domain-convergence check.
struct FiniteDifferenceSettings {
    int asset_step_count = 800;                                             ///< Number of spatial grid steps; must be at least three.
    int time_step_count = 200;                                              ///< Number of time steps; must be positive.
    FiniteDifferenceScheme scheme = FiniteDifferenceScheme::crank_nicolson; ///< Time-marching scheme.
    std::optional<double> asset_upper_boundary{};                           ///< Positive upper spot boundary, or automatic when absent.
};

namespace detail {

// Only built-in engines can be reconstructed from their settings.
template <typename Engine, typename Option>
inline constexpr bool is_builtin_finite_difference_engine = false;

} // namespace detail

/// Validates finite-difference settings shared by all engine factories.
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
