#pragma once

#include <concepts>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for European digital options.
/// Upper-edge values use BSM exercise probabilities at each time step.
/// Accepts 3..10,000 asset steps and 1..100,000 time steps.
/// Prices materially outside the non-negative payoff bounds return `invalid_result`.
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
        return detail::price_value(price_native(option, context, GreeksRequest{}));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires(std::same_as<Payoff, CashOrNothingPayoff> ||
                 std::same_as<Payoff, AssetOrNothingPayoff>) &&
                std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        GreeksRequest greeks, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, greeks, settings,
                                         [&](const auto& engine) {
                                             return engine.price_native(option, context, greeks);
                                         });
    }

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_native(
        const CashOrNothingOption&, const PricingContext&, GreeksRequest) const;
    [[nodiscard]] Result<PricingResult> price_native(
        const AssetOrNothingOption&, const PricingContext&, GreeksRequest) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
