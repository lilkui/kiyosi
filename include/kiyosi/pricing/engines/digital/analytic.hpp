#pragma once

#include <concepts>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form cash-or-nothing and asset-or-nothing valuation with analytic delta and gamma.
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
            return price_impl(option.type(), option.strike(), 1.0, true, option.effective(),
                              option.expiry(), context);
        else
            return price_impl(option.type(), option.strike(), option.payout(), false,
                              option.effective(), option.expiry(), context);
    }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        option_type, double, double, bool, date, date, const PricingContext&) const;
};

} // namespace kiyosi
