#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Closed-form Black-Scholes-Merton engine for European vanilla options.
class KIYOSI_EXPORT AnalyticVanillaEngine {
public:
    /// Computes only price; returns intrinsic value at expiry_date.
    /// @return Price, or a contract or context error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<double> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context, detail::RiskMeasureOutput::price_only));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context, level == GreeksLevel::basic
                    ? detail::RiskMeasureOutput::basic : detail::RiskMeasureOutput::all);
            }, /* native_complete: missing values are unsupported analytic limits */ true);
    }

private:
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price_native(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context, detail::RiskMeasureOutput output) const
    {
        return price_impl(option, context, output);
    }

    [[nodiscard]] Result<PricingResult> price_impl(
        const EuropeanOption&, const PricingContext&, detail::RiskMeasureOutput) const;
};

} // namespace kiyosi
