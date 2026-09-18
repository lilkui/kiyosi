#pragma once

#include <cstdint>
#include <optional>

namespace kiyosi {

/// Path simulation over a uniform time grid, validated when price() is called; an absent seed
/// draws from the system entropy source.
struct MonteCarloSettings {
    int path_count = 100'000;
    int step_count = 50;
    std::optional<std::uint64_t> seed;
};

/// Structured products step the trading calendar directly, so no step count is needed; settings
/// are validated when price() is called.
struct StructuredMonteCarloSettings {
    int path_count = 20'000;
    std::optional<std::uint64_t> seed = 1;
};

} // namespace kiyosi
