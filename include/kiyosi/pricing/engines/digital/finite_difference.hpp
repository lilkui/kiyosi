#pragma once

#include <type_traits>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceDigitalEngine;
[[nodiscard]] Result<FiniteDifferenceDigitalEngine> make_finite_difference_digital_engine(FiniteDifferenceSettings settings);

/// Uniform-grid finite-difference engine for European digital options.
/// Upper-edge values use BSM exercise probabilities at each time step.
/// Crank-Nicolson starts with two implicit Euler half-steps per initial interval,
/// for up to two intervals. Euler schemes retain their configured time steps.
/// Returned prices stay within theoretical payoff bounds. CN upper-bound checks allow
/// scalar carry discretization error from startup and negative carry; other violations
/// beyond roundoff return `invalid_result`.
class KIYOSI_EXPORT FiniteDifferenceDigitalEngine {
public:
    /// Creates an engine with valid default settings.
    FiniteDifferenceDigitalEngine() = default;

    /// Prices a European cash-or-nothing or asset-or-nothing option.
    /// @return Price, or a contract, context, or numerical error.
    [[nodiscard]] Result<double> price(const CashOrNothingOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AssetOrNothingOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// Delta, gamma, and speed reuse the solved layer.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceDigitalEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceDigitalEngine> make_finite_difference_digital_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceDigitalEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceDigitalEngine>)
    {
        return make_finite_difference_digital_engine(settings);
    }

    FiniteDifferenceSettings settings_;
};

/// Creates an engine after validating its configuration.
/// @return The engine, or an `invalid_parameter` error.
[[nodiscard]] inline Result<FiniteDifferenceDigitalEngine> make_finite_difference_digital_engine(FiniteDifferenceSettings settings)
{
    const auto valid = validate_finite_difference_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return FiniteDifferenceDigitalEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceDigitalEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
