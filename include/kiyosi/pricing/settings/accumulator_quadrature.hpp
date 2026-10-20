#pragma once

#include <cmath>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Fixed spatial budget; check space and domain convergence separately.
struct AccumulatorQuadratureSettings {
    int asset_step_count = 512;
    double integration_standard_deviations = 8.0;
};

[[nodiscard]] inline Result<void> validate_accumulator_quadrature_settings(const AccumulatorQuadratureSettings& settings)
{
    if (settings.asset_step_count < 4 || !std::isfinite(settings.integration_standard_deviations) ||
        settings.integration_standard_deviations <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "quadrature requires at least four asset steps and a finite positive integration extent"});
    return {};
}

} // namespace kiyosi
