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
        requires(std::same_as<Payoff, CashOrNothingPayoff> ||
                 std::same_as<Payoff, AssetOrNothingPayoff>) &&
                std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Payoff, AssetOrNothingPayoff>)
            return price_impl(option.option_type(), option.strike(), 1.0, true, option.effective_date(),
                              option.expiry_date(), context);
        else
            return price_impl(option.option_type(), option.strike(), option.payout(), false,
                              option.effective_date(), option.expiry_date(), context);
    }

private:
    [[nodiscard]] Result<PricingResult> price_impl(
        OptionType, double, double, bool, Date, Date, const PricingContext&) const;
};

} // namespace kiyosi
