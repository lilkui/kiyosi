#pragma once

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

struct MonteCarloSettings {
    int path_count = 100'000;
    int step_count = 50;
    std::optional<std::uint64_t> seed;
};

using MonteCarloEuropeanSettings = MonteCarloSettings;
using MonteCarloAmericanSettings = MonteCarloSettings;

class MonteCarloEuropeanEngine {
public:
    explicit MonteCarloEuropeanEngine(MonteCarloSettings settings = {}) : settings_(settings) {}
    MonteCarloEuropeanEngine(int path_count, int step_count,
                             std::optional<std::uint64_t> seed = std::nullopt)
        : settings_{path_count, step_count, seed} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, EuropeanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const EuropeanOption&, const PricingContext&) const;
    MonteCarloSettings settings_;
};

class MonteCarloAmericanEngine {
public:
    explicit MonteCarloAmericanEngine(MonteCarloSettings settings = {}) : settings_(settings) {}
    MonteCarloAmericanEngine(int path_count, int step_count,
                             std::optional<std::uint64_t> seed = std::nullopt)
        : settings_{path_count, step_count, seed} {}

    template <OptionPayoff Payoff, OptionExercise Exercise>
        requires std::same_as<Payoff, VanillaPayoff> && std::same_as<Exercise, AmericanExercise>
    [[nodiscard]] result<PricingResult> price(
        const ExerciseBasedOption<Payoff, Exercise>& option, const PricingContext& context) const
    {
        return price_impl(option, context);
    }

    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    [[nodiscard]] result<PricingResult> price_impl(
        const AmericanOption&, const PricingContext&) const;
    MonteCarloSettings settings_;
};

using McEuropeanEngine = MonteCarloEuropeanEngine;
using McAmericanEngine = MonteCarloAmericanEngine;

} // namespace kiyosi
