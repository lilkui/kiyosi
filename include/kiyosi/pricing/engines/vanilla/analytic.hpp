#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Closed-form Black-Scholes-Merton engine for European vanilla options.
class KIYOSI_EXPORT AnalyticVanillaEngine {
public:
    /// Computes only price; returns intrinsic value at expiry_date.
    /// @return Price, or a contract or context error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<double> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context, GreeksRequest{}));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        GreeksRequest greeks, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine) { return engine.price_native(option, context, greeks); }, /* native_complete: missing values are unsupported analytic limits */ true);
    }

private:
    [[nodiscard]] Result<PricingResult> price_native(
        const EuropeanOption&, const PricingContext&, GreeksRequest) const;
};

} // namespace kiyosi
