#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/settings/binomial.hpp>

namespace kiyosi {

/// Cox-Ross-Rubinstein binomial-tree engine for vanilla European and American options.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks use numerical differences.
class KIYOSI_EXPORT CoxRossRubinsteinVanillaEngine {
public:
    /// Creates an engine with aggregate binomial settings.
    explicit CoxRossRubinsteinVanillaEngine(BinomialSettings settings = {}) : settings_(settings) {}
    /// Creates an engine with an explicit tree step count.
    explicit CoxRossRubinsteinVanillaEngine(int step_count) : settings_{step_count} {}

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
    BinomialSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_native(const EuropeanOption&, const PricingContext&, GreeksRequest) const;
    [[nodiscard]] Result<PricingResult> price_native(const AmericanOption&, const PricingContext&, GreeksRequest) const;
    BinomialSettings settings_;
};

} // namespace kiyosi
