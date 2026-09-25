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
/// draws from the system entropy source. Vanilla engines accept 1..10,000,000 paths and
/// 2..10,000 steps; American pricing before expiry requires at least 3 steps.
struct MonteCarloSettings {
    int path_count = 100'000;                           ///< Number of simulated paths; 1..10,000,000.
    int step_count = 50;                                ///< Uniform time steps per path; 2..10,000 (American: >= 3).
    std::optional<std::uint64_t> seed;                  ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
};

/// Structured products step the trading calendar directly, so no step count is needed; settings
/// are validated when price() is called. Accumulator and autocallable engines accept
/// 1..10,000,000 paths.
struct TradingDayMonteCarloSettings {
    int path_count = 20'000;                            ///< Number of simulated paths; 1..10,000,000.
    std::optional<std::uint64_t> seed = 1;              ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
};

namespace detail {
inline constexpr int maximum_monte_carlo_path_count = 10'000'000;
inline constexpr int maximum_vanilla_monte_carlo_step_count = 10'000;
} // namespace detail

} // namespace kiyosi
