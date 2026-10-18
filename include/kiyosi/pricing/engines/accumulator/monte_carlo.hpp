#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

/// Simulates the trading-day accrual, terminating each path at the knock-out level.
/// Accepts 1..10,000,000 paths.
class KIYOSI_EXPORT MonteCarloAccumulatorEngine {
public:
    /// Creates an engine with aggregate trading-day simulation settings.
    explicit MonteCarloAccumulatorEngine(TradingDayMonteCarloSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit path count, seed, backend, and CPU concurrency cap.
    explicit MonteCarloAccumulatorEngine(
        int path_count, std::optional<std::uint64_t> seed = TradingDayMonteCarloSettings{}.seed,
        MonteCarloBackend backend = TradingDayMonteCarloSettings{}.backend,
        int thread_count = TradingDayMonteCarloSettings{}.thread_count)
        : settings_{path_count, seed, backend, thread_count} {}

    /// Prices an accumulator by Monte Carlo simulation.
    /// @return Price, or a contract, context, settings, or backend error.
    [[nodiscard]] Result<double> price(const Accumulator& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// CPU spot bumps share per-call random draws and growth factors with per-block reduction storage.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    TradingDayMonteCarloSettings settings_;
};

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloAccumulatorEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
