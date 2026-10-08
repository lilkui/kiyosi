#pragma once

#include <concepts>
#include <cstdint>
#include <optional>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Execution backends supported by Monte Carlo engines.
enum class MonteCarloBackend : unsigned char {
    cpu,  ///< Host CPU implementation.
    cuda, ///< CUDA implementation, when built and available.
};

/// Vanilla simulation settings, validated when price() is called; an absent seed
/// draws from the system entropy source. Vanilla engines accept 1..10,000,000 paths and
/// 2..10,000 steps; American pricing before expiry requires at least 3 steps.
/// European pricing samples the exact terminal distribution without intermediate steps.
struct MonteCarloSettings {
    int path_count = 100'000;                           ///< Number of simulated paths; 1..10,000,000.
    int step_count = 50;                                ///< American grid points; 2..10,000 (American: >= 3); validated but unused for European pricing.
    std::optional<std::uint64_t> seed;                  ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
};

/// Structured products step the trading calendar directly, so no step count is needed; settings
/// are validated when price() is called. Accumulator and autocallable engines accept
/// 1..10,000,000 paths.
/// CPU paths use separate generators, so early knock-out cannot shift later paths' draws.
/// Seeded prices are reproducible within a standard-library implementation and backend.
struct TradingDayMonteCarloSettings {
    int path_count = 20'000;                            ///< Number of simulated paths; 1..10,000,000.
    std::optional<std::uint64_t> seed = 1;              ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
};

namespace detail {
inline constexpr int maximum_monte_carlo_path_count = 10'000'000;
inline constexpr int maximum_vanilla_monte_carlo_step_count = 10'000;

/// Validates path count, uniform step count when present, and backend in that order.
template <typename Settings>
    requires(std::same_as<Settings, MonteCarloSettings> ||
             std::same_as<Settings, TradingDayMonteCarloSettings>)
[[nodiscard]] Result<void> validate_monte_carlo_settings(const Settings& settings)
{
    if (settings.path_count <= 0 || settings.path_count > maximum_monte_carlo_path_count)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     std::same_as<Settings, MonteCarloSettings>
                                         ? "Monte Carlo path count is out of range"
                                         : "structured Monte Carlo path count is out of range"});
    if constexpr (std::same_as<Settings, MonteCarloSettings>)
        if (settings.step_count < 2 || settings.step_count > maximum_vanilla_monte_carlo_step_count)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "Monte Carlo step count is out of range"});
    if (settings.backend != MonteCarloBackend::cpu && settings.backend != MonteCarloBackend::cuda)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "Monte Carlo backend is invalid"});
    return {};
}
} // namespace detail

} // namespace kiyosi
