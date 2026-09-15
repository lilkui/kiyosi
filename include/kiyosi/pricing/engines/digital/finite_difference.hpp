#pragma once

#include <concepts>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceDigitalEngine {
public:
    explicit FiniteDifferenceDigitalEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceDigitalEngine(int asset_steps, int time_steps,
                                  finite_difference_scheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_steps, time_steps, scheme} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires (std::same_as<Payoff, CashOrNothingPayoff> ||
                  std::same_as<Payoff, AssetOrNothingPayoff>) &&
                 std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Payoff, CashOrNothingPayoff>)
            return price_cash_or_nothing(option, context);
        else
            return price_asset_or_nothing(option, context);
    }

    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_cash_or_nothing(
        const EuropeanCashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price_asset_or_nothing(
        const EuropeanAssetOrNothingOption&, const PricingContext&) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
