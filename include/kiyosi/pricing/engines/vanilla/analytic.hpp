#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form Black-Scholes-Merton engine for European vanilla options.
class KIYOSI_EXPORT AnalyticVanillaEngine {
public:
    /// Returns intrinsic value with Greeks unavailable when valued at expiry_date.
    /// @return Pricing measures, or a contract or context error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

private:
    [[nodiscard]] Result<PricingResult> price_impl(
        const EuropeanOption&, const PricingContext&) const;
};

} // namespace kiyosi
