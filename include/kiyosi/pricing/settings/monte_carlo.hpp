#pragma once

#include <bit>
#include <concepts>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <random>
#include <type_traits>

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

/// Sampling rules, independent of the execution backend.
enum class MonteCarloSampling : unsigned char {
    pseudo_random, ///< Pseudorandom streams, independent of antithetic pairing.
    sobol,         ///< Seeded LMS-scrambled Sobol with a digital shift; CPU only.
};

/// CPU calls own their parallel work and complete synchronously. Concurrent calls on distinct
/// engines or const calls on a shared engine are safe without concurrent mutation. Thread caps
/// apply per call, not process-wide. Fixed seeds reproduce results across thread counts within
/// a library version, build and standard-library implementation; CUDA is a separate backend.
/// Allocation and internal runtime exceptions propagate; no cancellation interface is provided.
/// Vanilla and digital simulation settings, validated when creating an engine; an absent seed
/// draws from the system entropy source. These engines accept 1..10,000,000 paths and
/// 2..10,000 steps; American pricing before expiry requires at least 3 steps.
/// European pricing samples the exact terminal distribution without intermediate steps.
/// Sobol uses chronological inverse-normal increments and one seeded
/// scramble per call. It preserves the same concurrency contract; it does not report a
/// standard error or confidence interval. Selecting Sobol requires an explicit power-of-two
/// path count; the default path counts are not powers of two. CPU and CUDA streams differ.
struct MonteCarloSettings {
    int path_count = 100'000;                                        ///< Exact number of simulated paths; 1..10,000,000.
    int step_count = 50;                                             ///< American grid points; 2..10,000 (American: >= 3); validated but unused for European pricing.
    std::optional<std::uint64_t> seed{};                             ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu;              ///< Execution backend.
    int thread_count = 0;                                            ///< CPU concurrency cap: 0 automatic, 1 serial; CUDA requires 0.
    MonteCarloSampling sampling = MonteCarloSampling::pseudo_random; ///< Sobol requires a power-of-two path count; one point per base path.
    bool antithetic = false;                                         ///< Pair Z and -Z increments; path_count remains exact, with one unpaired path if odd. Sobol uses ceil(path_count/2) points when enabled.
};

/// Accumulator and autocallable products step the trading calendar directly, so no step
/// count is needed; settings are validated when creating an engine. These engines accept
/// 1..10,000,000 paths.
/// Pseudorandom CPU paths use separate PCG32 streams; Sobol uses globally indexed points,
/// so early knock-out cannot shift later paths' draws. Sobol permits at most 21,201 increments;
/// a longer active simulation grid returns invalid_parameter when pricing, before sampling.
/// Seeded prices are reproducible within a library version, standard-library implementation and backend.
struct TradingDayMonteCarloSettings {
    int path_count = 20'000;                                         ///< Exact number of simulated paths; 1..10,000,000.
    std::optional<std::uint64_t> seed = 1;                           ///< Deterministic seed, or system entropy when absent.
    MonteCarloBackend backend = MonteCarloBackend::cpu;              ///< Execution backend.
    int thread_count = 0;                                            ///< CPU concurrency cap: 0 automatic, 1 serial; CUDA requires 0.
    MonteCarloSampling sampling = MonteCarloSampling::pseudo_random; ///< Sobol requires a power-of-two path count; chronological increments, up to 21,201 dates.
    bool antithetic = false;                                         ///< Pair Z and -Z increments; path_count remains exact, with one unpaired path if odd. Sobol uses ceil(path_count/2) points when enabled.
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
            const auto seeded = make_engine(settings, std::type_identity<Engine>{});
            if (!seeded) return decltype(evaluate(engine)){std::unexpected(seeded.error())};
            return evaluate(*seeded);
        }
    }
    return evaluate(engine);
}

inline constexpr int maximum_monte_carlo_path_count = 10'000'000;
inline constexpr int maximum_vanilla_monte_carlo_step_count = 10'000;
inline constexpr std::size_t maximum_sobol_dimension = 21'201;

inline Result<void> validate_monte_carlo_dimension(MonteCarloSampling sampling, std::size_t dimension)
{
    if (sampling == MonteCarloSampling::sobol && dimension > maximum_sobol_dimension)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "Sobol simulation dimension exceeds 21,201"});
    return {};
}

/// Validates path count, uniform step count when present, backend, thread count, and sampling in that order.
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
    if (settings.sampling != MonteCarloSampling::pseudo_random && settings.sampling != MonteCarloSampling::sobol)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "Monte Carlo sampling is invalid"});
    if (settings.sampling == MonteCarloSampling::sobol) {
        if (!std::has_single_bit(static_cast<unsigned int>(settings.path_count)))
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "Sobol path count must be a power of two"});
        if (settings.backend != MonteCarloBackend::cpu)
            return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                         "Sobol sampling is supported only on the CPU"});
    }
    return {};
}
} // namespace detail

} // namespace kiyosi
