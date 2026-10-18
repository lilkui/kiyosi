#pragma once

#include <concepts>
#include <cstdint>
#include <optional>
#include <random>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Execution backends supported by Monte Carlo engines.
/// CUDA operations use the calling thread's current device and leave it unchanged.
/// Each call owns a nonblocking stream and waits only for its own work, including cleanup.
/// The device and runtime must support CUDA stream-ordered memory allocation.
enum class MonteCarloBackend : unsigned char {
    cpu,  ///< Host CPU implementation.
    cuda, ///< CUDA implementation, when built and available.
};

/// CPU calls own their parallel work and complete synchronously. Concurrent calls on distinct
/// engines or const calls on a shared engine are safe without concurrent mutation. Thread caps
/// apply per call, not process-wide. Fixed seeds reproduce results across thread counts within
/// a library version, build and standard-library implementation; CUDA is a separate backend.
/// Allocation and internal runtime exceptions propagate; no cancellation interface is provided.
/// Vanilla simulation settings, validated when price() is called; an absent seed
/// draws from the system entropy source. Vanilla engines accept 1..10,000,000 paths and
/// 2..10,000 steps; American pricing before expiry requires at least 3 steps.
/// European pricing samples the exact terminal distribution without intermediate steps.
struct MonteCarloSettings {
    int path_count = 100'000;                           ///< Number of simulated paths; 1..10,000,000.
    int step_count = 50;                                ///< American grid points; 2..10,000 (American: >= 3); validated but unused for European pricing.
    std::optional<std::uint64_t> seed{};                ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
    int thread_count = 0;                               ///< CPU concurrency cap: 0 automatic, 1 serial; CUDA requires 0.
};

/// Accumulator and autocallable products step the trading calendar directly, so no step
/// count is needed; settings are validated when price() is called. These engines accept
/// 1..10,000,000 paths.
/// CPU paths use separate PCG32 streams, so early knock-out cannot shift later paths' draws.
/// Seeded prices are reproducible within a library version, standard-library implementation and backend.
struct TradingDayMonteCarloSettings {
    int path_count = 20'000;                            ///< Number of simulated paths; 1..10,000,000.
    std::optional<std::uint64_t> seed = 1;              ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu; ///< Execution backend.
    int thread_count = 0;                               ///< CPU concurrency cap: 0 automatic, 1 serial; CUDA requires 0.
};

namespace detail {

// Only built-in engines can be reconstructed from their settings.
template <typename Engine, typename Option>
inline constexpr bool is_builtin_monte_carlo_engine = false;

// One synchronous request owns its seed; configured and custom engines pass through unchanged.
template <typename Engine, typename Option, typename Evaluate>
auto with_monte_carlo_seed(const Engine& engine, const Evaluate& evaluate)
{
    if constexpr (is_builtin_monte_carlo_engine<Engine, Option>) {
        auto settings = engine.settings();
        if (!settings.seed) {
            settings.seed = std::random_device{}();
            return evaluate(Engine{settings});
        }
    }
    return evaluate(engine);
}

inline constexpr int maximum_monte_carlo_path_count = 10'000'000;
inline constexpr int maximum_vanilla_monte_carlo_step_count = 10'000;

/// Validates path count, uniform step count when present, backend, and thread count in that order.
template <typename Settings>
    requires(std::same_as<Settings, MonteCarloSettings> ||
             std::same_as<Settings, TradingDayMonteCarloSettings>)
[[nodiscard]] Result<void> validate_monte_carlo_settings(const Settings& settings)
{
    if (settings.path_count <= 0 || settings.path_count > maximum_monte_carlo_path_count)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     std::same_as<Settings, MonteCarloSettings>
                                         ? "Monte Carlo path count is out of range"
                                         : "trading-day Monte Carlo path count is out of range"});
    if constexpr (std::same_as<Settings, MonteCarloSettings>)
        if (settings.step_count < 2 || settings.step_count > maximum_vanilla_monte_carlo_step_count)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "Monte Carlo step count is out of range"});
    if (settings.backend != MonteCarloBackend::cpu && settings.backend != MonteCarloBackend::cuda)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "Monte Carlo backend is invalid"});
    if (settings.thread_count < 0 ||
        (settings.backend == MonteCarloBackend::cuda && settings.thread_count != 0))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "Monte Carlo thread count is invalid for the backend"});
    return {};
}
} // namespace detail

} // namespace kiyosi
