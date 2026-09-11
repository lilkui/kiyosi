#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

struct BinomialSettings {
    int steps = 256;
};

/// Cox-Ross-Rubinstein American engine.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks are unavailable.
class KIYOSI_EXPORT BinomialAmericanEngine {
public:
    explicit BinomialAmericanEngine(BinomialSettings settings = {}) : settings_(settings) {}
    explicit BinomialAmericanEngine(int steps) : settings_{steps} {}

    [[nodiscard]] result<PricingResult> price(
        const AmericanOption& option, const PricingContext& context) const;

    BinomialSettings settings() const noexcept { return settings_; }

private:
    BinomialSettings settings_;
};
class KIYOSI_EXPORT BinomialEuropeanEngine {
public:
    explicit BinomialEuropeanEngine(BinomialSettings settings = {}) : settings_(settings) {}
    explicit BinomialEuropeanEngine(int steps) : settings_{steps} {}

    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption& option, const PricingContext& context) const;

    BinomialSettings settings() const noexcept { return settings_; }

private:
    BinomialSettings settings_;
};
class KIYOSI_EXPORT CrrEngine {
public:
    explicit CrrEngine(BinomialSettings settings = {}) : settings_(settings) {}
    explicit CrrEngine(int steps) : settings_{steps} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> &&
                 (std::same_as<Exercise, EuropeanExercise> || std::same_as<Exercise, AmericanExercise>)
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        if constexpr (std::same_as<Exercise, EuropeanExercise>) return price_european(option, context);
        else return price_american(option, context);
    }

    BinomialSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_european(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price_american(const AmericanOption&, const PricingContext&) const;
    BinomialSettings settings_;
};

} // namespace kiyosi
