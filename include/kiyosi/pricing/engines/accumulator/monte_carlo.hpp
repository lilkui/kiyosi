#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

/// Simulates the trading-day accrual, terminating each path at the knock-out level.
class KIYOSI_EXPORT MonteCarloAccumulatorEngine {
public:
    /// Creates an engine with aggregate trading-day simulation settings.
    explicit MonteCarloAccumulatorEngine(TradingDayMonteCarloSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit path count, seed, and backend.
    explicit MonteCarloAccumulatorEngine(
        int path_count, std::optional<std::uint64_t> seed = TradingDayMonteCarloSettings{}.seed,
        MonteCarloBackend backend = TradingDayMonteCarloSettings{}.backend)
        : settings_{path_count, seed, backend} {}

    /// Prices an accumulator by Monte Carlo simulation.
    /// @return Pricing measures, or a contract, context, settings, or backend error.
    [[nodiscard]] Result<PricingResult> price(const Accumulator&, const PricingContext&) const;
    /// Returns the engine settings.
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    TradingDayMonteCarloSettings settings_;
};

} // namespace kiyosi
