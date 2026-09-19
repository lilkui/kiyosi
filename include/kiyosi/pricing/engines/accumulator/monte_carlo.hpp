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
    explicit MonteCarloAccumulatorEngine(StructuredMonteCarloSettings settings = {}) : settings_(settings) {}
    explicit MonteCarloAccumulatorEngine(
        int path_count, std::optional<std::uint64_t> seed = StructuredMonteCarloSettings{}.seed,
        monte_carlo_backend backend = StructuredMonteCarloSettings{}.backend)
        : settings_{path_count, seed, backend} {}

    [[nodiscard]] result<PricingResult> price(const Accumulator&, const PricingContext&) const;
    StructuredMonteCarloSettings settings() const noexcept { return settings_; }

private:
    StructuredMonteCarloSettings settings_;
};

} // namespace kiyosi
