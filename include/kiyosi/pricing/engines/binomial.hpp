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
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    explicit BinomialAmericanEngine(BinomialAmericanSettings settings = {}) : settings_(settings) {}
    explicit BinomialAmericanEngine(int steps) : settings_{steps} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context,
        PricingRequest request = {}) const
    {
        return price_impl(option, context, request);
    }

    BinomialAmericanSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const AmericanOption&, const PricingContext&, PricingRequest) const;
    BinomialAmericanSettings settings_;
};

} // namespace kiyosi
