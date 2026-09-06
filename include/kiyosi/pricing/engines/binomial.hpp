#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

struct BinomialAmericanSettings {
    int steps = 256;
};

/// Cox-Ross-Rubinstein American engine.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks are unsupported and zero.
class BinomialAmericanEngine {
public:
    explicit BinomialAmericanEngine(BinomialAmericanSettings settings = {}) : settings_(settings) {}
    explicit BinomialAmericanEngine(int steps) : settings_{steps} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

    BinomialAmericanSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const AmericanOption&, const PricingContext&) const;
    BinomialAmericanSettings settings_;
};

} // namespace kiyosi
