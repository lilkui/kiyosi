#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/digital.hpp>

namespace kiyosi {

class AnalyticDigitalEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires (std::same_as<Payoff, CashOrNothingPayoff> ||
                  std::same_as<Payoff, AssetOrNothingPayoff>) &&
                 std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        PricingRequest request = {}) const
    {
        if constexpr (std::same_as<Payoff, AssetOrNothingPayoff>)
            return price_impl(option, 1.0, true, context, request);
        else
            return price_impl(option, option.payout(), false, context, request);
    }

private:
    template <OptionPayoff Payoff, OptionExercise Exercise>
    [[nodiscard]] result<PricingResult> price_impl(
        const ExerciseBasedOption<Payoff, Exercise>& option, double payout, bool asset,
        const PricingContext& context, PricingRequest request) const
    {
        return price_impl(option.type(), option.strike(), payout, asset, option.expiry(), context, request);
    }

    [[nodiscard]] result<PricingResult> price_impl(
        option_type, double, double, bool, date, const PricingContext&, PricingRequest) const;
};

} // namespace kiyosi
