#pragma once

#include <concepts>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for European digital options.
/// Accepts 3..10,000 asset steps and 1..100,000 time steps.
class KIYOSI_EXPORT FiniteDifferenceDigitalEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceDigitalEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceDigitalEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices a European cash-or-nothing or asset-or-nothing option.
    /// @return Price, or a contract, context, or settings error.
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

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires(std::same_as<Payoff, CashOrNothingPayoff> ||
                 std::same_as<Payoff, AssetOrNothingPayoff>) &&
                std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price_native(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context, detail::RiskMeasureOutput output) const
    {
        if constexpr (std::same_as<Payoff, CashOrNothingPayoff>)
            return price_cash_or_nothing(option, context, output);
        else
            return price_asset_or_nothing(option, context, output);
    }
    [[nodiscard]] Result<PricingResult> price_cash_or_nothing(
        const CashOrNothingOption&, const PricingContext&, detail::RiskMeasureOutput) const;
    [[nodiscard]] Result<PricingResult> price_asset_or_nothing(
        const AssetOrNothingOption&, const PricingContext&, detail::RiskMeasureOutput) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
