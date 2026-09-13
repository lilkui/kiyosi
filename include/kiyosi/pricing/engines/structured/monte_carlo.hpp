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
class KIYOSI_EXPORT MonteCarloStructuredEngine {
public:
    explicit MonteCarloStructuredEngine(StructuredMonteCarloSettings settings = {}) : settings_(settings) {}
    explicit MonteCarloStructuredEngine(int path_count, std::optional<std::uint64_t> seed = 1)
        : settings_{path_count, seed} {}

    [[nodiscard]] result<PricingResult> price(const Note&, const PricingContext&) const;
    StructuredMonteCarloSettings settings() const noexcept { return settings_; }

private:
    StructuredMonteCarloSettings settings_;
};

using MonteCarloPhoenixEngine = MonteCarloStructuredEngine<PhoenixOption>;
using MonteCarloSnowballEngine = MonteCarloStructuredEngine<SnowballOption>;
using MonteCarloBinarySnowballEngine = MonteCarloStructuredEngine<BinarySnowballOption>;
using MonteCarloTernarySnowballEngine = MonteCarloStructuredEngine<TernarySnowballOption>;

} // namespace kiyosi
