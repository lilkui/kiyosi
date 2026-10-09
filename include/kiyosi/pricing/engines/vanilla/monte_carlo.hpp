#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

/// Monte Carlo valuation for vanilla European and American options.
/// Accepts 1..10,000,000 paths and 2..10,000 steps; American pricing before expiry needs 3 steps.
/// European prices sample the exact terminal distribution; the validated step count has no effect.
class KIYOSI_EXPORT MonteCarloVanillaEngine {
public:
    /// Creates an engine with aggregate Monte Carlo settings.
    explicit MonteCarloVanillaEngine(MonteCarloSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit path count, step count, seed, and backend.
    MonteCarloVanillaEngine(
        int path_count, int step_count, std::optional<std::uint64_t> seed = MonteCarloSettings{}.seed,
        MonteCarloBackend backend = MonteCarloSettings{}.backend)
        : settings_{path_count, step_count, seed, backend} {}

    /// Prices a European or American vanilla option.
    /// @return Price, or a contract, context, settings, or backend error.
    [[nodiscard]] Result<double> price(const EuropeanOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AmericanOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    MonteCarloSettings settings_;
};

} // namespace kiyosi
