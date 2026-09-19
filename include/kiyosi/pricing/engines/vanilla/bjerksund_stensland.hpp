#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Bjerksund-Stensland (2002) two-step American approximation.
class KIYOSI_EXPORT BjerksundStenslandVanillaEngine {
public:
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] Result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

private:
    [[nodiscard]] Result<PricingResult> price_impl(
        const AmericanOption&, const PricingContext&) const;
};

} // namespace kiyosi
