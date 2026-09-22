#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

/// Steps the trading calendar path by path, applying knock-in, knock-out, and coupon events.
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
    /// @return Pricing measures, or a contract, context, settings, or backend error.
    [[nodiscard]] Result<PricingResult> price(const Note&, const PricingContext&) const;
    /// Returns the engine settings.
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
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
