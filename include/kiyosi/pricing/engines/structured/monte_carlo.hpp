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
    explicit MonteCarloAutocallableEngine(TradingDayMonteCarloSettings settings = {}) : settings_(settings) {}
    explicit MonteCarloAutocallableEngine(
        int path_count, std::optional<std::uint64_t> seed = TradingDayMonteCarloSettings{}.seed,
        MonteCarloBackend backend = TradingDayMonteCarloSettings{}.backend)
        : settings_{path_count, seed, backend} {}

    [[nodiscard]] Result<PricingResult> price(const Note&, const PricingContext&) const;
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    TradingDayMonteCarloSettings settings_;
};

using MonteCarloPhoenixEngine = MonteCarloAutocallableEngine<PhoenixOption>;
using MonteCarloSnowballEngine = MonteCarloAutocallableEngine<SnowballOption>;
using MonteCarloBinarySnowballEngine = MonteCarloAutocallableEngine<BinarySnowballOption>;
using MonteCarloTernarySnowballEngine = MonteCarloAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
