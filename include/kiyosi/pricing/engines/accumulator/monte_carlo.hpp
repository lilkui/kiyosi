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
    explicit MonteCarloAccumulatorEngine(TradingDayMonteCarloSettings settings = {}) : settings_(settings) {}
    explicit MonteCarloAccumulatorEngine(
        int path_count, std::optional<std::uint64_t> seed = TradingDayMonteCarloSettings{}.seed,
        MonteCarloBackend backend = TradingDayMonteCarloSettings{}.backend)
        : settings_{path_count, seed, backend} {}

    [[nodiscard]] Result<PricingResult> price(const Accumulator&, const PricingContext&) const;
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    TradingDayMonteCarloSettings settings_;
};

} // namespace kiyosi
