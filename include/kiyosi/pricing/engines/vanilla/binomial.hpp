#pragma once

#include <concepts>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/binomial.hpp>

namespace kiyosi {

/// Cox-Ross-Rubinstein binomial-tree engine for vanilla European and American options.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks are unavailable.
class KIYOSI_EXPORT CoxRossRubinsteinVanillaEngine {
public:
    explicit CoxRossRubinsteinVanillaEngine(BinomialSettings settings = {}) : settings_(settings) {}
    explicit CoxRossRubinsteinVanillaEngine(int step_count) : settings_{step_count} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> &&
                 (std::same_as<Exercise, EuropeanExercise> || std::same_as<Exercise, AmericanExercise>)
    [[nodiscard]] Result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Exercise, EuropeanExercise>) return price_european(option, context);
        else return price_american(option, context);
    }

    BinomialSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] Result<PricingResult> price_european(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price_american(const AmericanOption&, const PricingContext&) const;
    BinomialSettings settings_;
};

} // namespace kiyosi
