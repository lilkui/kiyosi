#pragma once

#include <cmath>
#include <kiyosi/core/types.hpp>


namespace kiyosi {

enum class finite_difference_scheme : unsigned char {
    explicit_euler,
    implicit_euler,
    crank_nicolson,
};

struct FiniteDifferenceSettings {
    int asset_steps = 200;
    int time_steps = 200;
    finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson;
    double upper_boundary = 0.0;
};

[[nodiscard]] inline result<void> validate_finite_difference_settings(
    const FiniteDifferenceSettings& settings)
{
    if (settings.asset_steps < 3 || settings.time_steps <= 0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    if (settings.upper_boundary != 0.0 &&
        (!std::isfinite(settings.upper_boundary) || settings.upper_boundary <= 0.0))
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must be finite and positive"});
    if (settings.scheme != finite_difference_scheme::explicit_euler &&
        settings.scheme != finite_difference_scheme::implicit_euler &&
        settings.scheme != finite_difference_scheme::crank_nicolson)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference scheme is invalid"});
    return {};
}

} // namespace kiyosi
