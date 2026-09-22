#pragma once

#include <cstdint>
#include <optional>

namespace kiyosi {

/// Execution backends supported by Monte Carlo engines.
enum class MonteCarloBackend : unsigned char {
    cpu,  ///< Host CPU implementation.
    cuda, ///< CUDA implementation, when built and available.
};

/// Path simulation over a uniform time grid, validated when price() is called; an absent seed
/// draws from the system entropy source.
struct MonteCarloSettings {
    int path_count = 100'000;                           ///< Number of simulated paths; must be positive.
    int step_count = 50;                                ///< Uniform time steps per path; must be positive.
    std::optional<std::uint64_t> seed;                  ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
};

/// Structured products step the trading calendar directly, so no step count is needed; settings
/// are validated when price() is called.
struct TradingDayMonteCarloSettings {
    int path_count = 20'000;                            ///< Number of simulated paths; must be positive.
    std::optional<std::uint64_t> seed = 1;              ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
};

} // namespace kiyosi
