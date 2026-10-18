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
/// American implied-volatility solves reuse request-local path and cash-flow buffers
/// on CPU and device workspaces on CUDA; every trial recomputes its exercise decisions.
class KIYOSI_EXPORT MonteCarloVanillaEngine {
public:
    /// Creates an engine with aggregate Monte Carlo settings.
    explicit MonteCarloVanillaEngine(MonteCarloSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit path count, step count, seed, backend, and CPU concurrency cap.
    MonteCarloVanillaEngine(
        int path_count, int step_count, std::optional<std::uint64_t> seed = MonteCarloSettings{}.seed,
        MonteCarloBackend backend = MonteCarloSettings{}.backend,
        int thread_count = MonteCarloSettings{}.thread_count)
        : settings_{path_count, step_count, seed, backend, thread_count} {}

    /// Prices a European or American vanilla option.
    /// @return Price, or a contract, context, settings, or backend error.
    [[nodiscard]] Result<double> price(const EuropeanOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AmericanOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// CPU European requests with feasible stencils reuse per-call samples, with storage linear in path count.
    /// Requests without feasible stencils stream the scalar price without retaining samples.
    /// CPU and CUDA American requests reuse a path buffer; spot stencils also cache growth factors,
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

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloVanillaEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
