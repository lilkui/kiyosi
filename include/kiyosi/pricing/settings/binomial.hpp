#pragma once

#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Aggregate configuration validated when creating a binomial engine.
struct BinomialSettings {
    int step_count = 256; ///< Number of tree time steps; must be between 1 and 1,000,000.
};

/// Validates the binomial tree size.
[[nodiscard]] inline Result<void> validate_binomial_settings(BinomialSettings settings)
{
    if (settings.step_count <= 0 || settings.step_count > 1'000'000)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "binomial step count must be between 1 and 1000000"});
    return {};
}

} // namespace kiyosi
