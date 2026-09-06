#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

struct BinomialSettings {
    int steps = 256;
};
using BinomialAmericanSettings = BinomialSettings;
using BinomialEuropeanSettings = BinomialSettings;
using CrrSettings = BinomialSettings;

/// Cox-Ross-Rubinstein American engine.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks are unsupported and zero.
class KIYOSI_EXPORT BinomialAmericanEngine {
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
class KIYOSI_EXPORT BinomialEuropeanEngine {
public:
    explicit BinomialEuropeanEngine(BinomialEuropeanSettings settings = {}) : settings_(settings) {}
    explicit BinomialEuropeanEngine(int steps) : settings_{steps} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

    BinomialEuropeanSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const EuropeanOption&, const PricingContext&) const;
    BinomialEuropeanSettings settings_;
};
using CrrAmericanEngine = BinomialAmericanEngine;
using CrrEuropeanEngine = BinomialEuropeanEngine;
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
using BinomialTreeVanillaEngine = CrrEngine;
using CrrVanillaEngine = CrrEngine;

} // namespace kiyosi
