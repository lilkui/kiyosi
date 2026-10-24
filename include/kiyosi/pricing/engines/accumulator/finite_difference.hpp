#pragma once

#include <type_traits>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceAccumulatorEngine;
[[nodiscard]] Result<FiniteDifferenceAccumulatorEngine> make_finite_difference_accumulator_engine(FiniteDifferenceSettings settings);

/// Marches the accrual value back as an affine function of the accumulated quantity, so a single
/// sweep prices every opening position.
/// Terminal payoffs and observation changes are locally averaged at strike and knock-out levels.
/// Crank-Nicolson restarts with two intervals of implicit half-steps after each observation and expiry.
/// Current observations and expiry settlements use the actual spot without averaging.
class KIYOSI_EXPORT FiniteDifferenceAccumulatorEngine {
public:
    /// Creates an engine with valid default settings.
    FiniteDifferenceAccumulatorEngine() = default;

    /// Prices an accumulator by finite differences.
    /// @return Price, or a contract, context, or numerical error.
    [[nodiscard]] Result<double> price(const Accumulator& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// Delta, gamma, and speed reuse the solved layers; vanna/zomma and charm/color difference native delta/gamma shifted in volatility and time.
    /// Other measures use numerical price differences.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceAccumulatorEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceAccumulatorEngine> make_finite_difference_accumulator_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceAccumulatorEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceAccumulatorEngine>)
    {
        return make_finite_difference_accumulator_engine(settings);
    }

    FiniteDifferenceSettings settings_;
};

/// Creates an engine after validating its configuration.
/// @return The engine, or an `invalid_parameter` error.
[[nodiscard]] inline Result<FiniteDifferenceAccumulatorEngine> make_finite_difference_accumulator_engine(FiniteDifferenceSettings settings)
{
    const auto valid = validate_finite_difference_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return FiniteDifferenceAccumulatorEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceAccumulatorEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
