#pragma once

#include <concepts>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Closed-form cash-or-nothing and asset-or-nothing valuation with analytic delta and gamma.
class KIYOSI_EXPORT AnalyticDigitalEngine {
public:
    /// Prices a European cash-or-nothing or asset-or-nothing option.
    /// @return Price, or a contract or context error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires(std::same_as<Payoff, CashOrNothingPayoff> ||
                 std::same_as<Payoff, AssetOrNothingPayoff>) &&
                std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<double> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context, detail::RiskMeasureOutput::price_only));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires(std::same_as<Payoff, CashOrNothingPayoff> ||
                 std::same_as<Payoff, AssetOrNothingPayoff>) &&
                std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
                                         [&](const auto& engine) {
                                             return engine.price_native(option, context, level == GreeksLevel::basic ? detail::RiskMeasureOutput::basic : detail::RiskMeasureOutput::all);
                                         });
    }

private:
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires(std::same_as<Payoff, CashOrNothingPayoff> ||
                 std::same_as<Payoff, AssetOrNothingPayoff>) &&
                std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price_native(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context, detail::RiskMeasureOutput output) const
    {
        if constexpr (std::same_as<Payoff, AssetOrNothingPayoff>)
            return price_impl(option.option_type(), option.strike(), 1.0, true, option.effective_date(),
                              option.expiry_date(), context, output);
        else
            return price_impl(option.option_type(), option.strike(), option.payout(), false,
                              option.effective_date(), option.expiry_date(), context, output);
    }

    [[nodiscard]] Result<PricingResult> price_impl(
        OptionType, double, double, bool, Date, Date, const PricingContext&, detail::RiskMeasureOutput) const;
};

} // namespace kiyosi
