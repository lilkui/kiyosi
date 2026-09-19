#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for vanilla European and American options.
class KIYOSI_EXPORT FiniteDifferenceVanillaEngine {
public:
    explicit FiniteDifferenceVanillaEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceVanillaEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> &&
                 (std::same_as<Exercise, EuropeanExercise> || std::same_as<Exercise, AmericanExercise>)
    [[nodiscard]] Result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Exercise, EuropeanExercise>) return price_european(option, context);
        else return price_american(option, context);
    }

    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_european(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price_american(const AmericanOption&, const PricingContext&) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
