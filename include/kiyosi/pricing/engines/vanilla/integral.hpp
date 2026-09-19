#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Simpson quadrature over the terminal lognormal density.
class KIYOSI_EXPORT QuadratureVanillaEngine {
public:
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
