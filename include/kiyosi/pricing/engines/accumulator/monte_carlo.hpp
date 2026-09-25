#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

/// Simulates the trading-day accrual, terminating each path at the knock-out level.
/// Accepts 1..10,000,000 paths.
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
    /// @return Price, or a contract, context, settings, or backend error.
    [[nodiscard]] Result<double> price(const Accumulator& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                          GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
                                         [&](const auto& engine) {
                                             return engine.price_native(option, context);
                                         });
    }

    /// Returns the engine settings.
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_native(const Accumulator& option, const PricingContext& context) const;
    TradingDayMonteCarloSettings settings_;
};

} // namespace kiyosi
