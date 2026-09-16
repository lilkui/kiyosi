#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

class KIYOSI_EXPORT AnalyticVanillaEngine {
public:
    /// Returns intrinsic value with Greeks unavailable when valued at expiry.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const EuropeanOption&, const PricingContext&) const;
};

} // namespace kiyosi
