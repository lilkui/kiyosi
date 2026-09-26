#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

/// Steps the trading calendar path by path, applying knock-in, knock-out, and coupon events.
/// Accepts 1..10,000,000 paths.
template <typename Note>
class KIYOSI_EXPORT MonteCarloAutocallableEngine {
public:
    /// Creates an engine with aggregate trading-day simulation settings.
    explicit MonteCarloAutocallableEngine(TradingDayMonteCarloSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit path count, seed, and backend.
    explicit MonteCarloAutocallableEngine(
        int path_count, std::optional<std::uint64_t> seed = TradingDayMonteCarloSettings{}.seed,
        MonteCarloBackend backend = TradingDayMonteCarloSettings{}.backend)
        : settings_{path_count, seed, backend} {}

    /// Prices an autocallable note by Monte Carlo simulation.
    /// @return Price, or a contract, context, settings, or backend error.
    [[nodiscard]] Result<double> price(const Note& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const Note& option, const PricingContext& context,
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
    [[nodiscard]] Result<PricingResult> price_native(const Note& note, const PricingContext& context) const;
    TradingDayMonteCarloSettings settings_;
};

/// Monte Carlo engine for Phoenix options.
using MonteCarloPhoenixEngine = MonteCarloAutocallableEngine<PhoenixOption>;
/// Monte Carlo engine for snowball options.
using MonteCarloSnowballEngine = MonteCarloAutocallableEngine<SnowballOption>;
/// Monte Carlo engine for binary snowball options.
using MonteCarloBinarySnowballEngine = MonteCarloAutocallableEngine<BinarySnowballOption>;
/// Monte Carlo engine for ternary snowball options.
using MonteCarloTernarySnowballEngine = MonteCarloAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
