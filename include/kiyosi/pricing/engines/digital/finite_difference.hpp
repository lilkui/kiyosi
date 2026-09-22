#pragma once

#include <concepts>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

/// Uniform-grid finite-difference engine for European digital options.
class KIYOSI_EXPORT FiniteDifferenceDigitalEngine {
public:
    /// Creates an engine with aggregate finite-difference settings.
    explicit FiniteDifferenceDigitalEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with explicit grid dimensions and scheme.
    FiniteDifferenceDigitalEngine(int asset_step_count, int time_step_count,
                                  FiniteDifferenceScheme scheme = FiniteDifferenceSettings{}.scheme)
        : settings_{asset_step_count, time_step_count, scheme} {}

    /// Prices a European cash-or-nothing or asset-or-nothing option.
    /// @return Pricing measures, or a contract, context, or settings error.
    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires(std::same_as<Payoff, CashOrNothingPayoff> ||
                 std::same_as<Payoff, AssetOrNothingPayoff>) &&
                std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] Result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Payoff, CashOrNothingPayoff>)
            return price_cash_or_nothing(option, context);
        else
            return price_asset_or_nothing(option, context);
    }

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_cash_or_nothing(
        const CashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price_asset_or_nothing(
        const AssetOrNothingOption&, const PricingContext&) const;
    FiniteDifferenceSettings settings_;
};

} // namespace kiyosi
