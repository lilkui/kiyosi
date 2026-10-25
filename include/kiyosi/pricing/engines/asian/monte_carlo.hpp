#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT MonteCarloAsianEngine;
[[nodiscard]] Result<MonteCarloAsianEngine> make_monte_carlo_asian_engine(MonteCarloSettings settings);

/// Continuous arithmetic/geometric average-price valuation under constant-coefficient BSM.
/// Arithmetic uses step_count equal trapezoid intervals over the remaining averaging window,
/// with the first interval analytically conditioned out. A forward-start lead is separate.
/// Geometric and single-fixing prices sample their exact lognormal distribution; steps are unused.
/// Realized history is fixed under spot differentiation. Delta/gamma use conditional derivatives
/// for arithmetic and pathwise/hybrid likelihood estimates for geometric; other Greeks and all
/// expiry Greeks are unavailable. Sampling error and arithmetic integration bias remain; gamma
/// can have high variance near expiry or at tiny volatility. Seasoned geometric gamma may be negative.
/// CPU supports pseudorandom/Sobol; optional CUDA supports pseudorandom. Seeded CPU results
/// reproduce across thread counts in the same build. All storage is request-local; concurrent
/// const calls on shared or distinct instances are safe without concurrent assignment/mutation.
/// Allocation/internal exceptions propagate as in MonteCarloSettings; no cancellation is provided.
class KIYOSI_EXPORT MonteCarloAsianEngine {
public:
    MonteCarloAsianEngine() = default;
    [[nodiscard]] Result<double> price(const ArithmeticAveragePriceOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const GeometricAveragePriceOption& option, const PricingContext& context) const;
    /// Validates numerical shifts for API consistency; these estimators do not use spot bumps.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const ArithmeticAveragePriceOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const GeometricAveragePriceOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    explicit MonteCarloAsianEngine(MonteCarloSettings settings) : settings_(settings) {}
    friend Result<MonteCarloAsianEngine> make_monte_carlo_asian_engine(MonteCarloSettings);
    friend Result<MonteCarloAsianEngine> make_engine(MonteCarloSettings settings, std::type_identity<MonteCarloAsianEngine>)
    {
        return make_monte_carlo_asian_engine(settings);
    }
    MonteCarloSettings settings_;
};

[[nodiscard]] inline Result<MonteCarloAsianEngine> make_monte_carlo_asian_engine(MonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return MonteCarloAsianEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloAsianEngine, Option> = true;
} // namespace detail
} // namespace kiyosi
