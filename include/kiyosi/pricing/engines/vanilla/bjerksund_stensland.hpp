#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Bjerksund-Stensland (2002) two-step American approximation.
class KIYOSI_EXPORT BjerksundStenslandVanillaEngine {
public:
    /// Prices an American vanilla option with the two-step approximation.
    /// @return Price, or a contract or context error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] Result<double> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }

private:
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] Result<PricingResult> price_native(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

    [[nodiscard]] Result<PricingResult> price_impl(
        const AmericanOption&, const PricingContext&) const;
};

} // namespace kiyosi
