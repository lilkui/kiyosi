#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for vanilla European and American options.
/// Accepts 3..10,000 asset steps and 1..100,000 time steps.
class KIYOSI_EXPORT FiniteDifferenceVanillaEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceVanillaEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceVanillaEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices a European or American vanilla option.
    /// @return Price, or a contract, context, or settings error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> &&
                 (std::same_as<Exercise, EuropeanExercise> || std::same_as<Exercise, AmericanExercise>)
    [[nodiscard]] Result<double> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context, GreeksRequest{}));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> &&
                 (std::same_as<Exercise, EuropeanExercise> || std::same_as<Exercise, AmericanExercise>)
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
    [[nodiscard]] Result<PricingResult> price_native(const EuropeanOption&, const PricingContext&, GreeksRequest) const;
    [[nodiscard]] Result<PricingResult> price_native(const AmericanOption&, const PricingContext&, GreeksRequest) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
