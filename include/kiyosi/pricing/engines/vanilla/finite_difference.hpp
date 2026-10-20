#pragma once

#include <type_traits>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceVanillaEngine;
[[nodiscard]] Result<FiniteDifferenceVanillaEngine> make_finite_difference_vanilla_engine(FiniteDifferenceSettings settings);

/// Uniform-grid finite-difference engine for vanilla European and American options.
/// European upper-edge values include exact BSM time value; American edges also permit immediate exercise.
/// Crank-Nicolson starts with two implicit Euler half-steps per initial interval,
/// for up to two intervals. American exercise is applied after every substep.
class KIYOSI_EXPORT FiniteDifferenceVanillaEngine {
public:
    /// Creates an engine with valid default settings.
    FiniteDifferenceVanillaEngine() = default;

    /// Prices a European or American vanilla option.
    /// @return Price, or a contract, context, or numerical error.
    [[nodiscard]] Result<double> price(const EuropeanOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AmericanOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// Delta, gamma, and speed reuse the solved layer.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceVanillaEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceVanillaEngine> make_finite_difference_vanilla_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceVanillaEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceVanillaEngine>)
    {
        return make_finite_difference_vanilla_engine(settings);
    }

    FiniteDifferenceSettings settings_;
};

/// Creates an engine after validating its configuration.
/// @return The engine, or an `invalid_parameter` error.
[[nodiscard]] inline Result<FiniteDifferenceVanillaEngine> make_finite_difference_vanilla_engine(FiniteDifferenceSettings settings)
{
    const auto valid = validate_finite_difference_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return FiniteDifferenceVanillaEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceVanillaEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
