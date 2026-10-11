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
/// American calls with nonpositive dividends and nonnegative rates use that distribution too,
/// retaining the immediate-exercise lower bound. Rate stencils crossing zero use exercise-grid
/// simulations on both sides to avoid mixing sampled estimators.
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
    /// CPU European requests reuse per-call samples, with transient storage linear in path count.
    /// CPU American requests reuse a path buffer; spot stencils also cache growth factors,
    /// adding about 8 bytes per rounded-even path per time interval. All storage is request-local.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings) const;

    /// American exercise decisions amplify small-bump sampling noise. The default spot bump
    /// is 8% of spot; other shifts retain NumericalShiftSettings defaults. Explicit shifts are absolute.
    /// Estimates still have sampling error and bump-size bias; check convergence for the valuation.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks) const
    {
        return price_with_greeks(option, context, greeks,
                                 {.spot_shift = default_american_spot_shift(context)});
    }

    /// Core-owned default shared by C++ and language bindings.
    [[nodiscard]] static double default_american_spot_shift(const PricingContext& context) noexcept
    {
        return 0.08 * context.spot_price();
    }

    /// Returns the engine settings.
    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    MonteCarloSettings settings_;
};

} // namespace kiyosi
