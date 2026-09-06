#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/digital.hpp>

namespace kiyosi {

class KIYOSI_EXPORT AnalyticDigitalEngine {
public:
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires (std::same_as<Payoff, CashOrNothingPayoff> ||
                  std::same_as<Payoff, AssetOrNothingPayoff>) &&
                 std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Payoff, AssetOrNothingPayoff>)
            return price_impl(option, 1.0, true, context);
        else
            return price_impl(option, option.payout(), false, context);
    }

private:
    template <OptionPayoff Payoff, OptionExercise Exercise>
    [[nodiscard]] result<PricingResult> price_impl(
        const ExerciseBasedOption<Payoff, Exercise>& option, double payout, bool asset,
        const PricingContext& context) const
    {
        return price_impl(option.type(), option.strike(), payout, asset, option.expiry(), context);
    }

    [[nodiscard]] result<PricingResult> price_impl(
        option_type, double, double, bool, date, const PricingContext&) const;
};

} // namespace kiyosi
